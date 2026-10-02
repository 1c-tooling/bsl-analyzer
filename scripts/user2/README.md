# Локальный пилот USER2

Инструменты этого каталога запускают отдельный эксперимент. Они не переключают defaults анализатора. Исходники ИБ, qrels, веса, cache, результаты и секреты находятся в ignored `experiments/user2-pilot/`. Сначала получите разрешение на свой контур и подготовьте manifest; инструкции приёмки — `openspec/changes/add-user2-code-embedding-profile/evaluation-plan.md`.

Для token policy используйте отдельный cache и frozen evaluator для каждого корпуса.
Перед созданием candidate vectors сохраните план командой
`python3 scripts/user2/parent_evaluator.py --qrels <pilot>/local-qrels.json --plan <pilot>/results/token-budget-evaluator-v2.json`.
`runtime_acceptance.py --parent-aware` проверяет этот план и выполняет отдельные
запросы raw top-30 и public top-10. Качество считает по первым десяти различным
parent methods из raw top-30; дубли частей не увеличивают релевантность. Для demo
policy-on/off используйте один план и прежние frozen qrels; для УНФ — его собственный
план. `--evaluator-plan` задаёт другой ранее замороженный файл. Числовой порог
качества этим режимом не задаётся. Вопросы, размеченные по исходникам, не являются
независимым пользовательским holdout.

## Окружение и serving

Для нового пилота скопируйте `scripts/user2/pyproject.toml` в `experiments/user2-pilot/model-env/pyproject.toml`, затем выполните `uv sync --project experiments/user2-pilot/model-env`. Шаблон закрепляет Python 3.12, FP32 runtime и revision `1C-RB`. Не заменяйте действующее общее Python-окружение. `HF_HOME` и UV cache задавайте внутри пилота.

Запустите `scripts/user2/serve-st.sh` в управляемой сессии и сохраните её handle. Скрипт остаётся живым до завершения собственного worker. `/health` на `127.0.0.1:18881` сообщает revision, device, FP32, mean pooling, include_prompt, SDPA и лимит8192. Использованный ранее TEI CPU1.9 оказался несовместим с checkpoint. После решения пользователя о GPU-only его launcher `serve-tei.sh` запрещает запуск; фактическая приёмка использует CUDA SentenceTransformers adapter. `serve-st.sh`, adapter и E1 не переходят на CPU при отсутствии CUDA.

Alias фиксирует revision `b587ab2eaf543f8c2e6bdb6a795d656feaf16ca4` и размерность: `user2-code-rs-v1-both-768`, `user2-code-rs-v1-query-only-768` и соответствующие `*-256`. Клиент сам добавляет literal prefixes; adapter не добавляет второй prompt. Итоговый вход с prefix и special tokens свыше8192 получает HTTP400, без обрезки. 256d — первые256 координат с повторной L2-нормализацией. HTTP batch обрабатывается последовательно, model batch1 ограничивает стоимость padding для длинных текстов.

Для анализатора задайте `EMBEDDING_URL=http://127.0.0.1:18881`, `EMBEDDING_MODEL=<alias>`, `EMBEDDING_DIM=768` или256, `EMBEDDING_BATCH_SIZE=4`, `EMBEDDING_CONCURRENCY=1`; query/document prefixes — в собственном TOML по [документации конфигурации](../../docs/configuration/PROJECT_CONFIGURATION.md). Используйте отдельные `--cache-dir` и `XDG_CACHE_HOME`. Wire alias и storage identity различаются; смена prefix требует совместимого отдельного cache, автоматического стирания старых векторов нет.

## Данные и прогоны

Читайте `localhost/demo` через `ibcmd` в отдельный dataDir: проверьте RAC UUID/DBMS binding, выполните `config export info`, inventory, `config export` и `config export all-extensions`. Берите credentials из разрешённого локального источника; не помещайте их в команды, документацию или результаты. Не применяйте default target соседнего wrapper. В source manifest запишите version, roots и digest. Qrels размечаются по исходникам до candidate vectors и сохраняются неизменными.

Все Python-команды выполняются через `experiments/user2-pilot/model-env/.venv/bin/python`:

1. `scripts/user2/preflight.py`, `preflight-local.py` — длины pinned tokenizer на полном внешнем и локальном наборе; `e1.py` — parity, single/batch и границы8192/8193.
2. `evaluate.py --variant 768-both --local-enriched experiments/user2-pilot/local-qrels-enriched.json`, затем `768-query-only`; `choose-prefix.py` выбирает документную схему по заранее заданным порогам. Выполните выбранный `256-*`, затем `compare.py` для paired bootstrap1000/seed42. Результаты завершённого прогона не перезаписываются; committed batch checkpoints сохраняются, чужой input/runtime отвергается.
3. Enriched input создаётся штатным serializer: `cargo run -p bsl-search --example export_embedding_inputs -- STORE FROZEN_QRELS OUTPUT`. Store открывается read-only; qrels IDs и raw body сохраняются, добавляются native metadata и persisted graph context.
4. `runtime_acceptance.py --binary BIN --workspace COPY --cache CACHE --qrels QRELS --out OUTPUT` измеряет реальные30 warm MCP запросов и product quality; `--background` добавляет собственный fixture и требует совпадения всех запросов с индексацией. `--ready-budget` задаёт ожидание cold readiness и начала фоновой индексации. Idle превышение12s завершает проверку ошибкой. `runtime_lifecycle.py` с теми же binary/workspace/cache и `--out` проверяет marker, overlay, restart, чужой профиль, отказ/возврат собственного loopback proxy и rollback. PG публикации и partial failure проверяются отдельно на собственной schema.

Для Qwen используйте официальный `Qwen3-Embedding-4B-Q8_0.gguf`, pinned revision/checksum/runtime из change и локального `qwen-runtime.json`. Для этой связки собран pinned llama.cpp `185103dcf53222165ecd15ce8e406606e25bd091` с приватным CUDA SDK 13.2.51, `GGML_CUDA=ON`, `GGML_CUDA_FA=ON` и `CMAKE_CUDA_ARCHITECTURES=120a-real`; бинарник лежит в ignored каталоге пилота. Не устанавливайте toolkit, драйвер или библиотеки в систему.

При наличии SDK и исходников по зафиксированной revision configure/build повторяется так:

```bash
PILOT="$PWD/experiments/user2-pilot"
SDK="$PILOT/model-env/cuda-toolchain/cuda13-private/sdk"
SRC="$PILOT/llama-src/llama.cpp-185103dcf53222165ecd15ce8e406606e25bd091"
BUILD="$PILOT/llama-cuda-build-13.2"
cmake -G Ninja -S "$SRC" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON \
  -DGGML_CUDA=ON -DGGML_CUDA_FA=ON -DGGML_CUDA_FA_ALL_QUANTS=OFF \
  '-DGGML_CUDA_FA_QUANTS=q4_0-q4_0;q8_0-q8_0;f16-f16;bf16-bf16' -DGGML_CUDA_NCCL=OFF \
  -DCMAKE_CUDA_COMPILER="$SDK/bin/nvcc" -DCMAKE_CUDA_HOST_COMPILER=/usr/bin/gcc \
  -DCMAKE_CUDA_ARCHITECTURES=120a-real \
  -DCMAKE_BUILD_RPATH="$SDK/lib64" \
  -DCMAKE_EXE_LINKER_FLAGS="-Wl,-rpath-link,$SDK/lib64" \
  -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
cmake --build "$BUILD" --target llama-server -j4
```

GPU-only launch с полной загрузкой слоёв:

```bash
PILOT="$PWD/experiments/user2-pilot"
SDK="$PILOT/model-env/cuda-toolchain/cuda13-private/sdk"
BIN="$PILOT/llama-cuda-build-13.2/bin"
LD_LIBRARY_PATH="$SDK/lib64:$BIN" "$BIN/llama-server" \
  --model "$PILOT/Qwen3-Embedding-4B-Q8_0.gguf" \
  --alias qwen3-embedding-4b-q8-f460253 \
  --host 127.0.0.1 --port 18882 --device CUDA0 --n-gpu-layers all \
  --embedding --pooling last --parallel 1 --flash-attn on --fit off \
  --ctx-size 10048 --batch-size 512 --ubatch-size 512 --no-webui --log-verbosity 4
```

В фактическом запуске llama.cpp округлил `--ctx-size 10048` до эффективного `n_ctx=10240`; оба параметра batch остались512. Для causal модели с `pooling=last` сервер делит длинный prompt на физические шаги по512 токенов. Это не уменьшает контекст одного входа и не включает усечение: longest instructed query обработан целиком, 9829 входных токенов подтверждены `/tokenize` и usage embeddings. Перед E2 закрепите фактические binary hash, GPU offload, Flash Attention, KV cache dtype и буферы в runtime manifest. Запускайте модели последовательно, не освобождайте чужие GPU allocations.

Минимальные проверки tooling: `runtime_acceptance.py --self-test`, `serving_proxy.py`, `test-eval-layout.py`, `test-gpu-only.py`. Они не заменяют реальные E1–E5. Результаты и незавершённые gates записываются в change `verification.md`; модельные defaults, commit/push и archive выполняются только по отдельному основанию.

Режим `--background` запускает второй собственный MCP consumer: копия workspace, временный cache и 1000 дополнительных методов. Он индексирует документы через тот же serving, пока первый тёплый consumer отвечает на 30 контрольных вопросов. Перед каждым запросом проверяется фактическое состояние индексации второго consumer; после проверки оба процесса закрываются, временная копия удаляется. Изменение файла само по себе не считается доказательством работающей индексации.
