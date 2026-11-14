// 用本机 Gemini CLI 0.68 的 loadConversationRecord 判每个会话文件「有没有可恢复内容」，写成期望表。
// 用法：node cli_expect.mjs <dir>  → 打印 JSON：文件名 → true（有）/ false（没有）/ null（CLI 不认这个文件）
import { readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
const bundle = process.env.APPDATA + "/npm/node_modules/@google/gemini-cli/bundle/chunk-RCB6BI5H.js";
const m = await import(pathToFileURL(bundle).href);
const dir = process.argv[2];
const out = {};
for (const f of readdirSync(dir).filter((f) => f.endsWith(".jsonl")).sort()) {
  const full = await m.loadConversationRecord(dir + "/" + f);
  const meta = await m.loadConversationRecord(dir + "/" + f, { metadataOnly: true });
  const a = full ? (full.hasResumableContent ?? m.hasResumableConversationContent(full.messages)) : null;
  const b = meta ? meta.hasResumableContent : null;
  if (a !== b) console.error("全量与 metadataOnly 不一致", f, a, b);
  out[f] = a;
}
console.log(JSON.stringify(out, null, 2));
