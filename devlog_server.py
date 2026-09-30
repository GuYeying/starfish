# 诊断日志收集器：接收手机端 POST /log 的纯文本，落盘 devlog.txt 并打印
from http.server import BaseHTTPRequestHandler, HTTPServer

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n).decode("utf-8", "replace")
        with open("devlog.txt", "a", encoding="utf-8") as f:
            f.write(body + "\n")
        print("RECV:", body, flush=True)
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")
    def log_message(self, *a):
        pass

print("devlog collector on :9000", flush=True)
HTTPServer(("0.0.0.0", 9000), H).serve_forever()
