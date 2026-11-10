// 从 BrowserPane 拆出：ChatWindow 只为这一个纯函数静态引 BrowserPane，会把面板浏览器连同它引的
// 路径菜单、改动面板一起拉进对话窗主 chunk（vite 构建警告实测）。
/** 从对话文本里挑出本机地址（dev server），去重、最近出现的在前，最多 5 个。 */
export function localUrlsIn(texts: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  // 路径里遇到中文标点就停：agent 的中文回复里网址后面常直接跟「。」「，」。
  const pattern = /https?:\/\/(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])(?::\d+)?(?:\/[^\s"'`<>)\]，。；：！？、（）「」《》]*)?/gi;
  for (let i = texts.length - 1; i >= 0 && out.length < 5; i -= 1) {
    const matches = texts[i].match(pattern) ?? [];
    for (let k = matches.length - 1; k >= 0 && out.length < 5; k -= 1) {
      const url = matches[k].replace(/[.,;:!?]+$/, "");
      if (seen.has(url)) continue;
      seen.add(url);
      out.push(url);
    }
  }
  return out;
}
