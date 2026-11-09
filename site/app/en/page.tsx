import type { Metadata } from "next";
import HomeContent from "@/components/pages/HomeContent";

export const metadata: Metadata = {
  title: "Meowo — Desktop workbench for AI coding agents",
  description:
    "Meowo is a local-first desktop workbench for AI coding agents. Run Claude Code, Codex, Kimi, Gemini CLI and OpenCode sessions in one chat window with approvals, diffs, terminal and browser at hand; an island at the top of your screen and your phone keep you in the loop. Open source, MIT, Windows & macOS.",
  alternates: { canonical: "/en/", languages: { "zh-CN": "/", en: "/en/" } },
};

export default function HomeEn() {
  return <HomeContent lang="en" />;
}
