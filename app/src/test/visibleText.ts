import type { MatcherFunction } from "@testing-library/react";

/**
 * 按「用户看到的整段文字」找元素：testing-library 的字符串匹配只看元素自己的直接文本节点，
 * 文字被行内元素切开就找不到（对话正文段尾的四个字包在 .chat-md-keep 里，见 ChatMarkdown 的 keepCjkTail）。
 * 字符串要求 textContent 完全相等，正则要求 textContent 能匹配；都只认包住这段文字的最内层元素
 * （没有哪个子元素单独就满足），所以一段话只对应一个元素（通常是 <p>），不会连父容器一起命中。
 */
export function visibleText(text: string | RegExp): MatcherFunction {
  const hit = (value: string | null) => value !== null && (typeof text === "string" ? value === text : text.test(value));
  return (_content, element) => {
    if (!element || !hit(element.textContent)) return false;
    return ![...element.children].some((child) => hit(child.textContent));
  };
}
