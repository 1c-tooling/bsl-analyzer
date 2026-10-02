#!/usr/bin/env python3
"""Real stdio MCP client for the isolated pilot; records locations, not source bodies."""

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


class McpSession:
    def __init__(self, binary, workspace, cache, environment=None):
        env = dict(os.environ)
        env.update(environment or {})
        env["BSL_MCP_BROKER"] = "0"
        self.child = subprocess.Popen(
            [str(binary), "mcp", "serve", "--profile", "workspace", "--mode", "stdio",
             "--source-dir", str(workspace), "--cache-dir", str(cache)],
            env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )
        self.messages = queue.Queue()
        self.next_id = 0
        threading.Thread(target=self._read, daemon=True).start()
        try:
            self.request("initialize", {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "user2-pilot", "version": "1"},
            }, timeout=60)
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise

    def _read(self):
        for line in self.child.stdout:
            try:
                self.messages.put(json.loads(line))
            except json.JSONDecodeError:
                continue
        self.messages.put(None)

    def send(self, value):
        self.child.stdin.write(json.dumps(value, ensure_ascii=False) + "\n")
        self.child.stdin.flush()

    def request(self, method, params, timeout=20):
        self.next_id += 1
        ident = self.next_id
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("MCP response deadline exceeded")
            message = self.messages.get(timeout=remaining)
            if message is None:
                raise RuntimeError("Owned MCP process closed stdout")
            if message.get("id") == ident:
                if "error" in message:
                    # The raw error may carry provider details: do not log it.
                    raise RuntimeError("MCP returned an RPC error")
                return message["result"]

    def search(self, arguments, timeout=20):
        result = self.request("tools/call", {"name": "search", "arguments": arguments}, timeout)
        if result.get("isError"):
            raise RuntimeError("MCP search returned a tool error")
        if "structuredContent" in result:
            return result["structuredContent"]
        for block in result.get("content", []):
            if block.get("type") == "text":
                try:
                    return json.loads(block["text"])
                except json.JSONDecodeError:
                    continue
        raise RuntimeError("MCP search response lacks structured content")

    def close(self):
        if self.child.stdin:
            self.child.stdin.close()
        try:
            self.child.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.child.terminate()
            try:
                self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait()


def locations(answer):
    return [{key: hit[key] for key in (
        "path", "root_id", "symbol", "parent_symbol", "original_symbol", "line_start", "line_end",
        "location", "source_span", "score", "modality"
    ) if key in hit} for hit in answer.get("hits", [])]


def document_id(hit):
    path = hit.get("path", "").replace("\\", "/")
    root = hit.get("root_id") or "cf"
    if not path.startswith(root + "/"):
        path = root + "/" + path
    return path + "::" + hit.get("symbol", "")


def observation_search(session, arguments, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return None
    try:
        return session.search(arguments, timeout=min(20, remaining))
    except (TimeoutError, queue.Empty):
        return None


def wait_for_status(session, budget):
    deadline = time.monotonic() + budget
    while time.monotonic() < deadline:
        status = observation_search(session, {"action": "status"}, deadline)
        if status is None:
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
            continue
        targets = status.get("indexing", {}).get("targets", [])
        semantic = next((t for t in targets if t.get("kind") == "semantic"), None)
        if status.get("semantic_failure") or (semantic and semantic.get("state") == "failed"):
            raise RuntimeError("Pilot semantic indexing failed")
        return status
    raise TimeoutError("Owned pilot status observation exceeded ready budget")


def wait_ready(session, query, budget=1800):
    start = time.monotonic()
    deadline = start + budget
    previous, last_report = None, 0.0
    observation_search(session, {"action": "search_code", "query": query, "limit": 10}, deadline)
    while time.monotonic() < deadline:
        status = observation_search(session, {"action": "status"}, deadline)
        if status is None:
            now = time.monotonic()
            observed = {"state": "observation_timeout"}
            if observed != previous or now - last_report > 30:
                print(json.dumps({"readiness": observed, "seconds": now - start}), flush=True)
                previous, last_report = observed, now
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
            continue
        targets = status.get("indexing", {}).get("targets", [])
        semantic = next((t for t in targets if t.get("kind") == "semantic"), None)
        if status.get("semantic_failure") or (semantic and semantic.get("state") == "failed"):
            raise RuntimeError("Pilot semantic indexing failed")
        ready = (status.get("state") == "ready" and semantic and semantic.get("state") == "ready"
                 and all(t.get("state") not in ("waiting", "running") for t in targets))
        warmup = (observation_search(
            session, {"action": "search_code", "query": query, "limit": 10}, deadline)
                  if ready else {"status": "not_ready", "retry_after_ms": 1500})
        if warmup is None:
            warmup = {"status": "not_ready", "retry_after_ms": 100}
        observed = {"state": status.get("state"), "targets": [
            {"kind": t.get("kind"), "state": t.get("state"), "reason_code": t.get("reason_code")}
            for t in targets], "warmup_status": warmup.get("status"),
            "retry_after_ms": warmup.get("retry_after_ms")}
        now = time.monotonic()
        if observed != previous or now - last_report > 30:
            print(json.dumps({"readiness": observed, "seconds": now - start}), flush=True)
            previous, last_report = observed, now
        if ready and warmup.get("status") != "not_ready":
            return status, time.monotonic() - start
        time.sleep(min(warmup.get("retry_after_ms", 1500) / 1000, 5,
                       max(0, deadline - time.monotonic())))
    raise TimeoutError("Owned pilot did not become semantically ready")


def wait_background(session, query, budget=30):
    deadline = time.monotonic() + budget
    while time.monotonic() < deadline:
        # Workspace search starts lazy refresh; status only observes the current state.
        search = observation_search(
            session, {"action": "search_code", "query": query, "limit": 10}, deadline)
        if search is None:
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
            continue
        status = observation_search(session, {"action": "status"}, deadline)
        if status is None:
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
            continue
        targets = status.get("indexing", {}).get("targets", [])
        semantic = next((t for t in targets if t.get("kind") == "semantic"), None)
        if status.get("semantic_failure") or (semantic and semantic.get("state") == "failed"):
            raise RuntimeError("Pilot semantic indexing failed")
        if any(t.get("state") == "running" for t in targets):
            return
        time.sleep(min(.1, max(0, deadline - time.monotonic())))
    raise RuntimeError("Owned background indexing was not observed")


def self_test():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        binary = root / "fake-mcp"
        binary.write_text(
            "#!" + sys.executable + "\n"
            "import sys,json\n"
            "for line in sys.stdin:\n"
            " m=json.loads(line)\n"
            " if 'id' not in m: continue\n"
            " r={} if m['method']=='initialize' else {'structuredContent':"
            "{'hits':[{'path':'A.bsl','symbol':'Marker','snippet':'private body'}]}}\n"
            " print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)\n"
        )
        binary.chmod(0o700)
        session = McpSession(binary, root, root / "cache")
        try:
            answer = session.search({"action": "search_code", "query": "Marker"})
            assert locations(answer) == [{"path": "A.bsl", "symbol": "Marker"}]
        finally:
            session.close()
        assert session.child.returncode == 0
        assert document_id({"path": "A.bsl", "symbol": "F"}) == "cf/A.bsl::F"
        assert document_id({"root_id": "cfe/X", "path": "A.bsl", "symbol": "F"}) == "cfe/X/A.bsl::F"
    class Background:
        def __init__(self):
            self.started = False
            self.status_polls = 0

        def search(self, arguments, timeout=20):
            if arguments["action"] == "search_code":
                self.started = True
                return {}
            assert self.started
            self.status_polls += 1
            state = "running" if self.status_polls >= 2 else "waiting"
            return {"indexing": {"targets": [{"state": state}]}}

    background = Background()
    wait_background(background, "Marker", budget=1)
    assert background.status_polls == 2
    class Readiness:
        def __init__(self):
            self.polls = 0
            self.searches = 0

        def search(self, arguments, timeout=20):
            if arguments["action"] == "search_code":
                assert self.polls == 0 or self.polls >= 2
                self.searches += 1
                return {}
            self.polls += 1
            return {"state": "ready", "indexing": {"targets": [
                {"kind": "semantic", "state": "waiting" if self.polls == 1 else "ready"}],
                "retry_after_ms": 1}}

    readiness = Readiness()
    wait_ready(readiness, "Marker")
    assert readiness.searches == 2
    class TransientReadiness:
        def __init__(self):
            self.status_polls = 0
            self.searches = 0

        def search(self, arguments, timeout=20):
            assert 0 < timeout <= 2
            if arguments["action"] == "search_code":
                self.searches += 1
                return {"status": "ready"}
            self.status_polls += 1
            if self.status_polls == 1:
                raise queue.Empty
            state = "running" if self.status_polls == 2 else "ready"
            return {"state": state, "indexing": {"targets": [
                {"kind": "semantic", "state": state}], "retry_after_ms": 1}}

    transient = TransientReadiness()
    ready_status, _ = wait_ready(transient, "Marker", budget=2)
    assert ready_status["indexing"]["targets"][0]["state"] == "ready"
    assert transient.status_polls == 3 and transient.searches == 2

    class TransientStatus:
        def __init__(self):
            self.polls = 0
            self.actual = {"state": "indexing", "indexing": {"targets": [
                {"kind": "semantic", "state": "running"}]}}

        def search(self, arguments, timeout=20):
            assert arguments == {"action": "status"} and 0 < timeout <= 1
            self.polls += 1
            if self.polls == 1:
                raise queue.Empty
            return self.actual

    transient_status = TransientStatus()
    assert wait_for_status(transient_status, 1) is transient_status.actual
    assert transient_status.polls == 2

    class FailedReadiness:
        def __init__(self, failure):
            self.failure = failure

        def search(self, arguments, timeout=20):
            if arguments["action"] == "search_code":
                return {}
            if self.failure == "eof":
                raise RuntimeError("Owned MCP process closed stdout")
            return {"semantic_failure": True,
                    "indexing": {"targets": [{"kind": "semantic", "state": "failed"}]}}

    for failure, expected in (("eof", "closed stdout"), ("semantic", "semantic indexing failed")):
        try:
            wait_ready(FailedReadiness(failure), "Marker", budget=1)
        except RuntimeError as error:
            assert expected in str(error)
        else:
            raise AssertionError(f"{failure} readiness failure was swallowed")

    class NeverReady:
        def search(self, arguments, timeout=20):
            if arguments["action"] == "search_code":
                return {}
            raise queue.Empty

    bounded_start = time.monotonic()
    try:
        wait_ready(NeverReady(), "Marker", budget=0.03)
    except TimeoutError as error:
        assert "did not become semantically ready" in str(error)
    else:
        raise AssertionError("readiness timeout exceeded its configured budget")
    assert time.monotonic() - bounded_start < 0.25

    status_start = time.monotonic()
    try:
        wait_for_status(NeverReady(), 0.03)
    except TimeoutError as error:
        assert "status observation" in str(error)
    else:
        raise AssertionError("status observation exceeded its configured budget")
    assert time.monotonic() - status_start < 0.25
    print("MCP transport/shutdown and source-body filtering passed (fixture, not runtime acceptance)")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--qrels", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--ready-budget", type=float, default=1800,
                        help="Seconds to wait for cold readiness and background indexing to start")
    parser.add_argument("--background", action="store_true", help="Measure the same queries during owned fixture indexing")
    parser.add_argument("--parent-aware", action="store_true", help="Opt in to the frozen first-30 parent evaluator and separate public top-10 evidence")
    parser.add_argument("--evaluator-plan", type=Path, help="Previously frozen plan for this corpus")
    args = parser.parse_args()
    if args.out.exists():
        raise SystemExit("Refusing to overwrite existing runtime evidence")
    bundle = json.loads(args.qrels.read_text())
    queries = bundle["queries"]
    if len(queries) < 30:
        raise SystemExit("Warm acceptance needs at least 30 fixed questions")
    evaluator = None
    if args.parent_aware:
        from parent_evaluator import FrozenEvaluator
        summary = args.qrels.with_name("corpus-summary.json")
        evaluator = FrozenEvaluator.load(args.qrels, summary, args.evaluator_plan or args.qrels.parent / "results/token-budget-evaluator-v2.json")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    background, background_directory = None, None
    if args.background:
        names = ("user2-pilot", "user2-unf-pilot") if args.parent_aware else ("user2-pilot",)
        pilots = [Path(__file__).resolve().parents[2] / "experiments" / name for name in names]
        pilot = next((root for root in pilots if args.workspace.resolve().is_relative_to(root.resolve())), pilots[0])
        if not args.workspace.resolve().is_relative_to(pilot.resolve()):
            raise SystemExit("Background indexing requires an isolated pilot workspace")
    session = McpSession(args.binary.resolve(), args.workspace.resolve(), args.cache.resolve())
    try:
        wait_ready(session, next(iter(queries.values())), args.ready_budget)
        cold_seconds = time.monotonic() - start
        if args.background:
            background_directory = Path(tempfile.mkdtemp(prefix="background-", dir=pilot))
            background_root = background_directory
            background_workspace = background_root / "workspace"
            shutil.copytree(args.workspace, background_workspace)
            load_file = background_workspace / "cf/CommonModules/ПилотUSER2Нагрузка/Ext/Module.bsl"
            assert not load_file.exists() and not load_file.is_symlink()
            assert load_file.resolve().is_relative_to(background_workspace.resolve())
            load_file.parent.mkdir(parents=True, exist_ok=True)
            with load_file.open("x") as fixture:
                fixture.write("\n".join(
                    f"Функция ПилотНагрузка{i}() Экспорт\nПерем Результат;\nРезультат = {i};\nВозврат Результат;\nКонецФункции\n"
                    for i in range(1000)))
            background = McpSession(args.binary.resolve(), background_workspace, background_root / "cache")
            wait_background(background, next(iter(queries.values())), budget=args.ready_budget)
        with args.out.open("x") as record:
            status = wait_for_status(session, args.ready_budget)
            record.write(json.dumps({"stage": "ready", "cold_seconds": cold_seconds,
                                     "status": status, "retained_background_artifacts": str(background_directory) if background_directory else None}, ensure_ascii=False) + "\n")
            record.flush()
            elapsed = []
            rankings = []
            failures, active_samples = 0, 0
            for qid, query in queries.items():
                current = wait_for_status(background or session, args.ready_budget)
                active = any(t.get("state") == "running" for t in current.get("indexing", {}).get("targets", []))
                active_samples += int(active)
                raw_answer, raw_seconds = None, None
                if evaluator:
                    begin = time.monotonic()
                    try:
                        raw_answer = session.search({"action": "search_code", "query": query, "limit": 30})
                    except (RuntimeError, OSError, TimeoutError, queue.Empty):
                        if not args.background:
                            raise
                        raw_answer = {"status": "rpc_failure", "hits": []}
                    raw_seconds = time.monotonic() - begin
                begin = time.monotonic()
                try:
                    answer = session.search({"action": "search_code", "query": query, "limit": 10})
                except (RuntimeError, OSError, TimeoutError, queue.Empty):
                    if not args.background:
                        raise
                    answer = {"status": "rpc_failure", "hits": []}
                seconds = time.monotonic() - begin
                elapsed.append(seconds)
                if evaluator:
                    evaluated = evaluator.evaluate(qid, raw_answer.get("hits", []), locations(answer))
                    rankings.append(evaluated["ranked_doc_ids"])
                else:
                    rankings.append(list(dict.fromkeys(document_id(h) for h in answer.get("hits", []))))
                failed = bool(answer.get("semantic_failure")) or answer.get("status") in ("not_ready", "rpc_failure")
                if evaluator:
                    failed = failed or bool(raw_answer.get("semantic_failure")) or raw_answer.get("status") in ("not_ready", "rpc_failure")
                failures += int(failed)
                row = {"qid": qid, "seconds": seconds,
                    "owned_indexing_active": active,
                    "status": answer.get("status"), "hits": locations(answer),
                    "freshness": answer.get("freshness"),
                    "semantic_failure": answer.get("semantic_failure")}
                if evaluator:
                    row.update({"raw_seconds": raw_seconds, "raw_status": raw_answer.get("status"),
                                "raw_hits": locations({"hits": raw_answer.get("hits", [])[:30]}), **evaluated})
                record.write(json.dumps(row, ensure_ascii=False) + "\n")
                record.flush()
            ordered = sorted(elapsed)
            result = {"samples": len(elapsed), "background": args.background,
                      "owned_indexing_active_samples": active_samples, "failure_rate": failures / len(elapsed),
                      "p50": ordered[len(ordered) // 2],
                      "p95": ordered[min(len(ordered)-1, int(len(ordered)*.95))],
                      "all_under_12s": all(t < 12 for t in elapsed)}
            if evaluator:
                result["ranking_basis"] = "first_30_raw_hits_stable_parent_dedup_top_10"
                result["latency_basis"] = "separate_public_top_10_mcp_request"
            from c1rb.evaluation import compute_metrics
            result["metrics"] = compute_metrics(rankings, list(queries), bundle["qrels"], k_values=(10,))
            record.write(json.dumps({"stage": "latency", **result}) + "\n")
            print(json.dumps(result))
            if args.background and active_samples != len(queries):
                raise RuntimeError("Owned indexing did not overlap every required warm query")
            if not args.background and (not result["all_under_12s"] or result["p95"] >= 12):
                raise RuntimeError("Idle interactive latency exceeded the agreed 12-second gate")
    finally:
        session.close()
        if background:
            background.close()


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        main()
