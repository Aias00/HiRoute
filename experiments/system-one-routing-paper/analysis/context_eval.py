"""Provider-independent, offline scoring for contextual routing decisions.

This module never calls models and never derives normative action gold from a
router's combining formula. Language pairs and repeats remain within families.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import math
from pathlib import Path
import random
import statistics


def strict_json(text):
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise ValueError(f"duplicate JSON key: {key}")
            out[key] = value
        return out
    def invalid(value):
        raise ValueError(f"nonfinite JSON value: {value}")
    def finite_float(value):
        number = float(value)
        if not math.isfinite(number):
            raise ValueError("nonfinite JSON number")
        return number
    return json.loads(text, object_pairs_hook=pairs,
                      parse_constant=invalid, parse_float=finite_float)


def load_jsonl(path):
    rows = []
    for line, text in enumerate(Path(path).read_text().splitlines(), 1):
        if not text.strip():
            continue
        try:
            row = strict_json(text)
        except (ValueError, TypeError) as exc:
            raise ValueError(f"{path}:{line}: {exc}") from exc
        if not isinstance(row, dict):
            raise ValueError(f"{path}:{line}: expected an object")
        rows.append(row)
    return rows


def unique_index(rows, key="id"):
    out = {}
    for row in rows:
        ident = row[key]
        if ident in out:
            raise ValueError(f"duplicate {key}: {ident}")
        out[ident] = row
    return out


def canonical_hash(value):
    data = json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode()
    return hashlib.sha256(data).hexdigest()


def validate_dataset(inputs, labels):
    ii, ll = unique_index(inputs), unique_index(labels)
    if set(ii) != set(ll):
        raise ValueError("input and label ids differ")
    pairs = defaultdict(list)
    for ident, row in ii.items():
        if set(row) != {"id", "conversation"}:
            raise ValueError(f"unexpected input fields: {ident}")
        conv = row["conversation"]
        if set(conv) != {"prior_turns", "assessment_stage", "current_request"}:
            raise ValueError(f"unexpected conversation fields: {ident}")
        if not isinstance(conv["prior_turns"], list):
            raise ValueError(f"prior turns must be a list: {ident}")
        groups = []
        for turn in conv["prior_turns"]:
            if not isinstance(turn, dict) or set(turn) != {"messages"}:
                raise ValueError(f"invalid prior turn: {ident}")
            groups.append(turn["messages"])
        groups.append(conv["assessment_stage"])
        for messages in groups:
            if not isinstance(messages, list) or not messages:
                raise ValueError(f"empty or invalid message group: {ident}")
            for msg in messages:
                if not isinstance(msg, dict) or set(msg) != {"role", "content"}:
                    raise ValueError(f"invalid message fields: {ident}")
                if msg["role"] not in {"user", "assistant"}:
                    raise ValueError(f"invalid message role: {ident}")
                if not isinstance(msg["content"], str) or not msg["content"].strip():
                    raise ValueError(f"empty message: {ident}")
        if not isinstance(conv["current_request"], str) or not conv["current_request"].strip():
            raise ValueError(f"empty current request: {ident}")
        lab = ll[ident]
        if lab["language"] not in {"en", "zh"}:
            raise ValueError(f"unknown language: {ident}")
        if lab["current_demand"] not in {"routine", "reasoning", "underdetermined"}:
            raise ValueError(f"unknown demand: {ident}")
        if lab["stage_grade"] is not None and (type(lab["stage_grade"]) is not int or lab["stage_grade"] not in {0, 1, 2}):
            raise ValueError(f"invalid grade: {ident}")
        allowed = lab.get("allowed_grades")
        if not isinstance(allowed, list) or any(type(x) is not int or x not in {0, 1, 2} for x in allowed) or len(set(allowed)) != len(allowed):
            raise ValueError(f"invalid allowed grades: {ident}")
        if lab["stage_grade"] is not None and allowed != [lab["stage_grade"]]:
            raise ValueError(f"unique grade must have matching singleton: {ident}")
        if lab["stage_grade"] is None and len(allowed) == 1:
            raise ValueError(f"singleton grade must be explicit: {ident}")
        if lab["applicability"] not in {"applicable", "not_applicable", "underdetermined"}:
            raise ValueError(f"unknown applicability: {ident}")
        if lab["action_status"] not in {"determinate", "conditional", "insufficient"}:
            raise ValueError(f"unknown action status: {ident}")
        if lab["action_status"] == "determinate":
            if lab["action"] not in {"economy", "primary"}:
                raise ValueError(f"determinate action missing: {ident}")
            if lab.get("allowed_actions") != [lab["action"]]:
                raise ValueError(f"unique action must have matching singleton: {ident}")
        elif lab["action"] is not None:
            raise ValueError(f"non-determinate action must be null: {ident}")
        elif lab["action_status"] == "conditional":
            if sorted(lab.get("allowed_actions", [])) != ["economy", "primary"] or not isinstance(lab.get("action_condition"), str) or not lab["action_condition"].strip():
                raise ValueError(f"conditional action needs two branches and condition: {ident}")
        elif lab.get("allowed_actions") != []:
            raise ValueError(f"insufficient action must have empty action set: {ident}")
        if not isinstance(lab.get("rationale"), str) or not lab["rationale"].strip():
            raise ValueError(f"missing label rationale: {ident}")
        if lab.get("mode") not in {"human_feedback", "autonomous_continuation"} or lab.get("panel") not in {"clear", "boundary", "insufficient"}:
            raise ValueError(f"invalid mode or panel: {ident}")
        if not isinstance(lab.get("evidence"), dict) or set(lab["evidence"]) != set(METRICS):
            raise ValueError(f"evidence must cover the four targets: {ident}")
        text = "\n".join(m["content"] for g in groups for m in g) + "\n" + conv["current_request"]
        for spans in lab.get("evidence", {}).values():
            if not isinstance(spans, list) or any(not isinstance(s, str) or not s or s not in text for s in spans):
                raise ValueError(f"evidence span not found: {ident}")
        pairs[lab["pair_id"]].append(lab)
    for pair, rows in pairs.items():
        if len(rows) != 2 or {r["language"] for r in rows} != {"en", "zh"}:
            raise ValueError(f"invalid bilingual pair: {pair}")
        if len({r["family_id"] for r in rows}) != 1:
            raise ValueError(f"pair crosses families: {pair}")
    return {"records": len(ii), "pairs": len(pairs),
            "families": len({x["family_id"] for x in labels}),
            "input_sha256": canonical_hash(inputs),
            "labels_sha256": canonical_hash(labels)}


def routing_rule(p_simple, historical_score, p_applicable, *, threshold=0.5,
                 history_eligible=True, use_history=True, use_applicability=True):
    """Reference rule only. Never use this function to create action gold.

    Historical score is the raw 0..2 scalar. It is NOT the modal grade.
    Missing applicability conservatively preserves an otherwise open history gate.
    """
    def finite_range(x, lo, hi):
        return type(x) in (int, float) and math.isfinite(x) and lo <= x <= hi
    if not finite_range(threshold, 0, 1):
        raise ValueError("invalid simple threshold")
    if not finite_range(p_simple, 0, 1) or p_simple < threshold:
        return "primary"
    if not use_history or not history_eligible or not finite_range(historical_score, 0, 2):
        return "economy"
    if historical_score >= 1:
        return "economy"
    if not use_applicability or not finite_range(p_applicable, 0, 1):
        return "primary"
    return "primary" if p_applicable >= 0.5 else "economy"


METRICS = ("current_demand", "stage_grade", "applicability", "action")


def gold_for(label, metric):
    if metric == "action":
        return label[metric] if label["action_status"] == "determinate" else None
    value = label[metric]
    return None if value == "underdetermined" else value


def metric_valid(prediction, metric):
    """A malformed other answer must not erase a valid per-question result.

    Explicit per-metric validity is optional; whole-response validity is the
    fallback. End-to-end action must have its own validity when salvaging parts.
    """
    validity = prediction.get("metric_valid", {})
    value = validity.get(metric, prediction.get("valid", False))
    if type(value) is not bool:
        raise ValueError("metric validity must be boolean")
    return value and metric in prediction


def summarize(labels, predictions, *, repeats=1, metrics=METRICS):
    """One method per call. Missing/invalid responses count as end-to-end failures."""
    if type(repeats) is not int or repeats < 1:
        raise ValueError("repeats must be a positive integer")
    if not metrics or len(set(metrics)) != len(metrics) or any(m not in METRICS for m in metrics):
        raise ValueError("choose unique supported metrics")
    gold = unique_index(labels)
    seen = {}
    for pred in predictions:
        key = (pred["id"], pred.get("repeat", 0))
        if key in seen:
            raise ValueError(f"duplicate prediction: {key}")
        if key[0] not in gold or type(key[1]) is not int or not 0 <= key[1] < repeats:
            raise ValueError(f"unexpected prediction: {key}")
        if type(pred.get("valid")) is not bool:
            raise ValueError(f"valid must be explicit boolean: {key}")
        seen[key] = pred
    total = len(labels) * repeats
    out = {"planned_responses": total, "observed_responses": len(seen),
           "valid_responses": sum(p["valid"] for p in seen.values()),
           "missing_responses": total-len(seen), "metrics": {}}
    for metric in metrics:
        family = defaultdict(list)
        valid_family = defaultdict(list)
        class_family = defaultdict(lambda: defaultdict(list))
        confusion = defaultdict(Counter)
        eligible_records = 0
        correct = eligible_calls = valid_calls = valid_correct = 0
        for lab in labels:
            target = gold_for(lab, metric)
            if target is None:
                continue
            eligible_records += 1
            for repeat in range(repeats):
                p = seen.get((lab["id"], repeat), {})
                valid = metric_valid(p, metric)
                result = p.get(metric) if valid else "invalid_or_missing"
                score = int(valid and type(result) is type(target) and result == target)
                family[lab["family_id"]].append(score)
                class_family[str(target)][lab["family_id"]].append(score)
                confusion[str(target)][str(result)] += 1
                eligible_calls += 1
                correct += score
                if valid:
                    valid_calls += 1
                    valid_correct += score
                    valid_family[lab["family_id"]].append(score)
        by_class = {key: statistics.mean(statistics.mean(xs) for xs in fam.values())
                    for key, fam in class_family.items()}
        out["metrics"][metric] = {
            "eligible_records": eligible_records,
            "record_coverage": eligible_records / len(labels) if labels else None,
            "eligible_calls": eligible_calls, "correct_calls": correct,
            "record_accuracy": correct/eligible_calls if eligible_calls else None,
            "family_accuracy": statistics.mean(map(statistics.mean, family.values())) if family else None,
            "family_count": len(family),
            "valid_output_calls": valid_calls,
            "valid_output_record_accuracy": valid_correct/valid_calls if valid_calls else None,
            "valid_output_family_accuracy": statistics.mean(map(statistics.mean, valid_family.values())) if valid_family else None,
            "family_recall_by_class": by_class,
            "macro_family_recall": statistics.mean(by_class.values()) if by_class else None,
            "confusion": {k: dict(v) for k, v in confusion.items()},
        }
    if "action" not in metrics:
        return out
    conditional = [l for l in labels if l["action_status"] != "determinate"]
    out["non_determinate_actions"] = dict(Counter(
        seen.get((l["id"], repeat), {}).get("action", "invalid_or_missing")
        if metric_valid(seen.get((l["id"], repeat), {}), "action") else "invalid_or_missing"
        for l in conditional for repeat in range(repeats)))
    out["non_determinate_actions_by_status"] = {
        status: dict(Counter(
            seen.get((l["id"], repeat), {}).get("action", "invalid_or_missing")
            if metric_valid(seen.get((l["id"], repeat), {}), "action") else "invalid_or_missing"
            for l in conditional if l["action_status"] == status for repeat in range(repeats)))
        for status in ("conditional", "insufficient")}
    return out


def paired_comparison(labels, left, right, *, metric="action", repeats=1):
    """Right minus left, keeping all planned calls including failures.

    Calls are never independent bootstrap units: a whole workflow family is.
    Reuse summarize for duplicate, range and explicit-validity checks.
    """
    summarize(labels, left, repeats=repeats, metrics=(metric,))
    summarize(labels, right, repeats=repeats, metrics=(metric,))
    li = {(p["id"], p.get("repeat", 0)): p for p in left}
    ri = {(p["id"], p.get("repeat", 0)): p for p in right}
    delta = defaultdict(list)
    changes = Counter()
    for lab in labels:
        target = gold_for(lab, metric)
        if target is None:
            continue
        for rep in range(repeats):
            flags = []
            for source in (li, ri):
                pred = source.get((lab["id"], rep), {})
                flags.append(int(metric_valid(pred, metric) and type(pred[metric]) is type(target) and pred[metric] == target))
            delta[lab["family_id"]].append(flags[1]-flags[0])
            changes[{(0, 0): "both_wrong", (0, 1): "improved", (1, 0): "regressed", (1, 1): "both_correct"}[tuple(flags)]] += 1
    return {"metric": metric, "direction": "right_minus_left", "call_changes": dict(changes),
            **paired_family_interval(delta)}


def paired_family_interval(deltas_by_family, *, draws=10000, seed=20261018):
    """Descriptive paired cluster bootstrap of family means; no equivalence claim."""
    means = [statistics.mean(xs) for xs in deltas_by_family.values() if xs]
    if not means:
        return {"family_count": 0, "difference": None, "ci95": None}
    if len(means) == 1:
        return {"family_count": 1, "difference": means[0], "ci95": None}
    rng = random.Random(seed)
    boot = sorted(statistics.mean(rng.choices(means, k=len(means))) for _ in range(draws))
    def quantile(q):
        x = (len(boot)-1)*q
        lo = int(x)
        return boot[lo] + (boot[min(lo+1, len(boot)-1)]-boot[lo])*(x-lo)
    return {"family_count": len(means), "difference": statistics.mean(means),
            "ci95": [quantile(.025), quantile(.975)], "draws": draws, "seed": seed}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--labels", required=True)
    parser.add_argument("--inputs")
    parser.add_argument("--predictions")
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--metrics", nargs="+", choices=METRICS, default=METRICS)
    parser.add_argument("--out")
    args = parser.parse_args()
    labels = load_jsonl(args.labels)
    result = {}
    if args.inputs:
        result["dataset"] = validate_dataset(load_jsonl(args.inputs), labels)
    if args.predictions:
        result["scores"] = summarize(labels, load_jsonl(args.predictions), repeats=args.repeats, metrics=args.metrics)
    text = json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False)+"\n"
    if args.out:
        Path(args.out).write_text(text)
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
