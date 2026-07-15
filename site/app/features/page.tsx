import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 的功能：展开贴纸 / 收起电子红绿灯、点击直达终端、配额与上下文监控、官方多账号一键切与 API 中转、按工具设置代理、一键安装登录 AI CLI、多风格多配色切换。",
};

export default function FeaturesPage() {
  return <FeaturesContent lang="zh" />;
}
