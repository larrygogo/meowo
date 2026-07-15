import type { Metadata } from "next";
import ChangelogContent from "@/components/pages/ChangelogContent";

export const metadata: Metadata = {
  title: "更新日志 · Meowo",
  description: "Meowo 各版本的发布说明，取自 GitHub Releases。",
};

export default function ChangelogPage() {
  return <ChangelogContent lang="zh" />;
}
