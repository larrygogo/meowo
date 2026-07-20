import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "Features · Meowo",
  description:
    "Meowo features: sticker expanded / traffic light collapsed, click to jump to the terminal, quota & context monitoring, multi-account switching & API relay, per-tool proxy, one-click install & sign-in, and styles/colors.",
  alternates: { canonical: "/en/features/", languages: { "zh-CN": "/features/", en: "/en/features/" } },
};

export default function FeaturesEn() {
  return <FeaturesContent lang="en" />;
}
