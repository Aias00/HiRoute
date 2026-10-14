#!/usr/bin/env python3
"""Bounded manual GC using local-rust's existing preview/apply safety policy.

Preview is the default. Age never overrides keep, evidence, identity or ownership
checks. For modified sources or remounted volumes, use owner-led debug retirement
as documented in docs/local-rust-validation.md instead of relaxing these checks.
"""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


spec = importlib.util.spec_from_file_location(
    "hiroute_local_rust_gc_runner", Path(__file__).with_name("local-rust.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
ID = re.compile(r"[0-9a-f]{32}\Z")


def sweep(store, apply=False, older_than_hours=48, limit=None):
    if older_than_hours < 48 or limit is not None and limit < 1:
        raise ValueError("Age must be at least 48 hours and limit must be positive")
    cutoff = time.time() - older_than_hours * 3600
    selected, retained, removed = [], [], []
    for path in sorted((store.root / "runs").glob("*/result.json")):
        rid = path.parent.name
        if not ID.fullmatch(rid):
            continue
        try:
            row = store.load(rid)
            if row.get("kind") != "managed" or row.get("status") != "terminal" or row.get("removed"):
                continue
            finished = row.get("finished_at")
            if (isinstance(finished, bool) or not isinstance(finished, (int, float))
                    or not math.isfinite(finished)):
                raise ValueError("Missing or invalid finish time")
            if finished >= cutoff:
                continue
            if row.get("id") != rid:
                raise ValueError("Record ID does not match its directory")
            candidate = store.preview_one(row)
            if candidate["state"] != "candidate":
                retained.append(dict(id=rid, state="retained", reason=candidate["reason"]))
                continue
            selected.append(rid)
            if apply:
                # The shared owner re-reads the record, checks the snapshot token,
                # exports evidence and holds both locks throughout deletion.
                removed.append(store.apply(rid, candidate["token"])["id"])
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
            retained.append(dict(id=rid, state="retained", reason=str(error)))
        if limit is not None and len(selected) >= limit:
            break  # Bound attempts, including candidates refused during apply.
    return dict(mode="apply" if apply else "preview", older_than_hours=older_than_hours,
                selected=selected, removed=removed, retained=retained,
                free_gib=round(shutil.disk_usage(store.root).free / 1024 ** 3, 1))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="Apply the shared cleanup policy")
    parser.add_argument("--older-than-hours", type=int, default=48)
    parser.add_argument("--limit", type=int, help="Maximum candidates attempted in this pass")
    args = parser.parse_args()
    if args.older_than_hours < 48 or args.limit is not None and args.limit < 1:
        parser.error("Age must be at least 48 hours and limit must be positive")
    print(json.dumps(sweep(runner.Store(), args.apply, args.older_than_hours, args.limit),
                     ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
