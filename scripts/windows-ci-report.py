#!/usr/bin/env python3
"""Attach existing validation feedback to native Windows case records."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil

spec = importlib.util.spec_from_file_location(
    "validation_report", Path(__file__).with_name("validation-report.py"))
reporting = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reporting)


def read_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate native evidence field')
            result[key] = value
        return result
    return json.loads(Path(path).read_text(encoding='utf-8-sig'), object_pairs_hook=unique)


def names(value):
    if (not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value)
            or len(value) != len(set(value))):
        raise ValueError('Nonempty unique native recipe names required')
    return value


def verify_artifacts(bundle, revision):
    """Use the original source/PE/package verifier, never a report-only adapter."""
    spec = importlib.util.spec_from_file_location('windows_feedback_artifacts', Path(__file__).with_name('windows-native-artifacts.py'))
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    return native.verify(argparse.Namespace(expected_sha=revision, evidence=bundle))


def assessment(provenance, records, evidence):
    mode = provenance.get('assessment')
    result = dict(assessment=mode or 'legacy', full_scope=False, candidate_complete=False,
                  background_acceptance='not_assessed', runtime_recipes=[], deferred_recipes=[])
    # Existing private focused drivers intentionally select a subset and predate
    # assessment. Their original native verdicts survive without invented scope.
    if mode is None:
        return result
    if mode not in ('full', 'hosted'):
        raise ValueError('Unknown native assessment')
    runtime, deferred = names(provenance.get('runtime_recipes')), names(provenance.get('deferred_recipes'))
    expected = runtime + deferred
    if not expected or len(expected) != len(set(expected)) or (mode == 'full' and deferred):
        raise ValueError('Complete disjoint native assessment inventory required')
    if mode == 'hosted' and (provenance.get('scope') != 'background' or not runtime or not deferred):
        raise ValueError('Hosted assessment is limited to complete background capture')
    if provenance.get('full_scope') is not (mode == 'full') or provenance.get('candidate_complete') is not False:
        raise ValueError('Assessment cannot claim complete candidate acceptance')
    if len(records) != len(expected) or {row['name'] for row in records} != set(expected):
        raise ValueError('Missing duplicate or unknown required native assessment record')
    result.update(runtime_recipes=runtime, deferred_recipes=deferred)
    failed = False
    by_name = {row['name']: row for row in records}
    for row in records:
        if row.get('assessment') != mode or row.get('scenario') not in ('green', 'red', 'not_assessed'):
            raise ValueError('Native record assessment differs')
        if not isinstance(row.get('cases'), list) or not row['cases'] or len(row['cases']) != len(set(row['cases'])):
            raise ValueError('Nonempty unique required native cases required')
        if type(row.get('executed')) is not int or row['executed'] < 0:
            raise ValueError('Actual native execution count required')
        if row['scenario'] == 'red':
            failed = True
            continue  # Preserve failed compilation/prerequisites; never relabel them deferred.
        if row['name'] in deferred:
            if (row['scenario'] != 'not_assessed' or row.get('runtime_executed') is not False
                    or type(row.get('executed')) is not int or row['executed'] != 0
                    or row.get('process_exit') is not None or type(row.get('ignored')) is not int or row['ignored'] != 0
                    or row.get('build_status') != 'compiled' or row.get('inventory_verified') is not True
                    or type(row.get('inventory_exit')) is not int or row['inventory_exit'] != 0
                    or row.get('artifact_capture_verified') is not True
                    or row.get('deferred_reason') != 'requires_real_logged_owner'
                    or any(key in row for key in ('tests', 'command_started_at', 'command_finished_at'))):
                raise ValueError('Deferred native record must prove capture/list and zero body execution')
        elif (row['scenario'] != 'green' or row.get('runtime_executed') is not True
                or type(row.get('process_exit')) is not int or row['process_exit'] != 0
                or type(row.get('executed')) is not int or row['executed'] != len(row['cases'])
                or type(row.get('ignored')) is not int or row['ignored'] != 0 or row.get('inventory_verified') is not True
                or type(row.get('inventory_exit')) is not int or row['inventory_exit'] != 0 or row.get('build_status') != 'compiled'
                or mode == 'hosted' and row.get('artifact_capture_verified') is not True):
            raise ValueError('Executed native record requires all real nonignored cases')
    outcome = read_json(evidence / 'scope-outcome.json')
    if (outcome.get('schema') != 'hiroute.windows-native-scope-outcome/v1'
            or outcome.get('source_revision') != provenance['source_revision']
            or outcome.get('assessment') != mode or outcome.get('scope') != provenance.get('scope')
            or type(outcome.get('failure_count')) is not int or outcome['failure_count'] < 0
            or type(outcome.get('invocation_failed')) is not bool):
        raise ValueError('Native scope outcome differs')
    failed = failed or outcome['invocation_failed'] or outcome['failure_count'] > 0
    if mode == 'hosted' and not failed:
        if (outcome.get('capture_requested') is not True or outcome.get('artifact_verification_passed') is not True
                or outcome.get('runtime_import_audit_passed') is not True):
            raise ValueError('Hosted assessment requires finalized verified artifacts and imports')
        bundle = Path(outcome.get('native_bundle', evidence))
        manifest = verify_artifacts(bundle, provenance['source_revision'])
        if (manifest.get('package') is None or not {'hiroute', 'hirouted'}.issubset(manifest.get('product_binaries', {}))
                or len(manifest['recipes']) != len(expected)
                or {row['name'] for row in manifest['recipes']} != set(expected)
                or hashlib.sha256((bundle / 'native-artifacts.json').read_bytes()).hexdigest() != outcome.get('native_manifest_sha256')):
            raise ValueError('Complete original background artifacts required')
        for recipe in manifest['recipes']:
            if by_name[recipe['name']]['cases'] != recipe['required_cases']:
                raise ValueError('Native case record differs from captured required cases')
        verified = read_json(evidence / 'artifact-verification.json')
        audited = read_json(evidence / 'runtime-audit.json')
        # The canonical CLI emits schema/source_sha/recipes. The in-process
        # verifier above checks every manifest file and complete package byte.
        if (set(verified) != {'schema', 'source_sha', 'recipes'}
                or verified.get('schema') != manifest.get('schema')
                or verified.get('source_sha') != provenance['source_revision'] or verified.get('recipes') != len(expected)
                or audited.get('source_sha') != provenance['source_revision']
                or audited.get('dependency_gate') != 'green' or audited.get('dll_resolution_and_runtime') != 'not_assessed'
                or audited.get('production_release_acceptance') != 'not_assessed'):
            raise ValueError('Native artifact verification or import audit differs')
    result.update(scenario='red' if failed else ('not_assessed' if deferred else 'green'),
                  assessment_checks_passed=not failed, full_scope=(mode == 'full' and not failed),
                  runtime_cases_executed=sum(row.get('executed', 0) for row in records if row['name'] in runtime),
                  deferred_cases=sum(len(by_name[name]['cases']) for name in deferred))
    if provenance.get('scope') == 'background' and mode == 'full':
        result['background_acceptance'] = 'red' if failed else 'green'
    return result


def collect(evidence):
    provenance = read_json(evidence / 'provenance.json')
    reports = []
    prerequisites = []
    records = []
    for source in sorted(evidence.glob("*-result.json")):
        if source.name in {"package-result.json", "clients-result.json"}:
            prerequisites.append(dict(name=source.stem, result_present=bool(source.stat().st_size)))
            continue  # Failed prerequisite commands may leave empty redirected output.
        native = read_json(source)
        if "cases" not in native:
            continue  # Package provenance is separate from Rust scenario execution.
        if native["source_revision"] != provenance["source_revision"]:
            raise ValueError("Native case record belongs to another candidate")
        records.append(native)
    derived = assessment(provenance, records, evidence)
    for native in records:
        run = evidence / "feedback" / native["name"]
        run.mkdir(parents=True, exist_ok=True)
        log = evidence / (native["name"] + ".log")
        if log.exists():
            shutil.copyfile(log, run / "command.log")
        record = dict(native, id=native["name"], revision=native["source_revision"],
                      status=('completed' if native['scenario'] == 'green' else
                              'not_assessed' if native['scenario'] == 'not_assessed' else 'failed'),
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
                     evidence_limit="Selected native cases only; missing, compiled-only and deferred reports are not passing business cases.",
                     **derived)
    (evidence / "validation-feedback.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    return aggregate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    print(json.dumps(collect(args.evidence), indent=2))


if __name__ == "__main__":
    main()
