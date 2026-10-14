#!/usr/bin/env python3
"""Materialize exact-commit client assertions as completeness-scored metadata.

The generated provider-scoped records retain provenance, but provenance does not
control their scenario eligibility. Eligibility is derived from field completeness.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import quote, urlparse


FULL_COMMIT_RE = re.compile(r"[0-9a-f]{40}")
SUPPORTED_PROTOCOLS = {
    "anthropic-messages": "messages",
    "anthropic_messages": "messages",
    "chat_completions": "chat_completions",
    "codex_responses": "responses",
    "openai-completions": "chat_completions",
    "openai-responses": "responses",
}
PROVIDER_COMPLETENESS_DOMAINS = (
    "authentication",
    "discovery",
    "endpoint",
    "identity",
)
MODEL_COMPLETENESS_DOMAINS = (
    "capabilities",
    "cost",
    "identity",
    "lifecycle",
    "limits",
    "modalities",
    "reasoning_rendering",
)
SCENARIO_REQUIREMENTS = {
    "adapter-planning": {
        "record_kind": "provider",
        "minimum": {"endpoint": "partial"},
        "requires_adapter_signal": True,
    },
    "context-limit-prefill": {
        "record_kind": "model",
        "minimum": {"limits": "partial"},
        "requires_known_field": "context_tokens",
    },
    "cost-hint-display": {"record_kind": "model", "minimum": {"cost": "partial"}},
    "credential-setup-hint": {
        "record_kind": "provider",
        "minimum": {"authentication": "partial"},
    },
    "custom-api-endpoint-prefill": {
        "record_kind": "provider",
        "minimum": {"endpoint": "complete"},
    },
    "custom-api-model-prefill": {
        "record_kind": "model",
        "minimum": {"identity": "partial"},
    },
    "lifecycle-warning": {"record_kind": "model", "minimum": {"lifecycle": "partial"}},
    "model-directory": {"record_kind": "model", "minimum": {"identity": "partial"}},
    "model-discovery-setup": {
        "record_kind": "provider",
        "minimum": {"discovery": "partial"},
    },
    "output-limit-prefill": {
        "record_kind": "model",
        "minimum": {"limits": "partial"},
        "requires_known_field": "max_output_tokens",
    },
    "provider-directory": {
        "record_kind": "provider",
        "minimum": {"identity": "partial"},
    },
    "reasoning-capability-prefill": {
        "record_kind": "model",
        "minimum": {"capabilities": "partial"},
        "requires_known_field": "reasoning",
    },
    "reasoning-rendering-hint": {
        "record_kind": "model",
        "minimum": {"reasoning_rendering": "complete"},
    },
    "request-limit-prefill": {"record_kind": "model", "minimum": {"limits": "complete"}},
    "vision-capability-prefill": {
        "record_kind": "model",
        "minimum": {"capabilities": "partial"},
        "requires_known_field": "vision",
    },
}


class MaterializeError(RuntimeError):
    pass


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise MaterializeError(f"cannot read valid JSON from {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise MaterializeError(f"expected a JSON object in {path}")
    return value


def canonical_json(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def unique_json(values: Iterable[Any]) -> list[Any]:
    by_encoding = {canonical_json(value): value for value in values}
    return [by_encoding[key] for key in sorted(by_encoding)]


def strings(values: Iterable[Any]) -> list[str]:
    return sorted({value for value in values if isinstance(value, str) and value})


def key_segment(value: str) -> str:
    return quote(value, safe="-._~")


def provider_key(profile_key: str, provider_id: str) -> str:
    return f"{key_segment(profile_key)}/{key_segment(provider_id)}"


def model_key(profile_key: str, provider_id: str, upstream_model_id: str) -> str:
    return (
        f"{key_segment(profile_key)}/{key_segment(provider_id)}/"
        f"{key_segment(upstream_model_id)}"
    )


def locations(records: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []
    for record in records:
        raw_locations = record.get("locations", [])
        if not isinstance(raw_locations, list):
            continue
        for location in raw_locations:
            if not isinstance(location, dict):
                continue
            kept = {
                key: location[key]
                for key in ("path", "line", "pointer", "git_object_id")
                if key in location
            }
            if kept:
                found.append(kept)
    return unique_json(found)


def assertion_dicts(record: dict[str, Any]) -> list[dict[str, Any]]:
    assertions = record.get("assertions", [])
    if not isinstance(assertions, list):
        return []
    return unique_json(value for value in assertions if isinstance(value, dict))


def collect_list(assertions: list[dict[str, Any]], field: str) -> list[str]:
    values: list[Any] = []
    for assertion in assertions:
        value = assertion.get(field)
        if isinstance(value, list):
            values.extend(value)
    return strings(values)


def collect_scalars(assertions: list[dict[str, Any]], *fields: str) -> list[str]:
    values: list[Any] = []
    for assertion in assertions:
        for field in fields:
            value = assertion.get(field)
            if isinstance(value, str) and value:
                values.append(value)
    return strings(values)


def positive_integer_fact(assertions: list[dict[str, Any]], field: str) -> dict[str, Any]:
    values = sorted(
        {
            value
            for assertion in assertions
            for value in [assertion.get(field)]
            if isinstance(value, int) and not isinstance(value, bool) and value > 0
        }
    )
    if len(values) == 1:
        return {"state": "known", "value": values[0]}
    if len(values) > 1:
        return {"candidates": values, "state": "conflict", "value": None}
    return {"state": "unknown", "value": None}


def boolean_capability(assertions: list[dict[str, Any]], field: str) -> str:
    values = {
        assertion[field]
        for assertion in assertions
        if isinstance(assertion.get(field), bool)
    }
    if values == {True}:
        return "supported"
    if values == {False}:
        return "unsupported"
    if len(values) > 1:
        return "conditional"
    return "unknown"


def vision_capability(assertions: list[dict[str, Any]]) -> str:
    inputs = [
        assertion.get("input")
        for assertion in assertions
        if isinstance(assertion.get("input"), list)
    ]
    if not inputs:
        return "unknown"
    states = {"image" in value for value in inputs}
    if states == {True}:
        return "supported"
    if states == {False}:
        return "unsupported"
    return "conditional"


def nested_boolean_capability(
    assertions: list[dict[str, Any]], container: str, field: str
) -> str:
    values: set[bool] = set()
    for assertion in assertions:
        nested = assertion.get(container)
        if isinstance(nested, dict) and isinstance(nested.get(field), bool):
            values.add(nested[field])
    if values == {True}:
        return "supported"
    if values == {False}:
        return "unsupported"
    if len(values) > 1:
        return "conditional"
    return "unknown"


def collect_dicts(assertions: list[dict[str, Any]], field: str) -> list[dict[str, Any]]:
    return unique_json(
        assertion[field]
        for assertion in assertions
        if isinstance(assertion.get(field), dict) and assertion[field]
    )


def collect_nested_dicts(
    assertions: list[dict[str, Any]], container: str, field: str
) -> list[dict[str, Any]]:
    values: list[dict[str, Any]] = []
    for assertion in assertions:
        nested = assertion.get(container)
        if isinstance(nested, dict) and isinstance(nested.get(field), dict) and nested[field]:
            values.append(nested[field])
    return unique_json(values)


def collect_nested_lists(
    assertions: list[dict[str, Any]], container: str, field: str
) -> list[str]:
    values: list[Any] = []
    for assertion in assertions:
        nested = assertion.get(container)
        if isinstance(nested, dict) and isinstance(nested.get(field), list):
            values.extend(nested[field])
    return strings(values)


def completeness_state(*, present: int, required: int, conflict: bool = False) -> str:
    if present <= 0:
        return "missing"
    if present >= required and not conflict:
        return "complete"
    return "partial"


def has_usable_cost_hint(cost_hints: list[dict[str, Any]]) -> bool:
    return any(
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
        and value >= 0
        for hint in cost_hints
        for value in hint.values()
    )


def provider_usable_for(
    completeness: dict[str, str],
    *,
    unsupported_api_styles: list[str],
    unsupported_transport_locators: list[str],
) -> list[str]:
    scenarios = ["provider-directory"]
    if completeness["endpoint"] == "complete":
        scenarios.append("custom-api-endpoint-prefill")
    elif unsupported_api_styles or unsupported_transport_locators:
        scenarios.append("adapter-planning")
    if completeness["authentication"] != "missing":
        scenarios.append("credential-setup-hint")
    if completeness["discovery"] != "missing":
        scenarios.append("model-discovery-setup")
    return sorted(scenarios)


def model_usable_for(
    completeness: dict[str, str],
    *,
    context_tokens: dict[str, Any],
    max_output_tokens: dict[str, Any],
    capabilities: dict[str, str],
) -> list[str]:
    scenarios = ["custom-api-model-prefill", "model-directory"]
    if context_tokens.get("state") == "known":
        scenarios.append("context-limit-prefill")
    if max_output_tokens.get("state") == "known":
        scenarios.append("output-limit-prefill")
    if completeness["limits"] == "complete":
        scenarios.append("request-limit-prefill")
    if capabilities.get("reasoning") != "unknown":
        scenarios.append("reasoning-capability-prefill")
    if capabilities.get("vision") != "unknown":
        scenarios.append("vision-capability-prefill")
    if completeness["reasoning_rendering"] == "complete":
        scenarios.append("reasoning-rendering-hint")
    if completeness["lifecycle"] != "missing":
        scenarios.append("lifecycle-warning")
    if completeness["cost"] != "missing":
        scenarios.append("cost-hint-display")
    return sorted(scenarios)


def build_provider_record(
    profile_key: str,
    provider_id: str,
    provider_records: list[dict[str, Any]],
    model_records: list[dict[str, Any]],
) -> dict[str, Any]:
    assertions = unique_json(
        assertion
        for record in provider_records
        for assertion in assertion_dicts(record)
    )
    display_names = collect_scalars(assertions, "display_name")
    names = collect_scalars(assertions, "name")
    descriptions = collect_scalars(assertions, "description")
    aliases = collect_list(assertions, "aliases") + collect_scalars(assertions, "alias")
    raw_base_urls = collect_scalars(assertions, "baseUrl", "base_url")
    base_urls = [
        value
        for value in raw_base_urls
        if urlparse(value).scheme in {"http", "https"} and urlparse(value).netloc
    ]
    unsupported_transport_locators = [value for value in raw_base_urls if value not in base_urls]
    api_styles = collect_scalars(assertions, "api", "api_mode")
    protocols = strings(SUPPORTED_PROTOCOLS[value] for value in api_styles if value in SUPPORTED_PROTOCOLS)
    unsupported_api_styles = strings(value for value in api_styles if value not in SUPPORTED_PROTOCOLS)
    auth_types = collect_scalars(assertions, "auth_type")
    default_models = collect_scalars(assertions, "defaultModel", "default_aux_model")
    default_models += collect_list(assertions, "fallback_models")
    discovery_modes = collect_scalars(assertions, "discovery")
    env_vars = collect_list(assertions, "env_vars")
    models_urls = collect_scalars(assertions, "models_url")
    signup_urls = collect_scalars(assertions, "signup_url")

    explicit_identity_count = len(set(display_names + names))
    completeness = {
        "authentication": completeness_state(
            present=len(auth_types) if auth_types else int(bool(env_vars)),
            required=1,
            conflict=len(auth_types) > 1,
        ),
        "discovery": completeness_state(
            present=int(bool(discovery_modes)) + int(bool(models_urls)),
            required=2,
        ) if discovery_modes or models_urls else (
            "partial" if default_models else "missing"
        ),
        "endpoint": completeness_state(
            present=(
                int(bool(base_urls or unsupported_transport_locators))
                + int(bool(protocols or unsupported_api_styles))
            ),
            required=2,
            conflict=(
                len(base_urls) != 1
                or len(protocols) != 1
                or bool(unsupported_api_styles)
                or bool(unsupported_transport_locators)
            ),
        ),
        "identity": "complete" if explicit_identity_count == 1 else "partial",
    }

    return {
        "provider_record_key": provider_key(profile_key, provider_id),
        "provenance_refs": [
            {"kind": "client-discovery-run", "source_key": profile_key}
        ],
        "provider_id": provider_id,
        "display_name": display_names[0] if len(display_names) == 1 else (
            names[0] if len(names) == 1 else provider_id
        ),
        "description_candidates": descriptions,
        "aliases": strings(aliases),
        "base_url_candidates": base_urls,
        "protocol_candidates": protocols,
        "unsupported_api_styles": unsupported_api_styles,
        "unsupported_transport_locators": unsupported_transport_locators,
        "authentication_candidates": auth_types,
        "environment_variable_names": env_vars,
        "default_model_ids": strings(default_models),
        "discovery_modes": discovery_modes,
        "models_url_candidates": models_urls,
        "signup_url_candidates": signup_urls,
        "metadata_completeness": completeness,
        "usable_for": provider_usable_for(
            completeness,
            unsupported_api_styles=unsupported_api_styles,
            unsupported_transport_locators=unsupported_transport_locators,
        ),
        "source_assertions": assertions,
        "source_locations": locations(provider_records or model_records),
    }


def build_model_record(
    profile_key: str,
    candidate: dict[str, Any],
) -> dict[str, Any]:
    provider_id = candidate.get("provider_id")
    upstream_model_id = candidate.get("model_id")
    if not isinstance(provider_id, str) or not provider_id:
        raise MaterializeError(f"model candidate lacks provider_id in profile {profile_key}")
    if not isinstance(upstream_model_id, str) or not upstream_model_id:
        raise MaterializeError(f"model candidate lacks model_id in profile {profile_key}")
    assertions = assertion_dicts(candidate)
    display_names = collect_scalars(assertions, "name")
    input_modalities = collect_list(assertions, "input")
    statuses = collect_scalars(assertions, "status")
    lifecycle = statuses[0] if len(statuses) == 1 and statuses[0] in {
        "active", "deprecated", "preview", "retired", "unknown"
    } else "unknown"
    roles = candidate.get("roles", [])
    matches = candidate.get("catalog_lexical_matches", [])
    replacement_ids = collect_scalars(assertions, "replacedBy")
    context_tokens = positive_integer_fact(assertions, "contextWindow")
    max_output_tokens = positive_integer_fact(assertions, "maxTokens")
    capability_hints = {
        "reasoning": boolean_capability(assertions, "reasoning"),
        "streaming": "unknown",
        "tool": nested_boolean_capability(assertions, "compat", "supportsTools"),
        "vision": vision_capability(assertions),
    }
    thinking_level_maps = collect_dicts(assertions, "thinkingLevelMap")
    reasoning_effort_maps = collect_nested_dicts(assertions, "compat", "reasoningEffortMap")
    supported_reasoning_efforts = collect_nested_lists(
        assertions, "compat", "supportedReasoningEfforts"
    )
    cost_hints = collect_dicts(assertions, "cost")
    known_capabilities = sum(
        value != "unknown" for value in capability_hints.values()
    )
    present_limits = sum(
        fact.get("state") != "unknown" for fact in (context_tokens, max_output_tokens)
    )
    limit_conflict = any(
        fact.get("state") == "conflict" for fact in (context_tokens, max_output_tokens)
    )
    completeness = {
        "capabilities": completeness_state(
            present=known_capabilities,
            required=len(capability_hints),
            conflict="conditional" in capability_hints.values(),
        ),
        "cost": "partial" if has_usable_cost_hint(cost_hints) else "missing",
        "identity": "complete" if len(display_names) == 1 else "partial",
        "lifecycle": "complete" if lifecycle != "unknown" else (
            "partial" if replacement_ids else "missing"
        ),
        "limits": completeness_state(
            present=present_limits,
            required=2,
            conflict=limit_conflict,
        ),
        "modalities": "complete" if input_modalities else "missing",
        "reasoning_rendering": "complete" if (
            thinking_level_maps or reasoning_effort_maps or supported_reasoning_efforts
        ) else (
            "partial" if capability_hints["reasoning"] != "unknown" else "missing"
        ),
    }

    return {
        "model_record_key": model_key(profile_key, provider_id, upstream_model_id),
        "provider_record_key": provider_key(profile_key, provider_id),
        "provenance_refs": [
            {"kind": "client-discovery-run", "source_key": profile_key}
        ],
        "provider_id": provider_id,
        "upstream_model_id": upstream_model_id,
        "display_name": display_names[0] if len(display_names) == 1 else upstream_model_id,
        "context_tokens": context_tokens,
        "max_output_tokens": max_output_tokens,
        "input_modalities": input_modalities,
        "capability_hints": capability_hints,
        "reasoning_rendering_hints": {
            "reasoning_effort_maps": reasoning_effort_maps,
            "supported_reasoning_efforts": supported_reasoning_efforts,
            "thinking_level_maps": thinking_level_maps,
        },
        "cost_hints": cost_hints,
        "lifecycle": lifecycle,
        "status_candidates": statuses,
        "replacement_upstream_ids": replacement_ids,
        "roles": strings(roles if isinstance(roles, list) else []),
        "normalized_model_matches": strings(matches if isinstance(matches, list) else []),
        "metadata_completeness": completeness,
        "usable_for": model_usable_for(
            completeness,
            context_tokens=context_tokens,
            max_output_tokens=max_output_tokens,
            capabilities=capability_hints,
        ),
        "source_assertions": assertions,
        "source_locations": locations([candidate]),
    }


def materialize(catalog: dict[str, Any], scans: list[dict[str, Any]]) -> dict[str, Any]:
    provider_records: list[dict[str, Any]] = []
    model_records: list[dict[str, Any]] = []
    seen_profiles: set[str] = set()

    for scan in scans:
        if scan.get("artifact_kind") != "hiroute.model-metadata-client-discovery":
            raise MaterializeError("every scan must be a client discovery artifact")
        profile = scan.get("profile", {})
        repository = scan.get("repository", {})
        profile_key = profile.get("profile_key") if isinstance(profile, dict) else None
        commit = repository.get("commit") if isinstance(repository, dict) else None
        if not isinstance(profile_key, str) or not profile_key:
            raise MaterializeError("scan lacks profile_key")
        if profile_key in seen_profiles:
            raise MaterializeError(f"duplicate scan profile {profile_key}")
        seen_profiles.add(profile_key)
        if not isinstance(commit, str) or not FULL_COMMIT_RE.fullmatch(commit):
            raise MaterializeError(f"scan {profile_key} is not pinned to a full commit")
        policy = scan.get("policy", {})
        if not isinstance(policy, dict) or policy.get("target_code_executed") is not False:
            raise MaterializeError(f"scan {profile_key} did not preserve the execution boundary")

        candidates = scan.get("candidates", {})
        if not isinstance(candidates, dict):
            raise MaterializeError(f"scan {profile_key} has invalid candidates")
        raw_providers = candidates.get("providers", [])
        raw_models = candidates.get("models", [])
        if not isinstance(raw_providers, list) or not isinstance(raw_models, list):
            raise MaterializeError(f"scan {profile_key} provider/model candidates must be arrays")
        providers = [value for value in raw_providers if isinstance(value, dict)]
        models = [value for value in raw_models if isinstance(value, dict)]
        provider_ids = strings(
            [record.get("provider_id") for record in providers]
            + [record.get("provider_id") for record in models]
        )
        for candidate_provider_id in provider_ids:
            matching_providers = [
                record for record in providers if record.get("provider_id") == candidate_provider_id
            ]
            matching_models = [
                record for record in models if record.get("provider_id") == candidate_provider_id
            ]
            provider_records.append(
                build_provider_record(
                    profile_key,
                    candidate_provider_id,
                    matching_providers,
                    matching_models,
                )
            )
        model_records.extend(build_model_record(profile_key, record) for record in models)

    result = dict(catalog)
    result["metadata_usage_policy"] = {
        "completeness_domains": {
            "model": list(MODEL_COMPLETENESS_DOMAINS),
            "provider": list(PROVIDER_COMPLETENESS_DOMAINS),
        },
        "completeness_states": ["complete", "missing", "partial"],
        "eligibility_basis": "field-completeness",
        "provenance_controls_scenario_eligibility": False,
        "runtime_execution_requires_qualification": True,
        "scenario_requirements": SCENARIO_REQUIREMENTS,
    }
    result["provider_metadata_records"] = sorted(
        provider_records, key=lambda record: record["provider_record_key"]
    )
    result["model_metadata_records"] = sorted(
        model_records, key=lambda record: record["model_record_key"]
    )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--scan", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = materialize(load_json(args.catalog), [load_json(path) for path in args.scan])
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
    except MaterializeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
