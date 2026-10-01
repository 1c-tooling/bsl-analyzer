#!/usr/bin/env python3
"""Apply the locked E2 gates to completed USER2 result files."""

import json
import random
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "experiments/user2-pilot/results"


def read(name):
    path = RESULTS / name / "result.json"
    if not path.is_file():
        raise SystemExit(f"missing completed E2 result: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def macro(result):
    return float(np.mean([result["external"][key]["metrics"]["ndcg@10"] for key in ("forum", "fastcode")]))


def task(result, group):
    return result["external"][group] if group in ("forum", "fastcode") else result[group]


def paired(a, b, group):
    qa, qb = task(a, group)["queries"], task(b, group)["queries"]
    if qa.keys() != qb.keys():
        raise ValueError(f"query IDs differ for {group}")
    return [(qb[q]["metrics"]["ndcg@10"] - qa[q]["metrics"]["ndcg@10"],
             qb[q]["metrics"]["recall@10"] - qa[q]["metrics"]["recall@10"]) for q in qa]


def lower_bound(values, seed=42, repeats=1000):
    rng = random.Random(seed)
    means = [sum(values[rng.randrange(len(values))] for _ in values) / len(values) for _ in range(repeats)]
    return float(np.quantile(means, 0.025))


def compare_results(both, query, reduced, choice):
    query_only = choice["query_only_eligible"]
    prefix = choice["prefix"]
    base = query if query_only else both
    groups = {}
    passes = True
    for group in ("forum", "fastcode", "local"):
        pairs = paired(base, reduced, group)
        ndcg_delta = task(reduced, group)["metrics"]["ndcg@10"] - task(base, group)["metrics"]["ndcg@10"]
        recall_delta = task(reduced, group)["metrics"]["recall@10"] - task(base, group)["metrics"]["recall@10"]
        lower = lower_bound([row[0] for row in pairs])
        passed = ndcg_delta >= -0.01 and recall_delta >= -0.02 and lower >= -0.02
        groups[group] = {"ndcg10_delta": ndcg_delta, "recall10_delta": recall_delta, "paired_bootstrap_ndcg95_lower": lower, "queries": len(pairs), "passes": passed}
        passes &= passed
    supplemental = {}
    if "local_enriched" in base and "local_enriched" in reduced:
        pairs = paired(base, reduced, "local_enriched")
        supplemental["local_enriched"] = {
            "ndcg10_delta": task(reduced, "local_enriched")["metrics"]["ndcg@10"] - task(base, "local_enriched")["metrics"]["ndcg@10"],
            "recall10_delta": task(reduced, "local_enriched")["metrics"]["recall@10"] - task(base, "local_enriched")["metrics"]["recall@10"],
            "paired_bootstrap_ndcg95_lower": lower_bound([row[0] for row in pairs]),
            "queries": len(pairs),
            "used_for_locked_dimension_gate": False,
        }
    selection = {
        "prefix": prefix,
        "dimensions": 256 if passes else 768,
        "experimental_recommendation_only": True,
        "query_only_eligible": query_only,
        "external_macro_ndcg10_768_both": macro(both),
        "external_macro_ndcg10_768_query_only": macro(query),
        "dimension_gates": groups,
        "supplemental": supplemental,
        "bootstrap": {"resamples": 1000, "seed": 42, "lower_quantile": 0.025},
    }
    return selection


def main():
    both, query = read("768-both"), read("768-query-only")
    choice_path = RESULTS / "prefix-choice.json"
    if not choice_path.is_file():
        raise SystemExit(f"missing prefix decision: {choice_path}")
    choice = json.loads(choice_path.read_text(encoding="utf-8"))
    prefix = choice["prefix"]
    reduced = read(f"256-{prefix}")
    selection = compare_results(both, query, reduced, choice)
    output = RESULTS / "selection.json"
    if output.exists():
        raise SystemExit(f"refusing to overwrite selection: {output}")
    output.write_text(json.dumps(selection, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(selection, ensure_ascii=False))


if __name__ == "__main__":
    main()
