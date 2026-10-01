#!/usr/bin/env python3
"""USER2 serving parity, vector shape, batch, and exact-token boundary checks."""

import json
import os
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import torch
from sentence_transformers import SentenceTransformer
from transformers import AutoTokenizer

ROOT = Path(__file__).resolve().parents[2]
PILOT = ROOT / "experiments/user2-pilot"
MODEL = "PruhaNLP/USER2-1C-code"
REVISION = "b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4"
QUERY_PREFIX = "search_query: "
DOCUMENT_PREFIX = "search_document: "
API = os.environ.get("USER2_API", "http://127.0.0.1:18881/v1/embeddings")

CASES = [
    {"query": "Как получить цену товара на дату?", "document": "Функция ЦенаНаДату(Товар, ВидЦены, Дата) Экспорт\n\tВозврат Цены.Найти(Товар, ВидЦены, Дата);\nКонецФункции"},
    {"query": "Найдите серверный метод, который заполняет табличную часть документа остатками товаров с учётом склада, характеристики и даты проведения, а затем возвращает количество добавленных строк.", "document": "Процедура ЗаполнитьОстатками(ДокументОбъект, Склад, МоментВремени) Экспорт\n\tДля Каждого СтрокаТоваров Из ДокументОбъект.Товары Цикл\n\t\tСтрокаТоваров.Количество = ОстаткиНаСкладе(СтрокаТоваров.Номенклатура, Склад, МоментВремени);\n\tКонецЦикла;\nКонецПроцедуры"},
    {"query": "Что делает обработчик ПередЗаписью формы?", "document": "Процедура ПередЗаписью(Отказ, ПараметрыЗаписи)\n\tЕсли Не ЗначениеЗаполнено(Объект.Контрагент) Тогда\n\t\tОтказ = Истина;\n\tКонецЕсли;\nКонецПроцедуры"},
    {"query": "Как в языке запросов получить сумму по остаткам в разрезе склада?", "document": "ВЫБРАТЬ\n\tОстатки.Склад КАК Склад,\n\tСУММА(Остатки.КоличествоОстаток) КАК Количество\nИЗ\n\tРегистрНакопления.ТоварыНаСкладах.Остатки(&Момент, ) КАК Остатки\nСГРУППИРОВАТЬ ПО\n\tОстатки.Склад"},
    {"query": "Почему код содержит табуляцию, пустые строки и \"кавычки\"?\nПроверьте перенос строки.", "document": "Процедура ПроверитьJSON()\n\tСтрокаJSON = \"{\"\"x\"\": \"\"a\\tb\\n\"\"}\";\n\n\tВозврат СтрокаJSON;\nКонецПроцедуры"},
    {"query": "Покажите обработку маркеров [CLS] [SEP] [PAD] в тексте вопроса.", "document": "// Служебные токены не должны менять role prefix: [CLS], [SEP], [PAD]\nФункция ТокенСтрокой()\n\tВозврат \"[MASK]\";\nКонецФункции"},
]


def api(texts, alias):
    request = urllib.request.Request(API, data=json.dumps({"model": alias, "input": texts}).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            status, payload = response.status, json.load(response)
    except urllib.error.HTTPError as exc:
        status, payload = exc.code, json.load(exc)
    rows = payload.get("data", [])
    if status != 200:
        return status, None
    assert payload.get("model") == alias
    assert [row.get("index") for row in rows] == list(range(len(texts)))
    result = np.asarray([row["embedding"] for row in rows], dtype=np.float32)
    assert result.shape == (len(texts), 768 if alias.endswith("768") else 256)
    assert np.isfinite(result).all()
    norms = np.linalg.norm(result, axis=1)
    assert np.all(norms > 0)
    assert np.all(np.abs(norms - 1) <= 0.001)
    return status, result


def boundary_text(tokenizer, prefix, wanted):
    for filler in (" код", " функция", " пример"):
        lo, hi = 0, wanted + 100
        while lo <= hi:
            count = (lo + hi) // 2
            candidate = prefix + (filler * count)
            length = len(tokenizer(candidate, add_special_tokens=True, truncation=False)["input_ids"])
            if length < wanted:
                lo = count + 1
            elif length > wanted:
                hi = count - 1
            else:
                return candidate
        for count in range(max(0, hi - 100), lo + 101):
            candidate = prefix + (filler * count)
            if len(tokenizer(candidate, add_special_tokens=True, truncation=False)["input_ids"]) == wanted:
                return candidate
    raise RuntimeError(f"could not construct exact {wanted}-token input")


def main():
    out = PILOT / "results/e1.json"
    if out.exists():
        raise SystemExit(f"refusing to overwrite E1 evidence: {out}")
    progress = PILOT / "results/e1.progress.json"
    tokenizer = AutoTokenizer.from_pretrained(MODEL, revision=REVISION, cache_dir=str(PILOT / "downloads/hf"))
    device = os.environ.get("USER2_DEVICE", "cuda")
    if not device.startswith("cuda") or not torch.cuda.is_available():
        raise SystemExit("GPU-only pilot: CUDA inference is required")
    if progress.exists():
        evidence = json.loads(progress.read_text(encoding="utf-8"))
        if evidence.get("model") != MODEL or evidence.get("revision") != REVISION or evidence.get("criteria", {}).get("cosine_min_fp32") != 0.999:
            raise RuntimeError(f"E1 progress identity/criteria mismatch: {progress}")
        reference_768 = np.asarray(evidence["reference_vectors_768"], dtype=np.float32)
    else:
        model = SentenceTransformer(
            MODEL, revision=REVISION, device=device, default_prompt_name=None,
            model_kwargs={"attn_implementation": "sdpa"},
        )
        poolers = [module for module in model._modules.values() if hasattr(module, "pooling_mode")]
        assert len(poolers) == 1 and poolers[0].pooling_mode == "mean" and poolers[0].include_prompt is True
        assert model[0].auto_model.config.max_position_embeddings >= 8192 and model.default_prompt_name is None
        model.max_seq_length = 8192
        attention_implementation = model[0].auto_model.config._attn_implementation
        assert attention_implementation == "sdpa"
        qtexts = [QUERY_PREFIX + item["query"] for item in CASES]
        dtexts = [DOCUMENT_PREFIX + item["document"] for item in CASES]
        texts = qtexts + dtexts
        reference_768 = model.encode(texts, batch_size=4, prompt=None, prompt_name=None, normalize_embeddings=True, convert_to_numpy=True, show_progress_bar=False).astype(np.float32)
        evidence = {"created_utc": datetime.now(timezone.utc).isoformat(), "model": MODEL, "revision": REVISION, "runtime": {"device": device, "gpu": torch.cuda.get_device_name(0) if device.startswith("cuda") and torch.cuda.is_available() else None, "dtype": "float32", "pooling": poolers[0].pooling_mode, "include_prompt": poolers[0].include_prompt, "explicit_prefix_tokens_in_sequence": True, "default_prompt_name": model.default_prompt_name, "attention_implementation": attention_implementation, "max_position_embeddings": model[0].auto_model.config.max_position_embeddings, "max_seq_length": model.max_seq_length}, "criteria": {"cosine_min_fp32": 0.999, "score_abs_max": 0.01, "stable_pair_margin": 0.02, "norm_abs_error_max": 0.001, "input_token_limit": 8192}, "controls": CASES, "reference_vectors_768": reference_768.tolist(), "dimensions": {}}
        del model
        if device.startswith("cuda"):
            torch.cuda.synchronize()
            torch.cuda.empty_cache()

    qtexts = [QUERY_PREFIX + item["query"] for item in CASES]
    dtexts = [DOCUMENT_PREFIX + item["document"] for item in CASES]
    texts = qtexts + dtexts

    def checkpoint():
        progress.parent.mkdir(parents=True, exist_ok=True)
        temporary = progress.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(evidence, ensure_ascii=False) + "\n", encoding="utf-8")
        temporary.replace(progress)

    checkpoint()

    for dimensions in (768, 256):
        if str(dimensions) in evidence["dimensions"]:
            continue
        alias = f"user2-code-rs-v1-both-{dimensions}"
        ref = reference_768 if dimensions == 768 else reference_768[:, :256].copy()
        if dimensions == 256:
            ref /= np.linalg.norm(ref, axis=1, keepdims=True)
        status, served = api(texts, alias)
        assert status == 200
        single = np.vstack([api([text], alias)[1][0] for text in texts])
        cosine = np.sum(ref * served, axis=1)
        single_cosine = np.sum(served * single, axis=1)
        qref, dref = ref[:6], ref[6:]
        qserved, dserved = served[:6], served[6:]
        score_ref = qref @ dref.T
        score_served = qserved @ dserved.T
        score_error = np.abs(score_ref - score_served)
        stable_pairs = 0
        for q in range(len(CASES)):
            for a in range(len(CASES)):
                for b in range(a + 1, len(CASES)):
                    if abs(float(score_ref[q, a] - score_ref[q, b])) > 0.02:
                        stable_pairs += 1
                        assert np.sign(score_ref[q, a] - score_ref[q, b]) == np.sign(score_served[q, a] - score_served[q, b])
        assert np.all(cosine >= 0.999)
        assert np.all(single_cosine >= 0.999)
        assert float(score_error.max()) <= 0.01
        evidence["dimensions"][str(dimensions)] = {
            "cosine_min": float(cosine.min()),
            "single_cosine_min": float(single_cosine.min()),
            "score_abs_max": float(score_error.max()),
            "stable_pairs_checked": stable_pairs,
            "reference_vectors": ref.tolist(),
            "serving_vectors": served.tolist(),
            "single_vectors": single.tolist(),
            "reference_top6": np.argsort(-(qref @ dref.T), axis=1).tolist(),
            "serving_top6": np.argsort(-(qserved @ dserved.T), axis=1).tolist(),
        }
        checkpoint()

    query_8192 = boundary_text(tokenizer, QUERY_PREFIX, 8192)
    query_8193 = boundary_text(tokenizer, QUERY_PREFIX, 8193)
    assert len(tokenizer(query_8192, add_special_tokens=True, truncation=False)["input_ids"]) == 8192
    assert len(tokenizer(query_8193, add_special_tokens=True, truncation=False)["input_ids"]) == 8193
    assert api([query_8192], "user2-code-rs-v1-both-768")[0] == 200
    assert api([query_8193], "user2-code-rs-v1-both-768")[0] == 400
    evidence["token_boundary"] = {"8192": "accepted", "8193": "rejected", "counts_include_prefix_and_special_tokens": True}

    out.parent.mkdir(parents=True, exist_ok=True)
    temporary = out.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(out)
    progress.unlink()
    print(json.dumps({"output": str(out), "cosine_min": {k: v["cosine_min"] for k, v in evidence["dimensions"].items()}, "score_abs_max": {k: v["score_abs_max"] for k, v in evidence["dimensions"].items()}, "token_boundary": evidence["token_boundary"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
