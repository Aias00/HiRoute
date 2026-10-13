#!/usr/bin/env python3
"""Deterministic package and install-recovery guards; no product-green claim."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
from contextlib import nullcontext
from types import SimpleNamespace


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


package = load("windows_package", "package-standalone.py")
installer = load("windows_install", "install-standalone-windows.py")


class WindowsPackage(unittest.TestCase):
    def test_inventory_is_closed_and_keeps_one_package_schema(self):
        self.assertEqual(package.SCHEMA, "hiroute.standalone-package/v1")
        files = package.expected_files("x86_64-pc-windows-msvc")
        self.assertIn("bin/hiroute.exe", files)
        self.assertIn("bin/hirouted.exe", files)
        self.assertIn("libexec/cliproxyapi.exe", files)
        self.assertNotIn("bin/hiroute", files)
        self.assertEqual(len(files), len(package.EXPECTED_FILES))

    def test_pe_architecture_is_verified_before_packaging(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "hiroute.exe"
            body = bytearray(70)
            body[:2] = b"MZ"
            struct.pack_into("<I", body, 0x3C, 64)
            body[64:68] = b"PE\0\0"
            struct.pack_into("<H", body, 68, 0x8664)
            path.write_bytes(body)
            package.verify_windows_image(path, "x86_64-pc-windows-msvc")
            with self.assertRaises(ValueError):
                package.verify_windows_image(path, "aarch64-pc-windows-msvc")
            path.write_bytes(b"not an executable")
            with self.assertRaises(ValueError):
                package.verify_windows_image(path, "x86_64-pc-windows-msvc")

    def test_external_archive_cannot_hide_another_pe_machine_behind_matching_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve(); target = "x86_64-pc-windows-msvc"
            files = {}
            for index, name in enumerate(sorted(package.expected_files(target))):
                path = root / str(index)
                if name in package.executable_names(target).values():
                    body = bytearray(70); body[:2] = b"MZ"; struct.pack_into("<I", body, 0x3C, 64)
                    body[64:68] = b"PE\0\0"; struct.pack_into("<H", body, 68, 0xAA64)
                    path.write_bytes(body)
                else:
                    path.write_bytes(b"synthetic package fixture")
                files[name] = path
            archive = root / "candidate.tar.gz"; again = root / "again.tar.gz"
            package.deterministic_archive(archive, files); package.deterministic_archive(again, files)
            self.assertEqual(archive.read_bytes(), again.read_bytes())
            inventory = {name: {"sha256": package.digest(path), "size": path.stat().st_size,
                         "mode": "executable" if name.startswith(("bin/", "libexec/")) else "read_only"} for name, path in files.items()}
            manifest = {"schema": package.SCHEMA, "version": "0.1.0", "revision": "fixture",
                        "target": target, "distribution": "integration-candidate", "files": inventory,
                        "archive": {"filename": archive.name, "sha256": package.digest(archive), "size": archive.stat().st_size},
                        "cpa": {"version": "fixture", "sha256": inventory[package.executable_names(target)["cpa"]]["sha256"]}}
            path = root / "manifest.json"; path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "architecture"):
                package.verify(path, archive)


class WindowsRecovery(unittest.TestCase):
    def fixture(self, directory, operation="install", old=True):
        root = Path(directory).resolve(); data = root / "data"; (data / "service").mkdir(parents=True)
        bin_root = root / ".local/bin"; bin_root.mkdir(parents=True)
        marker = data / "standalone.json"; definition = data / "service/windows-background.json"
        layout = {"home": str(root), "data_root": str(data), "marker_path": str(marker), "service_definition": str(definition)}
        old_marker = {"version": "0.1.0", "target": "x86_64-pc-windows-msvc", "install_root": str(root / "old")} if old else None
        old_definition = {"manager": "task", "daemon": "old"} if old else None
        new_marker = {"version": "0.2.0", "target": "x86_64-pc-windows-msvc", "install_root": str(root / "new")}
        new_definition = {"manager": "task", "daemon": "new"}
        destination = bin_root / "hiroute.exe"; source = root / "source"; source.write_bytes(b"new")
        backup = data / "service/install-backups/transaction/0"; backup.parent.mkdir(parents=True)
        if old:
            backup.write_bytes(b"old")
        destination.write_bytes(b"new")
        item = {"destination": str(destination), "source": str(source), "backup": str(backup),
                "old_sha256": package.digest(backup) if old else None, "new_sha256": package.digest(source), "kind": "hardlink"}
        journal = {"schema": installer.JOURNAL_SCHEMA, "operation": operation, "phase": "prepared",
                   "old_marker": old_marker, "new_marker": new_marker, "old_definition": old_definition,
                   "new_definition": new_definition, "files": [item]}
        journal_path = data / "service/install-journal.json"; journal_path.write_text(json.dumps(journal))
        marker.write_text(json.dumps(new_marker)); definition.write_text(json.dumps(new_definition))
        return layout, journal, journal_path, destination

    @staticmethod
    def write_record(_cli, path, value):
        Path(path).write_text(json.dumps(value))

    def test_first_install_recovery_removes_only_published_new_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory, old=False)
            with patch.object(installer, "native", return_value={}) as native, patch.object(installer, "write_record", self.write_record), patch.object(installer, "publish_marker", lambda _cli, value: self.write_record(_cli, layout["marker_path"], value)):
                installer.recover(Path(directory) / "hiroute.exe", layout, None)
            native.assert_called_once_with(Path(directory) / "hiroute.exe", ["uninstall"])
            self.assertFalse(destination.exists()); self.assertFalse(path.exists())
            self.assertFalse(Path(layout["marker_path"]).exists()); self.assertFalse(Path(layout["service_definition"]).exists())

    def test_upgrade_recovery_republishes_old_marker_and_registration_before_rollback(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory)
            registrations = []
            def register(_cli, manager, _password):
                self.assertEqual(installer.read_json(layout["marker_path"]), journal["old_marker"])
                self.assertEqual(installer.read_json(layout["service_definition"]), journal["new_definition"])
                registrations.append(manager)
                self.write_record(_cli, layout["service_definition"], journal["old_definition"])
            with patch.object(installer, "native", return_value={}), patch.object(installer, "write_record", self.write_record), patch.object(installer, "publish_marker", lambda _cli, value: self.write_record(_cli, layout["marker_path"], value)), patch.object(installer, "manager_install", register):
                installer.recover(Path(directory) / "hiroute.exe", layout, None)
            self.assertEqual(registrations, ["task"]); self.assertEqual(destination.read_bytes(), b"old")
            self.assertFalse(path.exists())

    def test_pending_manager_delete_retains_prepared_journal_and_owned_files(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory, operation="uninstall")
            with patch.object(installer, "native", side_effect=ValueError("service deletion pending")):
                with self.assertRaises(ValueError):
                    installer.recover(Path(directory) / "hiroute.exe", layout, None)
            self.assertEqual(installer.read_json(path), journal); self.assertEqual(destination.read_bytes(), b"new")

    def test_committed_install_cleans_backups_without_reverting_new_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory)
            journal["phase"] = "committed"; path.write_text(json.dumps(journal))
            with patch.object(installer, "native") as native:
                installer.recover(Path(directory) / "hiroute.exe", layout, None)
            native.assert_not_called(); self.assertEqual(destination.read_bytes(), b"new")
            self.assertFalse(path.exists()); self.assertFalse(Path(journal["files"][0]["backup"]).exists())

    def test_external_scm_reopen_replays_publication_but_keeps_admission_journal(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory)
            journal["new_definition"] = {"manager": "scm", "owner_sid": "S-1-5-21-fixture"}
            path.write_text(json.dumps(journal)); Path(layout["marker_path"]).write_text(json.dumps(journal["old_marker"]))
            destination.write_bytes(b"old")
            with patch.object(installer, "native", return_value={}) as native, patch.object(installer, "publish_marker", lambda cli, value: self.write_record(cli, layout["marker_path"], value)), patch.object(installer, "pending_admin") as pending:
                self.assertTrue(installer.recover(Path(directory) / "hiroute.exe", layout, None, external_manager=True))
            native.assert_called_once_with(Path(directory) / "hiroute.exe", ["stop"])
            pending.assert_called_once(); self.assertEqual(destination.read_bytes(), b"new")
            self.assertEqual(installer.read_json(path)["phase"], "prepared")
            self.assertEqual(installer.read_json(layout["marker_path"]), journal["new_marker"])

    def test_owner_finalize_requires_native_machine_proof_before_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            layout, journal, path, destination = self.fixture(directory)
            journal["new_definition"] = {"manager": "scm", "owner_sid": "S-1-5-21-fixture"}; path.write_text(json.dumps(journal))
            cli = Path(directory) / "hiroute.exe"; cli.write_bytes(b"fixture")
            args = SimpleNamespace(cli=cli)
            with patch.object(installer, "native", return_value=layout), patch.object(installer, "maintenance_lock", lambda _data: nullcontext()), patch.object(installer, "publish_marker", lambda cli, value: self.write_record(cli, layout["marker_path"], value)), patch.object(installer, "owner_finalize", side_effect=ValueError("SCM not actually registered")):
                with self.assertRaises(ValueError):
                    installer.finalize(args)
            self.assertEqual(installer.read_json(path)["phase"], "prepared")
            self.assertTrue(Path(journal["files"][0]["backup"]).exists())

    def test_internal_admin_credentials_are_stdin_only_and_response_is_closed(self):
        password = 'literal"; $(must-not-run) & no-shell'
        request = {"schema": "request", "operation": "install", "password": password}
        response = {"schema": "response", "operation": "install", "definition": {}, "completed": True}
        process = SimpleNamespace(returncode=0, stdout=json.dumps(response).encode())
        with patch.object(installer.subprocess, "run", return_value=process) as run:
            installer.internal(Path("trusted-cli.exe"), "fixed-internal-verb", request, "response", "install")
        argv = run.call_args.args[0]; self.assertNotIn(password, argv)
        self.assertEqual(json.loads(run.call_args.kwargs["input"])["password"], password)
        self.assertNotIn("shell", run.call_args.kwargs)
        process.stdout = json.dumps({**response, "password": password}).encode()
        with patch.object(installer.subprocess, "run", return_value=process), self.assertRaises(ValueError):
            installer.internal(Path("trusted-cli.exe"), "fixed-internal-verb", request, "response", "install")

    def test_partial_publish_rolls_back_exact_old_bytes_and_preserves_foreign_edit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            old, new, dest = (root / value for value in ("backup", "source", "destination"))
            old.write_bytes(b"old"); new.write_bytes(b"new"); dest.write_bytes(b"old")
            item = {"destination": str(dest), "source": str(new), "backup": str(old),
                    "old_sha256": package.digest(old), "new_sha256": package.digest(new), "kind": "copy"}
            installer.replace_owned(item)
            self.assertEqual(dest.read_bytes(), b"new")
            installer.replace_owned(item, rollback=True)
            self.assertEqual(dest.read_bytes(), b"old")
            dest.write_bytes(b"foreign")
            with self.assertRaises(ValueError):
                installer.replace_owned(item, rollback=True)
            self.assertEqual(dest.read_bytes(), b"foreign")

    def test_journal_rejects_unknown_fields_and_escaping_owned_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            layout = {"home": str(root), "data_root": str(root / "data")}
            journal = {"schema": installer.JOURNAL_SCHEMA, "operation": "install", "phase": "prepared",
                       "old_marker": None, "new_marker": {}, "old_definition": None, "new_definition": {}, "files": []}
            installer.validate_journal(journal, layout)
            with self.assertRaises(ValueError):
                installer.validate_journal({**journal, "password": "must never persist"}, layout)
            item = {"destination": str(root / "foreign"), "source": "ignored", "backup": str(root / "data/service/install-backups/x/1"), "old_sha256": None, "new_sha256": None, "kind": "remove"}
            with self.assertRaises(ValueError):
                installer.validate_journal({**journal, "files": [item]}, layout)

    def test_committed_uninstall_reopens_and_finishes_metadata_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve(); data = root / "data"; data.mkdir()
            marker = data / "standalone.json"; definition = data / "definition.json"
            old = {"version": "0.1.0", "target": "x86_64-pc-windows-msvc"}
            previous_definition = {"manager": "task"}
            marker.write_text(json.dumps(old)); definition.write_text(json.dumps(previous_definition))
            (data / "package-manifest.json").write_text(json.dumps(old))
            layout = {"marker_path": str(marker), "service_definition": str(definition), "data_root": str(data)}
            journal = {"old_marker": old, "old_definition": previous_definition}
            installer.finish_uninstall_metadata(layout, journal)
            installer.finish_uninstall_metadata(layout, journal)
            self.assertFalse(marker.exists()); self.assertFalse(definition.exists())


if __name__ == "__main__":
    unittest.main()
