import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 0.8.1 的功能：对话窗口、右侧的更改 / 文件 / 终端 / 浏览器面板、手机远程、灵动岛、额度与上下文、多账号与 API 中转、代理设置，以及五家 Agent 的支持对照。",
  alternates: { canonical: "/features/", languages: { "zh-CN": "/features/", en: "/en/features/" } },
};

export default function FeaturesPage() {
  return <FeaturesContent lang="zh" />;
}
