/// 带参切换命令（`/model sonnet`、`/effort high`）在对话中途弹的二次确认框：认出来、核对是这次要切的
/// 那一个，才给出确认键。规格由插件声明（ChatUi.switch_confirm，见后端 chat_ui::SwitchConfirm）。
///
/// 为什么需要：claude 2.1.325 在会话里已经有过回合时不直接切，先问 `Switch model?`
/// （真机取证 2026-11-14，tests/probe_claude_model.rs）。不认这个框，命令发出去后等不到确认行，
/// 只能报「未确认」，框留在终端里把之后发的消息吞掉；回合结束后自动执行的切换同样撞上。

import type { ChatUi } from "../../api";

type SwitchConfirm = NonNullable<ChatUi["switch_confirm"]>;

const squash = (text: string) => text.toLowerCase().replace(/[^a-z0-9]/g, "");

/// 屏幕上（本次命令之后的输出，逐行）有没有这次切换的确认框。有且对得上就返回确认键，否则 null：
/// - 标题行在，且它之后有带焦点的确认项；
/// - 确认项里要切到的目标含 `target`（模型家族名或思考档，比对时只看字母数字）。
/// 焦点在取消项上、或框里要切到的是别的目标：不按，由调用方照旧报未确认并给看终端画面的出口。
export function switchConfirmInput(lines: string[], spec: SwitchConfirm | null | undefined, target: string): string | null {
  if (!spec) return null;
  const title = new RegExp(spec.title_pattern, "i");
  const accept = new RegExp(spec.accept_pattern, "i");
  // 取最后一个标题：重绘会把同一个框画好几遍。
  let titleAt = -1;
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    if (title.test(lines[index])) { titleAt = index; break; }
  }
  if (titleAt < 0) return null;
  const wanted = squash(target);
  if (!wanted) return null;
  for (const line of lines.slice(titleAt + 1)) {
    const hit = accept.exec(line);
    if (hit) return squash(hit[1] ?? "").includes(wanted) ? spec.accept_input : null;
  }
  return null;
}
