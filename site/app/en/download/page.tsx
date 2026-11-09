import type { Metadata } from "next";
import DownloadContent from "@/components/pages/DownloadContent";

export const metadata: Metadata = {
  title: "Download · Meowo",
  description:
    "Download Meowo: a Windows x64 installer, and macOS DMGs for Apple silicon and Intel (signed and notarized). You can install and sign in to the AI coding CLIs from Meowo afterwards.",
  alternates: { canonical: "/en/download/", languages: { "zh-CN": "/download/", en: "/en/download/" } },
};

export default function DownloadEn() {
  return <DownloadContent lang="en" />;
}
