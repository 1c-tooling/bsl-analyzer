#!/usr/bin/env python3
"""Count exact USER2 token lengths in one frozen local corpus/qrels bundle."""
import argparse
import hashlib
import json
from pathlib import Path

from transformers import AutoTokenizer

ROOT = Path(__file__).resolve().parents[2]
PILOT = ROOT / "experiments/user2-pilot"
REVISION = "b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4"
LIMIT = 8192


def summary(keys, lengths):
    maximum = max(lengths, default=0)
    over = [{"id": key, "tokens": count} for key, count in zip(keys, lengths) if count > LIMIT]
    return {"count": len(lengths), "max_tokens": maximum, "max_ids": [key for key, count in zip(keys, lengths) if count == maximum], "over_limit_count": len(over), "over_limit_ids_and_tokens": over}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path)
    args = parser.parse_args()
    path = args.path if args.path.is_absolute() else ROOT / args.path
    raw = path.read_bytes()
    bundle = json.loads(raw)
    tokenizer = AutoTokenizer.from_pretrained("PruhaNLP/USER2-1C-code", revision=REVISION, cache_dir=str(PILOT / "downloads/hf"))
    qids, dids = list(bundle["queries"]), list(bundle["corpus"])
    q = tokenizer(["search_query: " + bundle["queries"][key] for key in qids], add_special_tokens=True, truncation=False)["input_ids"]
    d = tokenizer(["search_document: " + bundle["corpus"][key]["text"] for key in dids], add_special_tokens=True, truncation=False)["input_ids"]
    output = {"model_revision": REVISION, "source_path": str(path.relative_to(ROOT)), "source_sha256": hashlib.sha256(raw).hexdigest(), "token_limit": LIMIT, "add_special_tokens": True, "truncation": False, "prefixes": {"query": "search_query: ", "document": "search_document: "}, "query": summary(qids, [len(x) for x in q]), "document": summary(dids, [len(x) for x in d])}
    target = PILOT / "results" / f"{path.stem}-token-preflight.json"
    if target.exists():
        raise SystemExit(f"refusing to overwrite {target}")
    target.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(output, ensure_ascii=False))


if __name__ == "__main__":
    main()
