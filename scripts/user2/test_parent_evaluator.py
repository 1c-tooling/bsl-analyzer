"""Model-free self-check for the frozen USER2 parent evaluator."""

from parent_evaluator import FrozenEvaluator
import json
import tempfile
from pathlib import Path


def hit(start, end, score=1.0):
    return {"root_id": "", "path": "CommonModules/M/Ext/Module.bsl", "symbol": "Target",
            "line_start": start, "line_end": end,
            "location": {"enclosing_range": {"start_line": 9, "end_line": 20}},
            "source_span": {"parent_byte_start": 40, "parent_byte_end": 100}, "score": score}


gold = {"q1": {"cf/CommonModules/M/Ext/Module.bsl::Target": 1}}
metadata = {"q1": {"gold_locations": [{"path": "cf/CommonModules/M/Ext/Module.bsl",
                                        "symbol": "Target", "start_line": 10, "end_line": 20}]}}
evaluator = FrozenEvaluator(gold, metadata)
raw = [hit(10, 12), hit(13, 20, 0.9)] + [
    {"root_id": "", "path": f"CommonModules/M{i}/Ext/Module.bsl", "symbol": f"Other{i}",
    "line_start": 1, "line_end": 2} for i in range(28)
]
public = [{"path": "public-hit"}]
result = evaluator.evaluate("q1", raw, public)
assert result["raw_hit_count"] == 30
assert result["raw_hits_used"] == 30 and result["deduplicated_parent_count"] == 29
assert len(result["ranked_doc_ids"]) == 10
assert result["ranked_doc_ids"][0] == "cf/CommonModules/M/Ext/Module.bsl::Target"
assert result["parent_ranking"][0]["matched_gold"]
assert result["public_top10"] == public
extra = {"root_id": "", "path": "CommonModules/Last/Ext/Module.bsl", "symbol": "Last", "line_start": 1, "line_end": 2}
over_limit = evaluator.evaluate("q1", raw + [extra], public)
assert over_limit["raw_hit_count"] == 31 and over_limit["raw_hits_used"] == 30
assert over_limit["ranked_doc_ids"] == result["ranked_doc_ids"]
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    qrels, summary, plan = (root / name for name in ("qrels.json", "summary.json", "plan.json"))
    qrels.write_text(json.dumps({"source_digest": "demo-digest",
        "queries": {str(i): "query" for i in range(30)},
        "qrels": {str(i): gold["q1"] for i in range(30)},
        "query_metadata": {str(i): metadata["q1"] for i in range(30)}}))
    summary.write_text(json.dumps({"sha256": "demo-digest"}))
    FrozenEvaluator.freeze(qrels, plan)
    assert FrozenEvaluator.load(qrels, summary, plan).qrels["0"] == gold["q1"]
    qrels.write_text(qrels.read_text() + "\n")
    try:
        FrozenEvaluator.load(qrels, summary, plan)
        raise AssertionError("changed qrels must refuse the frozen plan")
    except ValueError:
        pass
print("frozen parent dedup, gold location, raw cap, and independent public top-10 passed")
