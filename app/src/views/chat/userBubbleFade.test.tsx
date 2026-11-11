import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Message } from "./Message";

// UI 调研 chat #13：用户长消息折叠后底部没有渐隐，「展开全部」直接压在半截文字上。
// 根因：渐隐靠叠一层「透明 → 气泡色」的渐变，而深色主题气泡色是 rgba(26,136,255,.16)，
// 叠上去文字照样透出来。改为只对文字层做 mask，气泡底色不参与。
const css = readFileSync(resolve(__dirname, "../../styles.css"), "utf8");

afterEach(cleanup);

describe("用户长消息渐隐", () => {
  it("收起限高与渐隐落在包住正文的内层，不落在带气泡底色的 .chat-text 上", () => {
    render(<Message item={{ type: "user_text", id: "fade", timestamp: null, text: "很长的日志" }} />);
    const clip = document.querySelector(".chat-text > .chat-user-clip.is-clampable");
    expect(clip?.textContent).toBe("很长的日志");
    expect(css).toMatch(/\.chat-user-text\.is-clamped \.chat-user-clip \{[^}]*mask-image/);
    // 不再用「渐变到气泡色」的遮罩层（半透明气泡色盖不住字）。
    expect(css).not.toMatch(/\.chat-user-text\.is-clamped \.chat-text::after/);
  });
});
