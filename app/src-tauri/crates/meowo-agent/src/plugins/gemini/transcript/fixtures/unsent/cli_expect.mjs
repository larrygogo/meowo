// 用本机 Gemini CLI 的 loadConversationRecord 判每个会话文件「有没有可恢复内容」，写成期望表。
// 期望值取 metadataOnly 读法：CLI `--resume` 列会话（chunk-VFM6AETN.js getAllSessionFiles）用的就是它（第四轮 L2）。
// 全量读法结果不同时打到 stderr 备查。用法：node cli_expect.mjs <dir>
// 输出 JSON：文件名 → true（有）/ false（没有）/ null（CLI 不认这个文件）
import { readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
const bundle = process.env.APPDATA + "/npm/node_modules/@google/gemini-cli/bundle/chunk-RCB6BI5H.js";
const m = await import(pathToFileURL(bundle).href);
const dir = process.argv[2];
const out = {};
for (const f of readdirSync(dir).filter((f) => f.endsWith(".jsonl")).sort()) {
  const meta = await m.loadConversationRecord(dir + "/" + f, { metadataOnly: true });
  const full = await m.loadConversationRecord(dir + "/" + f);
  const b = meta ? meta.hasResumableContent : null;
  const a = full ? (full.hasResumableContent ?? m.hasResumableConversationContent(full.messages)) : null;
  if (a !== b) console.error("全量与列表读法不同（以列表为准）", f, "全量", a, "列表", b);
  out[f] = b;
}
console.log(JSON.stringify(out, null, 2));
