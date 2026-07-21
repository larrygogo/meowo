// 带勾选标记的清单项：.checklist 下的 li 结构统一从这里出。
// 默认 CheckIcon；个别场景（如「点击直达」）可换成别的图标，视觉不变。
import { CheckIcon } from "@/components/icons";

export default function CheckItem({
  children,
  icon,
}: {
  children: React.ReactNode;
  icon?: React.ReactNode;
}) {
  return (
    <li>
      <span className="ck">{icon ?? <CheckIcon />}</span>
      <span>{children}</span>
    </li>
  );
}
