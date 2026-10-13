#!/usr/bin/env python3
"""Read exact Windows artifacts' ordinary/delay imports; never run an image."""

import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
SCHEMA = "hiroute.windows-native-runtime-audit/v1"
DEBUG_CRT = re.compile(r"^(?:ucrtbased|msvcrtd|msvcr\d+d|(?:msvcp|vcruntime)\d+(?:_\d+)?d(?:_[a-z0-9_]+)?|concrt\d+d)\.dll$", re.I)
RELEASE_VC_RUNTIME = re.compile(r"^(?:msvcr\d+|(?:msvcp|vcruntime|concrt)\d+(?:_[a-z0-9_]+)?)\.dll$", re.I)
DLL_NAME = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.-]{0,253}\.dll$", re.I)


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def pe_imports(data):
    """Read PE32+ AMD64 data directories; malformed/unterminated tables fail."""
    module("package-standalone").verify_windows_stream(io.BytesIO(data), "x86_64-pc-windows-msvc")

    def unpack(fmt, offset):
        size = struct.calcsize(fmt)
        if offset < 0 or offset + size > len(data):
            raise ValueError("truncated PE structure")
        return struct.unpack_from(fmt, data, offset)

    nt, = unpack("<I", 0x3C)
    sections, = unpack("<H", nt + 6)
    optional_size, = unpack("<H", nt + 20)
    optional = nt + 24
    magic, = unpack("<H", optional)
    if magic != 0x20B or not 1 <= sections <= 96 or optional_size < 112:
        raise ValueError("unsupported PE optional header or sections")
    base, = unpack("<Q", optional + 24)
    headers, = unpack("<I", optional + 60)
    count, = unpack("<I", optional + 108)
    if count > 16 or 112 + count * 8 > optional_size:
        raise ValueError("invalid PE data directory count")
    section_table = optional + optional_size
    if not section_table + sections * 40 <= headers <= len(data):
        raise ValueError("invalid PE header extent")
    ranges = []
    for number in range(sections):
        size, address, raw_size, raw = unpack("<IIII", section_table + number * 40 + 8)
        if address < headers or raw_size and (raw < headers or raw + raw_size > len(data)):
            raise ValueError("invalid PE section extent")
        end = address + max(size, raw_size)
        if end > 0x100000000 or any(address < other_end and other < end for other, other_end, _, _ in ranges):
            raise ValueError("overlapping or overflowing PE sections")
        ranges.append((address, end, raw, raw_size))

    def mapped(address, size):
        if address <= 0 or size <= 0 or address + size > 0x100000000:
            raise ValueError("invalid PE import RVA")
        if address < headers and address + size <= headers:
            return address, headers - address
        matches = [(raw + address - start, raw_size - (address - start))
                   for start, _, raw, raw_size in ranges if start <= address and address + size <= start + raw_size]
        if len(matches) != 1:
            raise ValueError("PE import RVA is not uniquely file-backed")
        return matches[0]

    def dll_name(address):
        offset, remaining = mapped(address, 1)
        end = data.find(b"\0", offset, offset + min(remaining, 256))
        if end < 0:
            raise ValueError("unterminated PE DLL name")
        try:
            name = data[offset:end].decode("ascii")
        except UnicodeDecodeError as error:
            raise ValueError("non-ASCII PE DLL name") from error
        if not DLL_NAME.fullmatch(name):
            raise ValueError("PE DLL name contains a path or invalid characters")
        return name.lower()

    result = {"ordinary": [], "delay": []}
    for index, name, descriptor_size in ((1, "ordinary", 20), (13, "delay", 32)):
        if index >= count:
            continue
        address, size = unpack("<II", optional + 112 + index * 8)
        if address == 0 and size == 0:
            continue
        if not address or not size or size < descriptor_size or size > 1024 * 1024:
            raise ValueError("invalid PE import directory extent")
        offset, _ = mapped(address, size)
        for entry in range(size // descriptor_size):
            values = unpack("<" + "I" * (descriptor_size // 4), offset + entry * descriptor_size)
            if not any(values):
                break
            if name == "ordinary":
                imported_name = values[3]
                if not values[4]:
                    raise ValueError("ordinary import has no address table")
            else:
                attributes, imported_name = values[:2]
                if attributes & ~1 or not values[3]:
                    raise ValueError("invalid delay import attributes or address table")
                if not attributes & 1:
                    if imported_name < base:
                        raise ValueError("delay import VA precedes image base")
                    imported_name -= base
            result[name].append(dll_name(imported_name))
        else:
            raise ValueError("PE import directory has no null descriptor")
        result[name] = sorted(set(result[name]))
    result["debug_crt"] = sorted({name for group in (result["ordinary"], result["delay"])
                                  for name in group if DEBUG_CRT.fullmatch(name)})
    result["release_vc_runtime"] = sorted({name for group in (result["ordinary"], result["delay"])
                                           for name in group if RELEASE_VC_RUNTIME.fullmatch(name)
                                           and not DEBUG_CRT.fullmatch(name)})
    return result


def image_record(data, path, origin):
    return {"path": path, "origin": origin, "size": len(data),
            "sha256": hashlib.sha256(data).hexdigest(), "pe_machine": 0x8664,
            "imports": pe_imports(data)}


def directives(args, artifacts):
    if args.cargo_json is None and args.dumpbin is None:
        return None
    if args.cargo_json is None or args.dumpbin is None:
        raise ValueError("object audit requires both --cargo-json and --dumpbin")
    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    versions = re.findall(r'(?ms)^\[\[package\]\]\s+name = "libz-ng-sys"\s+version = "([^"]+)"', lock)
    if versions != ["1.1.29"]:
        raise ValueError("object audit requires the investigated locked libz-ng-sys 1.1.29")
    encoded = artifacts.regular(args.cargo_json).read_text(encoding="utf-8-sig")
    if len(encoded) > artifacts.MAX_JSON:
        raise ValueError("Cargo JSON is oversized")
    events = [artifacts.decode_json(line) for line in encoded.splitlines() if line.strip()]
    if any(not isinstance(event, dict) for event in events):
        raise ValueError("Cargo JSON event is not an object")
    if [event.get("success") for event in events if event.get("reason") == "build-finished"] != [True]:
        raise ValueError("Cargo JSON lacks one successful build completion")
    package_id = "registry+https://github.com/rust-lang/crates.io-index#libz-ng-sys@1.1.29"
    builds = [event for event in events if event.get("reason") == "build-script-executed"
              and event.get("package_id") == package_id]
    if len(builds) != 1:
        raise ValueError("Cargo JSON does not select exactly one locked zlib-ng build")
    build = builds[0]
    selected = [name for name in build.get("linked_libs", []) if name in
                ("static=zlibstatic-ngd", "static=zlibstatic-ng")]
    if len(selected) != 1:
        raise ValueError("Cargo JSON does not select one known zlib-ng static library")
    output = artifacts.no_reparse(build["out_dir"])
    try:
        output.relative_to(artifacts.no_reparse(ROOT / "target"))
    except ValueError as error:
        raise ValueError("Cargo build output is outside this checkout's target") from error
    library = artifacts.regular(output / "lib" / (selected[0][len("static="):] + ".lib"))
    if "native=" + str(library.parent) not in build.get("linked_paths", []):
        raise ValueError("Cargo library search path differs from the selected library")
    before = artifacts.digest(library)
    dumper = artifacts.regular(args.dumpbin)
    run = subprocess.run([str(dumper), "/NOLOGO", "/DIRECTIVES", str(library)], check=True,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
    if len(run.stdout) > 32 * 1024 * 1024:
        raise ValueError("object directives are oversized")
    if before != artifacts.digest(library):
        raise ValueError("static library changed during audit")
    # DUMPBIN's headings can be localized. Directives themselves have the COFF spelling.
    raw = run.stdout.decode("utf-8", errors="replace")
    defaults = sorted({(quoted or bare).strip('"').lower() for quoted, bare in
                       re.findall(r'/DEFAULTLIB:(?:"([^"]+)"|([^\s]+))', raw, flags=re.I)})
    if not defaults:
        raise ValueError("object audit found zero compiler default-library directives")
    return {"package_id": package_id, "lock_version": versions[0],
            "library": library.relative_to(ROOT).as_posix(), "sha256": before,
            "dumpbin_sha256": artifacts.digest(dumper), "default_libraries": defaults,
            "debug_crt_requested": any(name.removesuffix(".lib") in ("msvcrtd", "libcmtd") for name in defaults),
            "raw_directives": raw, "runtime_dependency_proof": False}


def audit(args):
    artifacts = module("windows-native-artifacts")
    captured = artifacts.verify(args)
    images = []
    for item in captured["files"]:
        if item["pe_machine"] is not None:
            images.append(image_record(artifacts.regular(args.evidence / item["path"]).read_bytes(),
                                       item["path"], item["kind"]))
    if captured["package"] is not None:
        package = captured["package"]
        packager = artifacts.packager()
        verified = packager.verify(args.evidence / package["manifest"], args.evidence / package["archive"])
        members = packager.safe_members(args.evidence / package["archive"], verified["files"])
        expected = set(packager.executable_names(verified["target"]).values())
        actual = {name for name in members if name.lower().endswith((".exe", ".dll"))}
        if actual != expected:
            raise ValueError("package executable inventory is incomplete or unexpected")
        for name in sorted(expected):
            images.append(image_record(members[name][0], "package/" + name, "verified-package-executable"))
    if not images:
        raise ValueError("runtime audit selected zero images")
    source = directives(args, artifacts)
    artifacts.verify(args)
    blocked = [item["path"] for item in images if item["imports"]["debug_crt"]]
    return {"schema": SCHEMA, "source_sha": args.expected_sha,
            "cargo_lock_sha256": captured["build"]["cargo_lock_sha256"], "target": captured["build"]["target"],
            "images": images, "zlib_ng_object_directives": source,
            "dependency_gate": "red" if blocked else "green", "debug_crt_images": blocked,
            "gate_scope": "ordinary_and_delay_import_debug_crt_absence",
            "dll_resolution_and_runtime": "not_assessed", "production_release_acceptance": "not_assessed"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-sha", required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--cargo-json", type=Path)
    parser.add_argument("--dumpbin", type=Path)
    args = parser.parse_args()
    try:
        value = audit(args)
        print(json.dumps(value, indent=2) + "\n", end="")
        return 0 if value["dependency_gate"] == "green" else 1
    except (OSError, ValueError, KeyError, TypeError, struct.error, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
