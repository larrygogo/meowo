import { TaskCard, todoProgress } from "../api";

const COLUMNS: { key: "todo" | "doing" | "done"; label: string }[] = [
  { key: "todo", label: "待办" },
  { key: "doing", label: "进行中" },
  { key: "done", label: "完成" },
];

function Card({ card }: { card: TaskCard }) {
  const { done, total, percent } = todoProgress(card.todos);
  return (
    <div className="task-card">
      <div className="task-title">{card.task.title}</div>
      {card.task.current_activity && <div className="task-act">{card.task.current_activity}</div>}
      {total > 0 && (
        <>
          <div className="bar">
            <i style={{ width: `${percent}%` }} />
          </div>
          <div className="task-act">
            {done}/{total} · {percent}%
          </div>
        </>
      )}
    </div>
  );
}

export function ProjectBoard({ cards }: { cards: TaskCard[] }) {
  return (
    <div className="board">
      {COLUMNS.map((col) => {
        const inCol = cards.filter((c) => c.task.column === col.key);
        return (
          <div key={col.key}>
            <div className="col-title">
              {col.label}（{inCol.length}）
            </div>
            {inCol.length === 0 ? (
              <div className="empty">—</div>
            ) : (
              inCol.map((c) => <Card key={c.task.id} card={c} />)
            )}
          </div>
        );
      })}
    </div>
  );
}
