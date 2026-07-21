import type { Metadata } from "next";
import HomeContent from "@/components/pages/HomeContent";

export const metadata: Metadata = {
  alternates: { canonical: "/", languages: { "zh-CN": "/", en: "/en/" } },
};

export default function Home() {
  return <HomeContent lang="zh" />;
}
