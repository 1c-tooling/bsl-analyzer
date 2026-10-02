#!/usr/bin/env python3
"""Small OpenAI-compatible USER2 serving adapter for the pinned pilot."""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

import numpy as np
import torch
from sentence_transformers import SentenceTransformer

MODEL = "PruhaNLP/USER2-1C-code"
REVISION = "b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4"
LIMIT = 8192
DEVICE = os.environ.get("USER2_DEVICE", "cuda")
if not DEVICE.startswith("cuda") or not torch.cuda.is_available():
    raise SystemExit("GPU-only pilot: CUDA inference is required")

ALIASES = {
    "user2-code-rs-v1-both-768": (768, "search_query: ", "search_document: "),
    "user2-code-rs-v1-query-only-768": (768, "search_query: ", ""),
    "user2-code-rs-v1-both-256": (256, "search_query: ", "search_document: "),
    "user2-code-rs-v1-query-only-256": (256, "search_query: ", ""),
}

model = SentenceTransformer(
    MODEL, revision=REVISION, device=DEVICE, default_prompt_name=None,
    model_kwargs={"attn_implementation": "sdpa"},
)
tokenizer = model.tokenizer
model.eval()
attention_implementation = model[0].auto_model.config._attn_implementation
positions = model[0].auto_model.config.max_position_embeddings
poolers = [module for module in model._modules.values() if hasattr(module, "pooling_mode")]
if positions < LIMIT or len(poolers) != 1 or poolers[0].pooling_mode != "mean" or poolers[0].include_prompt is not True or model.default_prompt_name is not None:
    raise RuntimeError("pinned model pooling, prompt, or context configuration differs from E1 contract")
model.max_seq_length = LIMIT


class Handler(BaseHTTPRequestHandler):
    def log_message(self, _format, *_args):
        pass

    def reply(self, status, value):
        body = json.dumps(value, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if urlparse(self.path).path == "/health":
            return self.reply(200, {"status": "ok", "model": MODEL, "revision": REVISION, "device": DEVICE, "dtype": "float32", "pooling": "mean", "include_prompt": True, "max_seq_length": LIMIT, "default_prompt_name": None, "attention_implementation": attention_implementation})
        return self.reply(404, {"error": "not found"})

    def do_POST(self):
        if urlparse(self.path).path != "/v1/embeddings":
            return self.reply(404, {"error": "not found"})
        try:
            request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        except (json.JSONDecodeError, ValueError):
            return self.reply(400, {"error": "invalid JSON"})
        if not isinstance(request, dict) or "input" not in request:
            return self.reply(400, {"error": "input must be a string or list of strings"})
        try:
            texts = request["input"]
            texts = [texts] if isinstance(texts, str) else texts
            alias = request.get("model")
            if not isinstance(texts, list) or not texts or not isinstance(alias, str) or alias not in ALIASES:
                return self.reply(400, {"error": "model alias and non-empty input are required"})
            dims = ALIASES[alias][0]
            with lock:
                for text in texts:
                    if not isinstance(text, str):
                        return self.reply(400, {"error": "input items must be strings"})
                    if len(tokenizer(text, add_special_tokens=True, truncation=False)["input_ids"]) > LIMIT:
                        return self.reply(400, {"error": "input exceeds 8192 tokens"})
                vectors = model.encode(
                    texts, batch_size=1, prompt=None, prompt_name=None, normalize_embeddings=True,
                    convert_to_numpy=True, show_progress_bar=False,
                ).astype(np.float32)
            if vectors.shape != (len(texts), 768) or not np.isfinite(vectors).all():
                return self.reply(500, {"error": "checkpoint returned invalid embedding values"})
            norms = np.linalg.norm(vectors, axis=1, keepdims=True)
            if (norms == 0).any():
                return self.reply(500, {"error": "checkpoint returned a zero embedding"})
            if not np.all(np.abs(norms - 1) <= 0.001):
                return self.reply(500, {"error": "checkpoint returned a non-normalized embedding"})
            if dims == 256:
                vectors = vectors[:, :256]
                norms = np.linalg.norm(vectors, axis=1, keepdims=True)
                if not np.isfinite(vectors).all() or (norms == 0).any():
                    return self.reply(500, {"error": "invalid embedding"})
                vectors /= norms
            return self.reply(200, {
                "object": "list",
                "data": [{"object": "embedding", "index": i, "embedding": row.tolist()} for i, row in enumerate(vectors)],
                "model": request.get("model", "user2-code-rs-v1"),
                "usage": {"prompt_tokens": 0, "total_tokens": 0},
            })
        except (KeyError, TypeError, ValueError):
            return self.reply(400, {"error": "invalid embedding request"})
        except Exception:
            return self.reply(500, {"error": "embedding request failed"})


lock = threading.Lock()
ThreadingHTTPServer(("127.0.0.1", 18881), Handler).serve_forever()
