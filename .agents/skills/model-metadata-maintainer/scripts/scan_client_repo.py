#!/usr/bin/env python3
"""Extract reproducible model-metadata candidates from a pinned client commit.

The target repository is treated as untrusted, discovery-only input. The scanner
reads Git objects and parses JSON/Python syntax; it never imports or executes the
target repository's code.
"""

from __future__ import annotations

import argparse
import ast
import fnmatch
import hashlib
import json
import re
import subprocess
import sys
import tokenize
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import urlparse


PROFILE_RELATIVE_PATH = Path("tools/model-metadata/client-source-profiles.json")
SOURCE_REGISTRY_RELATIVE_PATH = Path("tools/model-metadata/source-registry.json")
FULL_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
HTTPS_RE = re.compile(r"https://[^\s\"'<>)}\]]+")
PROFILE_CALL_RE = re.compile(r"profile$", re.IGNORECASE)
STATIC_MODEL_NAME_RE = re.compile(r"(?:MODEL|CATALOG|FALLBACK|EXTRA)", re.IGNORECASE)
MODEL_LITERAL_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/+-]{0,159}$")
PROFILE_LITERAL_FIELDS = {
    "aliases",
    "api_mode",
    "auth_type",
    "base_url",
    "description",
    "default_aux_model",
    "default_max_tokens",
    "display_name",
    "env_vars",
    "fallback_models",
    "models_url",
    "name",
    "signup_url",
    "supports_health_check",
    "supports_prompt_cache_key",
    "supports_vision",
    "supports_vision_tool_messages",
}
PROFILE_INHERITED_DEFAULT_FIELDS = {
    "api_mode",
    "auth_type",
}
MODEL_FIELDS = {
    "compat",
    "contextWindow",
    "cost",
    "id",
    "input",
    "maxTokens",
    "name",
    "output",
    "reasoning",
    "replacedBy",
    "status",
    "thinkingLevelMap",
}


class ScanError(RuntimeError):
    """A deterministic input or profile failure."""


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ScanError(f"cannot read valid JSON from {path}: {exc}") from exc


def git(repo: Path, *args: str, text: bool = True) -> str | bytes:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        check=False,
        capture_output=True,
        text=text,
    )
    if result.returncode:
        stderr = result.stderr if text else result.stderr.decode("utf-8", "replace")
        raise ScanError(f"git {' '.join(args)} failed: {stderr.strip()}")
    return result.stdout


def normalize_repository_url(value: str) -> str:
    url = value.strip()
    if url.startswith("git+"):
        url = url[4:]
    ssh_match = re.fullmatch(r"git@([^:]+):(.+)", url)
    if ssh_match:
        url = f"https://{ssh_match.group(1)}/{ssh_match.group(2)}"
    return url.removesuffix(".git").rstrip("/")


def select_profile(profiles: Any, profile_key: str) -> dict[str, Any]:
    if not isinstance(profiles, dict) or profiles.get("artifact_kind") != (
        "hiroute.model-metadata-client-source-profiles"
    ):
        raise ScanError("unexpected client source profiles artifact_kind")
    records = profiles.get("profiles")
    if not isinstance(records, list):
        raise ScanError("client source profiles must contain a profiles array")
    matches = [record for record in records if record.get("profile_key") == profile_key]
    if len(matches) != 1:
        raise ScanError(f"profile {profile_key!r} must resolve exactly once")
    profile = matches[0]
    if profile.get("evidence_eligible") is not False:
        raise ScanError(f"profile {profile_key!r} must be discovery-only")
    return profile


TREE_PATTERN_FIELDS = {
    "exclude_paths",
    "excluded_aggregate_paths",
    "locator_paths",
    "manifest_paths",
    "profile_base_paths",
    "profile_paths",
    "static_catalog_paths",
}


def pathspec_root(pattern: str) -> str:
    wildcard = min(
        (position for token in "*[?" if (position := pattern.find(token)) >= 0),
        default=len(pattern),
    )
    raw_prefix = pattern[:wildcard]
    prefix = raw_prefix.rstrip("/")
    if wildcard == len(pattern):
        return pattern
    if raw_prefix.endswith("/"):
        return prefix or "."
    parent = prefix.rsplit("/", 1)[0] if "/" in prefix else ""
    return parent or prefix or "."


def profile_tree_patterns(profile: dict[str, Any]) -> list[str]:
    patterns = {
        value
        for parser in profile.get("parsers", [])
        if isinstance(parser, dict)
        for field in TREE_PATTERN_FIELDS
        for value in parser.get(field, [])
        if isinstance(value, str) and value
    }
    if not patterns:
        raise ScanError("client source profile does not select any paths")
    return sorted(patterns)


def list_tree(repo: Path, commit: str, patterns: Iterable[str]) -> dict[str, str]:
    selected_patterns = tuple(patterns)
    roots = sorted({pathspec_root(pattern) for pattern in selected_patterns})
    raw = git(
        repo,
        "ls-tree",
        "-r",
        "-z",
        "--full-tree",
        commit,
        "--",
        *roots,
        text=False,
    )
    records: dict[str, str] = {}
    for entry in raw.split(b"\0"):
        if not entry:
            continue
        header, path_bytes = entry.split(b"\t", 1)
        _mode, kind, object_id = header.decode("ascii").split(" ", 2)
        if kind != "blob":
            continue
        path = path_bytes.decode("utf-8", "surrogateescape")
        if not any(fnmatch.fnmatchcase(path, pattern) for pattern in selected_patterns):
            continue
        records[path] = object_id
    return records


def matching_paths(
    tree: dict[str, str], patterns: Iterable[str], excluded: Iterable[str] = ()
) -> list[str]:
    exclusions = set(excluded)
    return sorted(
        path
        for path in tree
        if path not in exclusions and any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)
    )


def read_blob(repo: Path, commit: str, path: str) -> bytes:
    return git(repo, "show", f"{commit}:{path}", text=False)


def source_location(path: str, blob: str, pointer: str | None = None, line: int | None = None) -> dict[str, Any]:
    result: dict[str, Any] = {"git_object_id": blob, "path": path}
    if pointer is not None:
        result["pointer"] = pointer
    if line is not None:
        result["line"] = line
    return result


def scan_python_literal_locators(
    raw: bytes,
    path: str,
    blob: str,
    candidates: "CandidateSet",
) -> None:
    try:
        source = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ScanError(f"cannot decode Python source at {path}: {exc}") from exc
    try:
        tokens = tokenize.generate_tokens(iter(source.splitlines(keepends=True)).__next__)
        for token in tokens:
            if token.type != tokenize.STRING:
                continue
            try:
                value = ast.literal_eval(token.string)
            except (ValueError, TypeError, SyntaxError, MemoryError, RecursionError):
                continue
            if not isinstance(value, str):
                continue
            for match in HTTPS_RE.finditer(value):
                candidates.add_locator(
                    match.group(0),
                    source_location(path, blob, line=token.start[0]),
                )
    except (tokenize.TokenError, IndentationError) as exc:
        raise ScanError(f"cannot tokenize Python source at {path}: {exc}") from exc


def location_key(location: dict[str, Any]) -> tuple[str, str, str, int]:
    return (
        str(location.get("path", "")),
        str(location.get("pointer", "")),
        str(location.get("git_object_id", "")),
        int(location.get("line", 0)),
    )


def literal(node: ast.AST) -> Any:
    try:
        return ast.literal_eval(node)
    except (ValueError, TypeError, SyntaxError, MemoryError, RecursionError):
        return None


def call_name(node: ast.Call) -> str:
    target = node.func
    if isinstance(target, ast.Name):
        return target.id
    if isinstance(target, ast.Attribute):
        return target.attr
    return ""


def iter_string_literals(node: ast.AST) -> Iterable[tuple[str, int]]:
    for child in ast.walk(node):
        if isinstance(child, ast.Constant) and isinstance(child.value, str):
            yield child.value, getattr(child, "lineno", 0)


def looks_like_model_id(value: str) -> bool:
    return bool(
        MODEL_LITERAL_RE.fullmatch(value)
        and not value.startswith(("http://", "https://"))
        and (any(char.isdigit() for char in value) or value in {"auto", "default"})
    )


class CandidateSet:
    def __init__(self) -> None:
        self.providers: dict[str, dict[str, Any]] = {}
        self.models: dict[tuple[str, str], dict[str, Any]] = {}
        self.lifecycle: dict[tuple[str, str, str], dict[str, Any]] = {}
        self.locators: dict[str, dict[str, Any]] = {}
        self.excluded_inputs: list[dict[str, Any]] = []
        self.metadata_shape_signals: set[str] = set()

    def add_provider(
        self, provider_id: str, assertion: dict[str, Any], location: dict[str, Any]
    ) -> None:
        if not provider_id:
            return
        record = self.providers.setdefault(
            provider_id, {"provider_id": provider_id, "assertions": [], "locations": []}
        )
        if assertion and assertion not in record["assertions"]:
            record["assertions"].append(assertion)
        if location not in record["locations"]:
            record["locations"].append(location)

    def add_model(
        self,
        provider_id: str,
        model_id: str,
        assertion: dict[str, Any],
        location: dict[str, Any],
        role: str,
    ) -> None:
        if not provider_id or not looks_like_model_id(model_id):
            return
        key = (provider_id, model_id)
        record = self.models.setdefault(
            key,
            {
                "provider_id": provider_id,
                "model_id": model_id,
                "assertions": [],
                "roles": [],
                "locations": [],
            },
        )
        if assertion and assertion not in record["assertions"]:
            record["assertions"].append(assertion)
        if role not in record["roles"]:
            record["roles"].append(role)
        if location not in record["locations"]:
            record["locations"].append(location)

    def add_lifecycle(
        self,
        provider_id: str,
        model_id: str,
        kind: str,
        assertion: dict[str, Any],
        location: dict[str, Any],
    ) -> None:
        key = (provider_id, model_id, kind)
        record = self.lifecycle.setdefault(
            key,
            {
                "provider_id": provider_id,
                "model_id": model_id,
                "kind": kind,
                "assertions": [],
                "locations": [],
            },
        )
        if assertion not in record["assertions"]:
            record["assertions"].append(assertion)
        if location not in record["locations"]:
            record["locations"].append(location)

    def add_locator(self, locator: str, location: dict[str, Any]) -> None:
        cleaned = locator.rstrip(".,;:")
        if not cleaned.startswith("https://"):
            return
        record = self.locators.setdefault(cleaned, {"locator": cleaned, "locations": []})
        if location not in record["locations"]:
            record["locations"].append(location)

    def normalize(self) -> None:
        for records in (self.providers.values(), self.models.values(), self.lifecycle.values()):
            for record in records:
                record["locations"].sort(key=location_key)
                record["assertions"].sort(key=canonical_json)
                if "roles" in record:
                    record["roles"].sort()
        for record in self.locators.values():
            record["locations"].sort(key=location_key)
        self.excluded_inputs.sort(key=canonical_json)


def scan_openclaw(
    repo: Path,
    commit: str,
    tree: dict[str, str],
    parser: dict[str, Any],
    candidates: CandidateSet,
    scanned: dict[str, dict[str, str]],
) -> None:
    patterns = parser.get("manifest_paths", [])
    excluded = parser.get("exclude_paths", [])
    for excluded_path in sorted(set(excluded)):
        if excluded_path in tree:
            candidates.excluded_inputs.append(
                {
                    "path": excluded_path,
                    "reason": "aggregate bridge excluded by discovery profile",
                }
            )
    for path in matching_paths(tree, patterns, excluded):
        raw = read_blob(repo, commit, path)
        try:
            manifest = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ScanError(f"invalid JSON at {path}: {exc}") from exc
        scanned[path] = {
            "content_digest": sha256_bytes(raw),
            "git_object_id": tree[path],
            "parser": "openclaw-plugin-manifests",
        }
        catalog = manifest.get("modelCatalog")
        if not isinstance(catalog, dict):
            continue
        for key in catalog:
            candidates.metadata_shape_signals.add(f"openclaw.modelCatalog.{key}")
        discovery = catalog.get("discovery", {})
        if not isinstance(discovery, dict):
            discovery = {}
        aggregate_mapping = catalog.get("modelsDev")
        if isinstance(aggregate_mapping, dict) and aggregate_mapping:
            candidates.excluded_inputs.append(
                {
                    "path": path,
                    "pointer": "/modelCatalog/modelsDev",
                    "reason": "embedded aggregate mapping is discovery-only and not extracted as evidence",
                }
            )
        providers = catalog.get("providers", {})
        if isinstance(providers, dict):
            for provider_id, provider in sorted(providers.items()):
                if not isinstance(provider_id, str) or not isinstance(provider, dict):
                    continue
                provider_assertion = {
                    key: provider[key]
                    for key in ("api", "baseUrl", "defaultModel")
                    if key in provider
                }
                if provider_id in discovery:
                    provider_assertion["discovery"] = discovery[provider_id]
                provider_location = source_location(
                    path, tree[path], f"/modelCatalog/providers/{provider_id}"
                )
                candidates.add_provider(provider_id, provider_assertion, provider_location)
                for key in ("baseUrl",):
                    value = provider.get(key)
                    if isinstance(value, str):
                        candidates.add_locator(value, provider_location)
                default_model = provider.get("defaultModel")
                if isinstance(default_model, str):
                    candidates.add_model(
                        provider_id,
                        default_model,
                        {},
                        source_location(
                            path,
                            tree[path],
                            f"/modelCatalog/providers/{provider_id}/defaultModel",
                        ),
                        "client-default",
                    )
                models = provider.get("models", [])
                if not isinstance(models, list):
                    continue
                for index, model in enumerate(models):
                    if not isinstance(model, dict) or not isinstance(model.get("id"), str):
                        continue
                    assertion = {key: model[key] for key in MODEL_FIELDS if key in model}
                    candidates.add_model(
                        provider_id,
                        model["id"],
                        assertion,
                        source_location(
                            path,
                            tree[path],
                            f"/modelCatalog/providers/{provider_id}/models/{index}",
                        ),
                        "client-catalog",
                    )
                    for key in assertion:
                        candidates.metadata_shape_signals.add(f"openclaw.model.{key}")
        aliases = catalog.get("aliases", {})
        if isinstance(aliases, dict):
            for alias, alias_config in sorted(aliases.items()):
                if not isinstance(alias, str) or not isinstance(alias_config, dict):
                    continue
                provider_id = alias_config.get("provider")
                if isinstance(provider_id, str):
                    candidates.add_provider(
                        provider_id,
                        {"alias": alias, **alias_config},
                        source_location(
                            path, tree[path], f"/modelCatalog/aliases/{alias}"
                        ),
                    )
        suppressions = catalog.get("suppressions", [])
        if isinstance(suppressions, list):
            for index, suppression in enumerate(suppressions):
                if not isinstance(suppression, dict):
                    continue
                provider_id = suppression.get("provider")
                model_id = suppression.get("model")
                if isinstance(provider_id, str) and isinstance(model_id, str):
                    candidates.add_lifecycle(
                        provider_id,
                        model_id,
                        "client-suppression",
                        suppression,
                        source_location(
                            path, tree[path], f"/modelCatalog/suppressions/{index}"
                        ),
                    )
        for match in HTTPS_RE.finditer(raw.decode("utf-8", "replace")):
            candidates.add_locator(match.group(0), source_location(path, tree[path]))


def assignment_name(node: ast.Assign | ast.AnnAssign) -> str:
    target: ast.AST | None
    if isinstance(node, ast.AnnAssign):
        target = node.target
    else:
        target = node.targets[0] if len(node.targets) == 1 else None
    return target.id if isinstance(target, ast.Name) else ""


def assignment_value(node: ast.Assign | ast.AnnAssign) -> ast.AST | None:
    return node.value


def provider_id_from_assignment(name: str, values: dict[str, Any]) -> str:
    explicit = values.get("name")
    if isinstance(explicit, str) and explicit.strip():
        return explicit.strip().lower()
    return name.strip("_").replace("_", "-").lower()


def add_profile_model_values(
    candidates: CandidateSet,
    provider_id: str,
    values: dict[str, Any],
    location: dict[str, Any],
) -> None:
    fallback = values.get("fallback_models")
    if isinstance(fallback, (list, tuple)):
        for model_id in fallback:
            if isinstance(model_id, str):
                candidates.add_model(provider_id, model_id, {}, location, "client-fallback")
    auxiliary = values.get("default_aux_model")
    if isinstance(auxiliary, str) and auxiliary:
        candidates.add_model(provider_id, auxiliary, {}, location, "client-aux-default")


def scan_hermes_profiles(
    repo: Path,
    commit: str,
    tree: dict[str, str],
    parser: dict[str, Any],
    candidates: CandidateSet,
    scanned: dict[str, dict[str, str]],
) -> None:
    profile_paths = matching_paths(tree, parser.get("profile_paths", []))
    profile_base_paths = matching_paths(tree, parser.get("profile_base_paths", []))
    static_paths = set(matching_paths(tree, parser.get("static_catalog_paths", [])))
    locator_paths = set(matching_paths(tree, parser.get("locator_paths", [])))
    for excluded_path in sorted(set(parser.get("excluded_aggregate_paths", []))):
        if excluded_path in tree:
            candidates.excluded_inputs.append(
                {
                    "git_object_id": tree[excluded_path],
                    "path": excluded_path,
                    "reason": "embedded aggregate registry excluded by discovery profile",
                }
            )
    all_paths = sorted(set(profile_paths) | set(profile_base_paths) | static_paths | locator_paths)
    pending_profiles: list[tuple[str, dict[str, Any], dict[str, Any]]] = []
    inherited_defaults: dict[str, tuple[Any, dict[str, Any]]] = {}
    for path in all_paths:
        raw = read_blob(repo, commit, path)
        syntax: ast.Module | None = None
        if path in profile_paths or path in profile_base_paths or path in static_paths:
            try:
                syntax = ast.parse(raw.decode("utf-8"), filename=path)
            except (UnicodeDecodeError, SyntaxError) as exc:
                raise ScanError(f"cannot parse Python syntax at {path}: {exc}") from exc
        parser_roles: list[str] = []
        if path in profile_paths:
            parser_roles.append("profile")
        if path in profile_base_paths:
            parser_roles.append("profile-defaults")
        if path in static_paths:
            parser_roles.append("static-catalog")
        if path in locator_paths:
            parser_roles.append("locator")
        scanned[path] = {
            "content_digest": sha256_bytes(raw),
            "git_object_id": tree[path],
            "parser": "+".join(parser_roles),
        }

        if path in profile_base_paths:
            assert syntax is not None
            for node in syntax.body:
                if not isinstance(node, ast.ClassDef) or node.name != "ProviderProfile":
                    continue
                for class_node in node.body:
                    if not isinstance(class_node, (ast.Assign, ast.AnnAssign)):
                        continue
                    field_name = assignment_name(class_node)
                    field_value = assignment_value(class_node)
                    if field_name not in PROFILE_INHERITED_DEFAULT_FIELDS or field_value is None:
                        continue
                    parsed = literal(field_value)
                    if parsed is not None:
                        inherited_defaults[field_name] = (
                            parsed,
                            source_location(
                                path,
                                tree[path],
                                line=getattr(class_node, "lineno", 0),
                            ),
                        )

        if path in profile_paths:
            assert syntax is not None
            for node in ast.walk(syntax):
                if not isinstance(node, (ast.Assign, ast.AnnAssign)):
                    continue
                name = assignment_name(node)
                value = assignment_value(node)
                if not name or not isinstance(value, ast.Call) or not PROFILE_CALL_RE.search(call_name(value)):
                    continue
                values: dict[str, Any] = {}
                for keyword in value.keywords:
                    if keyword.arg in PROFILE_LITERAL_FIELDS:
                        parsed = literal(keyword.value)
                        if parsed is not None:
                            values[keyword.arg] = parsed
                provider_id = provider_id_from_assignment(name, values)
                location = source_location(path, tree[path], line=getattr(node, "lineno", 0))
                pending_profiles.append((provider_id, values, location))

        if path in static_paths:
            assert syntax is not None
            for node in syntax.body:
                if not isinstance(node, (ast.Assign, ast.AnnAssign)):
                    continue
                name = assignment_name(node)
                value = assignment_value(node)
                if not name or value is None:
                    continue
                if name == "_PROVIDER_MODELS" and isinstance(value, ast.Dict):
                    for key_node, value_node in zip(value.keys, value.values):
                        provider_id = literal(key_node) if key_node is not None else None
                        if not isinstance(provider_id, str):
                            continue
                        for model_id, line in iter_string_literals(value_node):
                            if looks_like_model_id(model_id):
                                candidates.add_model(
                                    provider_id,
                                    model_id,
                                    {},
                                    source_location(path, tree[path], line=line),
                                    "client-static-catalog",
                                )
                elif STATIC_MODEL_NAME_RE.search(name):
                    provider_hint = (
                        name.strip("_")
                        .removesuffix("_MODELS")
                        .removesuffix("_MODEL")
                        .lower()
                        .replace("_", "-")
                    ) or "unscoped"
                    for model_id, line in iter_string_literals(value):
                        if looks_like_model_id(model_id):
                            candidates.add_model(
                                provider_hint,
                                model_id,
                                {},
                                source_location(path, tree[path], line=line),
                                "client-static-collection",
                            )

        if path in locator_paths:
            if syntax is None:
                scan_python_literal_locators(raw, path, tree[path], candidates)
            else:
                for node in ast.walk(syntax):
                    if not isinstance(node, ast.Constant) or not isinstance(node.value, str):
                        continue
                    for match in HTTPS_RE.finditer(node.value):
                        candidates.add_locator(
                            match.group(0),
                            source_location(path, tree[path], line=getattr(node, "lineno", 0)),
                        )

    for provider_id, values, location in pending_profiles:
        candidates.add_provider(provider_id, values, location)
        for field_name, (default_value, default_location) in inherited_defaults.items():
            if field_name not in values:
                candidates.add_provider(
                    provider_id,
                    {field_name: default_value, "inherited_from": "ProviderProfile"},
                    default_location,
                )
        add_profile_model_values(candidates, provider_id, values, location)
        for key in ("base_url", "models_url", "signup_url"):
            value_string = values.get(key)
            if isinstance(value_string, str):
                candidates.add_locator(value_string, location)

    for signal in (
        "hermes.provider.api_mode",
        "hermes.provider.auth_type",
        "hermes.provider.base_url",
        "hermes.provider.default_aux_model",
        "hermes.provider.default_max_tokens",
        "hermes.provider.display_name",
        "hermes.provider.fallback_models",
        "hermes.provider.models_url",
    ):
        candidates.metadata_shape_signals.add(signal)


def authorized_prefixes(source_registry: Any) -> list[tuple[str, str]]:
    if not isinstance(source_registry, dict):
        return []
    result: list[tuple[str, str]] = []
    for authority in source_registry.get("authorities", []):
        if not isinstance(authority, dict):
            continue
        authority_key = authority.get("authority_key")
        for prefix in authority.get("allowed_https_prefixes", []):
            if isinstance(authority_key, str) and isinstance(prefix, str):
                result.append((prefix, authority_key))
    return sorted(result, key=lambda item: (-len(item[0]), item[0]))


def classify_locators(
    candidates: CandidateSet, prefixes: list[tuple[str, str]], repository_url: str
) -> None:
    repository_host = urlparse(repository_url).netloc
    for locator, record in candidates.locators.items():
        matched = next(((prefix, authority) for prefix, authority in prefixes if locator.startswith(prefix)), None)
        if matched:
            record["classification"] = "authorized-first-party-candidate"
            record["registry_authority_key"] = matched[1]
        elif urlparse(locator).netloc == repository_host:
            record["classification"] = "client-controlled-discovery-only"
        else:
            record["classification"] = "unclassified-discovery-only"


def catalog_index(catalog: Any) -> dict[str, list[str]]:
    index: dict[str, set[str]] = defaultdict(set)
    if not isinstance(catalog, dict):
        return {}
    for model in catalog.get("models", []):
        if not isinstance(model, dict) or not isinstance(model.get("model_key"), str):
            continue
        for upstream_id in model.get("upstream_ids", []):
            if isinstance(upstream_id, str):
                index[upstream_id.casefold()].add(model["model_key"])
    return {key: sorted(values) for key, values in index.items()}


def annotate_catalog_matches(candidates: CandidateSet, index: dict[str, list[str]]) -> None:
    for record in candidates.models.values():
        model_id = record["model_id"]
        spellings = {model_id.casefold()}
        if "/" in model_id:
            spellings.add(model_id.rsplit("/", 1)[-1].casefold())
        matches = sorted({key for spelling in spellings for key in index.get(spelling, [])})
        record["catalog_lexical_matches"] = matches


def build_artifact(
    *,
    repo: Path,
    commit: str,
    profile_key: str,
    profiles_path: Path,
    source_registry_path: Path,
    catalog_path: Path | None,
    as_of: str,
) -> dict[str, Any]:
    if not FULL_COMMIT_RE.fullmatch(commit):
        raise ScanError("--commit must be a full 40-character lowercase hexadecimal commit")
    profiles = load_json(profiles_path)
    source_registry = load_json(source_registry_path)
    profile = select_profile(profiles, profile_key)
    repository_url = normalize_repository_url(str(git(repo, "remote", "get-url", "origin")).strip())
    expected_urls = {
        normalize_repository_url(value) for value in profile.get("expected_repository_urls", [])
    }
    if repository_url not in expected_urls:
        raise ScanError(
            f"repository origin {repository_url!r} is not allowed for profile {profile_key!r}"
        )
    resolved = str(git(repo, "rev-parse", "--verify", f"{commit}^{{commit}}")).strip()
    if resolved != commit:
        raise ScanError(f"requested commit resolved to unexpected object {resolved}")
    tree_id = str(git(repo, "rev-parse", f"{commit}^{{tree}}")).strip()
    tree = list_tree(repo, commit, profile_tree_patterns(profile))
    candidates = CandidateSet()
    scanned: dict[str, dict[str, str]] = {}
    for parser in profile.get("parsers", []):
        kind = parser.get("kind") if isinstance(parser, dict) else None
        if kind == "openclaw-plugin-manifests":
            scan_openclaw(repo, commit, tree, parser, candidates, scanned)
        elif kind == "hermes-python-profiles":
            scan_hermes_profiles(repo, commit, tree, parser, candidates, scanned)
        else:
            raise ScanError(f"profile {profile_key!r} has unsupported parser {kind!r}")
    classify_locators(candidates, authorized_prefixes(source_registry), repository_url)
    compared_catalog = load_json(catalog_path) if catalog_path is not None else None
    index = catalog_index(compared_catalog)
    annotate_catalog_matches(candidates, index)
    candidates.normalize()
    model_records = sorted(
        candidates.models.values(), key=lambda record: (record["provider_id"], record["model_id"])
    )
    unmatched_count = sum(not record["catalog_lexical_matches"] for record in model_records)
    artifact: dict[str, Any] = {
        "artifact_kind": "hiroute.model-metadata-client-discovery",
        "as_of": as_of,
        "policy": {
            "normalized_identity_inference_allowed": False,
            "evidence_eligible": False,
            "metadata_record_materialization_allowed": True,
            "provenance_controls_scenario_eligibility": False,
            "scenario_eligibility_basis": "field-completeness",
            "target_code_executed": False,
            "working_tree_bytes_read": False,
        },
        "profile": {
            "profile_key": profile_key,
            "profile_digest": sha256_bytes(canonical_json(profile)),
        },
        "repository": {
            "commit": commit,
            "repository_url": repository_url,
            "tree": tree_id,
        },
        "scan": {
            "scanned_blob_count": len(scanned),
            "scanned_blobs": [
                {"path": path, **scanned[path]} for path in sorted(scanned)
            ],
        },
        "candidates": {
            "excluded_inputs": candidates.excluded_inputs,
            "lifecycle": sorted(
                candidates.lifecycle.values(),
                key=lambda record: (record["provider_id"], record["model_id"], record["kind"]),
            ),
            "locators": sorted(candidates.locators.values(), key=lambda record: record["locator"]),
            "metadata_shape_signals": sorted(candidates.metadata_shape_signals),
            "models": model_records,
            "providers": sorted(candidates.providers.values(), key=lambda record: record["provider_id"]),
        },
        "comparison": {
            "catalog_supplied": catalog_path is not None,
            "catalog_digest": sha256_bytes(canonical_json(compared_catalog))
            if compared_catalog is not None
            else None,
            "lexically_matched_model_candidate_count": len(model_records) - unmatched_count,
            "unmatched_model_candidate_count": unmatched_count,
        },
        "counts": {
            "excluded_input": len(candidates.excluded_inputs),
            "lifecycle_candidate": len(candidates.lifecycle),
            "locator_candidate": len(candidates.locators),
            "metadata_shape_signal": len(candidates.metadata_shape_signals),
            "model_candidate": len(model_records),
            "provider_candidate": len(candidates.providers),
        },
    }
    artifact["canonical_digest"] = sha256_bytes(canonical_json(artifact))
    return artifact


def parse_date(value: str) -> str:
    try:
        return datetime.strptime(value, "%Y-%m-%d").date().isoformat()
    except ValueError as exc:
        raise argparse.ArgumentTypeError("expected YYYY-MM-DD") from exc


def main() -> int:
    repository = Path(__file__).resolve().parents[4]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--catalog", type=Path)
    parser.add_argument("--as-of", type=parse_date, required=True)
    parser.add_argument("--profiles", type=Path, default=repository / PROFILE_RELATIVE_PATH)
    parser.add_argument(
        "--source-registry", type=Path, default=repository / SOURCE_REGISTRY_RELATIVE_PATH
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        artifact = build_artifact(
            repo=args.repo.resolve(),
            commit=args.commit,
            profile_key=args.profile,
            profiles_path=args.profiles.resolve(),
            source_registry_path=args.source_registry.resolve(),
            catalog_path=args.catalog.resolve() if args.catalog else None,
            as_of=args.as_of,
        )
    except ScanError as exc:
        print(json.dumps({"result": "red", "error": str(exc)}, ensure_ascii=False), file=sys.stderr)
        return 1
    rendered = json.dumps(artifact, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    sys.exit(main())
