import type { Metadata } from "next";
import FaqContent from "@/components/pages/FaqContent";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo 常见问题：支持哪些 AI CLI、如何减少命令输入、代理设置、数据是否上传、自动接入、隐私与卸载。",
};

export default function FaqPage() {
  return <FaqContent lang="zh" />;
}
