// 贴纸看板与几处菜单共用的语义图标：形状在 icons/glyphs*，这里只给语义名和默认尺寸。纯展示，无状态。
import type { Tab } from "./types";
import { Icon, type IconSize } from "../../icons/Icon";
import { icChat, icCheck, icChevronDown, icCircleCheck, icClose, icFolderOpen, icHand, icMonitor, icMore, icPencil, icPin, icPlus, icRefreshCw, icRotateCcw, icSearch, icSettings, icStopCircle, icTerminal, icToTop } from "../../icons/glyphs";

/** 「标记完成 / 恢复进行中」的图标（底层仍是 archived 字段，名字沿用）。对照 Kimi 的
 *  state-done：未完成画圆圈对勾，已完成画回退箭头——此前是归档箱，和「标记完成」的文案对不上。 */
export function ArchiveIcon({ archived, size = 12 }: { archived: boolean; size?: IconSize }) {
  return <Icon glyph={archived ? icRotateCcw : icCircleCheck} size={size} />;
}

export function PencilIcon() {
  return (
    <Icon glyph={icPencil} size={12} />
  );
}

export function PlusIcon() {
  return (
    <Icon glyph={icPlus} size={14} />
  );
}

export function OpenIcon() {
  // lucide terminal：命令行 >_ 符号，明确表达「打开/跳转终端」（原 share 样图标易误读为分享）
  return (
    <Icon glyph={icTerminal} size={12} />
  );
}

export function MoreIcon() {
  // lucide ellipsis：卡片菜单按钮（card_menu_mode=button 时替代右键触发）
  return (
    <Icon glyph={icMore} size={12} />
  );
}

// lucide check：行内编辑器确认钮、下拉菜单选中标记共用一份形状（此前抄了 3 份，粗细 2.4/2.5 混用）。
export function CheckIcon({ className }: { className?: string } = {}) {
  return <Icon glyph={icCheck} size={12} className={className} />;
}

// lucide chevron-down：下拉触发钮的展开箭头（菜单/侧栏分组头/对话窗标题共用，此前抄了 3 份）。
export function ChevronDownIcon({ className }: { className?: string } = {}) {
  return <Icon glyph={icChevronDown} size={12} className={className} />;
}

export function XIcon() {
  // lucide x：行内编辑器取消钮
  return (
    <Icon glyph={icClose} size={12} />
  );
}

export function FolderIcon() {
  // lucide folder-open：右键菜单「打开项目目录」用
  return (
    <Icon glyph={icFolderOpen} size={12} />
  );
}

export function StopIcon() {
  // lucide circle-stop：右键菜单「结束会话」用
  return (
    <Icon glyph={icStopCircle} size={12} />
  );
}

/**
 * 会话置顶（排到列表最前）。lucide arrow-up-to-line——刻意**不用**图钉：图钉在贴纸窗
 * 顶栏已经是「置顶窗口(alwaysOnTop)」的图标，同图不同义会让两个功能互相冒充。
 * 存储与内部标识沿用 star 命名（meowo-starred），改键会丢用户已有数据。
 */
export function TopIcon() {
  return (
    <Icon glyph={icToTop} size={12} />
  );
}

export function PinIcon({ pinned }: { pinned: boolean }) {
  // lucide pin：未置顶描边、置顶时填充以示激活（尺寸与搜索/设置图标统一）
  return <Icon glyph={icPin} size={12} fill={pinned ? "currentColor" : "none"} />;
}

export function GearIcon() {
  // lucide settings：齿轮，打开设置
  return (
    <Icon glyph={icSettings} size={12} />
  );
}

export function SearchIcon() {
  // lucide search：放大镜
  return (
    <Icon glyph={icSearch} size={12} />
  );
}

export function CloseIcon() {
  // lucide x：关闭搜索
  return (
    <Icon glyph={icClose} size={12} />
  );
}

export function ChatIcon() {
  return (
    <Icon glyph={icChat} size={14} />
  );
}

export function EmptyIcon({ tab }: { tab: Tab }) {
  // 显示器 / 举手（待交互）/ 循环箭头。空态插画原是 28px，按四档收到最大的 20px。
  const glyph = tab === "all" ? icMonitor : tab === "waiting" ? icHand : icRefreshCw;
  return <Icon glyph={glyph} size={20} />;
}
