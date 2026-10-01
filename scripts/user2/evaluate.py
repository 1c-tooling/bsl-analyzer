#!/usr/bin/env python3
"""USER2 E2 over pinned 1C-RB data and the frozen local qrels."""

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import torch
from datasets import load_dataset
from c1rb.evaluation import _search, compute_metrics
from transformers import AutoTokenizer

ROOT = Path(__file__).resolve().parents[2]
PILOT = ROOT / "experiments/user2-pilot"
REVISION = "b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4"
DATASET_REVISION = "ae3af100584c51dcd43ff927b726bd27ba42d769"
DATASET = "PruhaNLP/1C-Ebench"
RB_REVISION = "5f2c67c6a50b74560c9f783900a119c83883e618"
PREFIX_Q = "search_query: "
PREFIX_D = "search_document: "
K = 10


def embed(texts, dimensions, document_prefix_present, checkpoint_dir, runtime_manifest):
    endpoint = os.environ.get("USER2_API", "http://127.0.0.1:18881/v1/embeddings")
    aliases = {
        (768, True): "user2-code-rs-v1-both-768",
        (768, False): "user2-code-rs-v1-query-only-768",
        (256, True): "user2-code-rs-v1-both-256",
        (256, False): "user2-code-rs-v1-query-only-256",
    }
    alias = aliases[(dimensions, document_prefix_present)]
    vectors = []
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    for offset in range(0, len(texts), 16):
        batch = texts[offset:offset + 16]
        manifest = {
            "model": "PruhaNLP/USER2-1C-code",
            "model_revision": REVISION,
            "runtime": runtime_manifest,
            "alias": alias,
            "dimensions": dimensions,
            "dtype": "float32",
            "pooling": "mean including prompt tokens",
            "normalization": "L2",
            "prefixes": {"query": PREFIX_Q, "document": PREFIX_D if document_prefix_present else ""},
            "token_limit": 8192,
        }
        fingerprint = hashlib.sha256(json.dumps({"manifest": manifest, "inputs": batch}, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        cache = checkpoint_dir / f"{offset:08d}-{fingerprint}.npy"
        metadata = cache.with_suffix(".json")
        pending = cache.with_suffix(".pending.json")
        if cache.exists() or metadata.exists():
            if not cache.exists() or not metadata.exists():
                raise RuntimeError(f"incomplete cache identity; refusing to adopt or overwrite {cache}")
            expected = {"fingerprint": fingerprint, "count": len(batch), "dimensions": dimensions, "manifest": manifest}
            if json.loads(metadata.read_text(encoding="utf-8")) != expected:
                raise RuntimeError(f"checkpoint identity mismatch; refusing to reuse {cache}")
            rows = np.load(cache, allow_pickle=False)
            if rows.shape != (len(batch), dimensions) or not np.isfinite(rows).all():
                raise RuntimeError(f"invalid saved checkpoint {cache}")
            norms = np.linalg.norm(rows, axis=1)
            if (norms == 0).any() or not np.all(np.abs(norms - 1) <= 0.001):
                raise RuntimeError(f"saved checkpoint contains invalid normalized vectors: {cache}")
            vectors.extend(rows)
            pending.unlink(missing_ok=True)
            continue
        if pending.exists():
            raise RuntimeError(f"previous request may have run without a saved response; root appointment required before retry: {pending}")
        pending.write_text(json.dumps({"fingerprint": fingerprint, "count": len(batch), "dimensions": dimensions, "manifest": manifest}) + "\n", encoding="utf-8")
        request = urllib.request.Request(
            endpoint,
            data=json.dumps({"model": alias, "input": batch}).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=120) as response:
            payload = json.load(response)
        rows = payload.get("data", [])
        if len(rows) != len(batch) or [r.get("index") for r in rows] != list(range(len(rows))) or payload.get("model") != alias:
            raise RuntimeError("serving returned an invalid count or index sequence")
        arr = np.asarray([r["embedding"] for r in rows], dtype=np.float32)
        if arr.shape != (len(batch), dimensions) or not np.isfinite(arr).all() or (np.linalg.norm(arr, axis=1) == 0).any():
            raise RuntimeError("serving returned invalid embedding dimensions or values")
        temp = cache.with_suffix(".npy.tmp")
        with temp.open("wb") as stream:
            np.save(stream, arr, allow_pickle=False)
        temp.replace(cache)
        meta_temp = metadata.with_suffix(".json.tmp")
        meta_temp.write_text(json.dumps({"fingerprint": fingerprint, "count": len(batch), "dimensions": dimensions, "manifest": manifest}) + "\n", encoding="utf-8")
        meta_temp.replace(metadata)
        pending.unlink(missing_ok=True)
        vectors.extend(arr)
    result = np.asarray(vectors, dtype=np.float32)
    if result.shape != (len(texts), dimensions) or not np.isfinite(result).all():
        raise RuntimeError("serving returned invalid embedding dimensions or values")
    norms = np.linalg.norm(result, axis=1)
    if (norms == 0).any():
        raise RuntimeError("serving returned a zero vector")
    return result / norms[:, None]


def evaluate_task(name, corpus, queries, qrels, tokenizer, dimensions, query_prefix, document_prefix, checkpoint_root, runtime_manifest):
    qids, dids = list(queries), list(corpus)
    qtexts = [query_prefix + queries[qid] for qid in qids]
    dtexts = [document_prefix + corpus[did]["text"] for did in dids]
    qlens = [len(ids) for ids in tokenizer(qtexts, add_special_tokens=True, truncation=False)["input_ids"]]
    dlens = [len(ids) for ids in tokenizer(dtexts, add_special_tokens=True, truncation=False)["input_ids"]]
    if max(qlens + dlens, default=0) > 8192:
        raise RuntimeError(f"{name}: found input over locked 8192-token limit")
    qvectors = embed(qtexts, dimensions, bool(document_prefix), checkpoint_root / name / "queries", runtime_manifest)
    dvectors = embed(dtexts, dimensions, bool(document_prefix), checkpoint_root / name / "documents", runtime_manifest)
    ranked = _search(qvectors, dvectors, dids, K, desc=f"{name}: ranking")
    metrics = compute_metrics(ranked, qids, qrels, k_values=(1, 3, 5, 10))
    per_query = {}
    for qid, docs, qlen in zip(qids, ranked, qlens):
        per_query[qid] = {
            "ranked_doc_ids": docs,
            "token_length": qlen,
            "metrics": compute_metrics([docs], [qid], {qid: qrels.get(qid, {})}, k_values=(1, 3, 5, 10)),
        }
    return {
        "metrics": metrics,
        "queries": per_query,
        "document_token_lengths": dict(zip(dids, dlens)),
        "token_lengths": {"query_max": max(qlens, default=0), "document_max": max(dlens, default=0), "query_count": len(qlens), "document_count": len(dlens)},
    }


def load_external(tokenizer, dimensions, query_prefix, document_prefix, checkpoint_root, runtime_manifest):
    # revision is the immutable Hub commit; cache is confined to this pilot.
    cache = str(PILOT / "downloads/hf/datasets")
    tasks = {}
    for subset in ("forum", "fastcode"):
        rows = load_dataset(DATASET, subset, split="test", revision=DATASET_REVISION, cache_dir=cache)
        corpus, queries, qrels = {}, {}, {}
        for row in rows:
            docid = str(row["id"])
            corpus[docid] = {"id": docid, "title": "", "text": row["code"]}
            queries[docid] = row["question"]
            qrels[docid] = {docid: 1}
        tasks[subset] = evaluate_task(subset, corpus, queries, qrels, tokenizer, dimensions, query_prefix, document_prefix, checkpoint_root, runtime_manifest)
    metrics = {"ndcg@10": float(np.mean([tasks[x]["metrics"]["ndcg@10"] for x in ("forum", "fastcode")]))}
    return tasks, metrics


def load_local(name, path, tokenizer, dimensions, query_prefix, document_prefix, checkpoint_root, runtime_manifest):
    source = Path(path)
    raw = source.read_bytes()
    bundle = json.loads(raw)
    corpus, queries, qrels = bundle["corpus"], bundle["queries"], bundle["qrels"]
    task = evaluate_task(name, corpus, queries, qrels, tokenizer, dimensions, query_prefix, document_prefix, checkpoint_root, runtime_manifest)
    return task, {"path": str(source), "sha256": hashlib.sha256(raw).hexdigest(), "source_digest": bundle.get("source_digest")}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--variant", choices=("768-both", "768-query-only", "256-both", "256-query-only"), required=True)
    parser.add_argument("--local-qrels", type=Path, default=PILOT / "local-qrels.json")
    parser.add_argument("--local-enriched", type=Path)
    parser.add_argument("--skip-local", action="store_true")
    args = parser.parse_args()
    dims, both = int(args.variant[:3]), args.variant.endswith("both")
    qprefix, dprefix = PREFIX_Q, (PREFIX_D if both else "")
    alias = f"user2-code-rs-v1-{'both' if both else 'query-only'}-{dims}"
    output = PILOT / "results" / args.variant
    if (output / "result.json").exists():
        raise SystemExit(f"refusing to overwrite completed/partial result: {output}")
    checkpoint_root = output / ".progress"

    tokenizer = AutoTokenizer.from_pretrained("PruhaNLP/USER2-1C-code", revision=REVISION, cache_dir=str(PILOT / "downloads/hf"))
    health_url = os.environ.get("USER2_API", "http://127.0.0.1:18881/v1/embeddings").removesuffix("/v1/embeddings") + "/health"
    with urllib.request.urlopen(health_url, timeout=10) as response:
        runtime_manifest = json.load(response)
    expected_health = {
        "model": "PruhaNLP/USER2-1C-code", "revision": REVISION, "device": "cuda",
        "dtype": "float32", "pooling": "mean", "include_prompt": True,
        "max_seq_length": 8192, "default_prompt_name": None,
        "attention_implementation": "sdpa",
    }
    if any(runtime_manifest.get(key) != value for key, value in expected_health.items()):
        raise RuntimeError(f"serving manifest does not match evaluation contract: {runtime_manifest}")
    runtime_manifest["software"] = {
        "python": platform.python_version(),
        "torch": torch.__version__,
        "sentence_transformers": importlib.metadata.version("sentence-transformers"),
        "transformers": importlib.metadata.version("transformers"),
        "numpy": np.__version__,
    }
    runtime_manifest["hardware"] = {
        "device_name": torch.cuda.get_device_name(0) if runtime_manifest["device"] == "cuda" else "cpu",
    }
    result = {
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "variant": args.variant,
        "alias": alias,
        "model": "PruhaNLP/USER2-1C-code",
        "model_revision": REVISION,
        "dataset": DATASET,
        "dataset_revision": DATASET_REVISION,
        "evaluation_code": "PruhaNLP/1C-RB",
        "evaluation_revision": RB_REVISION,
        "dimensions": dims,
        "query_prefix": qprefix,
        "document_prefix": dprefix,
        "pooling": "mean including prompt tokens",
        "normalization": "L2",
        "token_limit": 8192,
        "runtime": {"serving_manifest": runtime_manifest, "endpoint": "loopback:18881", "request_batch_size": 16, "model_encode_batch_size": 1, "concurrency": 1},
    }
    result["external"], result["external_macro"] = load_external(tokenizer, dims, qprefix, dprefix, checkpoint_root, runtime_manifest)
    if not args.skip_local:
        local_path = args.local_qrels if args.local_qrels.is_absolute() else ROOT / args.local_qrels
        result["local"], result["local_source"] = load_local("local", local_path, tokenizer, dims, qprefix, dprefix, checkpoint_root, runtime_manifest)
        if args.local_enriched is not None:
            enriched_path = args.local_enriched if args.local_enriched.is_absolute() else ROOT / args.local_enriched
            result["local_enriched"], result["local_enriched_source"] = load_local("local_enriched", enriched_path, tokenizer, dims, qprefix, dprefix, checkpoint_root, runtime_manifest)
    output.mkdir(parents=True, exist_ok=True)
    temp = output / "result.json.tmp"
    temp.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temp.replace(output / "result.json")
    print(json.dumps({"variant": args.variant, "external_macro": result["external_macro"], "local": None if args.skip_local else result["local"]["metrics"], "local_enriched": result.get("local_enriched", {}).get("metrics"), "output": str(output / "result.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
