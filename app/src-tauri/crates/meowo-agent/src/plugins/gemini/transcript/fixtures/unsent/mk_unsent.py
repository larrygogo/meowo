# 生成 R1 一致性样例（只重写本脚本生成的 *.jsonl；session-za* 是做空第四轮对抗样例，手工保留）。CLI 升级时：python mk_unsent.py . && node cli_expect.mjs . 对照 expected.json。
import json, os, sys
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for f in [f for f in os.listdir(out) if f.endswith(".jsonl") and not f.startswith("session-za")]:
    os.remove(os.path.join(out, f))
META = {"sessionId": "8b42a13c-8714-4a14-9d61-08036659e03e", "projectHash": "edc4", "startTime": "2026-11-13T16:46:15Z", "lastUpdated": "2026-11-13T16:46:15Z", "kind": "main"}
CTX = {"id": "d049", "timestamp": "2026-11-13T16:46:15.353Z", "type": "user", "content": [{"text": "<session_context>\nThis is the Gemini CLI."}]}
SET = {"$set": {"lastUpdated": "2026-11-13T16:46:15.354Z"}}
def u(i, content): return {"id": i, "timestamp": "2026-11-13T16:46:16Z", "type": "user", "content": content}
def g(i, **kw): return {"id": i, "timestamp": "2026-11-13T16:46:17Z", "type": "gemini", "content": kw.pop("content", ""), "model": "gemini-2.5-pro", **kw}
cases = {
    "a-plain": [META, CTX, SET],
    "b-leadhook": [META, CTX, u("d050", [{"text": "<hook_context>[lead] 这个项目有进行中的任务</hook_context>"}]), SET],
    "c-info": [META, CTX, {"id": "d051", "type": "info", "content": "Model set to gemini-2.5-pro."}, SET],
    "d-slash": [META, CTX, u("d052", [{"text": "/stats"}]), SET],
    "e-question": [META, CTX, u("d053", "?"), SET],
    "f-blank-user": [META, CTX, u("d054", [{"text": "   "}]), SET],
    "g-real-user": [META, CTX, u("d055", [{"text": "帮我看看这个报错"}]), SET],
    "h-user-string": [META, CTX, u("d056", "hi"), SET],
    "i-gemini-text": [META, CTX, g("d057", content="ok")],
    "j-gemini-empty": [META, CTX, g("d058", content="")],
    "k-gemini-toolcalls": [META, CTX, g("d059", content="", toolCalls=[{"id": "t1", "name": "ls", "args": {}}])],
    "l-gemini-thoughts": [META, CTX, g("d060", content="", thoughts=[{"subject": "s", "description": "d"}])],
    "m-function-response": [META, CTX, u("d061", [{"functionResponse": {"name": "ls", "response": {}}}])],
    "n-rewind-all": [META, CTX, u("d062", "hi"), {"$rewindTo": "d062"}],
    "o-rewind-missing": [META, CTX, u("d063", "hi"), {"$rewindTo": "nope"}],
    "p-set-messages": [META, CTX, {"$set": {"messages": [u("d064", "hi")]}}],
    "q-set-messages-empty": [META, CTX, u("d065", "hi"), {"$set": {"messages": []}}],
    "r-patch-to-slash": [META, CTX, u("d066", "hi"), {"$patch": {"id": "d066", "content": "/help"}}],
    "s-patch-remove": [META, CTX, u("d067", "hi"), {"$patch": {"removeIds": ["d067"]}}],
    "t-same-id-overwrite": [META, CTX, u("d068", "hi"), u("d068", "/stats")],
    "u-no-projecthash": [{"sessionId": META["sessionId"]}, CTX, SET],
    "w-error-warning": [META, CTX, {"id": "e1", "type": "error", "content": "boom"}, {"id": "w1", "type": "warning", "content": "careful"}],
    "x-meta-messages": [dict(META, messages=[u("d069", "hi")])],
}
for name, records in cases.items():
    with open(os.path.join(out, f"session-{name}-8b42a13c.jsonl"), "w", encoding="utf8", newline="\n") as fh:
        fh.write("\n".join(json.dumps(r, ensure_ascii=False) for r in records) + "\n")
# 半截行：CLI 跳过解析不了的行
with open(os.path.join(out, "session-v-partial-line-8b42a13c.jsonl"), "w", encoding="utf8", newline="\n") as fh:
    fh.write("\n".join(json.dumps(r, ensure_ascii=False) for r in [META, CTX]) + '\n{"id":"d070","type":"user","content":"hal')
