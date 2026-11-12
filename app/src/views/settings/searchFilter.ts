// 设置搜索的行级过滤（S-1）：纯 DOM 操作，从 About.tsx 抽出以便单测。
//
// 过滤粒度：
// - 行：`.row`（设置项行）与 `.usage-sticker-row`（账号卡深处的「在贴纸显示配额」——
//   埋在已登录账号卡里，经搜索可达即可，不必挪位）——标题+说明文本命中即保留；
// - 组：表头行 + 紧跟其后的 `.row-sub` 子行（「通知这些事」和它管的四个开关）同进退——
//   表头命中带出全部子行，只有子行命中则带上表头，都没命中才一起隐藏；
// - 卡：`.row-card` 自身文本命中（如 agent 名「kimi」）整卡保留；否则有命中行才保留；
// - 分组标题 `.sec-caption`：跟随其后（到下一个标题前）卡片的可见性；
// - 分区 `.main-sec`：导航名命中（如「外观」）整区保留，否则无可见行/卡时整区隐藏；
// - 页标题 `.sec-title`：跟随分区——搜索结果把各分区平铺，标题正好当分隔；
// - 其余直挂内容（.sec-hint / 外观预览 / 关于页脚等）：搜索时让位。
//
// 显示切换一律走 style.display 而不是 hidden 属性：.row 等类的 display:flex 会盖掉
// [hidden] 的 UA 样式；且 .main-sec 的 hidden 归 React 管（分区切换），不能抢。
const FILTER_ROW = ".row, .usage-sticker-row";

/**
 * 应用一次过滤；返回是否有可见行/卡（驱动「无结果」提示）。
 * q 为空白时清除全部过滤痕迹（返回 false，无搜索态该值不被使用）。
 */
export function applySettingsFilter(body: HTMLElement, q: string, navLabels: Record<string, string>): boolean {
  const query = q.trim().toLowerCase();
  let anyVisible = false;
  for (const secEl of body.querySelectorAll<HTMLElement>(".main-sec")) {
    const setDisplay = (el: HTMLElement, show: boolean) => {
      el.style.display = show ? "" : "none";
    };
    if (!query) {
      secEl.style.display = "";
      for (const el of secEl.querySelectorAll<HTMLElement>(`${FILTER_ROW}, .row-card, .sec-caption`)) {
        el.style.display = "";
      }
      for (const kid of Array.from(secEl.children)) (kid as HTMLElement).style.display = "";
      continue;
    }
    const secKey = secEl.getAttribute("data-sec") ?? "";
    const navHit = (navLabels[secKey] ?? "").toLowerCase().includes(query);
    let secVisible = navHit;
    for (const card of secEl.querySelectorAll<HTMLElement>(".row-card")) {
      // 卡片「自身文本」= 去掉可过滤行后的剩余文本（如 provider 卡的 agent 名）。
      // 行文本不能算进卡片匹配——否则任何一行命中都会让整卡文本命中，行级过滤形同虚设。
      const ownClone = card.cloneNode(true) as HTMLElement;
      for (const r of ownClone.querySelectorAll(FILTER_ROW)) r.remove();
      const cardHit = navHit || (ownClone.textContent ?? "").toLowerCase().includes(query);
      let cardVisible = cardHit;
      const rows = Array.from(card.querySelectorAll<HTMLElement>(FILTER_ROW));
      const hits = rows.map((row) => cardHit || (row.textContent ?? "").toLowerCase().includes(query));
      // 表头行和它的 .row-sub 子行是一组，不能逐行各判各的。实拍 settingswin-search（620×460）：
      // 搜「通知」留下一行 42px 高、右侧整段空着的「通知这些事」，它管的四个开关因为文字里
      // 没有「通知」全被藏掉，看上去像一行缺了开关的设置。
      for (let i = 0; i < rows.length; i++) {
        if (rows[i]!.classList.contains("row-sub")) continue;
        let end = i + 1;
        while (end < rows.length && rows[end]!.classList.contains("row-sub") && rows[end]!.previousElementSibling === rows[end - 1]) end++;
        if (end === i + 1) continue;
        if (hits[i]) {
          // 表头命中：子行就是它的内容，整组显示。
          for (let k = i + 1; k < end; k++) hits[k] = true;
        } else if (hits.slice(i + 1, end).some(Boolean)) {
          // 只有子行命中：带上表头——两组开关的子行同名（会话跑完 / 等待审批…），没有表头分不清是哪一组。
          hits[i] = true;
        }
        i = end - 1;
      }
      rows.forEach((row, i) => {
        setDisplay(row, hits[i]!);
        if (hits[i]) cardVisible = true;
      });
      setDisplay(card, cardVisible);
      if (cardVisible) secVisible = true;
    }
    // 不在卡片里的散行（当前布局没有，兜底）。
    for (const row of secEl.querySelectorAll<HTMLElement>(FILTER_ROW)) {
      if (row.closest(".row-card")) continue;
      const rowHit = navHit || (row.textContent ?? "").toLowerCase().includes(query);
      setDisplay(row, rowHit);
      if (rowHit) secVisible = true;
    }
    // 直挂孩子：分组标题跟随本组卡片可见性，其余（提示/预览/页脚）搜索时让位。
    let caption: HTMLElement | null = null;
    let groupVisible = false;
    const flush = () => {
      if (caption) setDisplay(caption, navHit || groupVisible);
      caption = null;
      groupVisible = false;
    };
    for (const kid of Array.from(secEl.children) as HTMLElement[]) {
      if (kid.classList.contains("sec-caption")) {
        flush();
        caption = kid;
      } else if (kid.classList.contains("row-card")) {
        if (kid.style.display !== "none") groupVisible = true;
      } else if (kid.classList.contains("sec-title")) {
        setDisplay(kid, secVisible);
      } else if (kid.classList.contains("account-agent-switch")) {
        // 7S-4：账号分区一次只渲染**当前** agent 的卡，别家的名字（"codex"）搜不到；
        // 而通往别家的唯一入口正是这个切换器，搜索时把它一并藏掉就成了死路——
        // 分区还在就留着它。
        setDisplay(kid, secVisible);
      } else {
        setDisplay(kid, false);
      }
    }
    flush();
    setDisplay(secEl, secVisible);
    if (secVisible) anyVisible = true;
  }
  return query !== "" && anyVisible;
}

const FOCUSABLE = "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";

/**
 * 搜索后第一条命中行里的第一个可操作控件（T-4，2026-11-12 走查 log：Enter 与 ↓ 之后焦点都停在
 * 搜索框，结果区在 DOM 里排在 9 个导航项和关闭钮之后，要按约 11 下 Tab 才进得去）。
 * 只认过滤后仍显示的行：被 style.display=none 或 hidden 藏起来的祖先一律跳过。
 */
export function firstHitControl(body: HTMLElement): HTMLElement | null {
  const visible = (el: HTMLElement) => {
    for (let cur: HTMLElement | null = el; cur && cur !== body; cur = cur.parentElement) {
      if (cur.hidden || cur.style.display === "none") return false;
    }
    return true;
  };
  for (const row of body.querySelectorAll<HTMLElement>(FILTER_ROW)) {
    if (!visible(row)) continue;
    const control = Array.from(row.querySelectorAll<HTMLElement>(FOCUSABLE)).find(visible);
    if (control) return control;
  }
  return null;
}
