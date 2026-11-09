import type { Metadata } from "next";
import FaqContent from "@/components/pages/FaqContent";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo FAQ: supported AI CLIs, phone access safety, the agent driving the browser, data privacy, accounts and proxies, the Windows install prompt, and uninstall.",
  alternates: { canonical: "/en/faq/", languages: { "zh-CN": "/faq/", en: "/en/faq/" } },
};

export default function FaqEn() {
  return <FaqContent lang="en" />;
}
