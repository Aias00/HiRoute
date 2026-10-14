#!/usr/bin/env python3
"""Review public paths, contents, messages and DCO across an outgoing Git range."""
from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = SKILL_DIR / 'references/public-surface.v1.json'
TEXT_RULES = {
    'private-key': re.compile(r'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----'),
    'access-key': re.compile(r'\b(?:LTAI|AKID)[A-Za-z0-9]{12,}\b'),
    'github-token': re.compile(r'\bgh[pousr]_[A-Za-z0-9_]{20,}\b'),
    'model-api-key': re.compile(r'\bsk-(?:or-v1-|proj-)?[A-Za-z0-9_-]{24,}\b'),
    'private-address': re.compile(r'\b(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b'),
    'private-host': re.compile(r'https?://[^\s/]+(?:\.(?:internal|intranet|corp|local)|-inc\.com)(?=[:/\s]|$)', re.I),
    'personal-home': re.compile(r'/(?:Users|home)/(?!example(?:/|\b)|user(?:/|\b)|test(?:/|\b)|Shared(?:/|\b)|<)[A-Za-z0-9_.-]+/'),
}


class SurfaceError(RuntimeError):
    pass


def run_git(repository: Path, *args: str) -> bytes:
    result = subprocess.run(['git', *args], cwd=repository, capture_output=True)
    if result.returncode:
        raise SurfaceError(result.stderr.decode(errors='replace').strip() or 'Git operation failed')
    return result.stdout


def load_manifest(path: Path) -> dict:
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        raise SurfaceError(f'cannot read public surface manifest: {exc}') from exc
    if not isinstance(value, dict) or value.get('schema') != 'hiroute.public-surface/v1':
        raise SurfaceError('unsupported public surface manifest schema')
    return value


def load_deny(path: Path | None) -> list[str]:
    if path is None:
        return []
    try:
        values = json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        raise SurfaceError('cannot read personal deny file') from exc
    if not isinstance(values, list) or any(not isinstance(v, str) or not v for v in values):
        raise SurfaceError('personal deny file must be a JSON array of nonempty strings')
    return values


def matches_prefix(path: str, prefixes: list[str]) -> bool:
    return any(path.startswith(prefix) for prefix in prefixes)


def classify_path(path: str, manifest: dict) -> str:
    if path in manifest['exclude_files'] or matches_prefix(path, manifest['exclude_roots']):
        return 'private'
    if any(fnmatch.fnmatchcase(path, rule) for rule in manifest.get('exclude_globs', [])):
        return 'private'
    if path.startswith('docs/'):
        if path in manifest['include_docs']:
            return 'public'
        if any(fnmatch.fnmatchcase(path, rule) for rule in manifest['exclude_doc_globs']):
            return 'private'
        return 'unknown'
    if path in manifest['include_files'] or matches_prefix(path, manifest['include_roots']):
        return 'public'
    return 'unknown'


def tree_paths(repository: Path, ref: str) -> list[str]:
    raw = run_git(repository, 'ls-tree', '-r', '--name-only', '-z', ref)
    return sorted(p.decode() for p in raw.split(b'\0') if p)


def changed_paths(repository: Path, base: str, head: str) -> list[str]:
    raw = run_git(repository, 'diff', '--no-renames', '--name-only', '-z', base, head)
    return sorted(p.decode() for p in raw.split(b'\0') if p)


def classify(paths: list[str], manifest: dict) -> dict:
    result = {name: [] for name in ('public', 'private', 'unknown')}
    for path in paths:
        result[classify_path(path, manifest)].append(path)
    return result


def classify_range(repository, base, head, manifest, manifest_path):
    groups = classify(changed_paths(repository, base, head), manifest)
    retired = set(groups['unknown']) - set(tree_paths(repository, head))
    try:
        relative = manifest_path.resolve().relative_to(repository)
        previous = json.loads(run_git(repository, 'show', f'{base}:{relative.as_posix()}'))
    except (ValueError, SurfaceError):
        return groups
    if not isinstance(previous, dict) or previous.get('schema') != manifest['schema']:
        return groups
    for path in sorted(retired):
        category = classify_path(path, previous)
        if category != 'unknown':
            groups['unknown'].remove(path)
            groups[category].append(path)
    return {name: sorted(paths) for name, paths in groups.items()}


def validate_required(paths: set[str], manifest: dict) -> None:
    missing = sorted((set(manifest['include_files']) | set(manifest['include_docs'])) - paths)
    if missing:
        raise SurfaceError('required public files are missing: ' + ', '.join(missing))


def scan(raw: bytes, path: str, deny=()) -> list[dict]:
    # Binary payloads still require human/attachment review.
    if b'\0' in raw:
        return []
    value = raw.decode('utf-8', errors='replace')
    findings = [{'path': path, 'rule': name} for name, pattern in TEXT_RULES.items() if pattern.search(value)]
    if any(needle in value for needle in deny):
        findings.append({'path': path, 'rule': 'personal-deny-pattern'})
    return findings


def text_findings(root: Path, deny=()) -> list[dict]:
    findings = []
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raw = os.readlink(path).encode()
        elif path.is_file():
            raw = path.read_bytes()
        else:
            continue
        findings.extend(scan(raw, path.relative_to(root).as_posix(), deny))
    return findings


def entries(repository, ref, paths=None):
    args = ['ls-tree', '-r', '-z', ref]
    if paths is not None:
        if not paths:
            return []
        args += ['--', *paths]
    result = []
    for row in run_git(repository, *args).split(b'\0'):
        if row:
            metadata, path = row.split(b'\t', 1)
            mode, kind, oid = metadata.decode().split()
            if kind == 'blob':
                result.append((path.decode(), oid))
    return result


def audit_range(repository, base, head, manifest, manifest_path, deny=()):
    base = run_git(repository, 'rev-parse', '--verify', base + '^{commit}').decode().strip()
    head = run_git(repository, 'rev-parse', '--verify', head + '^{commit}').decode().strip()
    if subprocess.run(['git', 'merge-base', '--is-ancestor', base, head], cwd=repository).returncode:
        raise SurfaceError('base must be an ancestor of the outgoing candidate')
    validate_required(set(tree_paths(repository, head)), manifest)
    commits = run_git(repository, 'rev-list', '--reverse', base + '..' + head).decode().splitlines()
    findings, seen = [], set()
    for sha in commits:
        raw_paths = run_git(repository, 'diff-tree', '--root', '-m', '-r', '--no-commit-id', '--no-renames', '--name-only', '-z', sha)
        paths = sorted(set(p.decode() for p in raw_paths.split(b'\0') if p))
        present = dict(entries(repository, sha, paths))
        # The selected base can classify deletions of retired public documents.
        removals = classify_range(repository, base, sha, manifest, manifest_path)
        for path in paths:
            category = classify_path(path, manifest)
            if path not in present and path in removals['public']:
                category = 'public'
            if category != 'public':
                findings.append({'commit': sha, 'path': path, 'rule': category + '-path'})
        for path, oid in present.items():
            if (path, oid) not in seen:
                seen.add((path, oid))
                findings.extend({**f, 'commit': sha} for f in scan(run_git(repository, 'cat-file', 'blob', oid), path, deny))
        message = run_git(repository, 'show', '-s', '--format=%B', sha)
        findings.extend({**f, 'commit': sha} for f in scan(message, '<commit-message>', deny))
        author = run_git(repository, 'show', '-s', '--format=%an <%ae>', sha).decode().strip()
        signoffs = re.findall(r'^Signed-off-by:\s*(.+)$', message.decode(errors='replace'), re.M | re.I)
        if author.casefold() not in (line.strip().casefold() for line in signoffs):
            findings.append({'commit': sha, 'path': '<commit-message>', 'rule': 'author-signoff'})
    return {'base': base, 'head': head, 'commits': len(commits), 'blobs_checked': len(seen), 'findings': findings}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', type=Path, default=Path.cwd())
    parser.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument('--deny-file', type=Path)
    sub = parser.add_subparsers(dest='command', required=True)
    for name in ('classify-range', 'audit-range'):
        command = sub.add_parser(name)
        command.add_argument('--base', required=True)
        command.add_argument('--head', default='HEAD')
    command = sub.add_parser('audit-text')
    command.add_argument('--file', type=Path, action='append', required=True)
    command = sub.add_parser('audit-tree')
    command.add_argument('--root', type=Path, required=True)
    try:
        args = parser.parse_args()
        manifest, deny = load_manifest(args.manifest), load_deny(args.deny_file)
        repository = args.repository.resolve()
        if args.command == 'audit-range':
            result = audit_range(repository, args.base, args.head, manifest, args.manifest, deny)
            blocked = bool(result['findings'])
        elif args.command == 'classify-range':
            result = classify_range(repository, args.base, args.head, manifest, args.manifest)
            blocked = bool(result['private'] or result['unknown'])
        elif args.command == 'audit-text':
            result = {'findings': [f for path in args.file for f in scan(path.read_bytes(), path.name, deny)]}
            blocked = bool(result['findings'])
        else:
            result = {'findings': text_findings(args.root.resolve(), deny)}
            blocked = bool(result['findings'])
        print(json.dumps(result, indent=2))
        return 2 if blocked else 0
    except (SurfaceError, OSError) as exc:
        print('public surface error: ' + str(exc), file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
