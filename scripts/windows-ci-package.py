#!/usr/bin/env python3
"""Prepare verified native CI packages using the production packaging boundary."""
import argparse
import importlib.util
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / f"{name}.py")
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def command(*args):
    return subprocess.check_output(list(map(str, args)), cwd=ROOT, text=True).strip()


def prepare(args):
    if platform.system() != "Windows":
        raise ValueError("Native Windows required")
    revision = command("git", "rev-parse", "HEAD")
    if not re.fullmatch(r"[a-f0-9]{40}", args.expected_sha) or revision != args.expected_sha:
        raise ValueError("Package source does not match exact candidate")
    command("git", "diff", "--exit-code", "HEAD", "--")
    packager = module("package-standalone")
    target = packager.host_target()
    if target != "x86_64-pc-windows-msvc":
        raise ValueError("This package acceptance fixture requires native Windows x64")
    pin, _ = module("build-cpa").pinned_source()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    if args.cpa_evidence is None:
        previous = output / "cpa"
        previous.mkdir()
        provenance = module("build-cpa").build(args.source_repo, target, previous / "cliproxyapi.exe",
                                             previous / "go-bootstrap-tests.log")
        provenance.update(build_run_id=args.build_run_id, validation_revision=revision)
        (previous / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        shutil.copyfile(previous / "cliproxyapi.LICENSE", previous / "CPA.LICENSE")
    else:
        previous = args.cpa_evidence.resolve(strict=True)
    provenance = json.loads((previous / "provenance.json").read_text(encoding="utf-8-sig"))
    if any(provenance.get(key) != value for key, value in pin.items()):
        raise ValueError("Reused CPA evidence does not match the unchanged source and patch")
    cpa = previous / "cliproxyapi.exe"
    if packager.digest(cpa) != provenance["sha256"] or cpa.stat().st_size != provenance["size"]:
        raise ValueError("CPA bytes do not match the original native build evidence")
    passed = []
    for line in (previous / "go-bootstrap-tests.log").read_text(encoding="utf-8").splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("Action") == "pass" and event.get("Test"):
            passed.append(event["Test"])
    if sorted(passed) != ["TestLocalMachinePasswordPreservesRemotePolicy", "TestLocalManagementPasswordPipeBounds"]:
        raise ValueError("Original CPA native build lacks the required bootstrap security cases")
    notices = output / "notices"
    command(sys.executable, ROOT / "scripts/collect-third-party-licenses.py", "--cargo-target", target,
            "--cpa-source-repo", args.source_repo.resolve(strict=True), "--output", notices)
    version = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text())["version"]
    base = [sys.executable, ROOT / "scripts/package-standalone.py", "build",
            "--version", version, "--revision", revision, "--target", target,
            "--hiroute", ROOT / "target/debug/hiroute.exe",
            "--hirouted", ROOT / "target/debug/hirouted.exe", "--cpa-binary", cpa,
            "--cpa-version", pin["version"], "--cpa-license", previous / "CPA.LICENSE",
            "--notices", notices]
    first = json.loads(command(*base, "--output", output / "package"))
    second = json.loads(command(*base, "--output", output / "repeated-package"))
    archive, manifest = Path(first["archive"]), Path(first["manifest"])
    if (packager.digest(archive) != packager.digest(Path(second["archive"]))
            or manifest.read_bytes() != Path(second["manifest"]).read_bytes()):
        raise ValueError("Production package generation is not deterministic")
    verified = packager.verify(manifest, archive)
    # The administrator receives a separate complete verified extraction. Its source
    # does not come from an owner-writable prepared installation or request parameter.
    trusted = output / "administrator-package"
    trusted.mkdir()
    for name, (payload, _) in packager.safe_members(archive, verified["files"]).items():
        path = trusted / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
    command("icacls", trusted, "/inheritance:r", "/grant:r",
            "*S-1-5-32-544:(OI)(CI)F", "*S-1-5-18:(OI)(CI)F", "*S-1-5-11:(OI)(CI)RX", "/T")
    command("icacls", trusted, "/setowner", "*S-1-5-32-544", "/T")
    result = dict(version=1, source_revision=revision, target=target,
                  manifest=str(manifest), archive=str(archive),
                  administrator_cli=str(trusted / "bin/hiroute.exe"),
                  worker_daemon=str(trusted / "bin/hirouted.exe"), python=sys.executable,
                  archive_sha256=packager.digest(archive), manifest_sha256=packager.digest(manifest),
                  deterministic_publication="green", production_runtime="not_executed",
                  cpa_original_run=provenance.get("build_run_id", args.cpa_original_run),
                  cpa_original_validation_revision=provenance.get("validation_revision", args.cpa_original_revision),
                  cpa_original_provenance=provenance)
    (output / "package-evidence.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-sha", required=True)
    parser.add_argument("--source-repo", required=True, type=Path)
    parser.add_argument("--cpa-evidence", type=Path)
    parser.add_argument("--build-run-id")
    parser.add_argument("--cpa-original-run")
    parser.add_argument("--cpa-original-revision")
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        print(json.dumps(prepare(arguments)))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
