"""Serve the course admin build and proxy its API to the local Rustsky server."""

from functools import partial
from http.client import HTTPConnection, HTTPException
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class PreviewHandler(SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/api/"):
            self.proxy_api()
        else:
            super().do_GET()

    def do_HEAD(self):
        if self.path.startswith("/api/"):
            self.proxy_api()
        else:
            super().do_HEAD()

    def do_POST(self):
        self.proxy_api()

    do_PUT = do_PATCH = do_DELETE = do_POST

    def proxy_api(self):
        if not self.path.startswith("/api/"):
            self.send_error(404)
            return
        if self.headers.get("Transfer-Encoding"):
            self.send_error(400, "Unsupported request encoding")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self.send_error(400, "Invalid Content-Length")
            return
        if not 0 <= length <= 2 * 1024 * 1024:
            self.send_error(413)
            return

        headers = {
            name: self.headers[name]
            for name in ("Content-Type", "Accept", "token")
            if name in self.headers
        }
        body = self.rfile.read(length) if length else None
        connection = HTTPConnection("127.0.0.1", 8080, timeout=10)
        try:
            target = "/admin/" + self.path.removeprefix("/api/")
            connection.request(self.command, target, body, headers)
            response = connection.getresponse()
            payload = response.read()
            status = response.status
            content_type = response.getheader("Content-Type")
        except (OSError, HTTPException):
            status, content_type = 502, "application/json; charset=utf-8"
            payload = '{"code":0,"msg":"Rustsky API 未启动或未响应","data":null}'.encode()
        finally:
            connection.close()

        self.send_response(status)
        if content_type:
            self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    static = root / ".preview/course/Web/nginx-1.20.2/html"
    if not (static / "sky/index.html").is_file():
        raise SystemExit(f"Missing admin build: {static / 'sky/index.html'}")
    handler = partial(PreviewHandler, directory=str(static))
    with ThreadingHTTPServer(("127.0.0.1", 8888), handler) as server:
        print("Rustsky admin preview: http://127.0.0.1:8888/sky/", flush=True)
        server.serve_forever()
