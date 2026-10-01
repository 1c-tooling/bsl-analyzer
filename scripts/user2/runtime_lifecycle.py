#!/usr/bin/env python3
"""Real MCP marker, overlay, restart, profile rejection and serving failure checks."""

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import tempfile
import time
import uuid
from pathlib import Path

from runtime_acceptance import McpSession, locations, observation_search, wait_ready
from serving_proxy import ServingProxy

ROOT = Path(__file__).resolve().parents[2]
PILOTS = tuple(ROOT / "experiments" / name for name in ("user2-pilot", "user2-unf-pilot"))
DEFAULT_MODEL = "user2-code-rs-v1-both-768"
DEFAULT_DIM = 768
MODEL_DIMS = {
    "user2-code-rs-v1-both-768": 768,
    "user2-code-rs-v1-query-only-768": 768,
    "user2-code-rs-v1-both-256": 256,
    "user2-code-rs-v1-query-only-256": 256,
}
TOKEN_LIMIT = 8192
TOKENIZER_SHA256 = "80d0433a2cfc55a4561b0e98b6f822decc48c9d457db498837223f9385ef3aff"
SAFE_TOKEN_FAILURES = {"embedding_invalid_config", "embedding_input_too_large"}


def embedding_profile(environment):
    model = environment.get("EMBEDDING_MODEL", DEFAULT_MODEL)
    try:
        dimension = int(environment.get("EMBEDDING_DIM", str(DEFAULT_DIM)))
    except ValueError as error:
        raise ValueError("EMBEDDING_DIM must be an integer") from error
    if MODEL_DIMS.get(model) != dimension:
        raise ValueError("EMBEDDING_MODEL and EMBEDDING_DIM are not a supported USER2 alias pair")
    return model, dimension


def demo_marker_source(name):
    return f'Функция {name}() Экспорт\n    Возврат "{name}";\nКонецФункции\n'


def owned_marker_source(source, name, demo):
    return source == demo_marker_source(name) if demo else source.count(name) == 1


def lifecycle_root(workspace, cache, output, prepared_cache=None):
    workspace = workspace.resolve()
    pilot = next((root.resolve() for root in PILOTS if workspace.is_relative_to(root.resolve())), None)
    if pilot is None:
        raise SystemExit("Lifecycle mutation is restricted to experiments/user2-pilot or user2-unf-pilot")
    if (pilot / "source").resolve() == workspace or (pilot / "source").resolve() in workspace.parents:
        raise SystemExit("Refusing to mutate the read-only UNF source tree; pass an owned copy")
    cache_paths = (cache, cache.with_name(cache.name + "-xdg"))
    for path in (*cache_paths, output):
        if not path.resolve().is_relative_to(pilot):
            raise SystemExit("Workspace, cache, and evidence must stay inside the same isolated pilot")
    if prepared_cache is not None:
        source = prepared_cache.resolve()
        source_pilot = next((root.resolve() for root in PILOTS if source.is_relative_to(root.resolve())), None)
        if source_pilot is None or any(
            (root / "source").resolve() == source or (root / "source").resolve() in source.parents
            for root in PILOTS
        ):
            raise SystemExit("Prepared cache must be an owned pilot cache, outside read-only source trees")
        if prepared_cache.is_symlink() or not prepared_cache.is_dir():
            raise SystemExit("Prepared cache must be a real directory, not a symlink")
        if source == cache.resolve() or source in cache.resolve().parents or cache.resolve() in source.parents:
            raise SystemExit("Prepared and destination caches must be separate directories")
    for path in cache_paths:
        if path.is_symlink():
            raise SystemExit("Refusing a cache symlink")
        if path.exists() and (not path.is_dir() or any(path.iterdir())):
            raise SystemExit("Refusing to reuse a cache that may contain old data; choose a fresh token-profile cache")
    return pilot


def cache_snapshot(cache):
    database = cache / "bsl-search.db"
    uri = f"file:{database}?mode=ro"
    with sqlite3.connect(uri, uri=True) as connection:
        connection.execute("BEGIN")
        chunk_count, embedded_count = connection.execute(
            "SELECT COUNT(*), COUNT(embedding) FROM chunks"
        ).fetchone()
        metadata = dict(connection.execute(
            "SELECT key, value FROM meta WHERE key IN "
            "('embedding_profile_identity', 'embedding_profile_dimension', "
            "'token_layout_claim_v1', 'embedding_generation')"
        ))
        dimension = int(metadata.get("embedding_profile_dimension", "0"))
        if dimension < 1:
            raise ValueError("prepared cache dimension must be a positive integer")
        rows = connection.execute(
            "SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id"
        )
        digest = hashlib.sha256()
        for ident, blob in rows:
            if len(blob) != dimension * 4:
                raise ValueError("prepared cache has an embedding BLOB with the wrong dimension")
            digest.update(ident.to_bytes(8, "little"))
            digest.update(blob)
    if chunk_count < 1 or embedded_count != chunk_count:
        raise ValueError("prepared cache must contain an embedding for every indexed chunk")
    identity = metadata.get("embedding_profile_identity")
    claim = metadata.get("token_layout_claim_v1")
    generation = metadata.get("embedding_generation")
    if not identity or not identity.startswith("profile-v1:") or claim != identity:
        raise ValueError("prepared cache must have a token-layout claim matching its profile identity")
    if not dimension or not generation:
        raise ValueError("prepared cache lacks profile dimension or embedding generation")
    return {
        "count": embedded_count,
        "chunk_count": chunk_count,
        "sha256": digest.hexdigest(),
        "identity": identity,
        "dimension": int(dimension),
        "claim": claim,
        "generation": int(generation),
    }


def backup_database(source, destination):
    source_uri = f"file:{source}?mode=ro"
    src = sqlite3.connect(source_uri, uri=True)
    dst = sqlite3.connect(destination)
    try:
        src.backup(dst)
        check = dst.execute("PRAGMA quick_check").fetchone()[0]
    finally:
        src.close()
        dst.close()
    if check != "ok":
        raise ValueError(f"prepared SQLite backup failed integrity check: {source.name}")


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def prepare_cache(source, destination, dimension):
    source = source.resolve()
    source_files = [source / name for name in (
        "bsl-search.db", "bsl-graph.db", "bsl-search.db.usearch", "bsl-search.db.usearch.json",
    )]
    for path in source_files:
        if path.is_symlink() or not path.resolve().is_relative_to(source):
            raise ValueError("prepared cache files must be owned regular files")
    before = cache_snapshot(source)
    if before["dimension"] != dimension:
        raise ValueError("prepared cache dimension does not match selected embedding profile")
    index_json = source / "bsl-search.db.usearch.json"
    index_file = source / "bsl-search.db.usearch"
    graph_file = source / "bsl-graph.db"
    if not graph_file.is_file():
        raise ValueError("prepared cache lacks its graph database")
    sidecars = []
    if index_json.is_file() and index_file.is_file():
        try:
            index_meta = json.loads(index_json.read_text(encoding="utf-8"))
            if (index_meta.get("model_id") == before["identity"]
                    and index_meta.get("dim") == dimension
                    and index_meta.get("count") == before["count"]
                    and index_meta.get("generation") == before["generation"]):
                sidecars = [index_file, index_json]
        except (OSError, json.JSONDecodeError):
            sidecars = []
    companion_hashes = {path.name: sha256_file(path) for path in sidecars}
    created_destination = not destination.exists()
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        raise ValueError("prepared-cache destination is not empty")
    try:
        backup_database(source / "bsl-search.db", destination / "bsl-search.db")
        backup_database(graph_file, destination / "bsl-graph.db")
        for name, digest in companion_hashes.items():
            shutil.copyfile(source / name, destination / name)
            if sha256_file(destination / name) != digest:
                raise ValueError("prepared HNSW companion changed while it was copied")
        if any(sha256_file(source / name) != digest
               for name, digest in companion_hashes.items()):
            raise ValueError("prepared HNSW source changed during copy")
        copied = cache_snapshot(destination)
        if copied != before:
            raise ValueError("prepared SQLite backup differs from its verified source snapshot")
        return copied
    except BaseException:
        for child in destination.iterdir():
            if child.is_file() and not child.is_symlink():
                child.unlink()
        if created_destination:
            destination.rmdir()
        raise


def assert_runtime_profile(status, model, dimension, prepared):
    if prepared is None:
        return
    profile = status.get("embedding_profile") or {}
    if (profile.get("wire_model") != model
            or profile.get("storage_identity") != prepared["identity"]
            or profile.get("dimension") != dimension):
        raise AssertionError("runtime did not accept the prepared cache's selected token embedding profile")


def wait_runtime_profile(session, model, dimension, prepared, timeout=60):
    if prepared is None:
        return session.search({"action": "status"})
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Runtime did not publish its embedding profile before the observation deadline")
        status = observation_search(session, {"action": "status"}, deadline)
        if status is None:
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
            continue
        semantic = next((target for target in status.get("indexing", {}).get("targets", [])
                         if target.get("kind") == "semantic"), None)
        if (status.get("semantic_failure")
                or (semantic and semantic.get("state") in ("failed", "cancelled", "superseded"))):
            raise RuntimeError("Runtime reported a terminal semantic state before profile publication")
        if status.get("embedding_profile") is not None:
            assert_runtime_profile(status, model, dimension, prepared)
            return status
        if status.get("state") not in ("loading", "busy"):
            assert_runtime_profile(status, model, dimension, prepared)
        time.sleep(min(0.1, max(0, deadline - time.monotonic())))


def prepared_cache_self_test():
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        source, destination = root / "prepared", root / "fresh"
        source.mkdir()
        identity = "profile-v1:self-test-token-layout"
        with sqlite3.connect(source / "bsl-search.db") as connection:
            connection.executescript(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);"
                "CREATE TABLE chunks (id INTEGER PRIMARY KEY, embedding BLOB);"
            )
            connection.executemany("INSERT INTO meta VALUES (?, ?)", [
                ("embedding_profile_identity", identity),
                ("embedding_profile_dimension", "256"),
                ("token_layout_claim_v1", identity),
                ("embedding_generation", "1"),
            ])
            connection.execute("INSERT INTO chunks VALUES (1, ?)", [bytes(256 * 4)])
        with sqlite3.connect(source / "bsl-graph.db") as connection:
            connection.execute("CREATE TABLE graph_fixture (value TEXT)")
        source_before = {path.name: sha256_file(path) for path in source.iterdir()}
        copied = prepare_cache(source, destination, 256)
        assert copied["count"] == copied["chunk_count"] == 1
        assert copied["identity"] == copied["claim"] == identity
        assert not (destination / "bsl-search.db.usearch").exists(), (
            "missing HNSW should use native blob-backed rebuild, not block complete SQLite reuse"
        )
        assert {path.name: sha256_file(path) for path in source.iterdir()} == source_before, (
            "prepared cache copy must leave its source untouched"
        )
        with sqlite3.connect(source / "bsl-search.db") as connection:
            connection.execute("UPDATE chunks SET embedding = ?", [bytes(255 * 4)])
        try:
            prepare_cache(source, root / "bad-width", 256)
            raise AssertionError("wrong-width vectors passed prepared-cache preflight")
        except ValueError as error:
            assert "wrong dimension" in str(error)
        assert not (root / "bad-width").exists()



def tokenizer_artifact():
    pointer = ROOT / "experiments/user2-unf-pilot/tokenizer-artifact.json"
    artifact = json.loads(pointer.read_text(encoding="utf-8"))
    for item in artifact.get("files", []):
        path = (ROOT / item["path"]).resolve()
        if not any(path.is_relative_to(root.resolve()) for root in PILOTS):
            continue
        if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == TOKENIZER_SHA256 == item.get("sha256"):
            return path
    raise SystemExit("Pinned USER2 tokenizer artifact is unavailable or does not match its recorded SHA256")


def safe_token_failure(status):
    failure = status.get("semantic_failure")
    return (isinstance(failure, dict)
            and failure.get("code") in SAFE_TOKEN_FAILURES
            and set(failure).issubset({"code", "request_bytes", "max_request_bytes"}))


def wait_foreign_profile(session, query, timeout=60):
    # Workspace semantic initialization is lazy: status alone does not start it.
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        session.search({"action": "search_code", "query": query, "limit": 10})
        status = session.search({"action": "status"})
        if is_foreign_profile_refusal(status):
            return status
        time.sleep(.1)
    raise AssertionError("Foreign profile did not reach the expected semantic refusal")


def is_foreign_profile_refusal(status):
    targets = {target["kind"]: target for target in status.get("indexing", {}).get("targets", [])}
    semantic = targets.get("semantic", {})
    reason = (semantic.get("state"), semantic.get("reason_code"))
    classified = (
        reason in (("failed", "native_failure"), ("unknown", "identity_unverified"),
                   ("failed", "embedding_invalid_config"))
        and safe_token_failure(status)
    )
    return targets.get("lexical", {}).get("state") == "ready" and (
        classified or reason == ("unknown", "identity_unverified")
    )


def vectors(cache):
    database = cache / "bsl-search.db"
    with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
        chunk_count, count = connection.execute(
            "SELECT COUNT(*), COUNT(embedding) FROM chunks"
        ).fetchone()
        identity = dict(connection.execute(
            "SELECT key, value FROM meta WHERE key LIKE 'embedding_profile_%' OR key = 'token_layout_claim_v1'"))
        rows = connection.execute(
            "SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id"
        )
        digest = hashlib.sha256()
        for ident, blob in rows:
            digest.update(ident.to_bytes(8, "little"))
            digest.update(blob)
    return {"count": count, "chunk_count": chunk_count,
            "sha256": digest.hexdigest(), "identity": identity}


def marker(session, name, expected_path, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        answer = session.search({"action": "search_code", "query": name, "limit": 10})
        hits = [h for h in locations(answer) if h.get("symbol") == name]
        if hits:
            assert all(h.get("line_start", 0) > 0 and h.get("line_end", 0) >= h["line_start"] for h in hits)
            assert all(h.get("root_id", "") == "" and h.get("path") == expected_path for h in hits)
            return answer, hits
        time.sleep(1)
    raise AssertionError("owned marker did not become searchable")


def main():
    parser = argparse.ArgumentParser()
    for name in ("binary", "workspace", "cache", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--prepared-cache", type=Path,
                        help="Complete, token-claimed cache to copy into the fresh --cache destination")
    args = parser.parse_args()
    pilot = lifecycle_root(args.workspace, args.cache, args.out, args.prepared_cache)
    if args.out.exists():
        raise SystemExit("Refusing to overwrite evidence")
    model, dimension = embedding_profile(os.environ)
    tokenizer = tokenizer_artifact()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    workspace = args.workspace.resolve()
    demo = pilot.name == "user2-pilot"
    if demo:
        module = workspace / "cf/CommonModules/ПилотUSER2/Ext/Module.bsl"
        if not module.resolve().is_relative_to(workspace):
            raise SystemExit("Demo marker must resolve inside the owned workspace")
        original = module.read_text(encoding="utf-8")
        old, new = "ПилотUSER2_Маркер_a7f91", "ПилотUSER2_Маркер_b8e02"
        injected = None
    else:
        module = workspace / "cf/CommonModules/ПилотUNFUSER2Lifecycle/Ext/Module.bsl"
        if module.exists() or module.is_symlink():
            raise SystemExit("Refusing to replace an existing UNF module")
        marker_id = uuid.uuid4().hex[:8]
        old, new = f"ПилотUNFUSER2_Маркер_{marker_id}_a", f"ПилотUNFUSER2_Маркер_{marker_id}_b"
        original = f"Функция {old}() Экспорт\n    Возврат Истина;\nКонецФункции\n"
        injected = original
        if not module.resolve().is_relative_to(workspace):
            raise SystemExit("Generated UNF module resolved outside the owned workspace")
    expected_path = module.relative_to(workspace / "cf").as_posix()
    config = args.workspace / "bsl-analyzer.toml"
    if not config.resolve().is_relative_to(workspace):
        raise SystemExit("Refusing to edit a config outside the owned workspace")
    original_config = config.read_text(encoding="utf-8")
    if (not owned_marker_source(original, old, demo) or new in original
            or original_config.count('queryPrefix = "search_query: "') != 1):
        raise SystemExit("Lifecycle fixture/config does not match the expected owned input")
    prepared = None
    if args.prepared_cache is not None:
        try:
            prepared = prepare_cache(args.prepared_cache.resolve(), args.cache.resolve(), dimension)
        except (OSError, sqlite3.Error, ValueError) as error:
            raise SystemExit(f"Prepared cache is incomplete or incompatible: {error}") from error
    environment = {
        "EMBEDDING_URL": "http://127.0.0.1:18884",
        "EMBEDDING_MODEL": model,
        "EMBEDDING_DIM": str(dimension),
        "EMBEDDING_BATCH_SIZE": "4",
        "EMBEDDING_CONCURRENCY": "1",
        "BSL_MCP_EFFECTIVE_EMBEDDING_MAX_INPUT_TOKENS": str(TOKEN_LIMIT),
        "BSL_MCP_EFFECTIVE_EMBEDDING_TOKENIZER_FILE": str(tokenizer),
        "BSL_MCP_EFFECTIVE_EMBEDDING_TOKENIZER_SHA256": TOKENIZER_SHA256,
        "XDG_CACHE_HOME": str(args.cache.with_name(args.cache.name + "-xdg").resolve()),
    }
    proxy, session = ServingProxy(), None
    evidence = {"prepared_cache": prepared} if prepared is not None else {}
    module_created = False
    try:
        if injected is not None:
            module.parent.mkdir(parents=True, exist_ok=True)
            with module.open("x", encoding="utf-8") as created:
                module_created = True
                created.write(injected)
        session = McpSession(args.binary.resolve(), args.workspace.resolve(), args.cache.resolve(), environment)
        if prepared is not None:
            session.search({"action": "search_code", "query": old, "limit": 10})
            wait_runtime_profile(session, model, dimension, prepared)
        status, cold = wait_ready(session, old)
        assert_runtime_profile(status, model, dimension, prepared)
        _, hits = marker(session, old, expected_path)
        evidence["baseline"] = {"cold_seconds": cold, "profile": status.get("embedding_profile"),
                                "model": model, "dimension": dimension, "max_input_tokens": TOKEN_LIMIT,
                                "hits": hits}
        updated = original.replace(old, new)
        module.write_text(updated, encoding="utf-8")
        _, hits = marker(session, new, expected_path)
        old_answer = session.search({"action": "search_code", "query": old, "limit": 10})
        assert not any(h.get("symbol") == old for h in old_answer.get("hits", []))
        evidence["overlay"] = {"hits": hits, "old_symbol_absent": True}
        session.close()
        session = McpSession(args.binary.resolve(), args.workspace.resolve(), args.cache.resolve(), environment)
        status, restart = wait_ready(session, new)
        assert_runtime_profile(status, model, dimension, prepared)
        marker(session, new, expected_path)
        evidence["compatible_restart"] = {"seconds": restart, "profile": status.get("embedding_profile")}
        proxy.close()
        proxy = None
        failed = session.search({"action": "search_code", "query": f"{new} служебная проверка недоступного embedding endpoint", "limit": 10})
        assert failed.get("semantic_failure"), "serving failure must be observable"
        assert any(h.get("symbol") == new for h in failed.get("hits", [])), "lexical marker must survive"
        evidence["serving_failure"] = {"failure": failed["semantic_failure"], "hits": locations(failed)}
        proxy = ServingProxy()
        recovered = session.search({"action": "search_code", "query": f"{new} проверка восстановленного embedding endpoint", "limit": 10})
        assert not recovered.get("semantic_failure")
        evidence["serving_recovery"] = {"hits": locations(recovered)}
        session.close()
        session = None
        before = vectors(args.cache)
        assert before["count"] > 0 and before["identity"]
        assert 'queryPrefix = "search_query: "' in original_config
        config.write_text(original_config.replace('queryPrefix = "search_query: "', 'queryPrefix = "foreign_query: "'))
        session = McpSession(args.binary.resolve(), args.workspace.resolve(), args.cache.resolve(), environment)
        foreign_status = wait_foreign_profile(session, new)
        if not safe_token_failure(foreign_status):
            raise AssertionError("Foreign token layout must report a structured safe semantic_failure")
        answer, hits = marker(session, new, expected_path)
        if not safe_token_failure(answer):
            raise AssertionError("Foreign token layout search must expose a structured safe semantic_failure")
        assert answer.get("degraded"), "foreign profile must visibly degrade semantic search"
        assert all(h.get("modality") == "L" for h in answer.get("hits", [])), "foreign vectors must not contribute hits"
        evidence["foreign_profile"] = {
            "hits": hits,
            "degraded": answer["degraded"],
            "failure": foreign_status["semantic_failure"],
            "status": {
                "state": foreign_status.get("state"),
                "targets": [
                    {key: target[key] for key in ("kind", "state", "reason_code") if key in target}
                    for target in foreign_status.get("indexing", {}).get("targets", [])
                ],
            },
        }
        session.close()
        session = None
        after = vectors(args.cache)
        assert before == after, "foreign profile must preserve old BLOBs and claim"
        evidence["foreign_profile"]["vectors_preserved"] = before
        config.write_text(original_config)
        session = McpSession(args.binary.resolve(), args.workspace.resolve(), args.cache.resolve(), environment)
        status, _ = wait_ready(session, new)
        assert_runtime_profile(status, model, dimension, prepared)
        _, hits = marker(session, new, expected_path)
        evidence["rollback"] = {"hits": hits}
        args.out.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({"passed": list(evidence), "out": str(args.out)}))
    finally:
        try:
            if session:
                session.close()
        finally:
            try:
                if proxy:
                    proxy.close()
            finally:
                try:
                    config.write_text(original_config)
                finally:
                    if injected is None:
                        if module.is_symlink() or not module.resolve().is_relative_to(workspace):
                            raise RuntimeError("Demo marker no longer resolves to its owned workspace file")
                        module.write_text(original, encoding="utf-8")
                    elif (module_created and module.exists() and not module.is_symlink()
                          and module.resolve().is_relative_to(workspace)):
                        owned_versions = {injected, injected.replace(old, new)}
                        if module.read_text(encoding="utf-8") in owned_versions:
                            module.unlink()
                    if evidence and not args.out.exists():
                        args.out.with_suffix(".partial.json").write_text(
                            json.dumps(evidence, ensure_ascii=False, indent=2) + "\n"
                        )


if __name__ == "__main__":
    import sys
    if "--self-test" in sys.argv:
        assert embedding_profile({}) == (DEFAULT_MODEL, DEFAULT_DIM)
        assert embedding_profile({
            "EMBEDDING_MODEL": "user2-code-rs-v1-query-only-256",
            "EMBEDDING_DIM": "256",
        }) == ("user2-code-rs-v1-query-only-256", 256)
        try:
            embedding_profile({"EMBEDDING_MODEL": "user2-code-rs-v1-query-only-256", "EMBEDDING_DIM": "768"})
        except ValueError:
            pass
        else:
            raise AssertionError("mismatched model and dimension must fail before runtime startup")
        demo_source = demo_marker_source("ПилотUSER2_Маркер_a7f91")
        assert demo_source.count("ПилотUSER2_Маркер_a7f91") == 2
        assert owned_marker_source(demo_source, "ПилотUSER2_Маркер_a7f91", True)
        assert not owned_marker_source(
            demo_source + "// unrelated content\n", "ПилотUSER2_Маркер_a7f91", True
        )
        pilot = PILOTS[0]
        assert lifecycle_root(
            pilot / "runtime-token-v1", pilot / "unused-self-test-cache", pilot / "results/self-test.json"
        ) == pilot.resolve()
        try:
            lifecycle_root(ROOT / "crates", pilot / "unused-self-test-cache", pilot / "results/self-test.json")
        except SystemExit:
            pass
        else:
            raise AssertionError("workspace outside an owned pilot must be rejected")
        try:
            lifecycle_root(
                pilot / "runtime-token-v1", pilot / "source-manifest.json",
                pilot / "results/self-test.json",
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("a cache path pointing at existing pilot data must be rejected")
        unf = PILOTS[1]
        try:
            lifecycle_root(unf / "source", unf / "unused-self-test-cache", unf / "results/self-test.json")
        except SystemExit:
            pass
        else:
            raise AssertionError("the read-only UNF source tree must be rejected")

        safe_failure = {"code": "embedding_invalid_config", "request_bytes": None,
                        "max_request_bytes": None}
        waiting = {"indexing": {"targets": [
            {"kind": "lexical", "state": "ready"},
            {"kind": "semantic", "state": "waiting"},
        ]}}
        refused = {"indexing": {"targets": [
            {"kind": "lexical", "state": "ready"},
            {"kind": "semantic", "state": "failed", "reason_code": "native_failure"},
        ]}, "semantic_failure": safe_failure}
        legacy = {"indexing": {"targets": [
            {"kind": "lexical", "state": "ready"},
            {"kind": "semantic", "state": "unknown", "reason_code": "identity_unverified"},
        ]}}
        unsafe = {"indexing": {"targets": [
            {"kind": "lexical", "state": "ready"},
            {"kind": "semantic", "state": "failed", "reason_code": "native_failure"},
        ]}, "semantic_failure": {"code": "embedding_invalid_config", "message": "private detail"}}

        class Session:
            def __init__(self, statuses):
                self.statuses = statuses
                self.calls = 0
                self.started = False

            def search(self, arguments):
                if arguments["action"] == "search_code":
                    self.started = True
                    return {"hits": []}
                assert self.started, "status alone must not start semantic initialization"
                result = self.statuses[min(self.calls, len(self.statuses) - 1)]
                self.calls += 1
                return result

        matching_profile = {"wire_model": "user2-code-rs-v1-query-only-256",
                            "storage_identity": "profile-v1:self-test-token-layout", "dimension": 256}
        class ProfileSession:
            def __init__(self, statuses):
                self.statuses = statuses
                self.calls = 0

            def search(self, arguments, timeout=20):
                assert arguments == {"action": "status"}
                result = self.statuses[min(self.calls, len(self.statuses) - 1)]
                self.calls += 1
                return result

        profile_prepared = {"identity": matching_profile["storage_identity"]}
        profile_wait = ProfileSession([
            {"state": "loading", "indexing": {"targets": [
                {"kind": "semantic", "state": "disabled", "reason_code": "semantic_disabled"},
            ]}},
            {"state": "ready", "embedding_profile": matching_profile},
        ])
        assert wait_runtime_profile(
            profile_wait, "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.2
        )["embedding_profile"] == matching_profile
        assert profile_wait.calls == 2

        class TimeoutThenProfile(ProfileSession):
            def search(self, arguments, timeout=20):
                self.calls += 1
                if self.calls == 1:
                    raise TimeoutError("transient status observation timeout")
                return {"state": "ready", "embedding_profile": matching_profile}

        timeout_wait = TimeoutThenProfile([])
        assert wait_runtime_profile(
            timeout_wait, "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.2
        )["embedding_profile"] == matching_profile
        assert timeout_wait.calls == 2
        mismatch = dict(matching_profile, storage_identity="profile-v1:foreign")
        try:
            wait_runtime_profile(
                ProfileSession([{"state": "ready", "embedding_profile": mismatch}]),
                "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.2,
            )
        except AssertionError:
            pass
        else:
            raise AssertionError("a present mismatched profile must fail immediately")
        try:
            wait_runtime_profile(
                ProfileSession([{"state": "failed"}]),
                "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.2,
            )
        except AssertionError:
            pass
        else:
            raise AssertionError("a terminal status without a profile must fail immediately")
        for terminal in (
            {"state": "loading", "semantic_failure": {"code": "embedding_invalid_config"}},
            {"state": "busy", "indexing": {"targets": [
                {"kind": "semantic", "state": "failed", "reason_code": "native_failure"},
            ]}},
        ):
            try:
                wait_runtime_profile(
                    ProfileSession([terminal]),
                    "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.2,
                )
            except RuntimeError:
                pass
            else:
                raise AssertionError("terminal semantic failures must not be retried as startup")
        started = time.monotonic()
        try:
            wait_runtime_profile(
                ProfileSession([{"state": "loading"}]),
                "user2-code-rs-v1-query-only-256", 256, profile_prepared, timeout=0.02,
            )
        except TimeoutError:
            assert time.monotonic() - started < 0.2
        else:
            raise AssertionError("profile observation must respect its deadline")

        session = Session([waiting, refused])
        assert wait_foreign_profile(session, "Marker") == refused
        assert session.calls == 2
        assert safe_token_failure(refused)
        assert is_foreign_profile_refusal(legacy), "legacy identity refusal remains recognized"
        assert not safe_token_failure(unsafe), "provider/local detail must not qualify as safe failure"
        assert not is_foreign_profile_refusal(unsafe)
        try:
            wait_foreign_profile(Session([unsafe]), "Marker", timeout=0.01)
        except AssertionError as error:
            assert str(error) == "Foreign profile did not reach the expected semantic refusal"
        else:
            raise AssertionError("unstructured native failure must not satisfy foreign-profile wait")

        assert tokenizer_artifact().is_file()
        prepared_cache_self_test()
        prepared = {"identity": "profile-v1:self-test-token-layout"}
        assert_runtime_profile({"embedding_profile": {
            "wire_model": "user2-code-rs-v1-query-only-256",
            "storage_identity": prepared["identity"], "dimension": 256,
        }}, "user2-code-rs-v1-query-only-256", 256, prepared)
        try:
            assert_runtime_profile({"embedding_profile": {}}, "user2-code-rs-v1-query-only-256", 256, prepared)
        except AssertionError:
            pass
        else:
            raise AssertionError("runtime must reject a prepared cache with a foreign active profile")
        print("lifecycle profile, prepared-cache copy, foreign-layout refusal, and safe failure guards passed")
    else:
        main()
