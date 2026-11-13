"""Lead 原生 TUI 探针用的本地假模型（lead_native/probe.rs）：记录请求头/正文，按脚本回复。兼容 OpenAI chat.completions（kimi）与 Gemini generateContent。

用法：python -I fake_llm.py <port> <logdir>
脚本：看对话里最后几条消息决定回复，见 decide_*。
"""
import json, sys, os, re, time, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOG = sys.argv[2]
os.makedirs(LOG, exist_ok=True)
SEQ = [0]
LOCK = threading.Lock()
# 场景控制：由测试写 LOG/plan.json（gate_path、gate_body 等）
def plan():
    try:
        with open(os.path.join(LOG, "plan.json"), encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return {}

def log(entry):
    with LOCK:
        SEQ[0] += 1
        n = SEQ[0]
    with open(os.path.join(LOG, f"req-{n:03d}.json"), "w", encoding="utf-8") as f:
        json.dump(entry, f, ensure_ascii=False, indent=1)
    return n

def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        out = []
        for p in content:
            if isinstance(p, dict):
                out.append(p.get("text") or "")
        return "".join(out)
    return ""

ANSWER = "leader 已在 Meowo 里回答"
STATE = {}  # plan["phase"] -> 本阶段已做过的动作，防止重复写关卡 / 重复唤醒


def read_output_file(text):
    """kimi 的后台完成通知只给输出文件路径：像真模型一样去读它。"""
    m = re.search(r'<output-file path="([^"]+)"', text)
    if not m:
        return None
    try:
        with open(m.group(1), encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def decide(last_user, last_is_tool, last_tool_args, p, last_tool_text=""):
    """返回 ('text', str) / ('tool_write', path, body) / ('tool_bash_bg', cmd)。

    按 plan["phase"] 记住本阶段已做的事：真 TUI 会在工具结果后追加 "Please continue."、
    在会话摘要等旁路请求里重复整段对话，按「最后一句话」判断会反复写关卡。
    """
    lu = last_user or ""
    secret = p.get("secret", "PASS-NONE")
    st = STATE.setdefault(p.get("phase", "default"), {})
    if "Summary (max 80 chars)" in lu:
        return ("text", "probe session")
    # kimi：写关卡被 PreToolUse 拦下，原因里给了等待命令 → 后台启动它
    m = re.search(r"命令：`([^`]*lead wait[^`]*)`", last_tool_text if last_is_tool else "")
    if m:
        return ("tool_bash_bg", m.group(1))
    if last_is_tool and "lead wait" in last_tool_args and p.get("gate_path"):
        return ("tool_write", p["gate_path"], p.get("gate_body", "{}"))
    out = read_output_file(lu)
    woke = (out is not None and ANSWER in out) or (out is None and ANSWER in lu
             and ("Background command completed" in lu or st.get("gate_written") or p.get("offline")))
    if out is not None and ANSWER not in out:
        return ("text", "IGNORED " + out.strip()[:60])
    if woke and not st.get("consumed"):
        st["consumed"] = True
        if p.get("consume") and p.get("gate_path"):
            return ("tool_write", p["gate_path"], p.get("consumed_body", ""))
    if st.get("consumed") and not st.get("woke"):
        st["woke"] = True
        return ("text", f"WOKE {secret}")
    if "SLOW" in lu and not st.get("slow"):
        st["slow"] = True
        time.sleep(float(p.get("slow", 10)))
        return ("text", "SLOW-DONE")
    if "OPEN-GATE" in lu and not st.get("gate_written") and p.get("gate_path"):
        st["gate_written"] = True
        return ("tool_write", p["gate_path"], p.get("gate_body", "{}"))
    if last_is_tool or lu.strip().endswith("Please continue."):
        return ("text", "GATE-OPEN-WAITING")
    if "SAY" in lu:
        return ("text", "SAID " + lu.split("SAY", 1)[1].strip()[:40])
    return ("text", "READY")


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        log({"method": "GET", "path": self.path, "headers": dict(self.headers)})
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"data":[{"id":"fake-model","object":"model"}]}')

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n) if n else b""
        try:
            body = json.loads(raw or b"{}")
        except Exception:
            body = {"_raw": raw.decode("utf-8", "replace")}
        p = plan()
        if "generateContent" in self.path or "streamGenerateContent" in self.path or "countTokens" in self.path:
            return self.gemini(body, p)
        if "chat/completions" in self.path:
            return self.openai(body, p)
        log({"method": "POST", "path": self.path, "headers": dict(self.headers), "body": body})
        self.send_response(404)
        self.end_headers()

    # ---------------- OpenAI（kimi） ----------------
    def openai(self, body, p):
        msgs = body.get("messages", [])
        last = msgs[-1] if msgs else {}
        # 末尾连续的 user 消息合起来看：kimi 会在真正的消息后面追加 system-reminder（如 -p 的 auto 权限提示）。
        tail = []
        for m in reversed(msgs):
            if m.get("role") != "user":
                break
            tail.append(text_of(m.get("content")))
        last_user = chr(10).join(reversed(tail))
        last_is_tool = last.get("role") == "tool"
        last_tool_args = ""
        for m in reversed(msgs):
            if m.get("role") == "assistant" and m.get("tool_calls"):
                last_tool_args = json.dumps(m["tool_calls"], ensure_ascii=False)
                break
        d = decide(last_user if last.get("role") == "user" else "", last_is_tool, last_tool_args, p,
                   text_of(last.get("content")) if last_is_tool else "")
        n = log({"method": "POST", "path": self.path, "headers": dict(self.headers), "decision": d, "last_role": last.get("role"), "body_meta": {k: v for k, v in body.items() if k not in ("messages", "tools")}, "tool_names": [t.get("function", {}).get("name") for t in body.get("tools", [])], "msgs_tail": msgs[-3:], "last_user": last_user[-1500:], "n_messages": len(msgs)})
        if p.get("delay"):
            time.sleep(float(p["delay"]))
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        def chunk(delta, finish=None):
            obj = {"id": f"c{n}", "object": "chat.completion.chunk", "created": int(time.time()), "model": body.get("model", "fake"),
                   "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(("data: " + json.dumps(obj, ensure_ascii=False) + "\n\n").encode())
            self.wfile.flush()
        if d[0] == "text":
            chunk({"role": "assistant", "content": d[1]})
            chunk({}, "stop")
        else:
            if d[0] == "tool_write":
                name, args = "Write", {"path": d[1], "content": d[2]}
            else:
                name, args = "Bash", {"command": d[1], "run_in_background": True, "disable_timeout": True, "description": "Meowo lead gate waiter"}
            chunk({"role": "assistant", "content": None, "tool_calls": [{"index": 0, "id": f"call_{n}", "type": "function", "function": {"name": name, "arguments": json.dumps(args, ensure_ascii=False)}}]})
            chunk({}, "tool_calls")
        usage = {"id": f"c{n}", "object": "chat.completion.chunk", "choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}}
        self.wfile.write(("data: " + json.dumps(usage) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    # ---------------- Gemini ----------------
    def gemini(self, body, p):
        if "countTokens" in self.path:
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"totalTokens": 10}')
            return
        contents = body.get("contents", [])
        last = contents[-1] if contents else {}
        parts = last.get("parts", [])
        last_is_tool = any("functionResponse" in x for x in parts)
        # 末尾几条非 system 消息的全部文本（含工具结果）：后台完成注入的位置不固定。
        tail_texts = []
        for c in contents[-3:]:
            if c.get("role") == "system":
                continue
            for x in c.get("parts", []):
                if isinstance(x, dict):
                    tail_texts.append(x.get("text") or json.dumps(x.get("functionResponse", ""), ensure_ascii=False))
        last_user = chr(10).join(tail_texts) if last.get("role") == "user" else ""
        last_tool_args = ""
        for c in reversed(contents):
            if c.get("role") == "model":
                calls = [x["functionCall"] for x in c.get("parts", []) if isinstance(x, dict) and "functionCall" in x]
                if calls:
                    last_tool_args = json.dumps(calls, ensure_ascii=False)
                    break
        # 系统/路由类请求（如下一位发言者判定、标题生成）走 JSON 简答
        gen = body.get("generationConfig", {}) or {}
        if gen.get("responseMimeType") == "application/json" or gen.get("responseJsonSchema") or gen.get("responseSchema"):
            n = log({"method": "POST", "path": self.path, "headers": dict(self.headers), "kind": "json", "last_user": last_user[-800:]})
            return self.gemini_send(self.path, [{"text": json.dumps({"reasoning": "fake", "next_speaker": "user", "model_choice": "flash", "complexity_score": 1})}])
        d = decide(last_user, last_is_tool, last_tool_args, p)
        n = log({"method": "POST", "path": self.path, "headers": dict(self.headers), "decision": d, "last_user": last_user[-1500:], "n_contents": len(contents), "last_is_tool": last_is_tool, "contents_tail": contents[-3:]})
        if p.get("delay"):
            time.sleep(float(p["delay"]))
        if d[0] == "text":
            out = [{"text": d[1]}]
        elif d[0] == "tool_write":
            out = [{"functionCall": {"name": "write_file", "args": {"file_path": d[1], "content": d[2]}}}]
        else:
            out = [{"functionCall": {"name": "run_shell_command", "args": {"command": d[1], "is_background": True, "description": "lead waiter"}}}]
        self.gemini_send(self.path, out)

    def gemini_send(self, path, parts):
        resp = {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2, "totalTokenCount": 12}, "modelVersion": "fake"}
        self.send_response(200)
        if "alt=sse" in path:
            self.send_header("content-type", "text/event-stream")
            self.end_headers()
            self.wfile.write(("data: " + json.dumps(resp, ensure_ascii=False) + "\r\n\r\n").encode())
        else:
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(resp, ensure_ascii=False).encode())
        self.wfile.flush()

ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
