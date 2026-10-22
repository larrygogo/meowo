import { useMemo } from "react";
import { useT } from "../../i18n";
import { approvalScope } from "./approvalScope";

/// 审批卡正文里的一行影响范围标签（判定见 approvalScope.ts）。一个标签都没有时不占位。
export function ApprovalScopeTags({ tool, input, dirs }: { tool: string | null | undefined; input: string; dirs: readonly string[] }) {
  const t = useT();
  const tags = useMemo(() => approvalScope(tool, input, dirs), [tool, input, dirs]);
  if (tags.length === 0) return null;
  return (
    <div className="chat-approval-scope">
      {tags.map((tag, index) => {
        switch (tag.kind) {
          case "outside":
            return <span key={index} className="is-warn" data-tip={tag.path}>{t.chat.scopeOutside(tag.path)}</span>;
          case "delete":
            return <span key={index} className="is-warn">{t.chat.scopeDelete}</span>;
          case "risky":
            return <span key={index} className="is-warn">{t.chat.scopeRisky}</span>;
          case "network":
            return <span key={index}>{t.chat.scopeNetwork}</span>;
          case "inside":
            return <span key={index} className="is-ok">{t.chat.scopeInside}</span>;
        }
      })}
    </div>
  );
}
