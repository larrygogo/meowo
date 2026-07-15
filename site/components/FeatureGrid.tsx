import type { ReactNode } from "react";
import Reveal from "./Reveal";
import { getDict, type Lang } from "@/lib/i18n";
import {
  BoardIcon,
  BellIcon,
  TerminalIcon,
  CardsIcon,
  TrafficIcon,
  ChartIcon,
  PlugIcon,
  UsersIcon,
  NetworkIcon,
  PaletteIcon,
  ShieldIcon,
} from "./icons";

// 顺序与 lib/i18n.ts 的 dict.features 一一对应。
const ICONS: ReactNode[] = [
  <BoardIcon key="board" />,
  <BellIcon key="bell" />,
  <TerminalIcon key="term" />,
  <CardsIcon key="cards" />,
  <TrafficIcon key="traffic" />,
  <ChartIcon key="chart" />,
  <PlugIcon key="plug" />,
  <UsersIcon key="users" />,
  <NetworkIcon key="net" />,
  <PaletteIcon key="palette" />,
  <ShieldIcon key="shield" />,
];

export default function FeatureGrid({ lang = "zh" }: { lang?: Lang }) {
  const features = getDict(lang).features;
  return (
    <div className="grid grid-3">
      {features.map((f, i) => (
        <Reveal key={f.title}>
          <div className="fcard">
            <div className="fi">{ICONS[i]}</div>
            <h3>{f.title}</h3>
            <p>{f.body}</p>
          </div>
        </Reveal>
      ))}
    </div>
  );
}
