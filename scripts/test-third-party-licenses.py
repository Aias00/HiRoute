#!/usr/bin/env python3

import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import subprocess
import sys
import unittest


REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "third_party_licenses", REPO / "scripts/collect-third-party-licenses.py"
)
licenses = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(licenses)


class ThirdPartyLicenseTests(unittest.TestCase):
    def test_unicode_notices_are_utf8_bytes_even_with_an_ascii_process_locale(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "notices"
            program = (
                "import importlib.util; from pathlib import Path; "
                f"s=importlib.util.spec_from_file_location('licenses',{str(SPEC.origin)!r}); "
                "m=importlib.util.module_from_spec(s); s.loader.exec_module(m); "
                "notice=('Copyright '+chr(322)+chr(20013)+'\\n').encode('utf-8'); "
                "r=m.package_record('cargo','unicode','1.0.0','MIT',None,[('LICENSE',notice)]); "
                f"m.render([r],{{}},Path({str(output)!r}))"
            )
            environment = dict(os.environ, LC_ALL="C", PYTHONUTF8="0", PYTHONCOERCECLOCALE="0")
            subprocess.run([sys.executable, "-c", program], env=environment, check=True, capture_output=True)
            rendered = (output / "THIRD-PARTY-LICENSES.txt").read_bytes()
            self.assertIn("Copyright ł中\n".encode("utf-8"), rendered)
            self.assertNotIn(b"\r\n", rendered)

    def test_source_archive_extraction_is_portable_and_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = io.BytesIO()
            with tarfile.open(fileobj=archive, mode="w") as bundle:
                valid = tarfile.TarInfo("source/LICENSE")
                valid.size = len(b"license\n")
                bundle.addfile(valid, io.BytesIO(b"license\n"))
            archive.seek(0)
            with tarfile.open(fileobj=archive, mode="r:") as bundle:
                licenses.extract_regular_tree(bundle, root / "valid")
            self.assertEqual(
                (root / "valid/source/LICENSE").read_text(), "license\n"
            )

            unsafe = io.BytesIO()
            with tarfile.open(fileobj=unsafe, mode="w") as bundle:
                traversal = tarfile.TarInfo("../outside")
                traversal.size = 1
                bundle.addfile(traversal, io.BytesIO(b"x"))
            unsafe.seek(0)
            with tarfile.open(fileobj=unsafe, mode="r:") as bundle, \
                    self.assertRaisesRegex(ValueError, "unsafe path"):
                licenses.extract_regular_tree(bundle, root / "unsafe")
            self.assertFalse((root / "outside").exists())

    def test_license_file_discovery_is_bounded_and_includes_unlicense(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("LICENSE", "NOTICE.txt", "UNLICENSE", "README.md"):
                (root / name).write_text(name)
            self.assertEqual(
                [path.name for path in licenses.license_files(root)],
                ["LICENSE", "NOTICE.txt", "UNLICENSE"],
            )

    def test_fallback_chooses_a_permissive_declared_alternative(self):
        name, value = licenses.fallback_document(
            "MIT OR Apache-2.0 OR LGPL-2.1-or-later", "Example", b"apache"
        )
        self.assertEqual(name, "LICENSE-Apache-2.0")
        self.assertEqual(value, b"apache")
        with self.assertRaisesRegex(ValueError, "reviewed fallback"):
            licenses.fallback_document("GPL-3.0-only", "Example", b"apache")

    def test_json_sequence_accepts_go_mod_download_stream(self):
        self.assertEqual(
            licenses.json_sequence('{"Path":"one"}\n{"Path":"two"}\n'),
            [{"Path": "one"}, {"Path": "two"}],
        )

    def test_render_is_deterministic_and_has_no_local_paths(self):
        def records():
            return [
                licenses.package_record(
                    "cargo", "zeta", "1.0.0", "MIT", "registry", [("LICENSE", b"same\n")]
                ),
                licenses.package_record(
                    "npm", "alpha", "2.0.0", "MIT", None, [("COPYING", b"same\n")]
                ),
            ]

        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first"
            second = Path(directory) / "second"
            licenses.render(records(), {"lock": "abc"}, first)
            licenses.render(list(reversed(records())), {"lock": "abc"}, second)
            for name in ("third-party-licenses.json", "THIRD-PARTY-LICENSES.txt"):
                self.assertEqual((first / name).read_bytes(), (second / name).read_bytes())
                self.assertNotIn(directory.encode(), (first / name).read_bytes())
                self.assertEqual((first / name).stat().st_mode & 0o777, 0o644)
            manifest = json.loads((first / "third-party-licenses.json").read_text())
            self.assertEqual(manifest["schema"], licenses.SCHEMA)
            self.assertEqual(len(manifest["documents"]), 1)
            self.assertEqual(
                manifest["documents"][0]["packages"],
                ["cargo:zeta@1.0.0", "npm:alpha@2.0.0"],
            )


if __name__ == "__main__":
    unittest.main()
