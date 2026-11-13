/// 输入框草稿：附件的形状、草稿在 localStorage 里的存取、正文加附件拼成发给 CLI 的文本。
///
/// 从 ChatWindow 拆出（对照 Kimi 把输入区状态与持久化放在 composer 自己的模块里）：这些都是
/// 不碰 React 的纯函数，单独放着可以脱离五千行的组件单测。

export type Attachment = { path: string; name: string; image: boolean;
  /** 字节数，仅粘贴来源有（File.size 现成）；文件选择器只给路径，拿大小要多一次 IPC，不值。 */
  size?: number };

/** 附件卡片的体积行。附件都是本地小文件，GB 档没有出场机会。 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 100 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]);

export function attachmentOf(path: string): Attachment {
  const name = path.split(/[\\/]/).pop() || path;
  const extension = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return { path, name, image: IMAGE_EXTENSIONS.has(extension) };
}

/** 草稿持久化（localStorage）：关窗/重启不丢——此前只在内存 Map，窗口一关半屏提示词
 *  就没了（附件反而因落盘临时文件幸存）。只存正式会话（负数临时 id 认领后会换号）；
 *  LRU 保 20 条防膨胀。
 *  每会话一个 key：可同时开多个对话窗，整表「读-改-写」会让后写方覆盖先写方（另一窗
 *  草稿静默丢），各写各的 key 才没有竞态。 */
const DRAFTS_KEY = "meowo-chat-drafts"; // 旧版整表格式，首次读取时拆成按会话 key 后删除
const DRAFT_KEY_PREFIX = "meowo-chat-draft:";
export type ManagedDraftSubmission = { deliveryId: string; prompt: string; attachments: Attachment[] };
export function newManagedDeliveryId(): string {
  // 手机HTTP页面没有secure-context限定的randomUUID，getRandomValues仍可生成随机回执id。
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
type StoredDraft = { prompt: string; attachments: Attachment[]; at?: number; managedSubmissions?: ManagedDraftSubmission[] };
export function storedManagedSubmissions(sessionId: number): ManagedDraftSubmission[] {
  const raw = localStorage.getItem(DRAFT_KEY_PREFIX + sessionId);
  const value = raw ? JSON.parse(raw) as StoredDraft : null;
  return (value?.managedSubmissions ?? []).filter((entry) => /^[A-Za-z0-9_-]{1,160}$/.test(entry.deliveryId) && typeof entry.prompt === "string" && Array.isArray(entry.attachments));
}
function sameSubmission(entry: { prompt: string; attachments: Attachment[] }, prompt: string, attachments: Attachment[]): boolean {
  return entry.prompt === prompt && JSON.stringify(entry.attachments.map((file) => file.path)) === JSON.stringify(attachments.map((file) => file.path));
}
export function persistManagedSubmission(sessionId: number, prompt: string, attachments: Attachment[]): string {
  try {
    const entries = storedManagedSubmissions(sessionId);
    const existing = entries.find((entry) => sameSubmission(entry, prompt, attachments));
    if (existing) return existing.deliveryId;
    const deliveryId = newManagedDeliveryId();
    // 先把原文、附件与id放进同一条草稿，再发网络请求；重载不能把未知送达变成新消息。
    const draft: StoredDraft = { prompt, attachments, at: Date.now(), managedSubmissions: [...entries, { deliveryId, prompt, attachments }] };
    localStorage.setItem(DRAFT_KEY_PREFIX + sessionId, JSON.stringify(draft));
    return deliveryId;
  } catch { throw new Error("conversation/client_persistence_failed"); }
}
export function acknowledgeManagedSubmission(sessionId: number, deliveryId: string): ManagedDraftSubmission | null {
  const key = DRAFT_KEY_PREFIX + sessionId;
  const raw = localStorage.getItem(key);
  if (!raw) return null;
  const draft = JSON.parse(raw) as StoredDraft;
  const entries = storedManagedSubmissions(sessionId);
  const submitted = entries.find((entry) => entry.deliveryId === deliveryId);
  if (!submitted) return null;
  draft.managedSubmissions = entries.filter((entry) => entry.deliveryId !== deliveryId);
  if (sameSubmission(draft, submitted.prompt, submitted.attachments)) { draft.prompt = ""; draft.attachments = []; }
  if (!draft.prompt.trim() && !draft.attachments.length && !draft.managedSubmissions.length) localStorage.removeItem(key);
  else localStorage.setItem(key, JSON.stringify(draft));
  return submitted;
}
/** 校验一条草稿的形状；坏数据返回 null（当无草稿）。 */
function parseStoredDraft(value: unknown): { prompt: string; attachments: Attachment[] } | null {
  const draft = value as StoredDraft | null;
  if (typeof draft?.prompt !== "string") return null;
  const attachments = Array.isArray(draft.attachments)
    ? draft.attachments.filter((file) => file && typeof file.path === "string")
    : [];
  return { prompt: draft.prompt, attachments };
}
export function loadStoredDrafts(): Map<number, { prompt: string; attachments: Attachment[] }> {
  const map = new Map<number, { prompt: string; attachments: Attachment[] }>();
  try {
    // 旧版整表迁移：逐条落成按会话 key 再删整表；中途失败留下次启动再迁（整表还在）。
    const legacy = localStorage.getItem(DRAFTS_KEY);
    if (legacy) {
      for (const [id, value] of Object.entries(JSON.parse(legacy) as Record<string, StoredDraft>)) {
        // 落盘用 parseStoredDraft 的规范化结果而不是原始 value——否则未清洗的
        // attachments(以及非数字 at)会被原样迁进新格式。at 非数字时按现在计,
        // 保证 LRU 清扫有可比较的时间戳。
        const parsed = parseStoredDraft(value);
        if (!parsed) continue;
        const at = typeof value?.at === "number" ? value.at : Date.now();
        localStorage.setItem(DRAFT_KEY_PREFIX + id, JSON.stringify({ ...parsed, at }));
      }
      localStorage.removeItem(DRAFTS_KEY);
    }
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key?.startsWith(DRAFT_KEY_PREFIX)) continue;
      const id = Number(key.slice(DRAFT_KEY_PREFIX.length));
      if (!Number.isSafeInteger(id)) continue;
      const draft = parseStoredDraft(JSON.parse(localStorage.getItem(key) || "null"));
      if (draft) map.set(id, draft);
    }
  } catch { /* 坏数据当无草稿 */ }
  return map;
}

/** 写一条会话草稿（空草稿删条目，发送成功清空输入框即自动清理），再按 LRU 保 20 条。
 *  每会话一个 key，只写 / 删自己这条：多窗并存互不覆盖，无读-改-写竞态。存储失败静默。 */
export function saveStoredDraft(sessionId: number, prompt: string, attachments: Attachment[]): void {
  try {
    const managedSubmissions = storedManagedSubmissions(sessionId);
    if (!prompt.trim() && attachments.length === 0 && managedSubmissions.length === 0) {
      localStorage.removeItem(DRAFT_KEY_PREFIX + sessionId);
    } else {
      const draft: StoredDraft = { prompt, attachments, at: Date.now(), managedSubmissions };
      localStorage.setItem(DRAFT_KEY_PREFIX + sessionId, JSON.stringify(draft));
    }
    // LRU 保 20 条：先收集再删——localStorage.key 按下标枚举，边删边数会跳项。
    const stored: { key: string; at: number }[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key?.startsWith(DRAFT_KEY_PREFIX)) continue;
      let at = 0;
      try { at = (JSON.parse(localStorage.getItem(key) || "{}") as StoredDraft).at ?? 0; } catch { /* 坏数据排最旧 */ }
      // 未对账投递不是可丢弃的LRU草稿：删掉id会使刷新后的重试重复执行。
      const pending = (JSON.parse(localStorage.getItem(key) || "{}") as StoredDraft).managedSubmissions;
      if (!pending?.length) stored.push({ key, at });
    }
    if (stored.length > 20) {
      stored.sort((a, b) => b.at - a.at);
      for (const stale of stored.slice(20)) localStorage.removeItem(stale.key);
    }
  } catch { /* 存储失败不影响输入 */ }
}

export function promptWithAttachments(prompt: string, attachments: Attachment[], instruction: (files: string) => string, mention = false): string {
  if (!attachments.length) return prompt;
  // @提及是提交时的原生附加(claude/gemini 实测,由插件按版本声明 attachment_mention)。
  // 三个硬性例外整条退回指令文本,保持单一注入形态:
  // - 路径含空白:提及解析在空白处截断(claude 实测),后半截变成正文;
  // - 图片:@提及不会作为图像块附加(claude -p 实测,模型明说看不到),指令文本反而
  //   能引导 agent 用自己的读图工具打开。
  // - 没有正文:opencode 的 @ 补全框在「@路径」收尾时吃掉回车(补空格也不收,实测 1.19.2),
  //   消息就卡在输入框里。只剩附件时指令文本对谁都成立。
  if (mention && prompt.trim() && attachments.every((file) => !file.image && !/\s/.test(file.path))) {
    return `${attachments.map((file) => `@${file.path}`).join(" ")} ${prompt.trim()}`;
  }
  // 指令文案随界面语言（i18n），不能硬编码中文——英文界面会把中文指令发给 agent。
  const directive = instruction(attachments.map((file) => `- ${file.path}`).join("\n"));
  return prompt.trim() ? `${prompt.trim()}\n\n${directive}` : directive;
}

/// ArrayBuffer → base64。btoa 只吃字符串且实参展开有调用栈上限，分块拼接。
export function base64OfBuffer(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}
