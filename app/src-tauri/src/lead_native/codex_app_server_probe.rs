//! 真机探针：Codex 原生 TUI（--remote 连每会话 app-server）在三种状态下收到 Lead 关卡答复。
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
                let last_user = request["input"]
                    .as_array()
                    .and_then(|items| items.iter().rev().find(|i| i["role"] == "user"))
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                let reply = nonce_re
                    .find_iter(&last_user)
                    .last()
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_else(|| "OK".into());
                let header = |k: &str| headers.iter().find(|(h, _)| h == k).map(|(_, v)| v.clone());
                let meta: Value = header("x-codex-turn-metadata")
                    .and_then(|m| serde_json::from_str(&m).ok())
                    .unwrap_or_default();
                log.lock().unwrap().push(json!({
                    "reply": reply,
                    "originator": header("originator"),
                    "userAgent": header("user-agent"),
                    "turnTrigger": meta["turn_trigger"],
                }));
                let events = [
                    json!({"type":"response.created","response":{"id":"r"}}),
                    json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"m","content":[{"type":"output_text","text":reply}]}}),
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
    real: bool,
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
    // 冷路径的 `codex queue` 继承本进程环境：必须指向隔离目录，否则会写进用户真实的 ~/.codex。
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
        real,
    };
    let mut evidence = json!({
        "real": real,
        "codex": String::from_utf8_lossy(&version.stdout).trim(),
    });
    let policy: &[&str] = &["-a", "never", "-s", "read-only"];

    // ① TUI 在线：答复后同一会话自动开新回合，TUI 上看得到；输入框草稿不被覆盖。
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
    let project = store
        .upsert_project_by_root(&probe.work.to_string_lossy(), "probe", 1)
        .unwrap();
    let (sid, _) = store.start_session(project, &thread, 2).unwrap();
    store.set_session_provider(sid, "codex").unwrap();
    // 生产里这一步由 hook（owner_pid = app-server）完成；探针隔离目录没装 hook，照同一语义补上。
    store
        .set_session_shared_pid(sid, i64::from(server), 3)
        .unwrap();
    let task = probe.gate("Q-online");
    std::fs::write(task.join(".owner"), &thread).unwrap();
    let draft = format!("draft-{}", &crate::remote::generate_token().unwrap()[..6]);
    tui.send(&draft);
    std::thread::sleep(Duration::from_millis(800));
    let online = nonce("LEAD");
    let (result, status, answer_ms) = probe.answer("Q-online", &online);
    let shown = tui.wait(180, |s| assistant_says(s, &online));
    std::thread::sleep(Duration::from_secs(1));
    let screen = tui.text();
    evidence["online"] = json!({
        "nonce": online, "answer": result, "status": status.state, "answerMs": answer_ms,
        "tuiShowsReply": shown, "draftKept": screen.contains(&draft), "screen": screen,
    });
    assert_eq!(result, Ok(true));
    assert_eq!(status.state, "accepted");
    assert!(shown, "在线投递未在 TUI 显示：\n{screen}");
    assert!(screen.contains(&draft), "草稿被覆盖：\n{screen}");
    // 已送达的不重发：同一关卡再触发一次投递（巡检/重复作答）不应新增回合。
    crate::lead_conversation::reconcile_task(&probe.db, &task, None);

    // ② TUI 退出、服务仍在：当场跑完这一轮，resume 时可见。
    tui.quit();
    assert!(!tui.alive(), "TUI 未退出");
    probe.gate("Q-linger");
    let linger = nonce("LEAD");
    let (result, status, _) = probe.answer("Q-linger", &linger);
    let ran_ms = probe.wait_rollout(&linger, 180);
    let (mut tui2, _) = probe.launch(sid, &[policy, &["resume", &thread]].concat());
    let shown = tui2.wait(60, |s| assistant_says(s, &linger));
    evidence["linger"] = json!({
        "nonce": linger, "answer": result, "status": status.state, "headlessRunMs": ran_ms,
        "resumeShowsReply": shown, "screen": tui2.text(),
    });
    assert_eq!(status.state, "accepted");
    assert!(ran_ms.is_some(), "TUI 退出后服务未执行答复");
    assert!(shown, "resume 后看不到离线回合：\n{}", tui2.text());

    // ③ TUI 与服务都不在（如 Meowo 重启后服务已到期）：落持久队列，下次 resume 由 TUI 消费。
    tui2.quit();
    retire_for_session(&probe.db, sid);
    assert!(states(&probe.db).is_empty());
    probe.gate("Q-cold");
    let cold = nonce("LEAD");
    let (result, status, _) = probe.answer("Q-cold", &cold);
    assert_eq!(status.state, "queued", "{status:?}");
    assert!(
        !rollout_assistant_says(&probe.home, &cold),
        "冷队列不应自行调用模型"
    );
    let (mut tui3, _) = probe.launch(sid, &[policy, &["resume", &thread]].concat());
    let shown = tui3.wait(180, |s| assistant_says(s, &cold));
    evidence["cold"] = json!({
        "nonce": cold, "answer": result, "status": status.state,
        "resumeConsumed": shown, "screen": tui3.text(),
    });
    tui3.quit();
    evidence["fakeModelRequests"] = json!(*fake_log.lock().unwrap());
    let out = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("codex-remote-probe")
        .join(format!(
            "{}-{}",
            crate::now_ms(),
            if real { "real" } else { "fake" }
        ));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("result.json"),
        serde_json::to_string_pretty(&evidence).unwrap(),
    )
    .unwrap();
    println!("证据：{}", out.display());
    assert!(shown, "冷队列 resume 后未消费：\n{}", tui3.text());
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
