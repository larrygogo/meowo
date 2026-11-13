//! 真机探针：kimi / gemini 原生 TUI 的 Lead 关卡自动续跑（任务 .lead/20261113-lead-tui，C 路）。
//!
//! 每个探针在隔离目录里（临时 HOME/KIMI_CODE_HOME/GEMINI_CLI_HOME、独立 board.db 与登记表）用 Lead 安装
//! 的真实路径写配置，在 PTY 里拉起真实 TUI，走与手机作答同一个后端入口（answer_and_deliver），验证：
//! 1. 在线空闲：作答后同一会话自动开新回合输出口令，TUI 画面可见，输入框草稿不被覆盖；
//! 2. 运行中作答：本回合结束后（或下一步）自动接着做；
//! 3. TUI 退出：等待器随之退出（无孤儿）；作答后离线续写同一会话、关卡被消费，重新打开可见。
//!
//! 默认用本地假模型（probe_assets/fake_llm.py，不耗额度）；`MEOWO_PROBE_REAL=1` 时复制最小认证副本用真模型
//! （gemini 用 gemini-3.5-flash-lite）。证据（屏幕、请求日志）留在 target/lead-provider-probes/tui-waiter/，
//! 认证副本跑完即删。先 `cargo build -p meowo-reporter`（hook 与等待器用它）。
//!
//! `cargo test -p meowo-app --lib lead_native::probe -- --ignored --nocapture --test-threads=1`
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ROWS: u16 = 40;
const COLS: u16 = 140;

struct Tui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn spawn(argv: &[String], cwd: &Path, env: &[(String, String)]) -> Tui {
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
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        // 本会话由 Meowo/Claude 托管：别让被测 CLI 继承这些变量（嵌套污染接续链，见 memory）。
        for key in [
            "MEOWO_PTY_LAUNCH",
            "MEOWO_PROTOCOL_MANAGED",
            "CLAUDE_CODE_CHILD_SESSION",
        ] {
            command.env_remove(key);
        }
        for (key, value) in env {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 2000)));
        let (p2, w2) = (parser.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16384];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let bytes = &chunk[..n];
                // kimi/gemini 启动时问终端能力（DA1）；不答就停在空白首帧（memory kimi-probe-da1）。
                if bytes.windows(3).any(|w| w == b"\x1b[c")
                    || bytes.windows(4).any(|w| w == b"\x1b[0c")
                {
                    let _ = w2.lock().unwrap().write_all(b"\x1b[?1;2c");
                }
                p2.lock().unwrap().process(bytes);
            }
        });
        Tui {
            child,
            writer,
            parser,
            _master: pair.master,
        }
    }
    fn screen(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }
    fn wait(&self, what: &str, secs: u64, f: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if f(&self.screen()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        eprintln!("[probe] 等不到 {what}，末屏：\n{}", self.screen());
        false
    }
    /// 画面连续 `quiet` 秒不变即视为回合结束（TUI 的运行指示会持续刷新）。
    fn settle(&self, quiet: u64, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut last = self.screen();
        let mut since = Instant::now();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
            let now = self.screen();
            if now != last {
                last = now;
                since = Instant::now();
            } else if since.elapsed() >= Duration::from_secs(quiet) {
                return true;
            }
        }
        false
    }
    fn send(&self, text: &str) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
    /// 逐字输入（TUI 对整段粘贴有自己的处理，逐字更像人手）。
    fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.send(&c.to_string());
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    db: PathBuf,
    evidence: PathBuf,
    task: PathBuf,
    server: Option<std::process::Child>,
    port: u16,
    secrets: Vec<PathBuf>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        // 认证副本一律删掉；证据目录留给人看。
        for path in &self.secrets {
            let _ = std::fs::remove_file(path);
        }
        // MEOWO_PROBE_KEEP=1：失败排查时保留隔离目录（认证副本照删）。
        if std::env::var("MEOWO_PROBE_KEEP").as_deref() != Ok("1") {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn token() -> String {
    crate::remote::generate_token().unwrap()[..10].to_owned()
}

fn real() -> bool {
    std::env::var("MEOWO_PROBE_REAL").as_deref() == Ok("1")
}

fn reporter() -> String {
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target-verify/debug/meowo-reporter.exe");
    let exe = std::env::var("MEOWO_PROBE_REPORTER")
        .map(PathBuf::from)
        .unwrap_or(exe);
    let exe = dunce::canonicalize(&exe)
        .unwrap_or_else(|_| panic!("先 cargo build -p meowo-reporter：{}", exe.display()));
    exe.to_string_lossy().into_owned()
}

impl Sandbox {
    fn new(provider: &str) -> Sandbox {
        let id = format!("{provider}-{}", token());
        // 工作目录放仓库外：project_root 会向上找 .git，放 target/ 下会落到 worktree 的 .lead。
        let root = std::env::temp_dir().join(format!("meowo-tui-waiter-{id}"));
        let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/lead-provider-probes/tui-waiter")
            .join(&id);
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("cwd")).unwrap();
        std::fs::create_dir_all(&evidence).unwrap();
        let root = dunce::canonicalize(root).unwrap();
        let cwd = root.join("cwd");
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&cwd)
            .status()
            .unwrap()
            .success());
        let task = cwd.join(".lead").join("20261113-probe");
        std::fs::create_dir_all(&task).unwrap();
        std::fs::write(task.join("SPEC.md"), "# probe\n").unwrap();
        std::fs::write(task.join("progress.md"), "phase: implement\n").unwrap();
        std::fs::write(
            task.join("acceptance.json"),
            r#"[{"id":1,"check":"probe","passes":false}]"#,
        )
        .unwrap();
        std::fs::write(cwd.join(".lead/active"), "20261113-probe").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        Sandbox {
            home: root.join("home"),
            db: root.join("board.db"),
            evidence: dunce::canonicalize(evidence).unwrap(),
            task,
            cwd,
            root,
            server: None,
            port,
            secrets: Vec::new(),
        }
    }
    fn start_fake_model(&mut self) {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/lead_native/probe_assets/fake_llm.py");
        let python = std::env::var("MEOWO_PROBE_PYTHON").unwrap_or_else(|_| "python".into());
        let log = std::fs::File::create(self.evidence.join("fake-llm.log")).unwrap();
        let child = std::process::Command::new(python)
            .arg("-I")
            .arg(script)
            .arg(self.port.to_string())
            .arg(self.evidence.join("requests"))
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("需要 python（MEOWO_PROBE_PYTHON 可指定）");
        self.server = Some(child);
        let port = self.port;
        assert!(
            wait_until(15, || std::net::TcpStream::connect(("127.0.0.1", port))
                .is_ok()),
            "假模型没起来，见 fake-llm.log"
        );
    }
    fn plan(&self, plan: Value) {
        std::fs::create_dir_all(self.evidence.join("requests")).unwrap();
        std::fs::write(self.evidence.join("requests/plan.json"), plan.to_string()).unwrap();
    }
    fn gate_body(&self, id: &str, status: &str) -> String {
        json!({"id":id,"type":"Q","title":"probe","summary":"probe","status":status,"options":[{"id":"go","label":"Go"}]}).to_string()
    }
    fn save(&self, name: &str, text: &str) {
        std::fs::write(self.evidence.join(name), text).unwrap();
    }
    fn gate(&self) -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(self.task.join("gate.json")).unwrap_or_default(),
        )
        .unwrap_or(Value::Null)
    }
    fn wait_waiter(
        &self,
        gate: &str,
        secs: u64,
    ) -> Option<meowo_reporter::lead_waiter::WaiterRecord> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if let Some(record) = meowo_reporter::lead_waiter::live_waiter(&self.task, gate) {
                return Some(record);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        None
    }
    fn owner(&self) -> String {
        std::fs::read_to_string(self.task.join(".owner"))
            .unwrap_or_default()
            .trim()
            .to_owned()
    }
    /// 宿主库里登记这个原生会话（真实环境由 reporter 主路径 hook 写入；探针只装了 Lead hook）。
    fn register(&self, provider: &str, native: &str) -> i64 {
        let store = crate::open_store(&self.db).unwrap();
        if let Some(id) = store.find_session_id_pub(native).unwrap() {
            return id;
        }
        let cwd = self.cwd.to_string_lossy();
        let project = store.upsert_project_by_root(&cwd, "probe", 1).unwrap();
        let (id, _) = store.start_session(project, native, 2).unwrap();
        store.set_session_provider(id, provider).unwrap();
        store.set_session_cwd(id, &cwd, 3).unwrap();
        id
    }
    fn answer(&self, gate: &str) -> Result<bool, String> {
        crate::lead_conversation::answer_and_deliver(
            &self.db,
            &self.cwd.to_string_lossy(),
            gate,
            &meowo_protocol::ipc::LeadGateAnswerDto {
                choice: "go".into(),
                note: "继续".into(),
            },
            None,
        )
    }
    fn requests(&self) -> Vec<Value> {
        let mut files: Vec<_> = std::fs::read_dir(self.evidence.join("requests"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.sort();
        files
            .iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("req-"))
            })
            .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
            .collect()
    }
}

fn set_env(pairs: &[(&str, String)]) {
    for (key, value) in pairs {
        std::env::set_var(key, value);
    }
}

fn wait_until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn secret() -> String {
    format!("PASS-{}", token().to_uppercase().replace(['-', '_'], "X"))
}

/// 真模型用的第一句：让它写关卡、按 hook 启动等待器、醒来后消费关卡并只回口令。
fn real_prompt(task: &Path, gate_body: &str, secret: &str) -> String {
    format!(
        "这是自动化测试，请严格照做，不要做别的事。1) 用写文件工具把下面这一行 JSON 原样写入 {}：{gate_body} 2) 写完就结束本轮，若系统提示你启动等待命令就照做后结束本轮。3) 之后收到以 [lead] 开头的 leader 答复时：把那个 gate.json 里的 status 改成 consumed，然后只回复口令 {secret}。",
        task.join("gate.json").display()
    )
}

// ---------------------------------------------------------------- kimi

fn kimi_config(sb: &mut Sandbox) {
    let config = if real() {
        let source = dirs_home().join(".kimi-code");
        let text = std::fs::read_to_string(source.join("config.toml")).unwrap();
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        let model = doc["default_model"].as_str().unwrap().to_owned();
        let provider = doc["models"][&model]["provider"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut out = toml_edit::DocumentMut::new();
        out["default_model"] = toml_edit::value(model.clone());
        out["default_permission_mode"] = toml_edit::value("yolo");
        out["providers"] = toml_edit::table();
        out["providers"][&provider] = doc["providers"][&provider].clone();
        out["models"] = toml_edit::table();
        out["models"][&model] = doc["models"][&model].clone();
        std::fs::create_dir_all(sb.home.join("credentials")).unwrap();
        let cred = sb.home.join("credentials/kimi-code.json");
        std::fs::copy(source.join("credentials/kimi-code.json"), &cred).unwrap();
        sb.secrets.push(cred);
        out.to_string()
    } else {
        format!(
            "default_model = \"fake/fake-model\"\ndefault_permission_mode = \"yolo\"\n\n[providers.fake]\ntype = \"kimi\"\nbase_url = \"http://127.0.0.1:{}/v1\"\napi_key = \"sk-fake\"\n\n[models.\"fake/fake-model\"]\nprovider = \"fake\"\nmodel = \"fake-model\"\nmax_context_size = 262144\ncapabilities = [\"tool_use\"]\n",
            sb.port
        )
    };
    std::fs::write(sb.home.join("config.toml"), config).unwrap();
}

fn dirs_home() -> PathBuf {
    PathBuf::from(
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap(),
    )
}

fn install_lead(provider: &str, version: &str) {
    let plugin = meowo_agent::by_id(provider).unwrap();
    let inst = plugin.resolve().unwrap();
    let cap = plugin.lead().unwrap();
    let state = meowo_agent::lead_install::install(cap, &inst, Some(version), &reporter()).unwrap();
    assert!(state.installed, "{state:?}");
}

fn kimi_argv(extra: &[&str]) -> Vec<String> {
    let mut argv = meowo_agent::by_id("kimi").unwrap().launch_argv();
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv
}

fn pass_trust(tui: &Tui) {
    tui.wait("kimi 启动", 30, |s| {
        s.contains("Trust this folder")
            || s.contains("Welcome to Kimi")
            || s.contains("Ask When Needed")
    });
    if tui.screen().contains("Trust this folder") {
        tui.send("\r");
    }
    assert!(
        tui.wait("kimi 输入框", 30, |s| s.contains("Welcome to Kimi")
            || s.contains("Ask When Needed"))
    );
}

/// 发一句并回车。
fn submit(tui: &Tui, text: &str) {
    tui.type_text(text);
    std::thread::sleep(Duration::from_millis(300));
    tui.send("\r");
}

#[test]
#[ignore = "拉起真实 kimi TUI（默认假模型）；手动复跑"]
fn probe_kimi_tui_waiter() {
    let mut sb = Sandbox::new("kimi");
    let env = vec![
        ("KIMI_CODE_HOME", sb.home.to_string_lossy().into_owned()),
        ("KIMI_CODE_NO_AUTO_UPDATE", "1".into()),
        ("MEOWO_DB", sb.db.to_string_lossy().into_owned()),
    ];
    set_env(&env);
    let env: Vec<(String, String)> = env.into_iter().map(|(k, v)| (k.into(), v)).collect();
    if !real() {
        sb.start_fake_model();
    }
    kimi_config(&mut sb);
    install_lead("kimi", "kimi 2.6.0");
    let config_after = std::fs::read_to_string(sb.home.join("config.toml")).unwrap();
    assert!(config_after.contains("lead wait --provider kimi --session *"));
    let mut report = serde_json::Map::new();

    // ---- 1. 在线空闲：作答后自动开新回合，草稿保留
    let s1 = secret();
    let gate_path = sb.task.join("gate.json").to_string_lossy().into_owned();
    sb.plan(json!({"phase":"Q-1","gate_path":gate_path,"gate_body":sb.gate_body("Q-1","pending"),"consume":true,"consumed_body":sb.gate_body("Q-1","consumed"),"secret":s1}));
    let mut tui = Tui::spawn(&kimi_argv(&[]), &sb.cwd, &env);
    pass_trust(&tui);
    let first = if real() {
        real_prompt(&sb.task, &sb.gate_body("Q-1", "pending"), &s1)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &first);
    let waiter = sb.wait_waiter("Q-1", if real() { 240 } else { 60 });
    sb.save("kimi-1-waiting.txt", &tui.screen());
    let waiter = waiter.expect("关卡挂起后会话里应有等待器");
    let native = sb.owner();
    assert!(
        native.starts_with("session_"),
        "kimi hook 会话 id 应带 session_ 前缀：{native}"
    );
    let sid = sb.register("kimi", &native);
    if !real() {
        assert!(tui.wait("回合结束", 60, |s| s.contains("GATE-OPEN-WAITING")));
    }
    assert!(tui.settle(4, 120), "回合应结束");
    tui.type_text("DRAFT-KEEP-1");
    let answered_at = Instant::now();
    assert_eq!(sb.answer("Q-1"), Ok(true));
    let woke = tui.wait(
        "在线空闲自动续跑",
        if real() { 240 } else { 60 },
        |s| {
            if real() {
                s.matches(&s1).count() >= 2
            } else {
                s.contains(&format!("WOKE {s1}"))
            }
        },
    );
    tui.settle(3, 60);
    let latency = answered_at.elapsed();
    sb.save("kimi-1-woke.txt", &tui.screen());
    assert!(woke, "作答后同一会话应自动开新回合");
    assert!(tui.screen().contains("DRAFT-KEEP-1"), "草稿不能被覆盖");
    assert!(
        !sb.task
            .join(meowo_reporter::lead_waiter::WAITER_FILE)
            .exists(),
        "等待器退出后应清掉登记"
    );
    report.insert("idle".into(), json!({"secret":s1,"waiter_pid":waiter.pid,"latency_ms":latency.as_millis() as u64,"draft_kept":true,"native":native}));
    // 清掉草稿
    for _ in 0.."DRAFT-KEEP-1".len() {
        tui.send("\x7f");
    }
    // 打开 modelSteering 后 gemini 运行中回车是插话：等这一轮（含会话摘要等旁路请求）真正结束再发下一句。
    tui.settle(5, 60);

    // ---- 2. 运行中作答
    if !real() {
        let s2 = secret();
        sb.plan(json!({"phase":"Q-2","gate_path":gate_path,"gate_body":sb.gate_body("Q-2","pending"),"consume":true,"consumed_body":sb.gate_body("Q-2","consumed"),"secret":s2,"slow":12}));
        submit(&tui, "OPEN-GATE");
        sb.wait_waiter("Q-2", 60).expect("第二个关卡的等待器");
        assert!(tui.settle(4, 60), "回合应结束");
        submit(&tui, "SLOW");
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(sb.answer("Q-2"), Ok(true));
        let woke = tui.wait("运行中作答续跑", 90, |s| s.contains(&s2));
        sb.save("kimi-2-running.txt", &tui.screen());
        assert!(woke, "运行中作答应在回合后自动续跑");
        assert!(tui.screen().contains("SLOW-DONE"));
        report.insert("running".into(), json!({"secret":s2}));
    }

    // ---- 3. TUI 退出：等待器自退；离线作答 → -p 续写同一会话并消费
    let s3 = secret();
    let gate3 = "Q-3";
    sb.plan(json!({"phase":"Q-3","offline":true,"gate_path":gate_path,"gate_body":sb.gate_body(gate3,"pending"),"secret":s3,"consume":true,"consumed_body":sb.gate_body(gate3,"consumed")}));
    let prompt3 = if real() {
        real_prompt(&sb.task, &sb.gate_body(gate3, "pending"), &s3)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &prompt3);
    let waiter = sb
        .wait_waiter(gate3, if real() { 240 } else { 60 })
        .expect("第三个关卡的等待器");
    assert!(tui.settle(4, 120), "回合应结束");
    submit(&tui, "/exit");
    assert!(wait_until(20, || !tui.alive()), "TUI 应退出");
    let orphan_gone = wait_until(15, || {
        meowo_reporter::lead_waiter::process_started(waiter.pid).is_none()
    });
    assert!(orphan_gone, "TUI 退出后等待器不能留成孤儿");
    assert_eq!(sb.answer(gate3), Ok(true));
    let done = wait_until(if real() { 600 } else { 120 }, || {
        crate::lead_native::waiter::offline_record(&sb.task, gate3)
            .is_some_and(|r| r.state != "running")
    });
    let record = crate::lead_native::waiter::offline_record(&sb.task, gate3);
    assert!(done, "离线续写应结束：{record:?}");
    assert_eq!(record.as_ref().unwrap().state, "done", "{record:?}");
    assert_eq!(sb.gate()["status"], "consumed", "离线续写后关卡应被消费");
    let status =
        crate::lead_conversation::delivery_status(&sb.db, &sb.cwd.to_string_lossy(), gate3);
    // 重新打开 TUI：这一轮可见
    let mut again = Tui::spawn(&kimi_argv(&["-r", &native]), &sb.cwd, &env);
    pass_trust(&again);
    let visible = again.wait("重新打开后可见", 30, |s| s.contains(&s3));
    sb.save("kimi-3-reopened.txt", &again.screen());
    again.send("/exit\r");
    std::thread::sleep(Duration::from_secs(2));
    again.kill();
    tui.kill();
    assert!(visible, "离线续写的这一轮重新打开后应可见");
    report.insert("offline".into(), json!({"secret":s3,"orphan_waiter_exited":true,"record":record,"status_after":status.map(|s| s.state),"session":sid}));
    if !real() {
        let requests = sb.requests();
        let ua: std::collections::BTreeSet<String> = requests
            .iter()
            .filter_map(|r| {
                r.pointer("/headers/User-Agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        report.insert("user_agents".into(), json!(ua));
    }
    sb.save(
        "report.json",
        &serde_json::to_string_pretty(&Value::Object(report)).unwrap(),
    );
    eprintln!("[probe] kimi 证据：{}", sb.evidence.display());
}

// ---------------------------------------------------------------- gemini

fn gemini_setup(sb: &mut Sandbox) -> Vec<(String, String)> {
    let gemini = sb.home.join(".gemini");
    std::fs::create_dir_all(&gemini).unwrap();
    let mut env = vec![
        (
            "GEMINI_CLI_HOME".to_owned(),
            sb.home.to_string_lossy().into_owned(),
        ),
        ("MEOWO_DB".to_owned(), sb.db.to_string_lossy().into_owned()),
        ("GEMINI_CLI_NO_RELAUNCH".to_owned(), "true".to_owned()),
    ];
    let mut settings = json!({
        "general": {"enableAutoUpdate": false, "enableAutoUpdateNotification": false, "defaultApprovalMode": "auto_edit"},
        "model": {"name": "gemini-3.5-flash-lite"},
        "ui": {"hideTips": true, "hideBanner": true},
        "privacy": {"usageStatisticsEnabled": false},
    });
    if real() {
        let source = dirs_home().join(".gemini");
        let user: Value =
            serde_json::from_str(&std::fs::read_to_string(source.join("settings.json")).unwrap())
                .unwrap();
        let selected = user
            .pointer("/security/auth/selectedType")
            .and_then(Value::as_str)
            .unwrap_or("oauth-personal")
            .to_owned();
        settings["security"] = json!({"auth": {"selectedType": selected}});
        for name in ["oauth_creds.json", "google_accounts.json"] {
            if source.join(name).is_file() {
                std::fs::copy(source.join(name), gemini.join(name)).unwrap();
                sb.secrets.push(gemini.join(name));
            }
        }
    } else {
        settings["security"] = json!({"auth": {"selectedType": "gemini-api-key"}});
        env.push(("GEMINI_API_KEY".into(), "fake-key".into()));
        env.push((
            "GOOGLE_GEMINI_BASE_URL".into(),
            format!("http://127.0.0.1:{}", sb.port),
        ));
    }
    std::fs::write(
        gemini.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    std::fs::write(
        gemini.join("trustedFolders.json"),
        json!({ sb.cwd.to_string_lossy(): "TRUST_FOLDER" }).to_string(),
    )
    .unwrap();
    env
}

fn gemini_ready(s: &str) -> bool {
    s.contains("Type your message")
}

#[test]
#[ignore = "拉起真实 gemini TUI（默认假模型）；手动复跑"]
fn probe_gemini_tui_waiter() {
    let mut sb = Sandbox::new("gemini");
    if !real() {
        sb.start_fake_model();
    }
    let env = gemini_setup(&mut sb);
    for (key, value) in &env {
        std::env::set_var(key, value);
    }
    install_lead("gemini", "0.68.0");
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(sb.home.join(".gemini/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        settings.pointer("/experimental/modelSteering"),
        Some(&json!(true))
    );
    assert_eq!(
        settings.pointer("/general/enableAutoUpdate"),
        Some(&json!(false)),
        "Lead 安装不能动其他设置"
    );
    let argv = meowo_agent::by_id("gemini").unwrap().launch_argv();
    let mut report = serde_json::Map::new();
    let gate_path = sb.task.join("gate.json").to_string_lossy().into_owned();

    // ---- 1. 在线空闲
    let s1 = secret();
    sb.plan(json!({"phase":"Q-1","gate_path":gate_path,"gate_body":sb.gate_body("Q-1","pending"),"consume":true,"consumed_body":sb.gate_body("Q-1","consumed"),"secret":s1}));
    let mut tui = Tui::spawn(&argv, &sb.cwd, &env);
    assert!(tui.wait("gemini 输入框", 90, gemini_ready));
    let first = if real() {
        real_prompt(&sb.task, &sb.gate_body("Q-1", "pending"), &s1)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &first);
    let waiter = sb.wait_waiter("Q-1", if real() { 240 } else { 60 });
    sb.save("gemini-1-waiting.txt", &tui.screen());
    let waiter = waiter.expect("AfterTool 尾调用应在后台启动等待器（不需要审批）");
    let native = sb.owner();
    sb.register("gemini", &native);
    if !real() {
        assert!(tui.wait("回合结束", 60, |s| s.contains("GATE-OPEN-WAITING")));
    }
    assert!(tui.settle(4, 120), "回合应结束");
    tui.type_text("DRAFT-KEEP-1");
    let answered_at = Instant::now();
    assert_eq!(sb.answer("Q-1"), Ok(true));
    let woke = tui.wait(
        "在线空闲自动续跑",
        if real() { 240 } else { 60 },
        |s| s.contains(&s1),
    );
    let latency = answered_at.elapsed();
    sb.save("gemini-1-woke.txt", &tui.screen());
    assert!(woke, "作答后同一会话应自动开新回合");
    assert!(tui.screen().contains("DRAFT-KEEP-1"), "草稿不能被覆盖");
    report.insert("idle".into(), json!({"secret":s1,"waiter_pid":waiter.pid,"latency_ms":latency.as_millis() as u64,"draft_kept":true,"native":native}));
    for _ in 0.."DRAFT-KEEP-1".len() {
        tui.send("\x7f");
    }
    // 打开 modelSteering 后 gemini 运行中回车是插话：等这一轮（含会话摘要等旁路请求）真正结束再发下一句。
    tui.settle(5, 60);

    // ---- 2. 运行中作答（插话语义下等待器的注入在下一步或回合后提交）
    if !real() {
        let s2 = secret();
        sb.plan(json!({"phase":"Q-2","gate_path":gate_path,"gate_body":sb.gate_body("Q-2","pending"),"consume":true,"consumed_body":sb.gate_body("Q-2","consumed"),"secret":s2,"slow":12}));
        submit(&tui, "OPEN-GATE");
        sb.wait_waiter("Q-2", 60).expect("第二个关卡的等待器");
        assert!(tui.settle(4, 60), "回合应结束");
        submit(&tui, "SLOW");
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(sb.answer("Q-2"), Ok(true));
        let woke = tui.wait("运行中作答续跑", 90, |s| s.contains(&s2));
        sb.save("gemini-2-running.txt", &tui.screen());
        assert!(woke, "运行中作答应自动续跑");
        report.insert("running".into(), json!({"secret":s2}));
    }

    // ---- 3. 退出 → 等待器自退 → 离线作答 → 宿主同款 argv 重新打开（--resume <id> -i <答复>）
    let s3 = secret();
    sb.plan(json!({"phase":"Q-3","offline":true,"gate_path":gate_path,"gate_body":sb.gate_body("Q-3","pending"),"secret":s3,"consume":true,"consumed_body":sb.gate_body("Q-3","consumed")}));
    let prompt3 = if real() {
        real_prompt(&sb.task, &sb.gate_body("Q-3", "pending"), &s3)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &prompt3);
    let waiter = sb
        .wait_waiter("Q-3", if real() { 240 } else { 60 })
        .expect("第三个关卡的等待器");
    assert!(tui.settle(4, 120), "回合应结束");
    submit(&tui, "/quit");
    assert!(wait_until(30, || !tui.alive()), "TUI 应退出");
    assert!(
        wait_until(15, || meowo_reporter::lead_waiter::process_started(
            waiter.pid
        )
        .is_none()),
        "TUI 退出后等待器不能留成孤儿"
    );
    // 没有 AppHandle 时宿主不开终端（等下一次带宿主的巡检）；这里按宿主同一模板拼 argv 自己开。
    assert_eq!(sb.answer("Q-3"), Ok(true));
    let gate = meowo_reporter::lead::read_gate(&sb.task).unwrap();
    let message = meowo_reporter::lead::answer_message(&sb.task, &gate);
    let Some(meowo_agent::lead_install::NativeContinue::InSessionWaiter {
        offline: meowo_agent::lead_install::OfflineResume::InteractiveInitialPrompt(template),
    }) = crate::lead_native::mechanism("gemini")
    else {
        panic!("gemini 应声明 InteractiveInitialPrompt");
    };
    let mut resume = meowo_agent::by_id("gemini")
        .unwrap()
        .resume_argv(&native)
        .unwrap();
    resume.extend(template.iter().map(|part| {
        part.replace("{prompt}", &message)
            .replace("{session}", &native)
    }));
    let mut again = Tui::spawn(&resume, &sb.cwd, &env);
    let woke = again.wait(
        "离线续写后可见",
        if real() { 300 } else { 90 },
        |s| s.contains(&s3),
    );
    sb.save("gemini-3-resumed.txt", &again.screen());
    let consumed = wait_until(30, || sb.gate()["status"] == "consumed");
    again.send("/quit\r");
    std::thread::sleep(Duration::from_secs(3));
    again.kill();
    tui.kill();
    assert!(woke, "重新打开后答复应自动提交，这一轮可见");
    assert!(consumed, "关卡应被消费");
    report.insert(
        "offline".into(),
        json!({"secret":s3,"orphan_waiter_exited":true,"consumed":true}),
    );
    // 安装写入的设置可卸载还原
    let plugin = meowo_agent::by_id("gemini").unwrap();
    meowo_agent::lead_install::uninstall(plugin.lead().unwrap(), &plugin.resolve().unwrap())
        .unwrap();
    let after: Value = serde_json::from_str(
        &std::fs::read_to_string(sb.home.join(".gemini/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(after.pointer("/experimental/modelSteering"), None);
    assert!(!sb.home.join(".gemini/policies/meowo-lead.toml").exists());
    sb.save(
        "report.json",
        &serde_json::to_string_pretty(&Value::Object(report)).unwrap(),
    );
    eprintln!("[probe] gemini 证据：{}", sb.evidence.display());
}
