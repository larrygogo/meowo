import type { Metadata } from "next";
import HomeContent from "@/components/pages/HomeContent";

export const metadata: Metadata = {
  title: "Meowo — Desktop workbench for AI coding agents",
  description:
    "Meowo is a local-first desktop workbench for AI coding agents. Track status, to-dos, and quotas for multiple CLI sessions; launch and resume without commands. Open source, MIT, Windows & macOS.",
  alternates: { canonical: "/en", languages: { "zh-CN": "/", en: "/en" } },
};

export default function HomeEn() {
  return <HomeContent lang="en" />;
}
