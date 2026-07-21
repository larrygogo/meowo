// 账号 / 中转展示卡：首页与功能页共用（原两处手写同构 JSX 抽取）。
export type AcctRow = { av: string; bg: string; name: string; badge: string; cls: string };

export default function AccountCard({
  title,
  body,
  rows,
}: {
  title: string;
  body: string;
  rows: AcctRow[];
}) {
  return (
    <div className="acct-card">
      <h3>{title}</h3>
      <p>{body}</p>
      <div className="acct-rows">
        {rows.map((r) => (
          <div className="acct-row" key={r.name}>
            <span className="avatar" style={{ background: r.bg }}>{r.av}</span>
            <span className="aname">{r.name}</span>
            <span className={`abadge ${r.cls}`}>{r.badge}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
