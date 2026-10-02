#!/usr/bin/env python3
"""Owned loopback endpoint for measuring serving failure without stopping model workers."""

import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class ServingProxy:
    def __init__(self, target="http://127.0.0.1:18881", port=18884):
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def forward(self):
                data = self.rfile.read(int(self.headers.get("Content-Length", "0"))) if self.command == "POST" else None
                request = urllib.request.Request(target + self.path, data=data,
                                                 headers={"Content-Type": "application/json"})
                try:
                    with urllib.request.urlopen(request, timeout=120) as response:
                        status, body = response.status, response.read()
                except urllib.error.HTTPError as error:
                    status, body = error.code, error.read()
                except (OSError, TimeoutError):
                    status, body = 503, b'{"error":"owned upstream unavailable"}'
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            do_GET = forward
            do_POST = forward

        self.server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


def self_test():
    proxy = ServingProxy(target="http://127.0.0.1:1", port=0)
    port = proxy.server.server_port
    try:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2)
        except urllib.error.HTTPError as error:
            assert error.code == 503
        else:
            raise AssertionError("failed upstream must be observable")
    finally:
        proxy.close()
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2)
    except urllib.error.URLError:
        pass
    else:
        raise AssertionError("owned endpoint must close")
    print("Owned serving failure and shutdown passed (fixture)")


if __name__ == "__main__":
    self_test()
