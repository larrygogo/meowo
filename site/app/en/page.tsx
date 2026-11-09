import type { Metadata } from "next";
import HomeContent from "@/components/pages/HomeContent";

export const metadata: Metadata = {
  title: "Meowo — Desktop workbench for AI coding agents",
  description:
    "Meowo is a desktop app for using several AI coding CLIs at once: Claude Code, Codex, Kimi, Gemini CLI and OpenCode. Sessions run in its chat window, next to the changes, a terminal and a browser. The island at the top of the screen alerts you when a session is waiting, and you can also handle it from your phone. Open source, MIT, Windows and macOS.",
  alternates: { canonical: "/en/", languages: { "zh-CN": "/", en: "/en/" } },
};

export default function HomeEn() {
  return <HomeContent lang="en" />;
}
