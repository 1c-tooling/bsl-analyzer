#!/usr/bin/env python3
"""Pinned Qwen Q8 comparison using the original 1C-RB ranking and metrics."""

import hashlib
import json
import re
import sys
import time
import urllib.request
from pathlib import Path

import numpy as np
from c1rb.evaluation import _search, compute_metrics
from datasets import load_dataset

ROOT = Path(__file__).resolve().parents[2] / "experiments/user2-pilot"
API = "http://127.0.0.1:18882"
INSTRUCTION = (
    "Instruct: Given a 1C developer question, retrieve the matching 1C code fragment "
    "(BSL, query language, or module) that answers it.\nQuery:"
)
DATASET_REVISION = "ae3af100584c51dcd43ff927b726bd27ba42d769"
MODEL_ALIAS = "qwen3-embedding-4b-q8-f460253"


def validate_gpu_runtime(manifest):
    if manifest.get("gpu_only") is not True:
        raise ValueError("Qwen E2 requires a manifest marked gpu_only=true")
    if manifest.get("device") not in {"CUDA", "Vulkan"}:
        raise ValueError("Qwen E2 requires CUDA or Vulkan, without CPU or hybrid execution")
    if manifest.get("n_gpu_layers") != "all" or manifest.get("all_layers_on_gpu") is not True:
        raise ValueError("Qwen E2 requires measured offload of all model layers to GPU")
    if manifest.get("model_alias") != MODEL_ALIAS:
        raise ValueError("Qwen E2 runtime alias does not match the pinned model")
    if manifest.get("llama_cpp_commit") != "185103dcf53222165ecd15ce8e406606e25bd091":
        raise ValueError("Qwen E2 requires the pinned llama.cpp revision")
    if manifest.get("gguf_revision") != "f4602530db1d980e16da9d7d3a70294cf5c190be":
        raise ValueError("Qwen E2 requires the pinned GGUF revision")
    if manifest.get("gguf_sha256") != "b60ae5ce2dd6a0b77f82cadf21def1f310a3e10cde380ad0081b07a9d416949d":
        raise ValueError("Qwen E2 requires the verified pinned Q8 GGUF")
    if not re.fullmatch(r"[0-9a-f]{64}", str(manifest.get("runtime_binary_sha256", ""))):
        raise ValueError("Qwen E2 requires a pinned actual runtime binary SHA-256")
    if int(manifest.get("token_limit", 0)) < 9829:
        raise ValueError("Qwen E2 token limit must cover the measured 9829-token query")


def self_test_gpu_runtime():
    base = {
        "gpu_only": True, "device": "Vulkan", "n_gpu_layers": "all",
        "all_layers_on_gpu": True, "model_alias": MODEL_ALIAS,
        "llama_cpp_commit": "185103dcf53222165ecd15ce8e406606e25bd091",
        "gguf_revision": "f4602530db1d980e16da9d7d3a70294cf5c190be",
        "gguf_sha256": "b60ae5ce2dd6a0b77f82cadf21def1f310a3e10cde380ad0081b07a9d416949d",
        "runtime_binary_sha256": "a" * 64, "token_limit": 10048,
    }
    validate_gpu_runtime(base)
    for changes in ({"gpu_only": False, "device": "CPU", "n_gpu_layers": 0},
                    {"device": "CUDA", "n_gpu_layers": 8, "all_layers_on_gpu": False}):
        try:
            validate_gpu_runtime({**base, **changes})
        except ValueError:
            continue
        raise AssertionError(f"unsafe Qwen runtime accepted: {changes}")
    print("Qwen GPU-only manifest guard passed")


def post(route, value):
    request = urllib.request.Request(API + route, data=json.dumps(value).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=600) as response:
        return json.load(response)


def encode(name, texts, manifest):
    directory = ROOT / "results/qwen/vectors" / name
    directory.mkdir(parents=True, exist_ok=True)
    identity = hashlib.sha256(json.dumps({
        "texts": texts, "runtime": manifest,
    }, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
    identity_file = directory / "identity.txt"
    if not identity_file.exists() and any(directory.glob("*.npz")):
        raise RuntimeError("Qwen checkpoints lack their input/runtime identity")
    if identity_file.exists() and identity_file.read_text() != identity:
        raise RuntimeError("Partial Qwen vectors belong to different inputs/runtime")
    identity_file.write_text(identity)
    vectors, lengths, seconds = [], [], 0.0
    for offset in range(0, len(texts), 8):
        output = directory / f"{offset:06d}.npz"
        if output.exists():
            with np.load(output, allow_pickle=False) as batch:
                rows, lens = batch["vectors"], batch["lengths"].tolist()
                duration = float(batch["seconds"])
        else:
            inputs = texts[offset:offset + 8]
            lens = [len(post("/tokenize", {"content": text, "add_special": True})["tokens"])
                    for text in inputs]
            if max(lens) > manifest["token_limit"]:
                raise RuntimeError("Input exceeds the configured Qwen batch token budget; no truncation")
            started = time.monotonic()
            response = post("/v1/embeddings", {"model": MODEL_ALIAS, "input": inputs})
            duration = time.monotonic() - started
            data = sorted(response["data"], key=lambda row: row["index"])
            if [row["index"] for row in data] != list(range(len(inputs))):
                raise RuntimeError("Qwen returned invalid count/index mapping")
            rows = np.asarray([row["embedding"] for row in data], dtype=np.float32)
            if rows.shape != (len(inputs), 2560) or not np.isfinite(rows).all():
                raise RuntimeError("Qwen returned invalid dimensions/values")
            norms = np.linalg.norm(rows, axis=1)
            if np.any(norms == 0):
                raise RuntimeError("Qwen returned a zero vector")
            rows /= norms[:, None]
            temporary = output.with_suffix(".tmp")
            with temporary.open("wb") as stream:
                np.savez(stream, vectors=rows, lengths=lens, seconds=duration)
            temporary.replace(output)
        vectors.extend(rows)
        lengths.extend(lens)
        seconds += duration
        print(json.dumps({"task": name, "completed": min(offset + 8, len(texts)),
                          "total": len(texts)}), flush=True)
    return np.asarray(vectors), lengths, seconds


def evaluate(name, bundle, manifest):
    qids, dids = list(bundle["queries"]), list(bundle["corpus"])
    documents = [bundle["corpus"][docid]["text"] for docid in dids]
    dvectors, dlens, dseconds = encode(name + "-documents", documents, manifest)
    result = {"document_lengths": dict(zip(dids, dlens)), "document_seconds": dseconds}
    for variant, prefix in [("no-instruction", ""), ("1c-instruction", INSTRUCTION)]:
        questions = [prefix + bundle["queries"][qid] for qid in qids]
        qvectors, qlens, qseconds = encode(name + "-" + variant, questions, manifest)
        ranked = _search(qvectors, dvectors, dids, 10, desc=name + ":" + variant)
        result[variant] = {
            "metrics": compute_metrics(ranked, qids, bundle["qrels"]),
            "per_query": {qid: {"ranked_doc_ids": docs, "token_length": length,
                "metrics": compute_metrics([docs], [qid], {qid: bundle["qrels"][qid]})}
                for qid, docs, length in zip(qids, ranked, qlens)},
            "query_seconds": qseconds,
        }
        target = ROOT / "results/qwen" / f"{name}-{variant}.json"
        target.write_text(json.dumps({"manifest": manifest, "result": result},
                                     ensure_ascii=False, indent=2))
    return result


def main():
    manifest = json.loads((ROOT / "qwen-runtime.json").read_text())
    if not manifest.get("gguf_hf_lfs_verified"):
        raise SystemExit("Official pinned GGUF checksum must be verified first")
    validate_gpu_runtime(manifest)
    results = {}
    for subset in ("forum", "fastcode"):
        rows = load_dataset("PruhaNLP/1C-Ebench", subset, split="test",
                            revision=DATASET_REVISION, cache_dir=str(ROOT / "downloads/hf/datasets"))
        bundle = {"corpus": {}, "queries": {}, "qrels": {}}
        for row in rows:
            ident = str(row["id"])
            bundle["corpus"][ident] = {"text": row["code"]}
            bundle["queries"][ident] = row["question"]
            bundle["qrels"][ident] = {ident: 1}
        results[subset] = evaluate(subset, bundle, manifest)
    raw = (ROOT / "local-qrels.json").read_bytes()
    results["local"] = evaluate("local", json.loads(raw), manifest)
    enriched = (ROOT / "local-qrels-enriched.json").read_bytes()
    results["local_enriched"] = evaluate("local_enriched", json.loads(enriched), manifest)
    summary = {"manifest": manifest, "dataset_revision": DATASET_REVISION,
        "evaluation_revision": "5f2c67c6a50b74560c9f783900a119c83883e618",
        "local_qrels_sha256": hashlib.sha256(raw).hexdigest(),
        "local_enriched_sha256": hashlib.sha256(enriched).hexdigest(), "tasks": results,
        "external_macro": {variant: {metric: float(np.mean([
            results[subset][variant]["metrics"][metric] for subset in ("forum", "fastcode")
        ])) for metric in ("ndcg@10", "recall@10", "mrr@10")}
            for variant in ("no-instruction", "1c-instruction")}}
    (ROOT / "results/qwen/result.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    print(json.dumps(summary["external_macro"]))


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test_gpu_runtime()
    else:
        main()
