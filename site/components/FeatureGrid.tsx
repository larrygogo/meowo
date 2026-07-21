import type { ReactNode } from "react";
import Reveal from "./Reveal";
import { getDict, type Lang } from "@/lib/i18n";
import {
  BoardIcon,
  TerminalIcon,
  TodoIcon,
  CardsIcon,
  UsersIcon,
  ShieldIcon,
  PlugIcon,
  ChartIcon,
} from "./icons";

// 顺序与 lib/i18n.ts 的 dict.features 一一对应（8 张卡）。
const ICONS: ReactNode[] = [
  <BoardIcon key="board" />,
  <TerminalIcon key="term" />,
  <TodoIcon key="todo" />,
  <CardsIcon key="cards" />,
  <UsersIcon key="users" />,
  <ShieldIcon key="shield" />,
  <PlugIcon key="plug" />,
  <ChartIcon key="chart" />,
];

export default function FeatureGrid({ lang = "zh" }: { lang?: Lang }) {
  const features = getDict(lang).features;
  return (
    <div className="grid grid-4">
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
