import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 的功能：结构化对话与按钮化审批、右侧改动 / 文件 / 终端 / 浏览器面板、手机远程、常驻屏幕顶部的灵动岛、额度与上下文监控、官方多账号与 API 中转、按 Agent 设置代理、一键安装登录 AI CLI。",
  alternates: { canonical: "/features/", languages: { "zh-CN": "/features/", en: "/en/features/" } },
};

export default function FeaturesPage() {
  return <FeaturesContent lang="zh" />;
}
