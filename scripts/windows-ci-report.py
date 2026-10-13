#!/usr/bin/env python3
"""Attach existing validation feedback to native Windows case records."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil

spec = importlib.util.spec_from_file_location(
    "validation_report", Path(__file__).with_name("validation-report.py"))
reporting = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reporting)


def collect(evidence):
    provenance = json.loads((evidence / "provenance.json").read_text(encoding="utf-8-sig"))
    reports = []
    prerequisites = []
    for source in sorted(evidence.glob("*-result.json")):
        if source.name in {"package-result.json", "clients-result.json"}:
            prerequisites.append(dict(name=source.stem, result_present=bool(source.stat().st_size)))
            continue  # Failed prerequisite commands may leave empty redirected output.
        native = json.loads(source.read_text(encoding="utf-8-sig"))
        if "cases" not in native:
            continue  # Package provenance is separate from Rust scenario execution.
        if native["source_revision"] != provenance["source_revision"]:
            raise ValueError("Native case record belongs to another candidate")
        run = evidence / "feedback" / native["name"]
        run.mkdir(parents=True, exist_ok=True)
        log = evidence / (native["name"] + ".log")
        if log.exists():
            shutil.copyfile(log, run / "command.log")
        record = dict(native, id=native["name"], revision=native["source_revision"],
                      status="completed" if native["scenario"] == "green" else "failed",
                      platform="win32", architecture=provenance["host_architecture"],
                      cargo=provenance["cargo"], rustc=provenance["rustc"],
                      command=native.get("command", []),
                      # External Actions IDs are provenance links, not local reports.
                      related_ci_runs=native.get("related_runs", []), related_runs=[])
        feedback = reporting.write_report(run, record)
        (run / "result.json").write_text(json.dumps(record, indent=2) + "\n")
        reports.append(dict(name=native["name"], scenario=native["scenario"], **feedback))
    aggregate = dict(source_revision=provenance["source_revision"], plan=provenance["plan"],
                     phase=provenance["phase"], related_ci_runs=provenance["related_runs"],
                     reports=reports, prerequisites=prerequisites,
                     evidence_limit="Selected native cases only; missing reports are not passing cases.")
    (evidence / "validation-feedback.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    return aggregate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    print(json.dumps(collect(args.evidence), indent=2))


if __name__ == "__main__":
    main()
