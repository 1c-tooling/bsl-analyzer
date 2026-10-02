#!/usr/bin/env python3
"""Read JSON string/list from stdin; emit tokenizer lengths without text."""

import argparse
import json
import os
import sys

from transformers import AutoTokenizer

parser = argparse.ArgumentParser()
parser.add_argument("--model-id", required=True)
parser.add_argument("--revision", required=True)
args = parser.parse_args()
texts = json.load(sys.stdin)
texts = [texts] if isinstance(texts, str) else texts
if not isinstance(texts, list) or not all(isinstance(text, str) for text in texts):
    raise SystemExit("stdin must contain a JSON string or list of strings")
tokenizer = AutoTokenizer.from_pretrained(
    args.model_id,
    revision=args.revision,
    cache_dir=os.environ.get("HF_HOME"),
)
print(json.dumps([len(ids) for ids in tokenizer(texts, add_special_tokens=True, truncation=False)["input_ids"]]))
