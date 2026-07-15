import type { Metadata } from "next";
import ChangelogContent from "@/components/pages/ChangelogContent";

export const metadata: Metadata = {
  title: "Changelog · Meowo",
  description: "Release notes for each Meowo version, pulled from GitHub Releases.",
  alternates: { canonical: "/en/changelog", languages: { "zh-CN": "/changelog", en: "/en/changelog" } },
};

export default function ChangelogEn() {
  return <ChangelogContent lang="en" />;
}
