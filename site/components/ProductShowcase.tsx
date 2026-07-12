import Reveal from "./Reveal";

type Props = {
  caption?: string;
  className?: string;
};

/** 深色贴纸产品图，套一层窗口壳；在浅色页面上形成高级的明暗对比。 */
export default function ProductShowcase({ caption, className = "" }: Props) {
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
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src="/demo.webp"
              alt="Meowo 实时会话看板演示：会话卡片、状态分类、底栏用量"
              loading="lazy"
            />
          </div>
        </div>
      </Reveal>
      {caption && <p className="showcase-cap">{caption}</p>}
    </div>
  );
}
