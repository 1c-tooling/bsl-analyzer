#!/usr/bin/env python3
"""Model-free self-test for the persisted external/local E2 result layout."""
import runpy

choose = runpy.run_path(__file__.replace("test-eval-layout.py", "choose-prefix.py"))
compare = runpy.run_path(__file__.replace("test-eval-layout.py", "compare.py"))


def result(forum, fastcode, local, enriched):
    def group(value, recall=0.6):
        return {"metrics": {"ndcg@10": value, "recall@10": recall}, "queries": {"q1": {"metrics": {"ndcg@10": value, "recall@10": recall}}}}
    return {"external": {"forum": group(forum), "fastcode": group(fastcode)}, "local": group(local), "local_enriched": group(enriched)}


both = result(0.50, 0.40, 0.50, 0.55)
query = result(0.52, 0.40, 0.50, 0.56)
choice = choose["choose_prefix"](both, query)
reduced = result(0.515, 0.395, 0.495, 0.555)
selection = compare["compare_results"](both, query, reduced, choice)
assert choice["prefix"] == "query-only" and choice["query_only_eligible"] and selection["dimensions"] == 256 and all(gate["passes"] for gate in selection["dimension_gates"].values()) and selection["supplemental"]["local_enriched"]["used_for_locked_dimension_gate"] is False and compare["paired"](both, query, "forum")[0] == (0.020000000000000018, 0.0) and compare["paired"](both, query, "local")[0] == (0.0, 0.0)
print("evaluation result layout self-test passed")
