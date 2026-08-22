#!/usr/bin/env python3
"""Deterministic loopback HTTP/TCP/UDP peer for Sovereign Grid acceptance."""

from __future__ import annotations

import argparse
import http.server
import json
import socket
import threading
import time
from pathlib import Path


HOST = "127.0.0.1"
HTTP_PORT = 18991
TCP_PORT = 18992
UDP_PORT = 18993
RAD_LISTENER_PORT = 18994


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _reply(self, body: bytes) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802 - stdlib callback name
        self._reply(b"get-ok")

    def do_POST(self) -> None:  # noqa: N802 - stdlib callback name
        length = int(self.headers.get("Content-Length", "0"))
        payload = self.rfile.read(length)
        self._reply(b"post:" + payload)


def tcp_echo(stop: threading.Event) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((HOST, TCP_PORT))
        listener.listen()
        listener.settimeout(0.1)
        while not stop.is_set():
            try:
                stream, _ = listener.accept()
            except TimeoutError:
                continue
            with stream:
                stream.settimeout(2.0)
                payload = stream.recv(65536)
                stream.sendall(payload)


def udp_echo(stop: threading.Event) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer:
        peer.bind((HOST, UDP_PORT))
        peer.settimeout(0.1)
        while not stop.is_set():
            try:
                payload, address = peer.recvfrom(65536)
            except TimeoutError:
                continue
            peer.sendto(payload, address)


def connect_to_rad_listener(stop: threading.Event) -> None:
    for payload in (b"accept-one", b"accept-two"):
        while not stop.is_set():
            try:
                with socket.create_connection((HOST, RAD_LISTENER_PORT), timeout=0.2) as stream:
                    stream.sendall(payload)
                    time.sleep(0.05)
                break
            except OSError:
                time.sleep(0.02)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ready", required=True, type=Path)
    parser.add_argument("--stop", required=True, type=Path)
    args = parser.parse_args()
    args.ready.parent.mkdir(parents=True, exist_ok=True)
    args.ready.unlink(missing_ok=True)
    args.stop.unlink(missing_ok=True)

    stop = threading.Event()
    httpd = http.server.ThreadingHTTPServer((HOST, HTTP_PORT), Handler)
    threads = [
        threading.Thread(target=httpd.serve_forever, daemon=True),
        threading.Thread(target=tcp_echo, args=(stop,), daemon=True),
        threading.Thread(target=udp_echo, args=(stop,), daemon=True),
        threading.Thread(target=connect_to_rad_listener, args=(stop,), daemon=True),
    ]
    for thread in threads:
        thread.start()
    args.ready.write_text(
        json.dumps({"http": HTTP_PORT, "tcp": TCP_PORT, "udp": UDP_PORT}),
        encoding="utf-8",
    )
    print("sovereign-grid protocol harness ready", flush=True)
    try:
        while not args.stop.exists():
            time.sleep(0.05)
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        httpd.shutdown()
        httpd.server_close()
        args.ready.unlink(missing_ok=True)
        args.stop.unlink(missing_ok=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
