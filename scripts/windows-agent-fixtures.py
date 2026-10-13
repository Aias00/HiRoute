#!/usr/bin/env python3
"""Verify locked native acceptance clients; never log in or send provider requests."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import struct
import subprocess
import sys
import urllib.request
import zipfile

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "tools/windows-agent-fixtures"
QODER_MANIFEST = "https://qoder-ide.oss-accelerate.aliyuncs.com/qodercli/channels/1.1.65/manifest.json"
QODER_MANIFEST_SHA = "c88c7f159a95aed57071636b867bc588276f762aaed4640d880dd3cfd062b61e"
QODER_ZIP = "https://qoder-ide.oss-accelerate.aliyuncs.com/qodercli/releases/1.1.65/qodercli-windows-x64.zip"
QODER_SHA = "7094b1b57e72f4fe351c516bbff7089ad5d6b62bef49c27df2738804ba611038"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def native(path):
    path = path.resolve(strict=True)
    with path.open("rb") as handle:
        header = handle.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("Native PE required: " + str(path))
        handle.seek(struct.unpack_from("<I", header, 60)[0])
        image = handle.read(6)
    if image != b"PE\0\0\x64\x86":
        raise ValueError("Pinned x64 native PE required: " + str(path))
    return path


def download(url, path, expected):
    with urllib.request.urlopen(url, timeout=120) as response, path.open("xb") as output:
        shutil.copyfileobj(response, output)
    if digest(path) != expected:
        raise ValueError("Fixed upstream artifact digest differs: " + url)


def prepare(output, daemon):
    if sys.platform != "win32":
        raise ValueError("Native Windows runner required")
    output.mkdir(parents=True, exist_ok=False)
    lock = json.loads((FIXTURES / "package-lock.json").read_text())
    modules = FIXTURES / "node_modules"
    declarations = json.loads((FIXTURES / "package.json").read_text())["dependencies"]
    for package, version in declarations.items():
        actual = json.loads((modules / package / "package.json").read_text())
        if actual["name"] != package or actual["version"] != version:
            raise ValueError("Installed npm producer differs from committed fixture: " + package)
        record = lock["packages"]["node_modules/" + package]
        if record["version"] != version or not record.get("integrity"):
            raise ValueError("Fixture lacks fixed npm integrity: " + package)
    node = native(Path(shutil.which("node") or "missing-node"))
    version = subprocess.check_output([node, "--version"], text=True).strip()
    if version != "v22.19.0":
        raise ValueError("Acceptance Node must be fixed v22.19.0, observed " + version)
    qoder_manifest, archive = output / "qoder-manifest.json", output / "qodercli.zip"
    download(QODER_MANIFEST, qoder_manifest, QODER_MANIFEST_SHA)
    download(QODER_ZIP, archive, QODER_SHA)
    with zipfile.ZipFile(archive) as source:
        for item in source.infolist():
            path = PurePosixPath(item.filename)
            if path.is_absolute() or ".." in path.parts or "\\" in item.filename or ":" in item.filename:
                raise ValueError("Unsafe upstream archive entry")
            if (item.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("Upstream archive symlink refused")
        source.extractall(output / "qoder")
    qoder = list((output / "qoder").rglob("qodercli.exe"))
    codex = list((modules / "@openai/codex-win32-x64").rglob("codex.exe"))
    claude = list((modules / "@anthropic-ai/claude-code-win32-x64").rglob("claude.exe"))
    if any(len(items) != 1 for items in (qoder, codex, claude)):
        raise ValueError("Pinned native producer must have one exact executable")
    env = {
        "HIROUTE_WORKER_NODE": str(node),
        "HIROUTE_WORKER_DAEMON_BINARY": str(native(daemon)),
        "HIROUTE_WORKER_CODEX_BINARY": str(native(codex[0])),
        "HIROUTE_WORKER_CLAUDE_BINARY": str(native(claude[0])),
        "HIROUTE_WORKER_QODER_BINARY": str(native(qoder[0])),
        "HIROUTE_WORKER_CODEX_ACP_ADAPTER": str((modules / "@agentclientprotocol/codex-acp/dist/index.js").resolve(strict=True)),
        "HIROUTE_WORKER_CLAUDE_ACP_ADAPTER": str((modules / "@agentclientprotocol/claude-agent-acp/dist/index.js").resolve(strict=True)),
        "HIROUTE_WORKER_PI_BINARY": str((modules / "@earendil-works/pi-coding-agent/dist/bundle/cli.js").resolve(strict=True)),
        "HIROUTE_WORKER_DSH_BINARY": str((modules / "@deepseek-ai/dsh/lib/bin.js").resolve(strict=True)),
    }
    if digest(Path(env["HIROUTE_WORKER_CODEX_ACP_ADAPTER"])) != "4b76310393d756a0f111687cd9df899720f36f7c59b1eb1d86429484034ff91b":
        raise ValueError("Reviewed Codex adapter entry differs")
    git = Path(shutil.which("git") or "missing-git").resolve(strict=True)
    bash = git.parent.parent / "bin/bash.exe"
    native(bash)
    pwsh = native(Path(shutil.which("pwsh") or "missing-pwsh"))
    record = dict(node_version=version, package_lock_sha256=digest(FIXTURES / "package-lock.json"),
                  declarations=declarations, environment=env,
                  binaries={key: {"path": value, "sha256": digest(Path(value))} for key, value in env.items()},
                  bash={"path": str(bash), "sha256": digest(bash)},
                  pwsh={"path": str(pwsh), "sha256": digest(pwsh)},
                  qoder_manifest_sha256=QODER_MANIFEST_SHA, qoder_archive_sha256=QODER_SHA,
                  runtime="not_executed", evidence_limit="Fixed artifact prerequisites; provider login is not supplied.")
    (output / "clients.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--daemon", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.output, args.daemon), indent=2))


if __name__ == "__main__":
    main()
