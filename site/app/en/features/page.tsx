import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "Features · Meowo",
  description:
    "Meowo features: structured chat with one-click approvals, a side panel for changes, files, terminal and browser, phone access, an always-on island at the top of the screen, quota & context monitoring, multiple accounts & API relay, per-agent proxy, and one-click install & sign-in.",
  alternates: { canonical: "/en/features/", languages: { "zh-CN": "/features/", en: "/en/features/" } },
};

export default function FeaturesEn() {
  return <FeaturesContent lang="en" />;
}
