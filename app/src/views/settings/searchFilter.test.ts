import { describe, expect, it } from "vitest";
import { applySettingsFilter, firstHitControl } from "./searchFilter";

const NAV = { general: "通用", appearance: "外观", account: "账号与用量" };

/** 模拟设置窗的 DOM 骨架：三分区，含分组标题、普通行、账号卡深埋的配额开关行。 */
function build(): HTMLElement {
  const body = document.createElement("div");
  body.innerHTML = `
    <div class="main-sec" data-sec="general">
      <div class="row-card">
        <div class="row"><div class="row-text"><div class="row-label">语言</div><div class="row-desc">界面语言</div></div></div>
        <div class="row"><div class="row-text"><div class="row-label">开机自启</div></div></div>
      </div>
      <div class="sec-hint">保存失败会显示在这里</div>
    </div>
    <div class="main-sec" data-sec="appearance">
      <div class="sec-caption">贴纸</div>
      <div class="row-card">
        <div class="row"><div class="row-text"><div class="row-label">主题</div></div></div>
        <div class="row"><div class="row-text"><div class="row-label">贴纸颜色</div></div></div>
      </div>
    </div>
    <div class="main-sec" data-sec="account">
      <div class="row-card provider-card">
        <div class="provider-name">Kimi</div>
        <div class="usage-sticker-row"><span>在贴纸显示配额</span></div>
      </div>
    </div>`;
  return body;
}

const shown = (el: Element | null) => (el as HTMLElement).style.display !== "none";
const sec = (body: HTMLElement, key: string) => body.querySelector(`[data-sec="${key}"]`);

describe("applySettingsFilter（设置搜索过滤，S-1）", () => {
  it("按行文本过滤：命中行保留，未命中行/分区隐藏", () => {
    const body = build();
    const any = applySettingsFilter(body, "语言", NAV);
    expect(any).toBe(true);
    expect(shown(sec(body, "general"))).toBe(true);
    const rows = body.querySelectorAll('[data-sec="general"] .row');
    expect(shown(rows[0])).toBe(true); // 语言
    expect(shown(rows[1])).toBe(false); // 开机自启
    expect(shown(sec(body, "appearance"))).toBe(false);
    expect(shown(sec(body, "account"))).toBe(false);
  });

  it("深埋行可达：「在贴纸显示配额」（usage-sticker-row）命中时保留账号卡", () => {
    const body = build();
    const any = applySettingsFilter(body, "配额", NAV);
    expect(any).toBe(true);
    expect(shown(sec(body, "account"))).toBe(true);
    expect(shown(body.querySelector(".provider-card"))).toBe(true);
    expect(shown(body.querySelector(".usage-sticker-row"))).toBe(true);
    expect(shown(sec(body, "general"))).toBe(false);
  });

  it("卡片自身文本命中（agent 名）整卡保留，卡内行不被单独过滤", () => {
    const body = build();
    applySettingsFilter(body, "kimi", NAV);
    expect(shown(sec(body, "account"))).toBe(true);
    expect(shown(body.querySelector(".usage-sticker-row"))).toBe(true);
  });

  it("导航名命中整区保留（含分组标题与全部行）", () => {
    const body = build();
    applySettingsFilter(body, "外观", NAV);
    expect(shown(sec(body, "appearance"))).toBe(true);
    expect(shown(body.querySelector(".sec-caption"))).toBe(true);
    for (const row of body.querySelectorAll('[data-sec="appearance"] .row')) {
      expect(shown(row)).toBe(true);
    }
    expect(shown(sec(body, "general"))).toBe(false);
  });

  it("分区页标题（.sec-title）跟随分区：有命中留着当分隔，没命中随分区一起藏", () => {
    const body = build();
    for (const sec of body.querySelectorAll(".main-sec")) {
      const title = document.createElement("h1");
      title.className = "sec-title";
      title.textContent = sec.getAttribute("data-sec") ?? "";
      sec.prepend(title);
    }
    applySettingsFilter(body, "语言", NAV);
    const title = (sec: string) => body.querySelector<HTMLElement>(`[data-sec="${sec}"] .sec-title`)!;
    expect(title("general").style.display).toBe("");
    expect(title("appearance").style.display).toBe("none");
  });

  it("分组标题跟随本组卡片：组内无命中时标题一并隐藏", () => {
    const body = build();
    applySettingsFilter(body, "主题", NAV);
    // 「贴纸颜色」未命中 → 行隐藏；「主题」命中 → 卡片保留 → 标题保留
    expect(shown(sec(body, "appearance"))).toBe(true);
    expect(shown(body.querySelector(".sec-caption"))).toBe(true);
    const rows = body.querySelectorAll('[data-sec="appearance"] .row');
    expect(shown(rows[0])).toBe(true);
    expect(shown(rows[1])).toBe(false);
    // 搜索时直挂的提示块让位
    expect(shown(body.querySelector(".sec-hint"))).toBe(false);
  });

  it("下拉未展开选项的隐藏语料（.dd-search）参与行匹配：搜 WezTerm 命中「恢复终端」行", () => {
    const body = document.createElement("div");
    body.innerHTML = `
      <div class="main-sec" data-sec="general">
        <div class="row-card">
          <div class="row"><div class="row-text"><div class="row-label">恢复终端</div></div>
            <div class="dd"><button class="dd-btn">Windows Terminal</button>
              <span class="dd-search" hidden aria-hidden="true">Windows Terminal WezTerm PowerShell</span>
            </div>
          </div>
        </div>
      </div>`;
    const any = applySettingsFilter(body, "wezterm", NAV);
    expect(any).toBe(true);
    expect(shown(body.querySelector(".row"))).toBe(true);
    // 语料不干扰清空复位（它不在过滤直写的选择器里）
    applySettingsFilter(body, "", NAV);
    expect((body.querySelector(".row") as HTMLElement).style.display).toBe("");
  });

  it("零命中返回 false 且全部分区隐藏（驱动「无结果」提示）", () => {
    const body = build();
    const any = applySettingsFilter(body, "不存在的设置xyz", NAV);
    expect(any).toBe(false);
    for (const key of Object.keys(NAV)) expect(shown(sec(body, key))).toBe(false);
  });

  it("清空查询：全部过滤痕迹复位", () => {
    const body = build();
    applySettingsFilter(body, "语言", NAV);
    applySettingsFilter(body, "", NAV);
    for (const el of body.querySelectorAll<HTMLElement>(".main-sec, .row, .row-card, .sec-caption, .sec-hint")) {
      expect(el.style.display).toBe("");
    }
  });
  // 实拍 settingswin-search（620×460）：搜「通知」时「通知这些事」表头留了下来（42px 高、右侧 208–572 全空），
  // 它管的四个开关行因为文字里没有「通知」全被藏掉，看上去像一行缺了开关的设置。
  describe("表头行与子开关行（.row-sub）成组显隐", () => {
    function buildGroup(): HTMLElement {
      const body = document.createElement("div");
      body.innerHTML = `
        <div class="main-sec" data-sec="general">
          <div class="row-card">
            <div class="row" data-k="notify"><div class="row-label">桌面通知</div></div>
            <div class="row row-col" data-k="head"><div class="row-label">通知这些事</div></div>
            <div class="row row-sub" data-k="done"><div class="row-label">会话跑完</div></div>
            <div class="row row-sub" data-k="approval"><div class="row-label">等待审批</div></div>
            <div class="row" data-k="sound"><div class="row-label">通知提示音</div></div>
            <div class="row" data-k="lang"><div class="row-label">语言</div></div>
          </div>
        </div>`;
      return body;
    }
    const row = (body: HTMLElement, k: string) => body.querySelector(`[data-k="${k}"]`);

    it("表头命中：子行一起显示，不留没有控件的空表头", () => {
      const body = buildGroup();
      expect(applySettingsFilter(body, "通知", NAV)).toBe(true);
      expect(shown(row(body, "head"))).toBe(true);
      expect(shown(row(body, "done"))).toBe(true);
      expect(shown(row(body, "approval"))).toBe(true);
      expect(shown(row(body, "notify"))).toBe(true);
      expect(shown(row(body, "sound"))).toBe(true);
      expect(shown(row(body, "lang"))).toBe(false);
    });

    it("只有子行命中：带上表头（否则分不清是哪一组的开关），没命中的兄弟子行仍隐藏", () => {
      const body = buildGroup();
      expect(applySettingsFilter(body, "审批", NAV)).toBe(true);
      expect(shown(row(body, "head"))).toBe(true);
      expect(shown(row(body, "approval"))).toBe(true);
      expect(shown(row(body, "done"))).toBe(false);
      expect(shown(row(body, "notify"))).toBe(false);
    });

    it("表头和子行都没命中：整组隐藏", () => {
      const body = buildGroup();
      expect(applySettingsFilter(body, "语言", NAV)).toBe(true);
      expect(shown(row(body, "head"))).toBe(false);
      expect(shown(row(body, "done"))).toBe(false);
      expect(shown(row(body, "approval"))).toBe(false);
      expect(shown(row(body, "lang"))).toBe(true);
    });
  });
});

// T-4（2026-11-12 走查）：搜索框里按 Enter / ↓ 要直接落到第一条命中行的控件上。
describe("firstHitControl（搜索后跳到第一条命中）", () => {
  it("跳过被过滤掉的行与分区，取第一条可见命中行里的控件", () => {
    const body = build();
    body.querySelectorAll(".row").forEach((row, i) => {
      const btn = document.createElement("button");
      btn.textContent = `ctl${i}`;
      row.appendChild(btn);
    });
    applySettingsFilter(body, "贴纸颜色", NAV);
    expect(firstHitControl(body)?.textContent).toBe("ctl3");
    applySettingsFilter(body, "开机", NAV);
    expect(firstHitControl(body)?.textContent).toBe("ctl1");
    applySettingsFilter(body, "完全不存在", NAV);
    expect(firstHitControl(body)).toBeNull();
  });
});
