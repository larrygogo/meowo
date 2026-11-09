import type { Metadata } from "next";
import FeaturesContent from "@/components/pages/FeaturesContent";

export const metadata: Metadata = {
  title: "Features · Meowo",
  description:
    "Features in Meowo 0.8.1: the chat window, a right panel with Changes, Files, Terminal and Browser, phone access, the island, quota and context, multiple accounts and API relay, proxy settings, and a comparison of the five agents.",
  alternates: { canonical: "/en/features/", languages: { "zh-CN": "/features/", en: "/en/features/" } },
};

export default function FeaturesEn() {
  return <FeaturesContent lang="en" />;
}
