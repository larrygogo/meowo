import Reveal from "./Reveal";
import DemoFrame from "./DemoFrame";
import { type Lang } from "@/lib/i18n";

type Props = {
  lang?: Lang;
  className?: string;
};

/**
 * Hero：把 app 里那段「真人操作」演示（app/src/demo，构建到 /demo/）用 iframe 引进来。
 * 与 webp 动画完全一致、循环播放，并随语言切换（?lang）——app 自带的 i18n + demo 文案翻译。
 * iframe 隔离样式，不影响官网。
 */
export default function ProductShowcase({ lang = "zh", className = "" }: Props) {
  return (
    <div className={`showcase ${className}`.trim()}>
      <Reveal>
        <div className="window">
          <div className="window-bar">
            <span className="tl r" />
            <span className="tl y" />
            <span className="tl g" />
          </div>
          <div className="window-body">
            <DemoFrame lang={lang} />
          </div>
        </div>
      </Reveal>
    </div>
  );
}
