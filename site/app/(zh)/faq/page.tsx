import type { Metadata } from "next";
import FaqContent from "@/components/pages/FaqContent";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo 常见问题：支持哪些 AI CLI、手机远程是否安全、Agent 操作浏览器、数据是否上传、多账号与代理、Windows 安装提示与卸载。",
  alternates: { canonical: "/faq/", languages: { "zh-CN": "/faq/", en: "/en/faq/" } },
};

export default function FaqPage() {
  return <FaqContent lang="zh" />;
}
