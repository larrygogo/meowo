pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    root_path   TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id    INTEGER NOT NULL REFERENCES projects(id),
    cc_session_id TEXT NOT NULL UNIQUE,
    status        TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    last_event_at INTEGER NOT NULL,
    ended_at      INTEGER
);

CREATE TABLE IF NOT EXISTS tasks (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id       INTEGER NOT NULL REFERENCES projects(id),
    session_id       INTEGER REFERENCES sessions(id),
    title            TEXT NOT NULL,
    column_name      TEXT NOT NULL,
    column_locked    INTEGER NOT NULL DEFAULT 0,
    current_activity TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS todos (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id   INTEGER NOT NULL REFERENCES tasks(id),
    content   TEXT NOT NULL,
    status    TEXT NOT NULL,
    order_idx INTEGER NOT NULL
);

-- events: 预留给后续计划的事件审计流，当前管线尚未写入。
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id),
    kind       TEXT NOT NULL,
    payload    TEXT,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_tasks_session ON tasks(session_id) WHERE session_id IS NOT NULL;
"#;
