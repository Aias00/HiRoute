#!/usr/bin/env python3
"""Independent PE import fixtures plus receipt-bound runtime audit tests."""

import importlib.util
import json
from pathlib import Path
import struct
from types import SimpleNamespace
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), HERE / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


audit = load("windows-native-runtime-audit")
receipts = load("test-windows-native-artifacts")


def image(ordinary=("KERNEL32.dll",), delayed=()):
    """Two-section PE32+ with ordinal thunks and separate delay descriptors."""
    result = bytearray(2048)
    result[:2] = b"MZ"
    struct.pack_into("<I", result, 0x3C, 0x80)
    result[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HHIIIHH", result, 0x84, 0x8664, 2, 0, 0, 0, 240, 0x22)
    optional = 0x98
    struct.pack_into("<HBBIIIIIQII", result, optional, 0x20B, 14, 0, 512, 1024, 0,
                     0x1000, 0x1000, 0x140000000, 0x1000, 0x200)
    struct.pack_into("<HHHHHH", result, optional + 40, 6, 0, 0, 0, 6, 0)
    struct.pack_into("<IIIHHQQQQII", result, optional + 56, 0x3000, 0x200, 0, 3, 0,
                     0x100000, 0x1000, 0x100000, 0x1000, 0, 16)
    table = optional + 240
    for number, (name, size, rva, raw_size, raw, flags) in enumerate(
            [(b".text", 3, 0x1000, 512, 512, 0x60000020),
             (b".idata", 1024, 0x2000, 1024, 1024, 0xC0000040)]):
        offset = table + number * 40
        result[offset:offset + 8] = name.ljust(8, b"\0")
        struct.pack_into("<IIIIIIHHI", result, offset + 8, size, rva, raw_size, raw, 0, 0, 0, 0, flags)
    result[512:515] = b"\x31\xc0\xc3"
    for imports, index, rva, name_rva, thunks in ((ordinary, 1, 0x2000, 0x2200, 0x2300),
                                               (delayed, 13, 0x2100, 0x2280, 0x2380)):
        if not imports:
            continue
        stride = 20 if index == 1 else 32
        struct.pack_into("<II", result, optional + 112 + 8 * index, rva, (len(imports) + 1) * stride)
        for number, name in enumerate(imports):
            encoded = name.encode("ascii") + b"\0"
            raw_name = 1024 + name_rva - 0x2000
            result[raw_name:raw_name + len(encoded)] = encoded
            thunk = thunks + number * 32
            raw_thunk = 1024 + thunk - 0x2000
            struct.pack_into("<QQQQ", result, raw_thunk, 0x8000000000000001, 0, 0x8000000000000001, 0)
            descriptor = 1024 + rva - 0x2000 + number * stride
            if index == 1:
                struct.pack_into("<IIIII", result, descriptor, thunk, 0, 0, name_rva, thunk + 16)
            else:
                struct.pack_into("<IIIIIIII", result, descriptor, 1, name_rva, 0x23E0, thunk + 16, thunk, 0, 0, 0)
            name_rva += len(encoded)
    return bytes(result)


class Imports(unittest.TestCase):
    def test_release_imports_and_delayed_debug_dependency_are_distinct(self):
        value = audit.pe_imports(image(("KERNEL32.dll", "api-ms-win-crt-heap-l1-1-0.dll"), ("VCRUNTIME140_1D.dll",)))
        self.assertEqual(value["ordinary"], ["api-ms-win-crt-heap-l1-1-0.dll", "kernel32.dll"])
        self.assertEqual(value["delay"], ["vcruntime140_1d.dll"])
        self.assertEqual(value["debug_crt"], ["vcruntime140_1d.dll"])
        self.assertEqual(audit.pe_imports(image(("MSVCRT.dll", "UCRTBASE.dll")))["debug_crt"], [])
        self.assertEqual(audit.pe_imports(image(("VCRUNTIME140_1.dll",)))["release_vc_runtime"], ["vcruntime140_1.dll"])

    def test_debug_crt_families_are_detected_without_general_d_suffix_rule(self):
        names = ["ucrtbased.dll", "MSVCRTD.dll", "msvcr120d.dll", "msvcp140d.dll",
                 "msvcp140_2d.dll", "msvcp140d_atomic_wait.dll", "vcruntime140d.dll", "concrt140d.dll"]
        for name in names:
            with self.subTest(name=name):
                self.assertEqual(audit.pe_imports(image((name,)))["debug_crt"], [name.lower()])
        self.assertEqual(audit.pe_imports(image(("thirdpartyd.dll",)))["debug_crt"], [])

    def test_wrong_machine_and_truncated_headers_fail_closed(self):
        bad = bytearray(image())
        struct.pack_into("<H", bad, 0x84, 0xAA64)
        for data in (bad, image()[:800], b"MZ", b""):
            with self.subTest(size=len(data)), self.assertRaises(ValueError):
                audit.pe_imports(data)

    def test_out_of_range_unterminated_and_unsafe_dll_names_are_rejected(self):
        variants = []
        outside = bytearray(image())
        struct.pack_into("<I", outside, 1024 + 12, 0x4000)
        variants.append(outside)
        missing_null = bytearray(image())
        struct.pack_into("<II", missing_null, 0x98 + 112 + 8, 0x2000, 20)
        variants.append(missing_null)
        variants.extend([image(("../VCRUNTIME140D.dll",)), image(("C:runtime.dll",))])
        bad_name = bytearray(image())
        bad_name[1536:1792] = b"x" * 256
        variants.append(bad_name)
        for data in variants:
            with self.subTest(prefix=bytes(data[1024:1044]).hex()), self.assertRaises(ValueError):
                audit.pe_imports(data)

    def test_old_delay_va_cannot_underflow_64bit_image_base(self):
        bad = bytearray(image(delayed=("VCRUNTIME140D.dll",)))
        struct.pack_into("<I", bad, 1280, 0)
        with self.assertRaisesRegex(ValueError, "precedes"):
            audit.pe_imports(bad)


class ReceiptAudit(unittest.TestCase):
    def setUp(self):
        self.fixture = receipts.NativeArtifactReceipts()
        self.fixture.setUp()
        self.original_module = audit.module
        self.root_patch = patch.object(audit, "ROOT", self.fixture.root)
        self.module_patch = patch.object(audit, "module", self.module)
        self.root_patch.start()
        self.module_patch.start()

    def tearDown(self):
        self.module_patch.stop()
        self.root_patch.stop()
        self.fixture.tearDown()

    def module(self, name):
        return receipts.module if name == "windows-native-artifacts" else self.original_module(name)

    def prepare(self, data=None):
        (self.fixture.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "libz-ng-sys"\nversion = "1.1.29"\n')
        inputs, _, _, _ = self.fixture.inputs(image=image() if data is None else data)
        receipts.module.capture(inputs)
        receipts.module.finalize(SimpleNamespace(expected_sha=receipts.EXPECTED, evidence=self.fixture.evidence,
                                                include_product_binaries=False, package_result=None))
        return SimpleNamespace(expected_sha=receipts.EXPECTED, evidence=self.fixture.evidence,
                               cargo_json=None, dumpbin=None)

    def test_exact_receipt_binds_imports_and_never_claims_runtime_or_release(self):
        args = self.prepare()
        value = audit.audit(args)
        self.assertEqual(value["dependency_gate"], "green")
        self.assertEqual(len(value["images"]), 1)
        self.assertEqual(value["images"][0]["imports"]["ordinary"], ["kernel32.dll"])
        self.assertEqual(value["dll_resolution_and_runtime"], "not_assessed")
        self.assertEqual(value["production_release_acceptance"], "not_assessed")

    def test_actual_ordinary_and_delay_debug_imports_make_gate_red(self):
        args = self.prepare(image(("ucrtbased.dll",), ("VCRUNTIME140D.dll",)))
        value = audit.audit(args)
        self.assertEqual(value["dependency_gate"], "red")
        self.assertEqual(value["debug_crt_images"], [value["images"][0]["path"]])

    def assert_static_context_rejection(self, data):
        args = self.prepare(data)
        self.assertEqual(audit.audit(args)['dependency_gate'], 'green')  # Old Debug-only gate.
        args.require_static_crt = True
        with self.assertRaisesRegex(ValueError, 'no redistributable VC runtime DLL imports'):
            audit.audit(args)

    def test_static_context_rejects_real_ordinary_release_crt_import(self):
        self.assert_static_context_rejection(image(("VCRUNTIME140.dll",)))

    def test_static_context_rejects_real_delay_release_crt_import(self):
        self.assert_static_context_rejection(image(("KERNEL32.dll",), ("MSVCP140.dll",)))

    def test_static_context_allows_native_os_crt_and_keeps_runtime_unassessed(self):
        args = self.prepare(image(("KERNEL32.dll", "MSVCRT.dll", "UCRTBASE.dll")))
        args.require_static_crt = True
        value = audit.audit(args)
        self.assertEqual(value['dependency_gate'], 'green')
        self.assertEqual(value['dll_resolution_and_runtime'], 'not_assessed')
        self.assertEqual(value['production_release_acceptance'], 'not_assessed')

    def test_complete_verified_package_and_all_captured_products_are_audited(self):
        with patch.object(receipts, "small_pe", return_value=image(delayed=("ucrtbased.dll",))):
            self.fixture.test_product_and_full_verified_package_bytes_use_portable_captured_paths()
        args = SimpleNamespace(expected_sha=receipts.EXPECTED, evidence=self.fixture.evidence,
                               cargo_json=None, dumpbin=None)
        value = audit.audit(args)
        self.assertEqual(len(value["images"]), 7)
        package = [item for item in value["images"] if item["origin"] == "verified-package-executable"]
        self.assertEqual({item["path"] for item in package},
                         {"package/bin/hiroute.exe", "package/bin/hirouted.exe", "package/libexec/cliproxyapi.exe"})
        self.assertEqual(value["dependency_gate"], "red")
        self.assertEqual(len(value["debug_crt_images"]), 7)

    def test_foreign_sha_and_changed_bytes_rejected_before_inventory(self):
        args = self.prepare()
        args.expected_sha = "b" * 40
        with self.assertRaisesRegex(ValueError, "expected SHA"):
            audit.audit(args)
        args.expected_sha = receipts.EXPECTED
        binary = next((args.evidence / "native-binaries/platform").iterdir())
        value = bytearray(binary.read_bytes())
        value[-1] ^= 1
        binary.write_bytes(value)
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            audit.audit(args)

    def directive_inputs(self, args):
        output = self.fixture.root / "target/debug/build/libz-ng-sys-fixture/out"
        library = output / "lib/zlibstatic-ngd.lib"
        library.parent.mkdir(parents=True)
        library.write_bytes(b"!<arch>\nfixture")
        args.dumpbin = self.fixture.root / "dumpbin.exe"
        args.dumpbin.write_bytes(b"trusted-tool-fixture")
        event = {"reason": "build-script-executed", "package_id": "registry+https://github.com/rust-lang/crates.io-index#libz-ng-sys@1.1.29",
                 "out_dir": str(output), "linked_libs": ["static=zlibstatic-ngd"], "linked_paths": ["native=" + str(library.parent)]}
        args.cargo_json = self.fixture.root / "objects.jsonl"
        events = [event, {"reason": "build-finished", "success": True}]
        args.cargo_json.write_text("\n".join(json.dumps(value) for value in events))
        return library, event, events

    def test_object_debug_directive_is_source_evidence_not_final_dll_proof(self):
        args = self.prepare()
        library, _, _ = self.directive_inputs(args)
        # Only DUMPBIN is mocked. PE import parsing and receipt/hash verification are real.
        completed = SimpleNamespace(stdout=b'Linker Directives\n /DEFAULTLIB:"MSVCRTD" /DEFAULTLIB:OLDNAMES\n', stderr=b"")
        with patch.object(audit.subprocess, "run", return_value=completed) as run:
            value = audit.audit(args)
        source = value["zlib_ng_object_directives"]
        self.assertTrue(source["debug_crt_requested"])
        self.assertFalse(source["runtime_dependency_proof"])
        self.assertEqual(value["dependency_gate"], "green")
        self.assertEqual(run.call_args.args[0], [str(args.dumpbin), "/NOLOGO", "/DIRECTIVES", str(library)])
        self.assertNotIn("shell", run.call_args.kwargs)

    def test_object_zero_duplicate_missing_library_and_foreign_output_rejected(self):
        args = self.prepare()
        _, event, events = self.directive_inputs(args)
        for selected in (events[1:], [event, event, events[1]],
                         [dict(event, out_dir=str(self.fixture.root)), events[1]],
                         [dict(event, linked_libs=[]), events[1]]):
            args.cargo_json.write_text("\n".join(json.dumps(value) for value in selected))
            with self.subTest(events=selected), self.assertRaises(ValueError):
                audit.audit(args)
        args.cargo_json.write_text("\n".join(json.dumps(value) for value in events))
        with patch.object(audit.subprocess, "run", return_value=SimpleNamespace(stdout=b"warning only\n", stderr=b"")):
            with self.assertRaisesRegex(ValueError, "zero"):
                audit.audit(args)


if __name__ == "__main__":
    unittest.main()
