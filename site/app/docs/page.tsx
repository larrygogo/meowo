import type { Metadata } from "next";
import DocsContent from "@/components/pages/DocsContent";

export const metadata: Metadata = {
  title: "文档 · Meowo",
  description:
    "Meowo 文档：工作原理、自动接入 AI 编程 CLI、手动接入 Claude Code、数据与配置文件的位置。",
};

export default function DocsPage() {
  return <DocsContent lang="zh" />;
}
