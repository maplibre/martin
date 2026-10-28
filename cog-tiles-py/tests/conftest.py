import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures"
SYNTHETIC = FIXTURES / "cog" / "synthetic"
CASSETTE = FIXTURES / "render_cassette" / "earth-search.aws.element84.com"
EARTH_SEARCH = "https://earth-search.aws.element84.com"


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
    threading.Thread(target=server.serve_forever, args=(0.01,), daemon=True).start()
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


def canonical(value):
    if isinstance(value, float):
        return round(value, 6)
    if isinstance(value, dict):
        return {key: canonical(item) for key, item in value.items()}
    if isinstance(value, list):
        return [canonical(item) for item in value]
    return value


class StacServer(ThreadingHTTPServer):
    """Replays the recorded Earth Search answers, its links pointed here and its NAIP assets at `files`."""

    daemon_threads = True

    def __init__(self, files):
        super().__init__(("127.0.0.1", 0), StacHandler)
        self.url = f"http://127.0.0.1:{self.server_port}"
        self.files = files
        self.searches = []
        self.user_agents = set()
        self.answer = None

    def handle_error(self, request, client_address):
        pass

    def rewrite(self, body):
        return body.replace(EARTH_SEARCH, self.url).replace("s3://naip-analytic/", f"{self.files}/stac/naip-analytic/")


class StacHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.user_agents.add(self.headers["User-Agent"])
        path = CASSETTE / self.path.lstrip("/")
        self.reply(path.read_text() if path.is_file() else None)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.user_agents.add(self.headers["User-Agent"])
        self.server.searches.append(body)
        if self.server.answer is not None:
            status, answer = self.server.answer(body)
            return self.reply(json.dumps(answer), status)
        for request in (CASSETTE / f"{self.path.lstrip('/')}.post").glob("*.request.json"):
            if canonical(json.loads(request.read_text())) == canonical(body):
                return self.reply(request.with_name(request.name.replace(".request", "")).read_text())
        self.reply(None)

    def reply(self, body, status=200):
        if body is None:
            status, body = 404, '{"code": "NotFound"}'
        data = self.server.rewrite(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, format, *args):
        pass


@pytest.fixture
def stac(fixture_server):
    server = StacServer(f"http://127.0.0.1:{fixture_server.server_port}")
    threading.Thread(target=server.serve_forever, args=(0.01,), daemon=True).start()
    yield server
    server.shutdown()
    server.server_close()
