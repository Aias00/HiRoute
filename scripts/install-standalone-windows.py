#!/usr/bin/env python3
"""Verified current-account Windows install/repair/uninstall (Python 3.9+).

The native CLI owns profile/SID resolution, private record publication and typed
manager calls. Python never evaluates a PowerShell expression or serializes a
password into argv, XML, environment or the recovery journal.
"""
import argparse
from contextlib import contextmanager
import getpass
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hiroute_standalone_package", Path(__file__).with_name("package-standalone.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
JOURNAL_SCHEMA = "hiroute.windows-install-journal/v1"
MARKER_SCHEMA = "hiroute.standalone-install/v1"


def regular(path):
    """Reject symlinks, junctions and other reparse points, including ancestors."""
    path = Path(path)
    if ".." in path.parts:
        raise ValueError("installation path contains parent traversal")
    for component in (path, *path.parents):
        try:
            value = component.lstat()
        except FileNotFoundError:
            continue
        if component.is_symlink() or getattr(value, "st_file_attributes", 0) & 0x400:
            raise ValueError("installation path contains a reparse point")
    if path.exists() and not path.is_file():
        raise ValueError("owned installation entry is not a regular file")
    return path


def native(cli, arguments, payload=None, secret=None):
    if payload is not None and secret is not None:
        raise ValueError("mixed native stdin sources")
    body = json.dumps(payload, separators=(",", ":")).encode() if payload is not None else secret
    result = subprocess.run([str(cli), "service", *arguments, "--output", "json"], input=body,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=65, check=False)
    if result.returncode:
        # Do not replay arbitrary CLI stderr or secret input into installer logs.
        raise ValueError("native installation or service operation failed")
    envelope = package.load_json_bytes(result.stdout)
    if envelope.get("status") != "succeeded" or not isinstance(envelope.get("data"), dict):
        raise ValueError("native installation response is invalid")
    return envelope["data"]


def read_json(path):
    regular(path)
    if not Path(path).exists():
        return None
    if Path(path).stat().st_size > 64 * 1024:
        raise ValueError("installation record is oversized")
    return package.load_json_bytes(Path(path).read_bytes())


def write_record(cli, path, record):
    native(cli, ["write-record", "--path", str(path), "--request-stdin"], payload=record)


def publish_marker(cli, record):
    native(cli, ["publish-install", "--request-stdin"], payload=record)


def manager_install(cli, manager, password):
    arguments = ["install", "--manager", manager]
    if manager == "scm":
        arguments.append("--password-stdin")
    native(cli, arguments, secret=password if manager == "scm" else None)


@contextmanager
def maintenance_lock(data):
    import msvcrt
    path = regular(Path(data) / "service/install.lock")
    if not path.exists():
        with path.open("xb") as stream:
            stream.write(b"0")
            stream.flush()
            os.fsync(stream.fileno())
    with path.open("r+b") as stream:
        stream.seek(0)
        msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        try:
            yield
        finally:
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)


def validate_journal(value, layout):
    if not isinstance(value, dict) or set(value) != {
        "schema", "operation", "phase", "old_marker", "new_marker", "old_definition", "new_definition", "files"
    } or value["schema"] != JOURNAL_SCHEMA or value["operation"] not in {"install", "uninstall"} or value["phase"] not in {"prepared", "committed"} or not isinstance(value["files"], list):
        raise ValueError("installation journal is invalid; preserve it for repair")
    home = Path(layout["home"])
    data = Path(layout["data_root"])
    allowed = [home / ".local/bin", data, home / ".agents/skills/hiroute-management", home / ".claude/skills/hiroute-management"]
    for item in value["files"]:
        if not isinstance(item, dict) or set(item) != {"destination", "source", "backup", "old_sha256", "new_sha256", "kind"} or item["kind"] not in {"hardlink", "copy", "remove"}:
            raise ValueError("installation journal entry is invalid")
        destination = Path(item["destination"])
        backup = Path(item["backup"])
        if not destination.is_absolute() or not any(root == destination or root in destination.parents for root in allowed) or data / "service/install-backups" not in backup.parents:
            raise ValueError("installation journal escapes owned directories")
        regular(destination)
        regular(backup)
        for digest in (item["old_sha256"], item["new_sha256"]):
            if digest is not None and (not isinstance(digest, str) or not package.HEX_SHA256.fullmatch(digest)):
                raise ValueError("installation journal digest is invalid")
    return value


def replace_owned(item, rollback=False):
    destination = Path(item["destination"])
    regular(destination)
    expected = item["old_sha256"] if rollback else item["new_sha256"]
    source = Path(item["backup"] if rollback else item["source"])
    if destination.exists():
        actual = package.digest(destination)
        if actual not in {item["old_sha256"], item["new_sha256"]}:
            raise ValueError("owned installation entry changed; preserve it for repair")
    if expected is None:
        if destination.exists():
            destination.unlink()
        return
    regular(source)
    if not source.is_file() or package.digest(source) != expected:
        raise ValueError("installation replacement source differs from journal")
    stage = destination.with_name("." + destination.name + "." + uuid.uuid4().hex + ".stage")
    if item["kind"] == "hardlink":
        os.link(source, stage)
    else:
        shutil.copyfile(source, stage)
        with stage.open("r+b") as stream:
            os.fsync(stream.fileno())
    try:
        os.replace(stage, destination)
    finally:
        if stage.exists():
            stage.unlink()


def finish_journal(journal_path, journal):
    for item in journal["files"]:
        backup = Path(item["backup"])
        regular(backup)
        if backup.exists():
            if package.digest(backup) != item["old_sha256"]:
                raise ValueError("installation backup changed; preserve it for repair")
            backup.unlink()
    journal_path.unlink()


def finish_uninstall_metadata(layout, journal):
    paths = [(Path(layout["marker_path"]), journal["old_marker"]),
             (Path(layout["service_definition"]), journal["old_definition"])]
    for path, expected in paths:
        current = read_json(path)
        if current is not None:
            if current != expected:
                raise ValueError("installation metadata changed; preserve it for repair")
            path.unlink()
    manifest_path = Path(layout["data_root"]) / "package-manifest.json"
    current = read_json(manifest_path)
    if current is not None:
        if current.get("version") != journal["old_marker"]["version"] or current.get("target") != journal["old_marker"]["target"]:
            raise ValueError("package metadata changed; preserve it for repair")
        manifest_path.unlink()


def recover(cli, layout, password):
    path = Path(layout["data_root"]) / "service/install-journal.json"
    journal = read_json(path)
    if journal is None:
        return
    validate_journal(journal, layout)
    if journal["phase"] == "committed":
        if journal["operation"] == "uninstall":
            finish_uninstall_metadata(layout, journal)
        finish_journal(path, journal)
        return
    if journal["operation"] == "uninstall":
        native(cli, ["uninstall"])
        for item in journal["files"]:
            replace_owned(item)
        journal["phase"] = "committed"
        write_record(cli, path, journal)
        finish_uninstall_metadata(layout, journal)
        finish_journal(path, journal)
        return
    old = journal["old_marker"]
    new = journal["new_marker"]
    definition_path = Path(layout["service_definition"])
    if old is None:
        # First-install rollback only touches the exact new registration/image.
        publish_marker(cli, new)
        write_record(cli, definition_path, journal["new_definition"])
        native(cli, ["uninstall"])
    else:
        # Accept either exact old/new registration after a process crash. Native
        # typed verification refuses a foreign registration in both cases.
        try:
            native(cli, ["stop"])
        except ValueError:
            # Registration may already reference the new image. Admission is
            # blocked by prepared journal; exact native old/new verification in
            # manager_install still rejects a live or foreign registration.
            pass
        write_record(cli, definition_path, journal["new_definition"] or journal["old_definition"])
        publish_marker(cli, old)
        manager_install(cli, journal["old_definition"]["manager"], password)
    for item in reversed(journal["files"]):
        replace_owned(item, rollback=True)
    if old is None:
        for target in (Path(layout["marker_path"]), definition_path):
            regular(target)
            if target.exists():
                target.unlink()
    finish_journal(path, journal)


def destinations(root, layout, manifest):
    names = package.executable_names(manifest["target"])
    home, data = Path(layout["home"]), Path(layout["data_root"])
    result = [(home / ".local/bin/hiroute.exe", root / "hiroute.exe", "hardlink", names["hiroute"]),
              (home / ".local/bin/hirouted.exe", root / "hirouted.exe", "hardlink", names["hirouted"])]
    for prefix in (".agents", ".claude"):
        result.append((home / prefix / "skills/hiroute-management/SKILL.md", root / "skills/hiroute-management/SKILL.md", "copy", "skills/hiroute-management/SKILL.md"))
    for name in sorted(manifest["files"]):
        if name.startswith(("licenses/", "docs/")):
            result.append((data / name, root / name, "copy", name))
    result.append((data / "package-manifest.json", root / "package-manifest.json", "copy", "__package_manifest__"))
    return result


def plan_files(entries, data, old_manifest, uninstall=False):
    backup_root = Path(data) / "service/install-backups" / uuid.uuid4().hex
    backup_root.mkdir(parents=True)
    result = []
    for index, (destination, source, kind, name) in enumerate(entries):
        regular(destination)
        destination.parent.mkdir(parents=True, exist_ok=True)
        old_digest = None
        backup = backup_root / str(index)
        if destination.exists():
            owned_manifest = name == "__package_manifest__" and read_json(destination) == old_manifest
            if old_manifest is None or (not owned_manifest and (name not in old_manifest["files"] or package.digest(destination) != old_manifest["files"][name]["sha256"])):
                raise ValueError("installer refuses to replace a foreign or changed entry")
            old_digest = package.digest(destination)
            if kind == "hardlink":
                os.link(destination, backup)
            else:
                shutil.copyfile(destination, backup)
                with backup.open("r+b") as stream:
                    os.fsync(stream.fileno())
        result.append({"destination": str(destination), "source": str(source), "backup": str(backup),
                       "old_sha256": old_digest, "new_sha256": None if uninstall else package.digest(source), "kind": kind})
    return result


def stage_package(manifest, archive, staging):
    for name, (body, _) in package.safe_members(archive, manifest["files"]).items():
        relative = name[4:] if name.startswith("bin/") else name
        target = staging / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(body)
            stream.flush()
            os.fsync(stream.fileno())


def password_input(args):
    if args.manager != "scm":
        return None
    if args.password_stdin:
        value = sys.stdin.buffer.read(4097)
        if not value or len(value) > 4096:
            raise ValueError("service password input is invalid")
        return value
    return getpass.getpass("Windows account password (stored only by SCM): ").encode("utf-8")


def install(args):
    manifest = package.verify(args.manifest, args.archive)
    if not manifest["target"].endswith("-pc-windows-msvc"):
        raise ValueError("package is not a Windows candidate")
    password = password_input(args)
    with tempfile.TemporaryDirectory(prefix="hiroute-verified-") as temporary:
        staging = Path(temporary)
        stage_package(manifest, args.archive, staging)
        cli = staging / "hiroute.exe"
        layout = native(cli, ["prepare-install"])
        if layout["target"] != manifest["target"]:
            raise ValueError("package differs from native host target")
        data, home = Path(layout["data_root"]), Path(layout["home"])
        with maintenance_lock(data):
            recover(cli, layout, password)
            old = read_json(layout["marker_path"])
            old_definition = read_json(layout["service_definition"])
            old_manifest = read_json(data / "package-manifest.json")
            if old and old_definition["manager"] != args.manager:
                raise ValueError("uninstall the previous manager before switching modes")
            root = home / ".local/lib/hiroute" / (manifest["version"] + "-" + manifest["revision"][:12])
            if root.exists():
                for name, facts in manifest["files"].items():
                    relative = name[4:] if name.startswith("bin/") else name
                    source = regular(root / relative)
                    if not source.is_file() or package.digest(source) != facts["sha256"]:
                        raise ValueError("existing versioned package differs; never overwrite it")
            else:
                candidate = root.with_name("." + root.name + "." + uuid.uuid4().hex + ".stage")
                shutil.copytree(staging, candidate)
                for copied in candidate.rglob("*"):
                    if copied.is_file():
                        with copied.open("r+b") as stream:
                            os.fsync(stream.fileno())
                os.replace(candidate, root)
            metadata = root / "package-manifest.json"
            body = json.dumps(manifest, separators=(",", ":"), sort_keys=True).encode()
            if metadata.exists():
                if metadata.read_bytes() != body:
                    raise ValueError("versioned package metadata differs; never overwrite it")
            else:
                with metadata.open("xb") as stream:
                    stream.write(body)
                    stream.flush()
                    os.fsync(stream.fileno())
            cli = root / "hiroute.exe"
            new = {"schema_version": MARKER_SCHEMA, "version": manifest["version"], "target": manifest["target"],
                   "install_root": str(root), "service_definition": layout["service_definition"],
                   "installed_skills": [str(home / prefix / "skills/hiroute-management/SKILL.md") for prefix in (".agents", ".claude")],
                   "cpa_binary": str(root / "libexec/cliproxyapi.exe"), "cpa_sha256": manifest["cpa"]["sha256"]}
            new_definition = native(cli, ["describe-install", "--manager", args.manager, "--daemon", str(root / "hirouted.exe")])
            journal = {"schema": JOURNAL_SCHEMA, "operation": "install", "phase": "prepared", "old_marker": old, "new_marker": new,
                       "old_definition": old_definition, "new_definition": new_definition,
                       "files": plan_files(destinations(root, layout, manifest), data, old_manifest)}
            journal_path = data / "service/install-journal.json"
            write_record(cli, journal_path, journal)
            try:
                if old:
                    native(Path(old["install_root"]) / "hiroute.exe", ["stop"])
                for item in journal["files"]:
                    replace_owned(item)
                publish_marker(cli, new)
                manager_install(cli, args.manager, password)
                journal["phase"] = "committed"
                write_record(cli, journal_path, journal)
                finish_journal(journal_path, journal)
            except Exception:
                recover(cli, layout, password)
                raise
            print(json.dumps({"installed": True, "version": manifest["version"], "manager": args.manager,
                              "cli": str(home / ".local/bin/hiroute.exe"), "started": False}, sort_keys=True))


def uninstall(args):
    cli = Path(args.cli).resolve(strict=True)
    layout = native(cli, ["layout"])
    data = Path(layout["data_root"])
    with maintenance_lock(data):
        recover(cli, layout, password_input(args))
        old = read_json(layout["marker_path"])
        definition = read_json(layout["service_definition"])
        manifest = read_json(data / "package-manifest.json")
        if old is None or definition is None or manifest is None:
            raise ValueError("installation ownership is unavailable")
        journal = {"schema": JOURNAL_SCHEMA, "operation": "uninstall", "phase": "prepared", "old_marker": old, "new_marker": old,
                   "old_definition": definition, "new_definition": definition,
                   "files": plan_files(destinations(Path(old["install_root"]), layout, manifest), data, manifest, uninstall=True)}
        journal_path = data / "service/install-journal.json"
        write_record(cli, journal_path, journal)
        try:
            native(cli, ["uninstall"])
            for item in journal["files"]:
                replace_owned(item)
            journal["phase"] = "committed"
            write_record(cli, journal_path, journal)
            finish_uninstall_metadata(layout, journal)
            finish_journal(journal_path, journal)
        except Exception:
            # Keep the prepared journal and exact binaries for the next repair;
            # SCM deletion can remain pending and must never be called complete.
            raise
        print(json.dumps({"uninstalled": True, "state_preserved": layout["state_root"]}, sort_keys=True))


def main():
    if sys.platform != "win32":
        raise ValueError("Windows installation requires a native Windows process")
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("install")
    create.add_argument("--manifest", type=Path, required=True)
    create.add_argument("--archive", type=Path, required=True)
    remove = sub.add_parser("uninstall")
    remove.add_argument("--cli", type=Path, required=True, help="versioned hiroute.exe, not the stable hardlink being removed")
    for leaf in (create, remove):
        leaf.add_argument("--manager", choices=("task", "scm"), default="task")
        leaf.add_argument("--password-stdin", action="store_true")
    args = parser.parse_args()
    if args.command == "install":
        install(args)
    else:
        uninstall(args)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
