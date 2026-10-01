"""Frozen parent-aware ranking rules for the USER2 UNF native acceptance."""

import hashlib
import json
from pathlib import Path

SOURCE_SHA256 = "72597d03af58b68ef7957cfa1a03561503fb7df9455117e2f4803364729dad22"
QRELS_SHA256 = "1898d4ffb77660c24425aa74eee435f09bfb67f4174bce392111b8602bf7bac6"
RAW_LIMIT = 30
K = 10
FROZEN_RETRIEVAL = {
    "raw_request_limit": 30,
    "selection": "first_30_raw_mcp_hits_in_returned_order",
    "deduplication": "stable_first_occurrence_by_root_path_original_parent_symbol_parent_location",
    "parent_location": "source_span_parent_byte_start_end; fallback_to_parent_or_enclosing_line_range",
    "evaluated_rank": "first_10_distinct_parents",
    "gold_match": "canonical_root_path_and_original_parent_symbol_and_overlapping_parent_line_range",
    "public_evidence": "separate_search_code_limit_10_request_preserved_without_evaluator_collapse",
}
FROZEN_RUNTIME = {
    "opt_in_flag": "--parent-aware",
    "policy_modes": ["off", "on"],
    "same_evaluator_for_both_modes": True,
    "public_top10_latency_gate_seconds": 12,
    "quality_threshold": None,
    "default_switch": False,
}


def _sha256(data):
    return hashlib.sha256(data).hexdigest()


def _path(hit):
    path = str(hit.get("path", "")).replace("\\", "/").strip("/")
    root = str(hit.get("root_id") or "")
    if not root:
        root = path.split("/", 1)[0] if path.startswith(("cf/", "cfe/")) else "cf"
    return path if path.startswith(root + "/") else f"{root}/{path}"


def _symbol(hit):
    span = hit.get("source_span") or {}
    return str(hit.get("original_symbol") or hit.get("parent_symbol") or span.get("parent_symbol") or hit.get("symbol") or "")


def _parent_location(hit):
    span = hit.get("source_span") or {}
    if span.get("parent_byte_start") is not None and span.get("parent_byte_end") is not None:
        return ("bytes", span["parent_byte_start"], span["parent_byte_end"])
    location = hit.get("location") or {}
    enclosing = location.get("enclosing_range") or {}
    start = span.get("parent_line_start", enclosing.get("start_line", hit.get("line_start")))
    end = span.get("parent_line_end", enclosing.get("end_line", hit.get("line_end")))
    return ("lines", start, end)


def _line_range(hit):
    span = hit.get("source_span") or {}
    location = hit.get("location") or {}
    enclosing = location.get("enclosing_range") or {}
    start = span.get("parent_line_start", enclosing.get("start_line", hit.get("line_start")))
    end = span.get("parent_line_end", enclosing.get("end_line", hit.get("line_end")))
    return start, end


class FrozenEvaluator:
    def __init__(self, qrels, metadata):
        self.qrels = qrels
        self.metadata = metadata

    @classmethod
    def load(cls, qrels_path, summary_path, plan_path):
        raw = Path(qrels_path).read_bytes()
        bundle = json.loads(raw)
        summary = json.loads(Path(summary_path).read_text(encoding="utf-8"))
        plan = json.loads(Path(plan_path).read_text(encoding="utf-8"))
        if _sha256(raw) != plan.get("qrels_sha256"):
            raise ValueError("qrels differ from the pre-vector frozen evaluator input")
        source_digest = summary.get("source_sha256", summary.get("sha256"))
        if source_digest != plan.get("source_sha256") or bundle.get("source_digest") != source_digest:
            raise ValueError("source digest differs from the pre-vector frozen evaluator input")
        if summary.get("qrels_sha256", _sha256(raw)) != _sha256(raw):
            raise ValueError("corpus summary qrels digest differs from the frozen evaluator input")
        if len(bundle.get("queries", {})) != 30 or len(bundle.get("qrels", {})) != 30:
            raise ValueError("frozen evaluator requires its exact set of 30 questions")
        if (plan.get("evaluator_sha256") != _sha256(Path(__file__).read_bytes())
                or plan.get("retrieval") != FROZEN_RETRIEVAL or plan.get("runtime") != FROZEN_RUNTIME):
            raise ValueError("frozen evaluator plan does not match source, qrels, and evaluator")
        return cls(bundle["qrels"], bundle["query_metadata"])

    @staticmethod
    def freeze(qrels_path, plan_path):
        """Write a new plan before creating candidate vectors; existing plans stay intact."""
        raw = Path(qrels_path).read_bytes()
        bundle = json.loads(raw)
        if len(bundle["queries"]) != 30 or any(
                not entry.get("gold_locations") for entry in bundle["query_metadata"].values()):
            raise ValueError("freezing requires 30 questions and source gold locations")
        plan = {"source_sha256": bundle["source_digest"], "qrels_sha256": _sha256(raw),
                "evaluator_sha256": _sha256(Path(__file__).read_bytes()),
                "retrieval": FROZEN_RETRIEVAL, "runtime": FROZEN_RUNTIME}
        with Path(plan_path).open("x") as output:
            json.dump(plan, output, ensure_ascii=False, indent=2)
            output.write("\n")

    def evaluate(self, qid, raw_hits, public_hits):
        unique, seen = [], set()
        for hit in raw_hits[:RAW_LIMIT]:
            key = (_path(hit), _symbol(hit), _parent_location(hit))
            if key not in seen:
                seen.add(key)
                unique.append(hit)
        parents = unique[:K]
        gold = self.metadata[qid]["gold_locations"]
        ranked_doc_ids = []
        evidence = []
        for hit in parents:
            path, symbol = _path(hit), _symbol(hit)
            start, end = _line_range(hit)
            matched = next((item for item in gold
                            if item["path"] == path and item["symbol"] == symbol
                            and start is not None and end is not None
                            and start <= item["end_line"] and item["start_line"] <= end), None)
            doc_id = next((key for key in self.qrels[qid] if key.rsplit("::", 1)[0] == path
                           and key.rsplit("::", 1)[-1] == symbol), None) if matched else None
            if doc_id is None:
                doc_id = f"{path}::{symbol}@{json.dumps(_parent_location(hit), separators=(',', ':'))}"
            ranked_doc_ids.append(doc_id)
            evidence.append({"doc_id": doc_id, "path": path, "symbol": symbol,
                             "parent_location": _parent_location(hit), "matched_gold": bool(matched)})
        return {"ranked_doc_ids": ranked_doc_ids, "parent_ranking": evidence,
                "raw_hit_count": len(raw_hits), "raw_hits_used": min(len(raw_hits), RAW_LIMIT),
                "deduplicated_parent_count": len(unique),
                "public_top10": public_hits[:K]}


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Freeze evaluator inputs before candidate vectors")
    parser.add_argument("--qrels", required=True, type=Path)
    parser.add_argument("--plan", required=True, type=Path)
    args = parser.parse_args()
    FrozenEvaluator.freeze(args.qrels, args.plan)
