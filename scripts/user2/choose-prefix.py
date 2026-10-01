#!/usr/bin/env python3
"""Choose the 256d document prefix only after both 768d runs complete."""

import json
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "experiments/user2-pilot/results"


def read(name):
    path = RESULTS / name / "result.json"
    if not path.is_file():
        raise SystemExit(f"missing completed E2 result: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def task(result, group):
    return result["external"][group] if group in ("forum", "fastcode") else result[group]


def choose_prefix(both, query):
    macro_both = float(np.mean([both["external"][x]["metrics"]["ndcg@10"] for x in ("forum", "fastcode")]))
    macro_query = float(np.mean([query["external"][x]["metrics"]["ndcg@10"] for x in ("forum", "fastcode")]))
    gains = {
        group: task(query, group)["metrics"]["ndcg@10"] - task(both, group)["metrics"]["ndcg@10"]
        for group in ("forum", "fastcode", "local")
    }
    eligible = macro_query - macro_both >= 0.005 and all(gains[group] >= -0.01 for group in gains)
    choice = {
        "prefix": "query-only" if eligible else "both",
        "query_only_eligible": eligible,
        "experimental_only": True,
        "macro_ndcg10_768_both": macro_both,
        "macro_ndcg10_768_query_only": macro_query,
        "ndcg10_gain_by_slice": gains,
        "candidate_256_variant": "256-query-only" if eligible else "256-both",
    }
    if "local_enriched" in both and "local_enriched" in query:
        choice["supplemental_local_enriched"] = {
            "ndcg10_both": task(both, "local_enriched")["metrics"]["ndcg@10"],
            "ndcg10_query_only": task(query, "local_enriched")["metrics"]["ndcg@10"],
            "ndcg10_gain_query_only": task(query, "local_enriched")["metrics"]["ndcg@10"] - task(both, "local_enriched")["metrics"]["ndcg@10"],
            "used_for_locked_prefix_gate": False,
        }
    return choice


def main():
    output = RESULTS / "prefix-choice.json"
    if output.exists():
        raise SystemExit(f"refusing to overwrite prefix choice: {output}")
    choice = choose_prefix(read("768-both"), read("768-query-only"))
    output.write_text(json.dumps(choice, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(choice, ensure_ascii=False))


if __name__ == "__main__":
    main()
