//! 真机探针：OpenCode 原生 TUI 会话里切模型 / agent（任务 .lead/20261114-tui-model-switch，验收 #3 #4 #7 #8）。
//!
//! 走与 `apply_session_settings` 同一套后端函数（只缺 AppHandle 那层）：清单 `session_settings::session_catalog`
//! （TuiDialog 机制有服务时即 `list_models`），按机制拆分 `session_settings::plan`，会话「谁在跑」
//! `session_presence`，切换 `switch_model` / `switch_agent`（写字与读屏换成本探针自己的 PTY，等价于宿主的
//! PTY 写入接口与仿真画面），当前值 `current_choice`（session_live_settings 的 TuiDialog 分支），新建页
//! `launch_defaults` / `dynamic_for`，新会话启动参数 `args_for_selections`。
//!
//! 全程隔离（HOME/USERPROFILE/XDG_*/OPENCODE_CONFIG_DIR/MEOWO_DB 指一次性目录），进程内假
//! openai-compatible 服务（不耗额度、不复制任何凭据），不装 Lead（服务通道 2026-11-14 起对所有会话开）。
//! 证据写 `MEOWO_TMS_EVIDENCE`（缺省 `$CARGO_TARGET_DIR/tms-probe/opencode`）。
//!
//! ```text
//! cargo test -p meowo-app --lib tms_probe_opencode -- --ignored --exact --test-threads=1 --nocapture
//! ```
use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::ffi::OsString;
use std::io::{BufRead, Read, Write};
use std::time::Instant;

const ROWS: u16 = 45;
const COLS: u16 = 150;

struct EnvGuard(Vec<(String, Option<OsString>)>);
impl EnvGuard {
    fn set(&mut self, key: &str, value: Option<&std::ffi::OsStr>) {
        if !self.0.iter().any(|(k, _)| k == key) {
            self.0.push((key.into(), std::env::var_os(key)));
        }
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.0.iter().rev() {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct Tui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    screen: Arc<Mutex<vt100::Parser>>,
    pid: Option<u32>,
}

impl Tui {
    fn spawn(argv: &[String], plan: &LaunchPlan, cwd: &Path) -> Tui {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        command.args(&plan.args);
        for (k, v) in &plan.env {
            command.env(k, v);
        }
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let pid = child.process_id();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let (answer, parser) = (writer.clone(), screen.clone());
        std::thread::spawn(move || {
            let mut bytes = [0; 16_384];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                let chunk = &bytes[..count];
                // OpenConsole 启动握手：CPR 由后端、DA1 由 xterm 应答（probe_opencode_input 同款）。
                if let Ok(mut w) = answer.lock() {
                    if chunk.windows(4).any(|p| p == b"\x1b[6n") {
                        let _ = w.write_all(b"\x1b[1;1R").and_then(|_| w.flush());
                    }
                    if chunk.windows(3).any(|p| p == b"\x1b[c") {
                        let _ = w.write_all(b"\x1b[?62;22c").and_then(|_| w.flush());
                    }
                }
                if let Ok(mut p) = parser.lock() {
                    p.process(chunk);
                }
            }
        });
        Tui {
            child,
            _master: pair.master,
            writer,
            screen,
            pid,
        }
    }
    fn text(&self) -> String {
        self.screen
            .lock()
            .map(|p| p.screen().contents())
            .unwrap_or_default()
    }
    fn send(&self, text: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(text.as_bytes()).and_then(|_| w.flush());
        }
    }
    fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.send(&c.to_string());
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn wait(&self, secs: u64, want: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if want(&self.text()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }
    /// 宿主的 TuiIo：写入 = PTY 写入接口，读屏 = 仿真画面（session_settings::dialog_switch 同款）。
    fn io(&self) -> impl TuiIo + '_ {
        (
            move |text: &str| {
                self.send(text);
                Ok(())
            },
            move || Some(self.text()),
        )
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            crate::proc::kill_descendants(pid);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 进程内假 openai-compatible 服务：记录 model 与最后一条用户消息。剧本：
/// 没带 tools（标题生成等旁路）→ 回标题；工具结果回来 → TOOL_DONE；ASK_PERM → 调 bash（配置成要问）；
/// SLOW → 12 秒后回 SLOW_DONE；其余回 `OK_<model>`。
fn fake_model(log: Arc<Mutex<Vec<Value>>>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
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
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let mut stream = stream;
                if !first.starts_with("POST") || !first.contains("/chat/completions") {
                    let _ = write!(stream, "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
                    return;
                }
                let request: Value = serde_json::from_slice(&body).unwrap_or_default();
                let model = request["model"].as_str().unwrap_or_default().to_owned();
                let messages = request["messages"].as_array().cloned().unwrap_or_default();
                let last_role = messages
                    .last()
                    .and_then(|m| m["role"].as_str())
                    .unwrap_or_default()
                    .to_owned();
                let last_user = messages
                    .iter()
                    .rev()
                    .find(|m| m["role"] == "user")
                    .map(|m| m["content"].to_string())
                    .unwrap_or_default();
                let has_tools = request["tools"].as_array().is_some_and(|t| !t.is_empty());
                let mut tool_call = None;
                let reply = if !has_tools {
                    "Probe title".to_owned()
                } else if last_role == "tool" {
                    "TOOL_DONE".to_owned()
                } else if last_user.contains("ASK_PERM") {
                    tool_call = Some(json!({"index":0,"id":format!("call_{}", &crate::remote::generate_token().unwrap()[..8]),"type":"function","function":{"name":"bash","arguments":json!({"command":"echo probe-perm","description":"probe permission"}).to_string()}}));
                    String::new()
                } else if last_user.contains("SLOW") {
                    "SLOW_DONE".to_owned()
                } else {
                    format!("OK_{model}")
                };
                let tail: String = last_user
                    .chars()
                    .rev()
                    .take(300)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let leaked = ["Fake One", "Fake Two", "Fake Three", "reviewer"]
                    .iter()
                    .any(|n| last_user.contains(n));
                let markers: Vec<&str> = MARKERS
                    .iter()
                    .copied()
                    .filter(|m| last_user.contains(m))
                    .collect();
                log.lock().unwrap().push(json!({
                    "model": model, "hasTools": has_tools, "lastRole": last_role,
                    // plan 等 agent 会在用户消息后追加很长的 system-reminder：口令按全文匹配记下来。
                    "markers": markers,
                    "searchLeaked": leaked,
                    "lastUser": tail, "reply": reply, "toolCall": tool_call.is_some(),
                    "atMs": now_ms(),
                }));
                // 先记再睡：探针要在回合进行中看到这条请求。
                if reply == "SLOW_DONE" {
                    std::thread::sleep(Duration::from_secs(12));
                }
                let chunk = |delta: Value, finish: Value| {
                    json!({"id":"c","object":"chat.completion.chunk","created":1,"model":model,
                           "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
                };
                let mut chunks = Vec::new();
                match tool_call {
                    Some(call) => {
                        chunks.push(chunk(
                            json!({"role":"assistant","content":null,"tool_calls":[call]}),
                            Value::Null,
                        ));
                        chunks.push(chunk(json!({}), json!("tool_calls")));
                    }
                    None => {
                        chunks.push(chunk(
                            json!({"role":"assistant","content":reply}),
                            Value::Null,
                        ));
                        chunks.push(chunk(json!({}), json!("stop")));
                    }
                }
                chunks.push(json!({"id":"c","object":"chat.completion.chunk","created":1,"model":model,"choices":[],
                                   "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}));
                let mut sse: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
                sse.push_str("data: [DONE]\n\n");
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

fn use_bundled_conpty() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = source.join(name);
        if from.exists() && !target.join(name).exists() {
            let _ = std::fs::copy(&from, target.join(name));
        }
    }
}

/// 底栏（`┃  Build · Fake One Fake`）。
fn footer(screen: &str) -> Option<String> {
    screen
        .lines()
        .rfind(|l| l.contains("Build · ") || l.contains("Plan · ") || l.contains("Reviewer · "))
        .map(|l| {
            let l = l.trim().trim_start_matches('┃').trim();
            l.split("   ").next().unwrap_or(l).to_owned()
        })
}

fn permission_shown(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("permission required") || lower.contains("allow once")
}

fn wait_idle(client: &Client, native: &str, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    std::thread::sleep(Duration::from_millis(1500));
    while Instant::now() < deadline {
        if client
            .get("/session/status", Duration::from_secs(3))
            .is_ok_and(|s| s.get(native).is_none())
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    false
}

fn remove_dir_retry(dir: &Path) {
    for _ in 0..40 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

struct Run {
    db: PathBuf,
    cwd: PathBuf,
    evidence: PathBuf,
    log: Arc<Mutex<Vec<Value>>>,
    checks: Vec<Value>,
    result: serde_json::Map<String, Value>,
}

impl Run {
    fn check(&mut self, name: &str, ok: bool, detail: Value) {
        eprintln!("[tms-opencode] {} {name}", if ok { "PASS" } else { "FAIL" });
        self.checks
            .push(json!({"check": name, "pass": ok, "detail": detail}));
        self.flush();
    }
    fn save(&self, name: &str, text: &str) {
        let _ = std::fs::write(self.evidence.join(name), text);
    }
    fn flush(&self) {
        let mut out = self.result.clone();
        out.insert("checks".into(), json!(self.checks));
        out.insert("requests".into(), json!(*self.log.lock().unwrap()));
        self.save(
            "result.json",
            &serde_json::to_string_pretty(&Value::Object(out)).unwrap(),
        );
    }
    /// 带 marker 那条主请求的 model（工具结果回合也算，取第一条）。
    fn model_for(&self, marker: &str) -> Option<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .find(|r| r["hasTools"] == true && r["markers"].as_array().is_some_and(|m| m.iter().any(|x| x == marker)))
            .and_then(|r| r["model"].as_str().map(str::to_owned))
    }
    fn wait_request(&self, marker: &str, secs: u64) -> bool {
        wait_until(secs, || self.model_for(marker).is_some())
    }
    fn cwd_text(&self) -> String {
        self.cwd.to_string_lossy().into_owned()
    }
}

fn wait_until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn turn(run: &Run, tui: &Tui, client: &Client, native: Option<&str>, text: &str) -> bool {
    tui.type_text(text);
    std::thread::sleep(Duration::from_millis(400));
    tui.send("\r");
    let seen = run.wait_request(text, 60);
    if let Some(native) = native {
        wait_idle(client, native, 60);
    } else {
        std::thread::sleep(Duration::from_secs(3));
    }
    std::thread::sleep(Duration::from_millis(800));
    seen
}

fn clear_input(tui: &Tui, text: &str) {
    for _ in 0..text.chars().count() + 2 {
        tui.send("\x7f");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
}

fn sel(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn outcome(r: &Result<SwitchOutcome, ControlError>) -> Value {
    match r {
        Ok(o) => json!({"ok": true, "confirmed": o.confirmed}),
        Err(ControlError::Unsupported) => json!({"err": "Unsupported"}),
        Err(ControlError::NotReady(r)) => json!({"err": "NotReady", "reason": r}),
        Err(ControlError::NotNow(r)) => json!({"err": "NotNow", "reason": r}),
        Err(ControlError::Failed(r)) => json!({"err": "Failed", "reason": r}),
    }
}

fn is_not_now(r: &Result<SwitchOutcome, ControlError>, reason: &str) -> bool {
    matches!(r, Err(ControlError::NotNow(x)) if x == reason)
}

/// 探针发的口令（model_for 按它们找请求）。
const MARKERS: &[&str] = &[
    "HELLO_ONE", "AFTER_SWITCH", "DRAFT_KEEP_1", "COMBINED_REQ", "SLOW_TURN", "AFTER_TURN_END",
    "ASK_PERM", "USER_SELF", "NEW_WITH_ARGS",
];

const IDLE: TuiState = TuiState {
    idle: true,
    composer_empty: true,
    approval_pending: false,
};

#[test]
#[ignore = "真机：拉起真实 OpenCode TUI（进程内假模型）；手动复跑"]
fn tms_probe_opencode() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.iter().any(|a| a == "--test-threads=1"),
        "探针改进程环境变量，必须单线程独占运行"
    );
    use_bundled_conpty();
    let keep = std::env::var("MEOWO_PROBE_KEEP").as_deref() == Ok("1");
    let evidence = std::env::var("MEOWO_TMS_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
                .join("tms-probe/opencode")
        });
    std::fs::create_dir_all(&evidence).unwrap();
    let root = std::env::temp_dir().join(format!(
        "meowo-tms-oc-{}",
        crate::remote::generate_token().unwrap()[..8].to_lowercase()
    ));
    let home = root.join("home");
    let cwd = root.join("cwd");
    for dir in [&home, &cwd] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let root = dunce::canonicalize(&root).unwrap();
    let (home, cwd) = (root.join("home"), root.join("cwd"));
    let db = root.join("board.db");

    let mut env = EnvGuard(vec![]);
    for (key, _) in std::env::vars() {
        if key.starts_with("MEOWO_") || key.starts_with("OPENCODE_") {
            env.set(&key, None);
        }
    }
    for key in ["HOME", "USERPROFILE"] {
        env.set(key, Some(home.as_os_str()));
    }
    let config_dir = home.join("opencode-config");
    for (key, rel) in [
        ("OPENCODE_CONFIG_DIR", "opencode-config"),
        ("XDG_CONFIG_HOME", "xdg-config"),
        ("XDG_DATA_HOME", "xdg-data"),
        ("XDG_CACHE_HOME", "xdg-cache"),
        ("XDG_STATE_HOME", "xdg-state"),
    ] {
        let dir = home.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        env.set(key, Some(dir.as_os_str()));
    }
    env.set("MEOWO_DB", Some(db.as_os_str()));
    for (key, value) in [
        ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
        ("OPENCODE_DISABLE_MODELS_FETCH", "1"),
        ("OPENCODE_DISABLE_CLAUDE_CODE", "1"),
        ("OPENCODE_DISABLE_LSP_DOWNLOAD", "1"),
    ] {
        env.set(key, Some(std::ffi::OsStr::new(value)));
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let port = fake_model(log.clone());
    let provider = json!({"fake":{"npm":"@ai-sdk/openai-compatible","name":"Fake",
        "options":{"baseURL":format!("http://127.0.0.1:{port}/v1"),"apiKey":"x"},
        "models":{"fake-model":{"name":"Fake One"},"fake-model-2":{"name":"Fake Two"},
                  "fake-model-3":{"name":"Fake Three","variants":{"high":{"reasoningEffort":"high"}}}}}});
    let write_config = |extra: Value| {
        let mut config = json!({"$schema":"https://opencode.ai/config.json","model":"fake/fake-model",
            "small_model":"fake/fake-model","autoupdate":false,"share":"disabled","provider":provider,
            "permission":{"edit":"allow","read":"allow","bash":{"*":"deny","echo *":"ask"},"webfetch":"deny"},
            // 自定义 primary agent：新建页 / 会话菜单的动态档（dynamic_for）。
            "agent":{"reviewer":{"mode":"primary","description":"Probe reviewer agent","prompt":"You review."}}});
        for (k, v) in extra.as_object().into_iter().flatten() {
            config[k] = v.clone();
        }
        std::fs::write(config_dir.join("opencode.json"), config.to_string()).unwrap();
    };
    write_config(json!({}));
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&cwd)
        .status()
        .unwrap()
        .success());

    let mut run = Run {
        db: db.clone(),
        cwd: cwd.clone(),
        evidence: evidence.clone(),
        log: log.clone(),
        checks: Vec::new(),
        result: serde_json::Map::new(),
    };
    let plugin = meowo_agent::by_id("opencode").unwrap();
    let exe = plugin.launch_argv();
    let version = std::process::Command::new(&exe[0])
        .args(&exe[1..])
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    run.result.insert("opencode".into(), json!(version));
    let cwd_text = run.cwd_text();
    let started = Instant::now();

    // ---- 0. 起 TUI：与生产同一启动计划（未装 Lead 也带 --port）。
    let launch = |selections: &HashMap<String, String>| -> (Tui, Vec<String>) {
        let mut argv = plugin.launch_argv();
        argv.extend(crate::session_settings::args_for_selections(
            plugin,
            selections,
            &[],
            Some(Path::new(&cwd_text)),
            false,
        ));
        let plan = launch_plan("opencode", &argv, &[]).expect("未装 Lead 也应带端口启动");
        let tui = Tui::spawn(&argv, &plan, Path::new(&cwd_text));
        plan.launched(tui.pid);
        (tui, argv)
    };
    let (tui, argv1) = launch(&HashMap::new());
    run.result.insert("argv1".into(), json!(argv1));
    let ready = tui.wait(90, |s| footer(s).is_some());
    run.check("tui1_ready", ready, json!(footer(&tui.text())));
    let endpoint = Registry::load(&registry_path(&db))
        .endpoints
        .into_iter()
        .find(|e| Some(e.pid) == tui.pid)
        .expect("TUI 启动后应登记端口");
    let client = Client::of(&endpoint);
    assert!(wait_until(40, || client
        .verify_identity(Duration::from_secs(1))
        .is_ok()));
    std::thread::sleep(Duration::from_secs(2));
    let first = turn(&run, &tui, &client, None, "HELLO_ONE");
    run.check("first_turn_fake_model", first && run.model_for("HELLO_ONE").as_deref() == Some("fake-model"), json!(run.model_for("HELLO_ONE")));
    let sessions = client.get("/session", Duration::from_secs(10)).unwrap_or_default();
    let native = sessions
        .as_array()
        .and_then(|a| a.first())
        .and_then(|s| s["id"].as_str())
        .expect("应已建会话")
        .to_owned();
    wait_idle(&client, &native, 60);
    let store = crate::open_store(&db).unwrap();
    let project = store.upsert_project_by_root(&cwd_text, "probe", 1).unwrap();
    let (sid, _) = store.start_session(project, &native, 2).unwrap();
    store.set_session_provider(sid, "opencode").unwrap();
    store.set_session_cwd(sid, &cwd_text, 3).unwrap();
    store
        .set_session_shared_pid(sid, i64::from(tui.pid.unwrap()), 4)
        .unwrap();
    // 宿主取 presence 的同一函数（无 AppHandle 时走会话库 pid）。认不出就按托管终端构造（生产托管 PTY 的分支）。
    let presence = match session_presence(&db, sid, None) {
        Ok(p @ Presence::Elsewhere(_)) => p,
        other => {
            run.result
                .insert("presenceFromDb".into(), json!(format!("{other:?}")));
            Presence::Managed(tui.pid)
        }
    };
    run.result.insert("presence".into(), json!(format!("{presence:?}")));
    let catalog = crate::session_settings::session_catalog(
        &db,
        plugin,
        Some(sid),
        &[],
        Some(&cwd_text),
        None,
    );
    let fake_two = catalog.iter().find(|e| e.id == "fake/fake-model-2");
    run.check(
        "catalog_from_server_has_fake_models",
        fake_two.is_some_and(|e| e.label == "Fake Two")
            && catalog.iter().any(|e| e.id == "fake/fake-model-3" && e.efforts.contains(&"high".to_string())),
        json!(catalog.iter().map(|e| json!({"id": e.id, "label": e.label, "group": e.group, "efforts": e.efforts})).collect::<Vec<_>>()),
    );
    let request = |state: TuiState| DialogSwitch {
        db: &db,
        presence,
        native_session: &native,
        cwd: Some(&cwd_text),
        state,
    };
    // apply_blocking 的 Dialog 分支：plan 拆出 dialog_model，按清单 label 搜索。
    let switch_model_via_plan = |tui: &Tui, id: &str, state: TuiState| {
        let plan = crate::session_settings::plan(plugin, Some(id), None, &HashMap::new(), &catalog, &[], &HashMap::new())
            .expect("plan");
        let model = plan.dialog_model.expect("TuiDialog 机制应走对话框");
        let label = catalog.iter().find(|e| e.id == model).unwrap().label.clone();
        let t = Instant::now();
        let r = switch_model(&request(state), &label, &mut tui.io());
        (r, t.elapsed().as_millis())
    };

    // ---- #3 空闲切模型：底栏变、下一条用新模型、会话 id 不变、历史在。
    let (r, ms) = switch_model_via_plan(&tui, "fake/fake-model-2", IDLE);
    std::thread::sleep(Duration::from_millis(800));
    let screen = tui.text();
    run.save("01-idle-switch-model.txt", &screen);
    run.check(
        "3_idle_switch_confirmed_footer",
        matches!(r, Ok(SwitchOutcome { confirmed: true })) && footer(&screen).is_some_and(|f| f.contains("Fake Two")),
        json!({"outcome": outcome(&r), "ms": ms, "footer": footer(&screen)}),
    );
    let next = turn(&run, &tui, &client, Some(&native), "AFTER_SWITCH");
    let sessions_after = client.get("/session", Duration::from_secs(10)).unwrap_or_default();
    let ids: Vec<&str> = sessions_after
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["id"].as_str())
        .collect();
    let history = client
        .get(&format!("/session/{native}/message"), Duration::from_secs(10))
        .map(|m| m.to_string())
        .unwrap_or_default();
    run.check(
        "3_next_turn_uses_new_model_same_session",
        next && run.model_for("AFTER_SWITCH").as_deref() == Some("fake-model-2")
            && ids == vec![native.as_str()]
            && history.contains("HELLO_ONE")
            && tui.text().contains("HELLO_ONE"),
        json!({"model": run.model_for("AFTER_SWITCH"), "sessions": ids}),
    );
    // #8 先验：下一条发出后 current_choice（session_live_settings 的 TuiDialog 分支）读到新值。
    let choice = current_choice(&db, presence, &native, Some(&cwd_text));
    run.check(
        "8_current_choice_after_switch",
        matches!(&choice, Ok(Some(c)) if c.model_id.as_deref() == Some("fake-model-2") && c.agent.as_deref() == Some("build")),
        json!(format!("{choice:?}")),
    );

    // ---- #4 有草稿：如实报告 composer_empty=false → 服务端不动、草稿原样；宿主误判为空时，对话框路线也不碰草稿。
    tui.type_text("DRAFT_KEEP_1");
    std::thread::sleep(Duration::from_millis(600));
    run.save("02a-draft-before.txt", &tui.text());
    let (r, _) = switch_model_via_plan(&tui, "fake/fake-model-3", TuiState { composer_empty: false, ..IDLE });
    std::thread::sleep(Duration::from_millis(500));
    let screen = tui.text();
    run.check(
        "4_draft_reported_not_now_untouched",
        is_not_now(&r, "lead/opencode_composer_not_empty")
            && screen.contains("DRAFT_KEEP_1")
            && footer(&screen).is_some_and(|f| f.contains("Fake Two")),
        json!({"outcome": outcome(&r), "footer": footer(&screen)}),
    );
    let (r, ms) = switch_model_via_plan(&tui, "fake/fake-model-3", IDLE);
    std::thread::sleep(Duration::from_millis(800));
    let screen = tui.text();
    run.save("02b-draft-switch-anyway.txt", &screen);
    run.check(
        "4_draft_kept_when_dialog_switch_runs",
        r.is_ok() && screen.contains("DRAFT_KEEP_1") && footer(&screen).is_some_and(|f| f.contains("Fake Three")),
        json!({"outcome": outcome(&r), "ms": ms, "footer": footer(&screen)}),
    );
    // agent 同理（草稿在）。
    let t = Instant::now();
    let r = switch_agent(&request(IDLE), "plan", &mut tui.io());
    let agent_ms = t.elapsed().as_millis();
    std::thread::sleep(Duration::from_millis(800));
    let screen = tui.text();
    run.save("02c-draft-switch-agent.txt", &screen);
    run.check(
        "4_draft_kept_when_agent_switch_runs",
        r.is_ok() && screen.contains("DRAFT_KEEP_1") && footer(&screen).is_some_and(|f| f.starts_with("Plan")),
        json!({"outcome": outcome(&r), "ms": agent_ms, "footer": footer(&screen)}),
    );
    // 草稿就是这一条要发的：发出去，消息里只有草稿原文（没混进搜索词），带的是新模型 + plan。
    tui.send("\r");
    let draft_sent = run.wait_request("DRAFT_KEEP_1", 60);
    wait_idle(&client, &native, 60);
    let last_user = run
        .log
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["hasTools"] == true && r["markers"].as_array().is_some_and(|m| m.iter().any(|x| x == "DRAFT_KEEP_1")))
        .cloned();
    let choice = current_choice(&db, presence, &native, Some(&cwd_text));
    run.check(
        "4_7_draft_message_clean_with_plan",
        draft_sent
            && last_user.as_ref().is_some_and(|u| u["searchLeaked"] == false)
            && matches!(&choice, Ok(Some(c)) if c.agent.as_deref() == Some("plan")),
        json!({"model": run.model_for("DRAFT_KEEP_1"), "lastUser": last_user, "choice": format!("{choice:?}")}),
    );

    // 记录：先切模型、再切 agent 时模型去哪了（apply_blocking 同一请求里先模型后 agent）。
    run.result.insert(
        "modelAfterAgentSwitch".into(),
        json!({"pickedModel": "fake-model-3", "requestModel": run.model_for("DRAFT_KEEP_1")}),
    );

    // ---- 同一请求同时带模型与 agent（前端运行中连点两次会合并成一个请求，mergeRequests）：
    //      apply_blocking → dialog_switch 先切模型、后切 agent。看最终底栏与下一条请求用的模型。
    let combined = crate::session_settings::plan(
        plugin,
        Some("fake/fake-model-2"),
        None,
        &sel(&[("agent", "build")]),
        &catalog,
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let label = catalog.iter().find(|e| Some(&e.id) == combined.dialog_model.as_ref()).unwrap().label.clone();
    let rm = switch_model(&request(IDLE), &label, &mut tui.io());
    let ra = switch_agent(&request(IDLE), combined.dialog_agent.as_deref().unwrap(), &mut tui.io());
    std::thread::sleep(Duration::from_millis(800));
    let screen = tui.text();
    run.save("02d-combined-model-then-agent.txt", &screen);
    turn(&run, &tui, &client, Some(&native), "COMBINED_REQ");
    run.check(
        "4_7_combined_model_and_agent_request_keeps_model",
        rm.is_ok() && ra.is_ok()
            && footer(&screen).is_some_and(|f| f.starts_with("Build") && f.contains("Fake Two"))
            && run.model_for("COMBINED_REQ").as_deref() == Some("fake-model-2"),
        json!({"model": outcome(&rm), "agent": outcome(&ra), "footer": footer(&screen), "requestModel": run.model_for("COMBINED_REQ")}),
    );

    // ---- #7 切回 build（经 plan() 的 Dialog 栏），自定义 agent 也能切（dynamic_for）。
    let dynamic = crate::session_settings::dynamic_for(plugin, &[], Some(Path::new(&cwd_text)));
    run.check(
        "7_dynamic_custom_agent_listed",
        dynamic.iter().any(|d| d.id == "reviewer"),
        json!(dynamic.iter().map(|d| json!({"option": d.option, "id": d.id, "label": d.label})).collect::<Vec<_>>()),
    );
    let p = crate::session_settings::plan(plugin, None, None, &sel(&[("agent", "reviewer")]), &[], &dynamic, &HashMap::new());
    let r = match p.as_ref().ok().and_then(|p| p.dialog_agent.clone()) {
        Some(agent) => switch_agent(&request(IDLE), &agent, &mut tui.io()),
        None => Err(ControlError::Failed(format!("plan: {p:?}"))),
    };
    std::thread::sleep(Duration::from_millis(800));
    let screen = tui.text();
    run.save("03a-switch-custom-agent.txt", &screen);
    run.check(
        "7_switch_custom_agent",
        r.is_ok() && footer(&screen).is_some_and(|f| f.starts_with("Reviewer")),
        json!({"outcome": outcome(&r), "footer": footer(&screen)}),
    );
    let p = crate::session_settings::plan(plugin, None, None, &sel(&[("agent", "build")]), &[], &[], &HashMap::new()).unwrap();
    let r = switch_agent(&request(IDLE), p.dialog_agent.as_deref().unwrap(), &mut tui.io());
    std::thread::sleep(Duration::from_millis(800));
    run.check(
        "7_switch_back_build",
        r.is_ok() && footer(&tui.text()).is_some_and(|f| f.starts_with("Build")),
        json!({"outcome": outcome(&r), "footer": footer(&tui.text())}),
    );

    // ---- #4 运行中：宿主说在跑 → 不动；宿主误判为空闲 → 服务端 /session/status 复核拦下；回合结束后执行，只影响下一条。
    tui.type_text("SLOW_TURN");
    std::thread::sleep(Duration::from_millis(300));
    tui.send("\r");
    let slow_seen = run.wait_request("SLOW_TURN", 30);
    run.save("04-slow-start.txt", &tui.text());
    run.check("4_slow_turn_started", slow_seen, json!(footer(&tui.text())));
    std::thread::sleep(Duration::from_millis(800));
    let before = tui.text();
    let r1 = switch_model_via_plan(&tui, "fake/fake-model", TuiState { idle: false, ..IDLE }).0;
    let r2 = switch_model_via_plan(&tui, "fake/fake-model", IDLE).0;
    std::thread::sleep(Duration::from_millis(500));
    let during = tui.text();
    run.save("04a-running.txt", &during);
    run.check(
        "4_running_refused_host_and_server",
        is_not_now(&r1, "lead/opencode_busy")
            && is_not_now(&r2, "lead/opencode_busy")
            && !during.contains("Select model")
            && footer(&during).is_some_and(|f| f.contains("Fake Three")),
        json!({"hostSaysBusy": outcome(&r1), "hostMisjudgedIdle": outcome(&r2), "footerBefore": footer(&before), "footerDuring": footer(&during)}),
    );
    let ended = wait_idle(&client, &native, 60);
    let (r, _) = switch_model_via_plan(&tui, "fake/fake-model", IDLE);
    std::thread::sleep(Duration::from_millis(800));
    turn(&run, &tui, &client, Some(&native), "AFTER_TURN_END");
    run.check(
        "4_deferred_switch_after_turn_affects_next_only",
        ended && r.is_ok()
            && run.model_for("SLOW_TURN").as_deref() == Some("fake-model-3")
            && run.model_for("AFTER_TURN_END").as_deref() == Some("fake-model"),
        json!({"outcome": outcome(&r), "slowModel": run.model_for("SLOW_TURN"), "afterModel": run.model_for("AFTER_TURN_END")}),
    );

    // ---- #4 待批权限框：宿主如实报 → 不动；宿主漏报 → 服务端 /permission 复核拦下；权限框仍在、没被答掉。
    tui.type_text("ASK_PERM now");
    std::thread::sleep(Duration::from_millis(300));
    tui.send("\r");
    let shown = tui.wait(60, permission_shown);
    std::thread::sleep(Duration::from_millis(800));
    let before = tui.text();
    run.save("05a-permission-before.txt", &before);
    let r1 = switch_model_via_plan(&tui, "fake/fake-model-2", TuiState { approval_pending: true, ..IDLE }).0;
    let r2 = switch_model_via_plan(&tui, "fake/fake-model-2", IDLE).0;
    let r3 = switch_agent(&request(IDLE), "plan", &mut tui.io());
    std::thread::sleep(Duration::from_secs(2));
    let during = tui.text();
    run.save("05b-permission-during.txt", &during);
    let pending = client.get("/permission", Duration::from_secs(5)).unwrap_or_default();
    let tool_done = run
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|r| r["reply"] == "TOOL_DONE");
    run.check(
        "4_permission_pending_never_written",
        shown
            && is_not_now(&r1, "lead/opencode_approval_pending")
            && is_not_now(&r2, "lead/opencode_approval_pending")
            && is_not_now(&r3, "lead/opencode_approval_pending")
            && permission_shown(&during)
            && pending.as_array().is_some_and(|a| !a.is_empty())
            && !tool_done,
        json!({"hostReports": outcome(&r1), "hostMisses": outcome(&r2), "agentHostMisses": outcome(&r3),
               "pendingPermissions": pending.as_array().map(Vec::len), "toolRanWithoutUser": tool_done}),
    );
    // 用户自己拒绝（Esc），回合停下。
    tui.send("\x1b");
    std::thread::sleep(Duration::from_secs(2));
    wait_idle(&client, &native, 30);
    run.save("05c-permission-rejected.txt", &tui.text());

    // ---- #8 用户在 TUI 里自己切：/models 对话框选 Fake Two + Tab 切 agent。发下一条之前读不到（上游限制），发出后跟上。
    clear_input(&tui, "xxxxxxxxxxxxxxxx");
    tui.type_text("/models");
    std::thread::sleep(Duration::from_millis(500));
    tui.send("\r");
    let opened = tui.wait(10, |s| s.contains("Select model"));
    tui.type_text("Fake Two");
    std::thread::sleep(Duration::from_millis(1200));
    tui.send("\r");
    std::thread::sleep(Duration::from_millis(1200));
    tui.send("\t");
    std::thread::sleep(Duration::from_millis(1200));
    let self_switched = tui.text();
    run.save("06a-user-self-switch.txt", &self_switched);
    let before_msg = current_choice(&db, presence, &native, Some(&cwd_text));
    turn(&run, &tui, &client, Some(&native), "USER_SELF");
    let after_msg = current_choice(&db, presence, &native, Some(&cwd_text));
    run.check(
        "8_user_self_switch_synced_after_next_message",
        opened
            && matches!(&after_msg, Ok(Some(c)) if c.model_id.as_deref() == Some("fake-model-2") && c.agent.as_deref() == Some("plan"))
            && run.model_for("USER_SELF").as_deref() == Some("fake-model-2"),
        json!({"footer": footer(&self_switched), "beforeNextMessage": format!("{before_msg:?}"), "afterNextMessage": format!("{after_msg:?}")}),
    );
    run.save("06b-after-user-self.txt", &tui.text());
    drop(tui);
    std::thread::sleep(Duration::from_secs(2));

    // ---- #7/#3 新建会话带模型与 agent 启动参数（OpenCode 首条消息前就有服务，不存在「未建会话」卡启动）。
    let selections = sel(&[("model", "fake/fake-model-3"), ("agent", "plan")]);
    let (tui2, argv2) = launch(&selections);
    let ready2 = tui2.wait(90, |s| footer(s).is_some());
    std::thread::sleep(Duration::from_secs(2));
    let screen = tui2.text();
    run.save("07a-new-session-args.txt", &screen);
    let endpoint2 = Registry::load(&registry_path(&db))
        .endpoints
        .into_iter()
        .find(|e| Some(e.pid) == tui2.pid)
        .unwrap();
    let client2 = Client::of(&endpoint2);
    turn(&run, &tui2, &client2, None, "NEW_WITH_ARGS");
    let sessions2 = client2.get("/session", Duration::from_secs(10)).unwrap_or_default();
    let native2 = sessions2
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["id"].as_str())
        .find(|id| *id != native)
        .unwrap_or_default()
        .to_owned();
    let presence2 = Presence::Managed(tui2.pid);
    let choice2 = current_choice(&db, presence2, &native2, Some(&cwd_text));
    run.check(
        "7_new_session_launch_args",
        ready2
            && argv2.windows(2).any(|w| w[0] == "-m" && w[1] == "fake/fake-model-3")
            && argv2.windows(2).any(|w| w[0] == "--agent" && w[1] == "plan")
            && footer(&screen).is_some_and(|f| f.starts_with("Plan") && f.contains("Fake Three"))
            && run.model_for("NEW_WITH_ARGS").as_deref() == Some("fake-model-3")
            && matches!(&choice2, Ok(Some(c)) if c.agent.as_deref() == Some("plan")),
        json!({"argv": argv2, "footer": footer(&screen), "model": run.model_for("NEW_WITH_ARGS"), "choice": format!("{choice2:?}")}),
    );
    drop(tui2);

    // ---- #8 改 CLI 配置文件默认值 → 新建页默认（agent_launch_defaults 背后的 launch_defaults）跟上。
    let before_defaults = plugin.launch_defaults(&[], Some(&cwd));
    write_config(json!({"model": "fake/fake-model-2", "default_agent": "plan"}));
    let after_defaults = plugin.launch_defaults(&[], Some(&cwd));
    run.check(
        "8_launch_defaults_follow_config",
        before_defaults.model.as_deref() == Some("fake/fake-model")
            && after_defaults.model.as_deref() == Some("fake/fake-model-2")
            && after_defaults.choices.get("agent").map(String::as_str) == Some("plan"),
        json!({"before": format!("{before_defaults:?}"), "after": format!("{after_defaults:?}")}),
    );
    // 项目级 opencode.json 覆盖全局（research §7 合并层）。
    std::fs::write(cwd.join("opencode.json"), json!({"model": "fake/fake-model-3"}).to_string()).unwrap();
    let project_defaults = plugin.launch_defaults(&[], Some(&cwd));
    run.check(
        "8_launch_defaults_project_layer",
        project_defaults.model.as_deref() == Some("fake/fake-model-3"),
        json!(format!("{project_defaults:?}")),
    );
    // 账号环境（profile env_of）给了另一个 OPENCODE_CONFIG_DIR 时，默认值读哪一份（只记录）。
    let other = home.join("other-config");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("opencode.json"), json!({"model": "other/profile-model"}).to_string()).unwrap();
    let _ = std::fs::remove_file(cwd.join("opencode.json"));
    let profile_defaults = plugin.launch_defaults(
        &[("OPENCODE_CONFIG_DIR".into(), other.to_string_lossy().into_owned())],
        Some(&cwd),
    );
    run.result.insert(
        "note_profileEnvDefaults".into(),
        json!({"envConfigDir": "other-config(model=other/profile-model)", "got": format!("{profile_defaults:?}"),
               "honorsProfileEnv": profile_defaults.model.as_deref() == Some("other/profile-model")}),
    );

    run.result
        .insert("elapsedMs".into(), json!(started.elapsed().as_millis()));
    run.flush();
    let _ = &run.db;
    let failed: Vec<_> = run
        .checks
        .iter()
        .filter(|c| c["pass"] != true)
        .map(|c| c["check"].clone())
        .collect();
    eprintln!("[tms-opencode] 证据：{}", evidence.display());
    drop(env);
    if !keep {
        remove_dir_retry(&root);
    }
    assert!(failed.is_empty(), "未通过：{failed:?}");
}
