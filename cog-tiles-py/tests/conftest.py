import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

SYNTHETIC = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "cog" / "synthetic"


class RangeServer(ThreadingHTTPServer):
    """Serves the files under `root` over HTTP range requests. While `holding`, each GET waits for `released`."""

    daemon_threads = True

    def __init__(self, root=SYNTHETIC):
        super().__init__(("127.0.0.1", 0), RangeHandler)
        self.root = root
        self.holding = False
        self.held = threading.Event()
        self.released = threading.Event()
        self.served_on_release = False
        self.gets = 0

    def handle_error(self, request, client_address):
        pass


class RangeHandler(BaseHTTPRequestHandler):
    def do_HEAD(self):
        self.send_headers(200, (self.server.root / self.path.lstrip("/")).stat().st_size)

    def do_GET(self):
        data = (self.server.root / self.path.lstrip("/")).read_bytes()
        self.server.gets += 1
        if self.server.holding:
            self.server.held.set()
            self.server.served_on_release = self.server.released.wait(timeout=5)
        start, end = map(int, self.headers["Range"].removeprefix("bytes=").split("-"))
        self.send_headers(206, end - start + 1, content_range=f"bytes {start}-{end}/{len(data)}")
        self.wfile.write(data[start : end + 1])

    def send_headers(self, status, length, content_range=None):
        self.send_response(status)
        self.send_header("Content-Length", str(length))
        self.send_header("Last-Modified", "Thu, 22 Oct 2026 00:00:00 GMT")
        if content_range:
            self.send_header("Content-Range", content_range)
        self.end_headers()

    def log_message(self, format, *args):
        pass


def serve(root):
    server = RangeServer(root)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield server
    server.released.set()
    server.shutdown()
    server.server_close()


@pytest.fixture
def server():
    yield from serve(SYNTHETIC)


@pytest.fixture
def fixture_server():
    yield from serve(SYNTHETIC.parent)
