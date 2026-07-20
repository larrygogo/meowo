import type { Metadata } from "next";
import FaqContent from "@/components/pages/FaqContent";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo FAQ: which AI CLIs are supported, how it cuts commands, proxy setup, data privacy, auto-connect, and uninstall.",
  alternates: { canonical: "/en/faq/", languages: { "zh-CN": "/faq/", en: "/en/faq/" } },
};

export default function FaqEn() {
  return <FaqContent lang="en" />;
}
