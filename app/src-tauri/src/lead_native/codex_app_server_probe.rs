//! 真机探针：Codex 原生 TUI（--remote 连每会话 app-server）在线时、以及退出后重开时收到 Lead 关卡答复。
//! 走与手机作答完全相同的后端路径（lead_conversation::answer_and_deliver → lead_native → 本模块）。
//!
//! 默认用进程内假 Responses 服务（不花额度）；`MEOWO_PROBE_REAL=1` 时复制本机 auth.json 到隔离
//! CODEX_HOME，用真实模型（推理强度压到 low）。只读用户 ~/.codex，不写；跑完删除临时目录与认证副本。
//! 证据（屏幕、状态、耗时，不含凭据）写到 `$CARGO_TARGET_DIR/codex-remote-probe/<时间戳>/result.json`。
//!
//! ```text
//! cargo test -p meowo-app --lib codex_remote_probe -- --ignored --nocapture --test-threads=1
//! ```
use super::*;
use portable_pty::{native_pty_system, PtySize};
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Instant;

struct Tui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    screen: Arc<Mutex<vt100::Parser>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn text(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }
    fn send(&self, text: &str) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
    fn wait(&self, secs: u64, pred: impl Fn(&str) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if pred(&self.text()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    fn quit(&mut self) {
        // 输入框可能有草稿：第一下 Ctrl-C 清草稿，再两下退出。
        for _ in 0..3 {
            self.send("\x03");
            std::thread::sleep(Duration::from_millis(600));
            if !self.alive() {
                return;
            }
        }
        let _ = self.child.kill();
    }
}

/// assistant 行以「• 」开头，用户行以「› 」开头：口令出现在 assistant 行才算模型输出了它。
fn assistant_says(screen: &str, nonce: &str) -> bool {
    screen
        .lines()
        .any(|line| line.trim_start().starts_with('•') && line.contains(nonce))
}

fn rollout_assistant_says(home: &Path, nonce: &str) -> bool {
    let mut stack = vec![home.join("sessions")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if std::fs::read_to_string(&path).is_ok_and(|body| {
                body.lines()
                    .any(|line| line.contains("\"role\":\"assistant\"") && line.contains(nonce))
            }) {
                return true;
            }
        }
    }
    false
}

fn nonce(tag: &str) -> String {
    format!(
        "{tag}_{}",
        crate::remote::generate_token().unwrap()[..10].to_uppercase()
    )
}

/// 假 Responses 服务：回最后一条用户消息里最后一个 `XXX_口令`，记录 originator 与 turn_trigger。
fn fake_model(log: Arc<Mutex<Vec<Value>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let nonce_re = regex::Regex::new(r"\b(?:FIRST|LEAD)_[A-Z0-9]{10}\b").unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = log.clone();
            let nonce_re = nonce_re.clone();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut headers = Vec::new();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = line.trim_end().split_once(": ") {
                        if k.eq_ignore_ascii_case("content-length") {
                            length = v.parse().unwrap_or(0);
                        }
                        headers.push((k.to_ascii_lowercase(), v.to_owned()));
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let request: Value = serde_json::from_slice(&body).unwrap_or_default();
                let items = request["input"].as_array().cloned().unwrap_or_default();
                let last_user = items
                    .iter()
                    .rev()
                    .find(|i| i["role"] == "user")
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                let last_is_tool_output = items
                    .last()
                    .is_some_and(|i| i["type"].as_str().is_some_and(|t| t.ends_with("_output")));
                // 剧本：APPROVE_ME → 调一条要提权的命令（停在审批框）；SPAWN_SUB → 派子代理；
                // SUB_TASK（子代理收到的任务）→ SUB_DONE；工具结果回来 → TOOL_DONE；其余回口令。
                let call = |name: &str, namespace: Option<&str>, args: Value| {
                    let mut item = json!({"type":"function_call","id":"fc","call_id":format!("call_{}", crate::remote::generate_token().unwrap()[..8].to_owned()),"name":name,"arguments":args.to_string()});
                    if let Some(ns) = namespace {
                        item["namespace"] = json!(ns);
                    }
                    item
                };
                let (reply, item) = if last_is_tool_output {
                    ("TOOL_DONE".to_owned(), None)
                } else if last_user.contains("APPROVE_ME") {
                    (
                        "<exec_command>".to_owned(),
                        Some(call(
                            "exec_command",
                            None,
                            json!({"cmd":"New-Item -ItemType File probe-approval.txt","sandbox_permissions":"require_escalated","justification":"probe approval"}),
                        )),
                    )
                } else if last_user.contains("SPAWN_SUB") {
                    (
                        "<spawn_agent>".to_owned(),
                        Some(call(
                            "spawn_agent",
                            Some("multi_agent_v1"),
                            json!({"message":"SUB_TASK reply SUB_DONE only"}),
                        )),
                    )
                } else if last_user.contains("SUB_TASK") {
                    ("SUB_DONE".to_owned(), None)
                } else {
                    (
                        nonce_re
                            .find_iter(&last_user)
                            .last()
                            .map(|m| m.as_str().to_owned())
                            .unwrap_or_else(|| "OK".into()),
                        None,
                    )
                };
                let header = |k: &str| headers.iter().find(|(h, _)| h == k).map(|(_, v)| v.clone());
                let meta: Value = header("x-codex-turn-metadata")
                    .and_then(|m| serde_json::from_str(&m).ok())
                    .unwrap_or_default();
                let tools: Vec<Value> = request["tools"]
                    .as_array()
                    .map(|tools| tools.iter().map(|t| t["name"].clone()).collect())
                    .unwrap_or_default();
                log.lock().unwrap().push(json!({
                    "reply": reply,
                    "originator": header("originator"),
                    "userAgent": header("user-agent"),
                    "turnTrigger": meta["turn_trigger"],
                    "tools": tools,
                    "toolsFull": if std::env::var("MEOWO_PROBE_DUMP_TOOLS").is_ok() { request["tools"].clone() } else { Value::Null },
                    "lastInput": items.last().cloned(),
                }));
                let output = item.unwrap_or_else(|| json!({"type":"message","role":"assistant","id":"m","content":[{"type":"output_text","text":reply}]}));
                let events = [
                    json!({"type":"response.created","response":{"id":"r"}}),
                    json!({"type":"response.output_item.done","item":output}),
                    json!({"type":"response.completed","response":{"id":"r","usage":{"input_tokens":1,"input_tokens_details":null,"output_tokens":1,"output_tokens_details":null,"total_tokens":2}}}),
                ];
                let sse: String = events
                    .iter()
                    .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
                    .collect();
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{sse}",
                    sse.len()
                );
            });
        }
    });
    port
}

struct Probe {
    root: PathBuf,
    home: PathBuf,
    work: PathBuf,
    db: PathBuf,
    env: Vec<(String, String)>,
    exe: Vec<String>,
}

impl Probe {
    fn launch(&self, session_id: i64, args: &[&str]) -> (Tui, u32) {
        let mut command = CommandBuilder::new(&self.exe[0]);
        command.args(&self.exe[1..]);
        command.args(args);
        // 关启动更新检查：探针曾按到「立即更新」把用户的 codex 升了级（probe_codex_backtrack）。
        command.args(["-c", "check_for_update_on_startup=false"]);
        command.cwd(&self.work);
        // 本进程（跑在 Meowo 会话里）继承了外层 PTY 凭据：一律摘掉，防探针认领外层会话。
        for (key, _) in std::env::vars() {
            if key.starts_with("MEOWO_") || key.starts_with("CODEX_") || key.starts_with("CLAUDE") {
                command.env_remove(key);
            }
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let launch =
            prepare_launch(session_id, "codex", &self.env, &mut command).expect("应进入远程模式");
        let argv: Vec<String> = command
            .get_argv()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(argv.iter().any(|a| a == "--remote"), "{argv:?}");
        assert!(!argv.iter().any(|a| a == "-c"), "TUI 不得带 -c：{argv:?}");
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        bind_tui(launch, child.process_id());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
        let (answer, parser) = (writer.clone(), screen.clone());
        std::thread::spawn(move || {
            let mut bytes = [0; 16_384];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                let chunk = &bytes[..count];
                let mut writer = answer.lock().unwrap();
                if chunk.windows(4).any(|part| part == b"\x1b[6n") {
                    let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
                }
                if chunk.windows(3).any(|part| part == b"\x1b[c") {
                    let _ = writer.write_all(b"\x1b[?1;2c").and_then(|_| writer.flush());
                }
                drop(writer);
                parser.lock().unwrap().process(chunk);
            }
        });
        let tui = Tui {
            child,
            writer,
            screen,
            _master: pair.master,
        };
        let ready = tui.wait(60, |s| s.contains("› ") || s.contains("Ask Codex"));
        assert!(ready, "TUI 未就绪：\n{}", tui.text());
        let server = states(&self.db)
            .into_iter()
            .map(|(_, state)| state.server_pid)
            .max()
            .unwrap();
        (tui, server)
    }

    fn gate(&self, id: &str) -> PathBuf {
        let task = self.work.join(".lead").join("probe");
        std::fs::create_dir_all(&task).unwrap();
        std::fs::write(
            task.join("gate.json"),
            json!({"id": id, "type": "Q", "title": "probe", "status": "pending",
                   "options": [{"id": "go", "label": "Continue"}]})
            .to_string(),
        )
        .unwrap();
        task
    }

    fn answer(&self, gate: &str, nonce: &str) -> (Result<bool, String>, LeadDeliveryDto, u128) {
        let started = Instant::now();
        let note = format!(
            "这是连通性测试：本轮只回复 {nonce} 这一个词，不要调用任何工具，不要读写文件。"
        );
        let result = crate::lead_conversation::answer_and_deliver(
            &self.db,
            &self.work.to_string_lossy(),
            gate,
            &meowo_protocol::ipc::LeadGateAnswerDto {
                choice: "go".into(),
                note,
            },
            None,
        );
        let ms = started.elapsed().as_millis();
        let status =
            crate::lead_conversation::delivery_status(&self.db, &self.work.to_string_lossy(), gate)
                .unwrap();
        (result, status, ms)
    }

    fn wait_rollout(&self, nonce: &str, secs: u64) -> Option<u128> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(secs) {
            if rollout_assistant_says(&self.home, nonce) {
                return Some(started.elapsed().as_millis());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        None
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        for (dir, _) in states(&self.db) {
            retire(&dir);
        }
        // 刚结束的进程可能还占着 sqlite/日志句柄：重试到删干净为止（含认证副本）。
        for _ in 0..40 {
            if std::fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if self.root.exists() {
            eprintln!("临时目录未删净，请手动删除：{}", self.root.display());
        }
    }
}

fn thread_of(db: &Path, first: &str) -> String {
    let (dir, state) = states(db).into_iter().next().unwrap();
    let token = std::fs::read_to_string(dir.join("token")).unwrap();
    let mut client = ws::Client::connect(state.port, token.trim(), RPC_TIMEOUT).unwrap();
    let init = client
        .call(
            "initialize",
            json!({"clientInfo": {"name": "meowo-probe", "version": "0"}}),
            RPC_TIMEOUT,
        )
        .unwrap();
    assert!(originator_is_tui(init["userAgent"].as_str()), "{init}");
    client.notify("initialized").unwrap();
    let loaded = client
        .call("thread/loaded/list", json!({}), RPC_TIMEOUT)
        .unwrap();
    for id in loaded["data"].as_array().unwrap() {
        let read = client
            .call(
                "thread/read",
                json!({"threadId": id, "includeTurns": false}),
                RPC_TIMEOUT,
            )
            .unwrap();
        if read["thread"]["preview"]
            .as_str()
            .is_some_and(|p| p.contains(first))
        {
            return id.as_str().unwrap().to_owned();
        }
    }
    panic!("找不到主 thread：{loaded}");
}

/// 本机 reporter（hook 用）：MEOWO_PROBE_REPORTER 优先，否则当前 CARGO_TARGET_DIR 下的 debug 构建。
fn reporter_path() -> String {
    let exe = std::env::var("MEOWO_PROBE_REPORTER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
                .join("debug/meowo-reporter.exe")
        });
    dunce::canonicalize(&exe)
        .unwrap_or_else(|_| panic!("先 cargo build -p meowo-reporter：{}", exe.display()))
        .to_string_lossy()
        .into_owned()
}

/// 服务上已加载的 thread：(id, 父 thread, 预览)。
fn threads(db: &Path) -> Vec<(String, Option<String>, String)> {
    let Some((dir, state)) = states(db).into_iter().next() else {
        return Vec::new();
    };
    let token = std::fs::read_to_string(dir.join("token")).unwrap();
    let mut client = ws::Client::connect(state.port, token.trim(), RPC_TIMEOUT).unwrap();
    client
        .call(
            "initialize",
            json!({"clientInfo": {"name": "meowo-probe", "version": "0"}}),
            RPC_TIMEOUT,
        )
        .unwrap();
    client.notify("initialized").unwrap();
    let loaded = client
        .call("thread/loaded/list", json!({}), RPC_TIMEOUT)
        .unwrap();
    loaded["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| {
            let read = client
                .call(
                    "thread/read",
                    json!({"threadId": id, "includeTurns": false}),
                    RPC_TIMEOUT,
                )
                .ok()?;
            Some((
                id.as_str()?.to_owned(),
                parent_of(&read["thread"]),
                read["thread"]["preview"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ))
        })
        .collect()
}

/// 主 thread 最近一回合（完整条目），看拒绝审批后回合与命令条目记成什么状态。
fn last_turn(db: &Path, thread: &str) -> Value {
    let Some((dir, state)) = states(db).into_iter().next() else {
        return Value::Null;
    };
    let token = std::fs::read_to_string(dir.join("token")).unwrap();
    let mut client = ws::Client::connect(state.port, token.trim(), RPC_TIMEOUT).unwrap();
    client
        .call(
            "initialize",
            json!({"clientInfo": {"name": "meowo-probe", "version": "0"}}),
            RPC_TIMEOUT,
        )
        .unwrap();
    client.notify("initialized").unwrap();
    client
        .call(
            "thread/turns/list",
            json!({"threadId": thread, "limit": 1, "itemsView": "full"}),
            RPC_TIMEOUT,
        )
        .unwrap_or(Value::Null)
}

fn rollouts(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![home.join("sessions")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// 某个 thread 的 rollout 里有没有含口令的**用户**消息（答复有没有被投进这个 thread）。
fn rollout_user_says(home: &Path, thread: &str, nonce: &str) -> bool {
    rollouts(home)
        .iter()
        .filter(|p| p.to_string_lossy().contains(thread))
        .any(|p| {
            std::fs::read_to_string(p).is_ok_and(|body| {
                body.lines()
                    .any(|line| line.contains("\"role\":\"user\"") && line.contains(nonce))
            })
        })
}

/// 每个 rollout 的 session_meta.originator（计费归属：真实模型下抓不到请求头，以 rollout 为准）。
fn rollout_originators(home: &Path) -> Vec<Value> {
    rollouts(home)
        .iter()
        .filter_map(|p| {
            let body = std::fs::read_to_string(p).ok()?;
            let meta = body.lines().find_map(|line| {
                let v: Value = serde_json::from_str(line).ok()?;
                (v["type"] == "session_meta").then_some(v)
            })?;
            Some(json!({
                "file": p.file_name()?.to_string_lossy(),
                "id": meta.pointer("/payload/id"),
                "originator": meta.pointer("/payload/originator"),
                "source": meta.pointer("/payload/source"),
            }))
        })
        .collect()
}

fn approval_shown(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("would you like to run")
        || lower.contains("yes, proceed")
        || lower.contains("allow command")
        || lower.contains("approve")
}

fn session_row(db: &Path, native: &str) -> Value {
    let store = crate::open_store(db).unwrap();
    match store.find_session_id_pub(native).unwrap() {
        Some(id) => json!({"id": id, "pid": store.session_pid(id).ok().flatten(),
                           "provider": store.session_provider(id).ok()}),
        None => Value::Null,
    }
}

fn save_evidence(out: &Path, evidence: &Value) {
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(
        out.join("result.json"),
        serde_json::to_string_pretty(evidence).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "拉起真实 codex（默认假模型；MEOWO_PROBE_REAL=1 用真实模型），手动复跑"]
fn codex_remote_probe() {
    let real = std::env::var("MEOWO_PROBE_REAL").as_deref() == Ok("1");
    let root = std::env::temp_dir().join(format!(
        "mcxp-{}",
        &crate::remote::generate_token().unwrap()[..8]
    ));
    let home = root.join("h");
    let work = root.join("w");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let root = dunce::canonicalize(root).unwrap();
    let (home, work) = (root.join("h"), root.join("w"));
    let db = root.join("board.db");
    // 先定位用户的真实数据目录（只读），再把本进程的 CODEX_HOME 指向隔离目录。
    let source = meowo_agent::installation(meowo_agent::id::CODEX)
        .unwrap()
        .data_dir;
    // db_path() 由 MEOWO_DB 决定：服务状态与投递记录都落在临时目录，不碰 ~/.meowo。
    std::env::set_var("MEOWO_DB", &db);
    // 本进程里起的 codex 进程都继承这个环境：必须指向隔离目录，否则会写进用户真实的 ~/.codex。
    std::env::set_var("CODEX_HOME", &home);
    let fake_log = Arc::new(Mutex::new(Vec::new()));
    let mut config = toml_edit::DocumentMut::new();
    config["check_for_update_on_startup"] = toml_edit::value(false);
    if real {
        std::fs::copy(source.join("auth.json"), home.join("auth.json")).unwrap();
        let user: toml_edit::DocumentMut = std::fs::read_to_string(source.join("config.toml"))
            .unwrap_or_default()
            .parse()
            .unwrap_or_default();
        for key in ["model", "model_provider"] {
            if let Some(value) = user.get(key).and_then(toml_edit::Item::as_str) {
                config[key] = toml_edit::value(value);
            }
        }
        if let Some(mode) = user
            .get("windows")
            .and_then(|w| w.get("sandbox"))
            .and_then(toml_edit::Item::as_str)
        {
            config["windows"]["sandbox"] = toml_edit::value(mode);
        }
        config["model_reasoning_effort"] = toml_edit::value("low");
    } else {
        let port = fake_model(fake_log.clone());
        config["model"] = toml_edit::value("fake-model");
        config["model_provider"] = toml_edit::value("fake");
        config["model_providers"]["fake"]["name"] = toml_edit::value("fake");
        config["model_providers"]["fake"]["base_url"] =
            toml_edit::value(format!("http://127.0.0.1:{port}/v1"));
        config["model_providers"]["fake"]["wire_api"] = toml_edit::value("responses");
        config["model_providers"]["fake"]["env_key"] = toml_edit::value("FAKE_KEY");
    }
    config["projects"][work.to_string_lossy().as_ref()]["trust_level"] =
        toml_edit::value("trusted");
    std::fs::write(home.join("config.toml"), config.to_string()).unwrap();
    // 装 Meowo 的通用 hook（SessionStart 等落库、pid=app-server），与生产同一接线；用于核实子代理
    // thread 会不会单独入库（做空 redteam-2 N4 遗留）。hook 用本分支构建的 reporter，库是 MEOWO_DB。
    let reporter = reporter_path();
    let meowo_dir = root.join("meowo");
    std::fs::create_dir_all(&meowo_dir).unwrap();
    let wired = meowo_agent::by_id("codex")
        .unwrap()
        .wire(&meowo_agent::wiring::WiringContext {
            cli_version: Some("0.167.2"),
            fallback_reporter: Some(&reporter),
            meowo_dir: &meowo_dir,
        });
    assert!(wired.is_none(), "hook 接线失败：{wired:?}");
    let env = vec![
        ("CODEX_HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("FAKE_KEY".to_owned(), "x".to_owned()),
        ("MEOWO_DB".to_owned(), db.to_string_lossy().into_owned()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
    ];
    crate::lead_runtime::update_installation(&meowo_protocol::ipc::LeadProviderInstallationDto {
        provider: "codex".into(),
        profile_id: None,
        profile_name: None,
        name: "Codex".into(),
        version: None,
        configured: true,
        supported: true,
        managed: true,
        installed: true,
        assets_installed: true,
        hooks_installed: true,
        automatic_continue: true,
        reason: None,
    });
    let exe = meowo_agent::by_id("codex").unwrap().launch_argv();
    let version = std::process::Command::new(&exe[0])
        .args(&exe[1..])
        .arg("--version")
        .output()
        .unwrap();
    let probe = Probe {
        root: root.clone(),
        home,
        work,
        db,
        env,
        exe,
    };
    let out = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("codex-remote-probe")
        .join(format!(
            "{}-{}",
            crate::now_ms(),
            if real { "real" } else { "fake" }
        ));
    let mut evidence = json!({
        "real": real,
        "codex": String::from_utf8_lossy(&version.stdout).trim(),
    });
    // on-request：模型要提权时弹审批（C 步要一个挂着的审批）；其余回合不调工具。
    let policy: &[&str] = &["-a", "on-request", "-s", "read-only"];
    let steps = std::env::var("MEOWO_PROBE_STEPS").unwrap_or_else(|_| "A,C,D,E".into());
    let step = |name: &str| steps.split(',').any(|s| s.trim() == name);

    // ---- A/B 在线：答复后同一会话自动开新回合，TUI 上看得到；输入框草稿不被覆盖。
    let (mut tui, server) = probe.launch(-1, policy);
    let first = nonce("FIRST");
    tui.send(&format!("只回复 {first} 这一个词，不要调用任何工具。"));
    std::thread::sleep(Duration::from_millis(500));
    tui.send("\r");
    assert!(
        tui.wait(180, |s| assistant_says(s, &first)),
        "首轮无回复：\n{}",
        tui.text()
    );
    let thread = thread_of(&probe.db, &first);
    let store = crate::open_store(&probe.db).unwrap();
    // hook 已把会话落库（pid=app-server）；没落上就照同一语义补一笔。
    let hooked = session_row(&probe.db, &thread);
    evidence["mainSessionRowFromHook"] = hooked.clone();
    let sid = match store.find_session_id_pub(&thread).unwrap() {
        Some(id) => id,
        None => {
            let project = store
                .upsert_project_by_root(&probe.work.to_string_lossy(), "probe", 1)
                .unwrap();
            let (sid, _) = store.start_session(project, &thread, 2).unwrap();
            store.set_session_provider(sid, "codex").unwrap();
            sid
        }
    };
    if store.session_pid(sid).ok().flatten().is_none() {
        store
            .set_session_shared_pid(sid, i64::from(server), 3)
            .unwrap();
    }
    let task = probe.gate("Q-online");
    std::fs::write(task.join(".owner"), &thread).unwrap();
    if step("A") {
        let draft = format!("draft-{}", &crate::remote::generate_token().unwrap()[..6]);
        tui.send(&draft);
        std::thread::sleep(Duration::from_millis(800));
        let online = nonce("LEAD");
        let (result, status, answer_ms) = probe.answer("Q-online", &online);
        let started = Instant::now();
        let shown = tui.wait(180, |s| assistant_says(s, &online));
        let shown_ms = started.elapsed().as_millis() + answer_ms;
        std::thread::sleep(Duration::from_secs(1));
        let screen = tui.text();
        evidence["A_online"] = json!({
            "nonce": online, "answer": result, "status": status.state, "answerMs": answer_ms,
            "replyOnScreenMs": shown_ms,
            "tuiShowsReply": shown, "draftKept": screen.contains(&draft), "screen": screen,
        });
        save_evidence(&out, &evidence);
        assert_eq!(result, Ok(true));
        assert_eq!(status.state, "accepted");
        assert!(shown, "在线投递未在 TUI 显示：\n{screen}");
        assert!(screen.contains(&draft), "草稿被覆盖：\n{screen}");
        // 已送达的不重发：同一关卡再触发一次投递（巡检/重复作答）不应新增回合。
        crate::lead_conversation::reconcile_task(&probe.db, &task, None);
        // 清掉草稿（Ctrl-U 清行）。
        tui.send("\x15");
        std::thread::sleep(Duration::from_millis(500));
    }

    // ---- C 待批审批：模型挂着一个提权命令的审批时作答。审批框不能被答掉；答复排队，拒绝审批后送达。
    if step("C") {
        let ask = if real {
            "用 exec_command 执行这条命令：New-Item -ItemType File probe-approval.txt ；参数 sandbox_permissions 设为 require_escalated（需要我批准）。不要做别的事，也不要换别的方式。".to_owned()
        } else {
            "APPROVE_ME".to_owned()
        };
        tui.send(&ask);
        std::thread::sleep(Duration::from_millis(500));
        tui.send("\r");
        let pending = tui.wait(180, approval_shown);
        let before = tui.text();
        let gate_c = nonce("LEAD");
        probe.gate("Q-approval");
        std::fs::write(task.join(".owner"), &thread).unwrap();
        let (result, status, answer_ms) = probe.answer("Q-approval", &gate_c);
        std::thread::sleep(Duration::from_secs(5));
        let during = tui.text();
        let still_pending = approval_shown(&during);
        let early = assistant_says(&during, &gate_c);
        let file_created = probe.work.join("probe-approval.txt").exists();
        // 手动处理审批：MEOWO_PROBE_APPROVAL=approve 按 y 批准；默认 Esc 拒绝（No, tell Codex what to do differently）。
        let approve = std::env::var("MEOWO_PROBE_APPROVAL").as_deref() == Ok("approve");
        let decided = Instant::now();
        tui.send(if approve { "y" } else { "\x1b" });
        // 模拟关卡巡检（生产每 10s 一次）：拒绝审批后由 kick_stalled 用 thread/queue/start 让排着的答复开跑。
        let mut shown = false;
        let mut patrols = 0;
        let mut kicked_ms = None;
        while decided.elapsed() < Duration::from_secs(120) {
            if tui.wait(3, |s| assistant_says(s, &gate_c)) {
                shown = true;
                break;
            }
            crate::lead_conversation::reconcile_task(&probe.db, &task, None);
            patrols += 1;
            let key = delivery_key(&task, "Q-approval", &thread);
            if kicked_ms.is_none()
                && read_record(&probe.db, &key).is_some_and(|r| r.queue.as_deref() == Some("started"))
            {
                kicked_ms = Some(decided.elapsed().as_millis());
            }
        }
        let after = tui.text();
        let queue_after = read_record(&probe.db, &delivery_key(&task, "Q-approval", &thread))
            .and_then(|r| r.queue);
        evidence["C_patrol"] = json!({
            "lastTurn": last_turn(&probe.db, &thread),
            "patrols": patrols, "kickedAtMs": kicked_ms, "queueState": queue_after,
            "statusAfter": crate::lead_conversation::delivery_status(&probe.db, &probe.work.to_string_lossy(), "Q-approval").map(|s| s.state),
            "fakeRequestsForNonce": fake_log.lock().unwrap().iter()
                .filter(|r| r["reply"].as_str() == Some(gate_c.as_str()))
                .map(|r| json!({"originator": r["originator"], "turnTrigger": r["turnTrigger"], "userAgent": r["userAgent"]}))
                .collect::<Vec<_>>(),
        });
        evidence["C_approval"] = json!({
            "nonce": gate_c, "approvalShown": pending, "answer": result, "status": status.state,
            "answerMs": answer_ms, "approvalStillPending5s": still_pending,
            "replyBeforeDecision": early, "fileCreatedBeforeDecision": file_created,
            "decision": if approve { "y(approve)" } else { "Esc(decline)" }, "replyAfterDecision": shown,
            "replyAfterDecisionMs": decided.elapsed().as_millis(),
            "fileCreatedAfter": probe.work.join("probe-approval.txt").exists(),
            "screenBefore": before, "screenDuring": during, "screenAfter": after,
        });
        save_evidence(&out, &evidence);
        if !shown {
            // 拒绝后没自动送达：发一句普通消息看排队的答复是否随后送达（只记录，不判过）。
            tui.send("继续");
            std::thread::sleep(Duration::from_millis(500));
            tui.send("\r");
            let late = tui.wait(120, |s| assistant_says(s, &gate_c));
            evidence["C_approval"]["replyAfterUserNudge"] = json!(late);
            evidence["C_approval"]["screenAfterNudge"] = json!(tui.text());
            save_evidence(&out, &evidence);
        }
        tui.send("\x15");
        std::thread::sleep(Duration::from_secs(2));
    }

    // ---- D 子代理：.owner 是子代理 thread 时作答，答复进主 thread（主 TUI 可见），不进子代理。
    if step("D") {
        let ask = if real {
            "调用 spawn_agent 派一个子代理，给它的任务是：用 exec_command 运行 Get-Location（只读命令），然后只回复 SUB_DONE 这一个词。派出后调用 wait_agent 等它结束，然后只回复 SPAWNED 一个词。不要做别的。".to_owned()
        } else {
            "SPAWN_SUB".to_owned()
        };
        tui.send(&ask);
        std::thread::sleep(Duration::from_millis(500));
        tui.send("\r");
        let spawned = (0..120).find_map(|_| {
            std::thread::sleep(Duration::from_secs(1));
            threads(&probe.db)
                .into_iter()
                .find(|(_, parent, _)| parent.as_deref() == Some(thread.as_str()))
        });
        let settled = tui.wait(180, |s| {
            s.lines().any(|l| {
                l.trim_start().starts_with('•')
                    && (l.contains("SPAWNED") || l.contains("TOOL_DONE"))
            })
        });
        let mut d = json!({
            "subagentSpawned": spawned.is_some(), "parentTurnSettled": settled,
            "threads": threads(&probe.db).iter().map(|(id, p, pre)| json!({"id": id, "parent": p, "preview": pre})).collect::<Vec<_>>(),
            "screenAfterSpawn": tui.text(),
        });
        if let Some((sub, _, _)) = spawned {
            // 遗留核实：子代理 thread 是否单独入库、有没有进程号（hook 写的）。
            std::thread::sleep(Duration::from_secs(3));
            d["subagentThread"] = json!(sub);
            d["subagentRowInDb"] = session_row(&probe.db, &sub);
            let gate_d = nonce("LEAD");
            probe.gate("Q-sub");
            std::fs::write(task.join(".owner"), &sub).unwrap();
            let (result, status, answer_ms) = probe.answer("Q-sub", &gate_d);
            let started = Instant::now();
            let shown = tui.wait(120, |s| assistant_says(s, &gate_d));
            d["nonce"] = json!(gate_d);
            d["answer"] = json!(result);
            d["status"] = json!(status.state);
            d["statusReason"] = json!(status.reason);
            d["answerMs"] = json!(answer_ms);
            d["mainTuiShowsReply"] = json!(shown);
            d["replyMs"] = json!(started.elapsed().as_millis() + answer_ms);
            d["deliveredToMainRollout"] = json!(rollout_user_says(&probe.home, &thread, &gate_d));
            d["deliveredToSubRollout"] = json!(rollout_user_says(&probe.home, &sub, &gate_d));
            d["screen"] = json!(tui.text());
        }
        evidence["D_subagent"] = d;
        save_evidence(&out, &evidence);
        std::fs::write(task.join(".owner"), &thread).unwrap();
    }

    // ---- E TUI 不在：服务随之收掉；作答只保存、不调模型、不起任何进程，记 waiting。
    //      重开会话（resume）后 TUI 握手，巡检经在线 queue/add 送达，TUI 上看得到。
    tui.quit();
    assert!(!tui.alive(), "TUI 未退出");
    let started = Instant::now();
    while !states(&probe.db).is_empty() && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let server_retired = states(&probe.db).is_empty();
    if step("E") {
        probe.gate("Q-reopen");
        std::fs::write(task.join(".owner"), &thread).unwrap();
        let reopen = nonce("LEAD");
        let marker = probe.root.to_string_lossy().into_owned();
        let procs_before = super::super::probe_util::snapshot();
        let (result, status, answer_ms) = probe.answer("Q-reopen", &reopen);
        std::thread::sleep(Duration::from_secs(5));
        let procs_after = super::super::probe_util::snapshot();
        let spawned = super::super::probe_util::new_related(&procs_before, &procs_after, &marker);
        let model_called = probe.wait_rollout(&reopen, 1).is_some();
        let gate_after = meowo_reporter::lead::read_gate(&task).map(|g| g.status);
        let (mut tui2, _) = probe.launch(sid, &[policy, &["resume", &thread]].concat());
        let started = Instant::now();
        crate::lead_conversation::reconcile_task(&probe.db, &task, None);
        let shown = tui2.wait(180, |s| assistant_says(s, &reopen));
        let delivered_ms = started.elapsed().as_millis();
        let status2 = crate::lead_conversation::delivery_status(
            &probe.db,
            &probe.work.to_string_lossy(),
            "Q-reopen",
        );
        evidence["E_offline"] = json!({
            "serverRetiredAfterQuit": server_retired,
            "nonce": reopen, "answer": result, "statusWhileClosed": status.state,
            "answerMs": answer_ms, "newProcessesWhileClosed": spawned,
            "procSnapshotOk": !procs_after.is_empty(),
            "modelCalledWhileClosed": model_called, "gateWhileClosed": gate_after,
            "statusAfterReopen": status2.map(|s| s.state),
            "deliveredAfterReopenMs": delivered_ms, "tuiShowsReply": shown, "screen": tui2.text(),
        });
        tui2.quit();
        save_evidence(&out, &evidence);
        assert_eq!(status.state, "waiting", "{status:?}");
        assert!(!model_called, "会话没开时不应调用模型");
        assert!(shown, "重开会话后答复未送达");
    }
    evidence["F_originators"] = json!(rollout_originators(&probe.home));
    evidence["fakeModelRequests"] = json!(*fake_log.lock().unwrap());
    save_evidence(&out, &evidence);
    println!("证据：{}", out.display());
    if !real {
        // 计费归属：所有模型请求的 originator 都是 codex-tui；排队投递 turn_trigger=queue。
        let log = fake_log.lock().unwrap();
        assert!(
            log.iter().all(|r| r["originator"] == "codex-tui"),
            "{log:?}"
        );
        assert!(log.iter().any(|r| r["turnTrigger"] == "queue"));
    }
    let _ = root;
}
