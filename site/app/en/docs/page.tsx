import type { Metadata } from "next";
import DocsContent from "@/components/pages/DocsContent";

export const metadata: Metadata = {
  title: "Docs · Meowo",
  description:
    "Meowo docs: how it works, auto-connecting AI coding CLIs, manual Claude Code setup, and where data and config files live.",
  alternates: { canonical: "/en/docs/", languages: { "zh-CN": "/docs/", en: "/en/docs/" } },
};

export default function DocsEn() {
  return <DocsContent lang="en" />;
}
