#!/usr/bin/env python3
"""Deterministic package and install-recovery guards; no product-green claim."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch


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
