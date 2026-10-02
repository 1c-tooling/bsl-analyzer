#!/usr/bin/env python3
"""Count exact external USER2 input token lengths before any evaluation request."""
import json
from pathlib import Path

from datasets import load_dataset
from transformers import AutoTokenizer

ROOT = Path(__file__).resolve().parents[2]
PILOT = ROOT / "experiments/user2-pilot"
MODEL_REVISION = "b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4"
DATASET_REVISION = "ae3af100584c51dcd43ff927b726bd27ba42d769"
DATASET = "PruhaNLP/1C-Ebench"
LIMIT = 8192


def summarize(ids, lengths):
    maximum = max(lengths, default=0)
    over = [(str(item), int(n)) for item, n in zip(ids, lengths) if n > LIMIT]
    return {
        "count": len(lengths),
        "max_tokens": maximum,
        "max_ids": [str(item) for item, n in zip(ids, lengths) if n == maximum],
        "over_limit_count": len(over),
        "over_limit_ids_and_tokens": over,
    }


def main():
    tokenizer = AutoTokenizer.from_pretrained(
        "PruhaNLP/USER2-1C-code", revision=MODEL_REVISION,
        cache_dir=str(PILOT / "downloads/hf"),
    )
    output = {
        "model_revision": MODEL_REVISION,
        "dataset_revision": DATASET_REVISION,
        "token_limit": LIMIT,
        "add_special_tokens": True,
        "truncation": False,
        "prefixes": {"query": "search_query: ", "document": "search_document: "},
        "subsets": {},
    }
    for subset in ("forum", "fastcode"):
        rows = load_dataset(
            DATASET, subset, split="test", revision=DATASET_REVISION,
            cache_dir=str(PILOT / "downloads/hf/datasets"),
        )
        ids = [str(row["id"]) for row in rows]
        q = tokenizer(["search_query: " + row["question"] for row in rows], add_special_tokens=True, truncation=False)["input_ids"]
        d = tokenizer(["search_document: " + row["code"] for row in rows], add_special_tokens=True, truncation=False)["input_ids"]
        output["subsets"][subset] = {
            "rows": len(rows),
            "query": summarize(ids, [len(x) for x in q]),
            "document": summarize(ids, [len(x) for x in d]),
        }
    target = PILOT / "results" / "token-preflight.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        raise SystemExit(f"refusing to overwrite {target}")
    target.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(output, ensure_ascii=False))


if __name__ == "__main__":
    main()
