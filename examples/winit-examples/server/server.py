#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# 这个代码案例用于展示如何让 web 基于 fetch 实现 io 文件的。
# 接口还是 io 接口，但是重点在于 web 上走 io 是通过 fetch 实现的。
# 本文件演示「保存 / 读取 / 存在探测」三个 io 操作的服务端实现，
# 并落地四项防护设计：
#   1. MD5 校验：上传完校验并回写 X-Content-MD5 响应头，防止传输损坏
#   2. 临时分片自动清理：分片临时目录定时清理（应用层分片扩展预留）
#   3. 大小上限：默认 100MB，防止恶意超大文件占满磁盘（超限返回 413）
#   4. 基础错误返回：404 分片/文件不存在、413 超限、400 非法路径（防目录穿越）
#
# 额外内置网络探针服务（net 模块验证用）：
#   TCP echo :8022（4 字节大端长度前缀分帧，与 TcpConn 契约一致）
#   UDP 发现 :8023（STARFISH_PROBE → STARFISH_SERVER；其余载荷回显）
#   WebSocket echo :8024（RFC6455 最小实现，net 的 Web TCP 走此端点）
#
# 运行（无第三方依赖，纯标准库）：
#   python examples/server/server.py --port 8021 --saves-dir ./saves --web-dir ./web
#
# 与 starfish::base::io 的契约（wasm 端）：
#   io::read("saves/recording.wav")   → GET  /saves/recording.wav  → 文件字节
#   io::write("saves/recording.wav")  → POST /saves/recording.wav（body=原始字节）
#   io::exists("saves/recording.wav") → HEAD /saves/recording.wav → 200/404
#
# 安全：保存名只允许单段安全字符（字母/数字/._-），写入固定在 saves 目录内，
#       拒绝路径分隔符与 ..（防目录穿越）；静态目录同样锁定在 --web-dir 内。

import argparse
import base64
import hashlib
import html
import http.server
import pathlib
import re
import socket
import struct
import threading
import time
from datetime import datetime

MAX_UPLOAD_BYTES = 100 * 1024 * 1024  # 单文件大小上限（100MB）
SAFE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")  # 单段安全文件名
CHUNK_TTL_SECONDS = 3600  # 分片临时目录的清理时限（1 小时未完成即删）

SAVES_DIR = pathlib.Path("./saves").resolve()
WEB_DIR = pathlib.Path("./web").resolve()
RESOURCES_DIR = pathlib.Path("./resources").resolve()
CHUNKS_DIR = pathlib.Path("./saves/.chunks").resolve()


def is_safe_name(name: str) -> bool:
    """单段安全文件名：阻断路径穿越 / 隐藏文件 / 子目录"""
    return bool(SAFE_NAME.match(name)) and ".." not in name


def md5_of(path: pathlib.Path) -> str:
    h = hashlib.md5()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(65536), b""):
            h.update(block)
    return h.hexdigest()


def cleanup_stale_chunks() -> int:
    """清理超时分片：CHUNKS_DIR 下 mtime 超过 TTL 的文件"""
    now = time.time()
    removed = 0
    if not CHUNKS_DIR.exists():
        return 0
    for f in CHUNKS_DIR.rglob("*"):
        if f.is_file() and now - f.stat().st_mtime > CHUNK_TTL_SECONDS:
            f.unlink(missing_ok=True)
            removed += 1
    return removed


class IoHandler(http.server.BaseHTTPRequestHandler):
    server_version = "StarfishIoDemo/1.0"

    def _cors(self) -> None:
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, HEAD, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")

    def _reply(self, code: int, body: bytes = b"", ctype: str = "text/plain; charset=utf-8") -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self._cors()
        self.end_headers()
        if body:
            self.wfile.write(body)

    def _resolve_saves(self):
        """解析 /saves/<name> → 磁盘路径；非法名返回 None（由调用方回 400/404）"""
        name = self.path.removeprefix("/saves/").strip("/")
        if not name or "/" in name or "\\" in name or ".." in name or not is_safe_name(name):
            return None
        return SAVES_DIR / name

    # ── io::exists（HEAD）与 io::read（GET）──
    def _serve_read(self, head_only: bool) -> None:
        if not self.path.startswith("/saves/") and not head_only:
            self._serve_static()
            return
        target = self._resolve_saves()
        if target is None:
            self._reply(400, b"bad save name")
            return
        if not target.is_file():
            self._reply(404, b"not found")
            return
        data = target.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("X-Content-MD5", md5_of(target))
        self._cors()
        self.end_headers()
        if not head_only:
            self.wfile.write(data)

    # ── io::write（POST）──
    def _handle_upload(self) -> None:
        target = self._resolve_saves()
        if target is None:
            self._reply(400, b"bad save name")
            return
        length = int(self.headers.get("Content-Length", "0"))
        if length > MAX_UPLOAD_BYTES:
            self._reply(413, b"file too large")
            return

        data = self.rfile.read(length)
        SAVES_DIR.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)

        checksum = md5_of(target)
        stamp = datetime.now().strftime("%H:%M:%S")
        print(f"[io] 已保存 {target.name}（{len(data)}B，md5 {checksum}）@ {stamp}")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("X-Content-MD5", checksum)
        self._cors()
        self.end_headers()
        self.wfile.write(checksum.encode())

    # ── 静态页服务（同源部署：wasm 页面与本服务器同源 → fetch 无 CORS 问题）──
    def _serve_static(self) -> None:
        path = self.path.strip("/")
        # 首页映射到 wasm 入口页
        if path in ("", "index.html"):
            file = WEB_DIR / "index.html"
            if file.is_file():
                self._reply(200, file.read_bytes(), "text/html; charset=utf-8")
            else:
                self._reply(200, b"starfish io demo server (web/index.html not found)")
            return
        file = WEB_DIR / path
        if not file.is_file():
            # 资源目录挂载：URL `resources/<rel>` → RESOURCES_DIR/<rel>
            # （probe 资产逻辑路径 = "resources/..."，三平台同一字符串；带穿越防护）
            if RESOURCES_DIR is not None and path.startswith("resources/"):
                rel = path[len("resources/"):]
                cand = (RESOURCES_DIR / rel).resolve()
                if cand.is_file() and cand.is_relative_to(RESOURCES_DIR):
                    file = cand
        if file.is_file():
            ext = file.suffix.lower()
            ctype = {
                ".html": "text/html; charset=utf-8", ".js": "text/javascript",
                ".wasm": "application/wasm", ".json": "application/json",
                ".mp4": "video/mp4", ".ttf": "font/ttf",
                ".wav": "application/octet-stream", ".png": "image/png",
                ".jpg": "image/jpeg",
            }.get(ext, "application/octet-stream")
            self._reply(200, file.read_bytes(), ctype)
        else:
            self._reply(404, b"not found")

    def do_GET(self) -> None:
        if self.path.startswith("/saves/"):
            self._serve_read(head_only=False)
        elif self.path == "/saves":
            names = sorted(f.name for f in SAVES_DIR.glob("*") if f.is_file())
            body = "<br>".join(html.escape(n) for n in names) or "(empty)"
            self._reply(200, f"<h3>saves</h3>{body}".encode(), "text/html; charset=utf-8")
        else:
            self._serve_static()

    def do_HEAD(self) -> None:
        if self.path.startswith("/saves/"):
            self._serve_read(head_only=True)
        else:
            self._reply(404)

    def do_POST(self) -> None:
        if self.path.startswith("/saves/"):
            self._handle_upload()
        elif self.path.startswith("/chunks/"):
            self._reply(501, b"chunked upload: application-layer extension")
        else:
            self._reply(404)

    def do_OPTIONS(self) -> None:
        self._reply(204)

    def log_message(self, fmt: str, *args) -> None:
        print(f"[http] {self.address_string()} {fmt % args}")


class IoServer(http.server.ThreadingHTTPServer):
    """定时清理超时分片"""

    def __init__(self, addr, handler):
        super().__init__(addr, handler)
        self._last_cleanup = time.time()

    def handle_error(self, request, client_address):
        import traceback
        traceback.print_exc()

    def service_actions(self):
        super().service_actions()
        if time.time() - self._last_cleanup > 600:
            self._last_cleanup = time.time()
            removed = cleanup_stale_chunks()
            if removed:
                print(f"[io] 清理超时分片 {removed} 个")


# ── net 探针服务：TCP echo（framing）+ UDP 发现/回显 + WebSocket echo ──────


def tcp_echo_server(port: int) -> None:
    """TCP echo：与 TcpConn 契约一致（4 字节大端长度前缀分帧）"""
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", port))
    srv.listen(8)
    print(f"[net] TCP echo 监听 :{port}")

    def client(conn: socket.socket) -> None:
        with conn:
            while True:
                head = b""
                while len(head) < 4:
                    chunk = conn.recv(4 - len(head))
                    if not chunk:
                        return
                    head += chunk
                (length,) = struct.unpack(">I", head)
                if length == 0 or length > 16 * 1024 * 1024:
                    return
                payload = b""
                while len(payload) < length:
                    chunk = conn.recv(length - len(payload))
                    if not chunk:
                        return
                    payload += chunk
                conn.sendall(head + payload)  # 原样回显

    while True:
        conn, addr = srv.accept()
        print(f"[net] TCP 连接来自 {addr}")
        threading.Thread(target=client, args=(conn,), daemon=True).start()


def udp_discovery_server(port: int) -> None:
    """UDP 发现应答（STARFISH_PROBE → STARFISH_SERVER）；其余载荷原样回显"""
    srv = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    srv.bind(("0.0.0.0", port))
    print(f"[net] UDP 发现应答监听 :{port}")
    while True:
        data, addr = srv.recvfrom(4096)
        if data == b"STARFISH_PROBE":
            srv.sendto(b"STARFISH_SERVER", addr)
        else:
            srv.sendto(data, addr)


WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def ws_accept_key(key: str) -> str:
    digest = hashlib.sha1((key + WS_GUID).encode()).digest()
    return base64.b64encode(digest).decode()


def ws_read_exact(conn: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = conn.recv(n - len(buf))
        if not chunk:
            raise ConnectionError
        buf += chunk
    return buf


def ws_echo_client(conn: socket.socket) -> None:
    # 握手
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = conn.recv(4096)
        if not chunk:
            return
        data += chunk
    key = ""
    for line in data.decode("latin-1").split("\r\n"):
        if line.lower().startswith("sec-websocket-key:"):
            key = line.split(":", 1)[1].strip()
    accept = ws_accept_key(key)
    resp = (
        "HTTP/1.1 101 Switching Protocols\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    conn.sendall(resp.encode())

    # 帧循环：客户端掩码帧解析 → 原样回显（服务端帧不掩码）
    try:
        while True:
            b0, b1 = ws_read_exact(conn, 2)
            opcode = b0 & 0x0F
            masked = b1 & 0x80
            length = b1 & 0x7F
            if length == 126:
                length = int.from_bytes(ws_read_exact(conn, 2), "big")
            elif length == 127:
                length = int.from_bytes(ws_read_exact(conn, 8), "big")
            mask = ws_read_exact(conn, 4) if masked else None
            payload = ws_read_exact(conn, length) if length else b""
            if masked and payload:
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
            if opcode == 0x8:  # close
                break
            if opcode in (0x1, 0x2) and payload:  # text/binary → 回显
                out_len = len(payload)
                header = bytearray([0x80 | opcode])
                if out_len < 126:
                    header.append(out_len)
                elif out_len < 65536:
                    header.append(126)
                    header += out_len.to_bytes(2, "big")
                else:
                    header.append(127)
                    header += out_len.to_bytes(8, "big")
                conn.sendall(bytes(header) + payload)
    except (ConnectionError, OSError):
        pass


def ws_echo_server(port: int) -> None:
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", port))
    srv.listen(8)
    print(f"[net] WebSocket echo 监听 :{port}")
    while True:
        conn, addr = srv.accept()
        threading.Thread(target=ws_echo_client, args=(conn,), daemon=True).start()


def start_net_services(net_port: int = 8022, disc_port: int = 8023, ws_port: int = 8024) -> None:
    threading.Thread(target=tcp_echo_server, args=(net_port,), daemon=True).start()
    threading.Thread(target=udp_discovery_server, args=(disc_port,), daemon=True).start()
    threading.Thread(target=ws_echo_server, args=(ws_port,), daemon=True).start()
    print(f"[net] TCP echo :{net_port} | UDP 发现 :{disc_port} | WS echo :{ws_port}")


def main() -> None:
    global SAVES_DIR, WEB_DIR, CHUNKS_DIR, RESOURCES_DIR
    parser = argparse.ArgumentParser(description="starfish io/net demo server")
    parser.add_argument("--port", type=int, default=8021)
    parser.add_argument("--ws-port", type=int, default=8024)
    parser.add_argument("--saves-dir", default="./saves")
    parser.add_argument("--web-dir", default="./web", help="wasm 构建输出目录（同源部署）")
    parser.add_argument("--resources-dir", default="./resources",
                        help="资源目录挂载（probe 家族的 assets 逻辑路径 = 相对路径；置空禁用）")
    args = parser.parse_args()

    SAVES_DIR = pathlib.Path(args.saves_dir).resolve()
    WEB_DIR = pathlib.Path(args.web_dir).resolve()
    RESOURCES_DIR = pathlib.Path(args.resources_dir).resolve() if args.resources_dir else None
    CHUNKS_DIR = SAVES_DIR / ".chunks"
    CHUNKS_DIR.mkdir(parents=True, exist_ok=True)
    SAVES_DIR.mkdir(parents=True, exist_ok=True)

    start_net_services(net_port=8022, disc_port=8023, ws_port=args.ws_port)

    server = IoServer(("0.0.0.0", args.port), IoHandler)
    print(f"[io] 保存目录: {SAVES_DIR}")
    print(f"[io] 静态目录: {WEB_DIR}（若存在则同源服务 wasm 页面）")
    print(f"[io] 资源挂载: {RESOURCES_DIR}（probe 资产逻辑路径直读）")
    print(f"[io] 上限: {MAX_UPLOAD_BYTES // (1024 * 1024)}MB | 分片 TTL: {CHUNK_TTL_SECONDS}s")
    print(f"[io] 监听 http://0.0.0.0:{args.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[io] 服务器退出")


if __name__ == "__main__":
    main()
