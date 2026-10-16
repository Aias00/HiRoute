#!/usr/bin/env python3
"""Derive the Windows path matrix without running the current Rust producer.

The original schema-23 SQL bytes remain frozen. This independent, test-only
projection changes /opt installation paths to C:/opt and recomputes only their
specified SHA-256 proofs. It first proves those formulas match the old fixture.
It is not evidence that a Windows schema-23 release ever shipped.
"""
import hashlib
import json
from pathlib import Path

DIRECTORY = Path(__file__).resolve().parent
SOURCE_SHA = "1886b84980aab4ddb7b3cd9ed21da0a654b13474e448f02883a233dc312d043f"


def encoded(value, canonical=False):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=canonical)


def digest_bytes(value):
    return "sha256:" + hashlib.sha256(value.encode()).hexdigest()


def digest(value):
    return digest_bytes(encoded(value, canonical=True))


def map_value(value, string_map):
    if isinstance(value, list):
        return [map_value(item, string_map) for item in value]
    if isinstance(value, dict):
        return {key: map_value(item, string_map) for key, item in value.items()}
    if isinstance(value, str):
        if value.startswith("{"):
            parsed = json.loads(value)
            changed = map_value(parsed, string_map)
            # Preserve the original struct member order and JSON number tokens.
            assert encoded(parsed) == value
            return encoded(changed)
        return string_map(value)
    return value


def step_inputs(operation):
    plan = operation["plan"]
    return [
        {"spec": plan["spec"], "accepted_digest": operation["accepted_digest"],
         "expected_revisions": operation["expected_revisions"]},
        [plan["secrets"], []],
        [plan["control"], plan["credential_pool"], plan["worker_dependency_selection"]],
        [], [], plan["runtime"],
    ]


def main():
    raw = (DIRECTORY / "schema23-worker-operations.json").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == SOURCE_SHA, "frozen producer bytes changed"
    original = json.loads(raw)
    windows = map_value(original, lambda s: "C:" + s if s.startswith("/opt/") else s)
    proofs = {}
    for old_case, new_case in zip(original["cases"], windows["cases"]):
        # Selection effects hash exact serialized selection bytes, not canonical
        # object order. Verify all original before/after proofs independently.
        for old_row, new_row in zip(
            old_case["tables"]["worker_dependency_selection_effects"]["rows"],
            new_case["tables"]["worker_dependency_selection_effects"]["rows"],
        ):
            for index in (4, 7):
                if old_row[index] is not None:
                    proofs[digest_bytes(old_row[index])] = digest_bytes(new_row[index])
        for key in ("effect", "previous_effect"):
            for field in ("before_fingerprint", "after_fingerprint"):
                value = old_case[key].get(field)
                if value is not None:
                    assert value in proofs, "frozen effect selection proof changed"
        for old_row, new_row in zip(old_case["tables"]["operations"]["rows"],
                                    new_case["tables"]["operations"]["rows"]):
            old_operation, new_operation = json.loads(old_row[9]), json.loads(new_row[9])
            old_accepted = digest({"spec": old_operation["plan"]["spec"],
                                   "revisions": old_operation["expected_revisions"]})
            assert old_accepted == old_operation["accepted_digest"] == old_row[6]
            new_accepted = digest({"spec": new_operation["plan"]["spec"],
                                   "revisions": new_operation["expected_revisions"]})
            proofs[old_accepted] = new_accepted
            new_operation["accepted_digest"] = new_accepted
            for old_step, old_input, new_input in zip(old_operation["steps"],
                    step_inputs(old_operation), step_inputs(new_operation)):
                old_proof = digest(["hiroute.operation-step-input/v1", old_step["kind"], old_input])
                assert old_proof == old_step["deterministic_input_digest"]
                new_proof = digest(["hiroute.operation-step-input/v1", old_step["kind"], new_input])
                if old_proof in proofs:
                    assert proofs[old_proof] == new_proof, "ambiguous proof rewrite"
                proofs[old_proof] = new_proof
    windows = map_value(windows, lambda s: proofs.get(s, s))
    # Never attribute the derived Windows matrix to the historical product.
    windows["producer_revision"] = "test-only-windows-path-projection-of-cdbb2cec"
    result = json.dumps(windows, ensure_ascii=False, indent=2, sort_keys=True).encode()
    target = DIRECTORY / "schema23-worker-operations-windows.json"
    target.write_bytes(result)
    print(hashlib.sha256(result).hexdigest())


if __name__ == "__main__":
    main()
