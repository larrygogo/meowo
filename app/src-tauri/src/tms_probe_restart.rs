//! 真机探针：Kimi / Gemini 原生 TUI 切模型、切模式（任务 .lead/20261114-tui-model-switch，验收 #3 #4 #7 #8）。
//!
//! 两家都是「带参数重启接回」机制（ModelSwitch::RestartResume）：后端 apply_session_settings 空闲时
//! 先 [`crate::session_settings::plan`] 拆计划，再 restart_session_with → prepare_resume_launch 拼
//! argv（resolve_resume_plan + splice_stored_launch_args + swap_unsent_resume）重启；未建会话的临时 id
//! 走 reopen_pending_with（launch_argv + args_for_selections）。这些都要 AppHandle / PtyBroker，探针不起
//! Meowo，就用**同一组拼 argv 的函数**算出参数，自己在 PTY 里重启（与 broker.stop 一样硬杀子进程），
//! 再看真实 TUI 的表现。本文件挂在 terminal 模块下，才能调到这几个私有函数。
//!
//! 全程隔离：KIMI_CODE_HOME / GEMINI_CLI_HOME / MEOWO_DB 指一次性目录，本地假模型（fake_llm.py），
//! 不读写用户真实的 ~/.kimi-code、~/.gemini、~/.meowo；hook 用本分支构建的 meowo-reporter 落临时库。
//! 证据写到 `MEOWO_TMS_EVIDENCE`（缺省 `$CARGO_TARGET_DIR/tms-probes`）下的 kimi/、gemini/。
//!
//! ```text
//! cargo build -p meowo-reporter
//! cargo test -p meowo-app --lib tms_probe_restart -- --ignored --nocapture --test-threads=1
//! ```
use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Tui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn spawn(argv: &[String], cwd: &Path, env: &[(String, String)], cols: u16, rows: u16) -> Tui {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        // 本进程跑在 Meowo 托管的会话里：外层 PTY 凭据一律摘掉，防探针认领外层会话（memory 嵌套污染）。
        for (key, _) in std::env::vars() {
            if key.starts_with("MEOWO_PTY_") || key.starts_with("CLAUDE") {
                command.env_remove(key);
            }
        }
        for (key, value) in env {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let (p2, w2) = (parser.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16384];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let bytes = &chunk[..n];
                // kimi/gemini 启动时问终端能力（DA1）与光标位置；不答就停在空白首帧（memory kimi-probe-da1）。
                let mut w = w2.lock().unwrap();
                if bytes.windows(3).any(|x| x == b"\x1b[c") || bytes.windows(4).any(|x| x == b"\x1b[0c") {
                    let _ = w.write_all(b"\x1b[?1;2c");
                }
                if bytes.windows(4).any(|x| x == b"\x1b[6n") {
                    let _ = w.write_all(b"\x1b[1;1R");
                }
                drop(w);
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
    /// 逐行（与前端 managedTerminalScreen 拿到的行一致：vt100 每行一条）。
    fn lines(&self) -> Vec<String> {
        let parser = self.parser.lock().unwrap();
        let (_, cols) = parser.screen().size();
        parser.screen().rows(0, cols).collect()
    }
    fn wait(&self, what: &str, secs: u64, f: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if f(&self.screen()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        eprintln!("[tms] 等不到 {what}，末屏：\n{}", self.screen());
        false
    }
    fn settle(&self, quiet: u64, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut last = self.screen();
        let mut since = Instant::now();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(400));
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
    fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.send(&c.to_string());
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn submit(&self, text: &str) {
        self.type_text(text);
        std::thread::sleep(Duration::from_millis(300));
        self.send("\r");
    }
    fn alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
    /// 与 PtyBroker::stop 同一刀：硬杀直接子进程（Windows TerminateProcess，CLI 来不及收尾）。
    fn hard_kill(&mut self) {
        let pid = self.child.process_id();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // broker 的 waiter 升级链随后杀树；探针直接杀树，免得 node 子进程占着隔离目录。
        if let Some(pid) = pid {
            crate::proc::kill_descendants(pid);
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.hard_kill();
    }
}

fn token() -> String {
    crate::remote::generate_token().unwrap()[..10].to_owned()
}

fn reporter() -> String {
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

struct Sb {
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    db: PathBuf,
    evidence: PathBuf,
    requests: PathBuf,
    fake: Option<std::process::Child>,
    port: u16,
    report: serde_json::Map<String, Value>,
    checks: Vec<Value>,
    started: Instant,
}

impl Drop for Sb {
    fn drop(&mut self) {
        if let Some(mut fake) = self.fake.take() {
            let _ = fake.kill();
            let _ = fake.wait();
        }
        if std::env::var("MEOWO_PROBE_KEEP").as_deref() != Ok("1") {
            for _ in 0..40 {
                if std::fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            if self.root.exists() {
                eprintln!("[tms] 临时目录未删净：{}", self.root.display());
            }
        }
    }
}

impl Sb {
    fn new(provider: &str) -> Sb {
        let root = std::env::temp_dir().join(format!("meowo-tms-{provider}-{}", token()));
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("cwd")).unwrap();
        let root = dunce::canonicalize(root).unwrap();
        let cwd = root.join("cwd");
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&cwd)
            .status()
            .unwrap()
            .success());
        let base = std::env::var("MEOWO_TMS_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
                    .join("tms-probes")
            });
        let evidence = base.join(provider);
        let _ = std::fs::remove_dir_all(&evidence);
        std::fs::create_dir_all(evidence.join("requests")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        Sb {
            home: root.join("home"),
            db: root.join("board.db"),
            requests: evidence.join("requests"),
            evidence,
            cwd,
            root,
            fake: None,
            port,
            report: serde_json::Map::new(),
            checks: Vec::new(),
            started: Instant::now(),
        }
    }
    fn start_fake(&mut self) {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/lead_native/probe_assets/fake_llm.py");
        let python = std::env::var("MEOWO_PROBE_PYTHON").unwrap_or_else(|_| "python".into());
        let log = std::fs::File::create(self.root.join("fake-llm.log")).unwrap();
        let child = std::process::Command::new(python)
            .arg("-I")
            .arg(script)
            .arg(self.port.to_string())
            .arg(&self.requests)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("需要 python");
        self.fake = Some(child);
        let port = self.port;
        assert!(wait_until(15, || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()));
    }
    fn plan(&self, plan: Value) {
        std::fs::write(self.requests.join("plan.json"), plan.to_string()).unwrap();
    }
    fn save(&self, name: &str, text: &str) {
        std::fs::write(self.evidence.join(name), text).unwrap();
    }
    fn check(&mut self, id: &str, ok: bool, detail: Value) {
        eprintln!("[tms] {} {id} {detail}", if ok { "PASS" } else { "FAIL" });
        self.checks.push(json!({"id": id, "ok": ok, "detail": detail, "at_s": self.started.elapsed().as_secs()}));
        self.flush();
    }
    fn note(&mut self, key: &str, value: Value) {
        self.report.insert(key.into(), value);
        self.flush();
    }
    fn flush(&self) {
        let mut out = self.report.clone();
        out.insert("checks".into(), json!(self.checks));
        out.insert("elapsed_s".into(), json!(self.started.elapsed().as_secs()));
        let _ = std::fs::write(
            self.evidence.join("result.json"),
            serde_json::to_string_pretty(&Value::Object(out)).unwrap(),
        );
    }
    /// 假模型收到的对话请求（不含 GET /models、JSON 旁路）：(序号, 路径或 model, 末句)。
    fn chat_requests(&self) -> Vec<Value> {
        let mut files: Vec<_> = std::fs::read_dir(&self.requests)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.sort();
        files
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("req-")))
            .filter_map(|p| serde_json::from_str::<Value>(&std::fs::read_to_string(p).ok()?).ok())
            .filter(|r| r["method"] == "POST" && r.get("decision").is_some())
            .collect()
    }
    /// 最后一条带 `marker` 的对话请求。
    fn request_with(&self, marker: &str) -> Option<Value> {
        self.chat_requests()
            .into_iter()
            .rev()
            .find(|r| r["last_user"].as_str().is_some_and(|u| u.contains(marker)))
    }
    fn store(&self) -> meowo_store::Store {
        crate::open_store(&self.db).unwrap()
    }
    /// 库里登记的会话（reporter 的 hook 写的）。
    fn sessions(&self) -> Vec<(i64, String)> {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        let mut stmt = match conn.prepare("SELECT id, cc_session_id FROM sessions ORDER BY id") {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .flatten()
            .collect()
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

/// 前端 screenShowsAny 的同款判定（modeChoices.ts）：各行用空格拼起来、空白折叠后找原文。
fn screen_shows_any(lines: &[String], markers: &[&str]) -> bool {
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let joined = squash(&lines.join(" "));
    markers.iter().any(|m| !m.is_empty() && joined.contains(&squash(m)))
}

fn selections(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// 后端「带参重启接回」的 argv：与 prepare_resume_launch 同一组函数（resolve_resume_plan →
/// splice_stored_launch_args 合并覆盖并写回 → swap_unsent_resume），调用前旧进程须已结束（与
/// restart_session_with 先 stop 再 prepare 的顺序一致）。返回 (argv, cwd)。
fn restart_argv(
    provider: &str,
    sid: i64,
    native: &str,
    cwd: &Path,
    overrides: &HashMap<String, String>,
) -> (Vec<String>, PathBuf) {
    let cwd_text = cwd.to_string_lossy().into_owned();
    let (resolved, mut resume) = resolve_resume_plan(native, Some(&cwd_text), provider);
    assert!(!resume.is_empty(), "{provider} 应声明 resume");
    splice_stored_launch_args(&mut resume, provider, sid, Some(overrides));
    // 探针里的会话都没在 Meowo 记过用户发言（与生产的 last_user_text 为空同口径）。
    swap_unsent_resume(&mut resume, provider, native, false);
    (resume, resolved.map(PathBuf::from).unwrap_or_else(|| cwd.to_path_buf()))
}

/// 后端「未建会话重开」的 argv：与 reopen_pending_with 同一组函数（launch_argv + args_for_selections，
/// resuming=false）。relay::augment_argv 读的 settings.json 在 MEOWO_DB 旁（临时目录），没配中转时原样返回。
fn fresh_argv(provider: &str, picks: &HashMap<String, String>, env: &[(String, String)], cwd: &Path) -> Vec<String> {
    let agent = meowo_agent::resolve(Some(provider)).unwrap();
    let mut argv = agent.launch_argv();
    argv.extend(crate::session_settings::args_for_selections(agent, picks, env, Some(cwd), false));
    crate::relay::augment_argv(agent.id(), argv)
}

/// apply_blocking 的校验与拆分（清单外的模型、未知的档报错）。
fn plan_for(
    provider: &str,
    model: Option<&str>,
    modes: &[(&str, &str)],
    env: &[(String, String)],
    cwd: &Path,
    current: &[(&str, &str)],
) -> Result<crate::session_settings::Plan, String> {
    let plugin = meowo_agent::resolve(Some(provider)).unwrap();
    let catalog = crate::session_settings::catalog_for(plugin, env, Some(cwd));
    crate::session_settings::plan(
        plugin,
        model,
        None,
        &selections(modes),
        &catalog,
        &[],
        &selections(current),
        &HashMap::new(),
    )
}

/// 假 broker 的认领端口：记下 reporter 发来的原始字节（认领消息里带 launch token 与会话 id）。
fn fake_claim_listener() -> (String, Arc<Mutex<Vec<u8>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let sink = sink.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                let mut buf = [0u8; 4096];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    sink.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            });
        }
    });
    (endpoint, got)
}

fn wire_hooks(provider: &str, sb: &Sb, version: &str, data_dir: &Path) {
    let plugin = meowo_agent::by_id(provider).unwrap();
    let inst = plugin.resolve().expect("CLI 已安装");
    // 安全闸：接线写的是 resolve() 指的数据目录，必须是隔离目录，绝不能落到用户真实目录。
    assert_eq!(
        dunce::canonicalize(&inst.data_dir).unwrap_or(inst.data_dir.clone()),
        dunce::canonicalize(data_dir).unwrap(),
        "数据目录没指到隔离目录，停止（防写用户配置）"
    );
    let meowo_dir = sb.root.join("meowo");
    std::fs::create_dir_all(&meowo_dir).unwrap();
    let reporter = reporter();
    let wired = plugin.wire(&meowo_agent::wiring::WiringContext {
        cli_version: Some(version),
        fallback_reporter: Some(&reporter),
        meowo_dir: &meowo_dir,
    });
    assert!(wired.is_none(), "hook 接线失败：{wired:?}");
}

fn kimi_ready(s: &str) -> bool {
    s.contains("Welcome to Kimi") || s.contains("Ask When Needed") || s.contains("Never Ask") || s.contains("Always Ask")
}

fn kimi_pass_trust(tui: &Tui) -> bool {
    let ok = tui.wait("kimi 启动", 40, |s| s.contains("Trust this folder") || kimi_ready(s));
    if tui.screen().contains("Trust this folder") {
        tui.send("\r");
    }
    ok && tui.wait("kimi 输入框", 30, kimi_ready)
}

fn kimi_approval(screen: &str) -> bool {
    let s = screen.to_lowercase();
    s.contains("↵ confirm") && (s.contains("write this file?") || s.contains("approve") || s.contains("run this command?"))
}

fn gemini_approval(screen: &str) -> bool {
    let s = screen.to_lowercase();
    s.contains("allow once") || s.contains("allow execution") || s.contains("allow for this session")
}

/// kimi 会话目录（KIMI_CODE_HOME/sessions/<wd>/<id>/）里的会话 id。
fn kimi_session_dirs(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for wd in std::fs::read_dir(home.join("sessions")).into_iter().flatten().flatten() {
        for s in std::fs::read_dir(wd.path()).into_iter().flatten().flatten() {
            let name = s.file_name().to_string_lossy().into_owned();
            if name.starts_with("session_") {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

fn kimi_wire(home: &Path, native: &str) -> String {
    for wd in std::fs::read_dir(home.join("sessions")).into_iter().flatten().flatten() {
        let p = wd.path().join(native).join("agents/main/wire.jsonl");
        if p.is_file() {
            return std::fs::read_to_string(p).unwrap_or_default();
        }
    }
    String::new()
}

/// kimi 会话 transcript 读到的模式（与对话页 transcript 增量同一个解析）。
fn kimi_modes(home: &Path, native: &str) -> Vec<(String, String)> {
    let spec = meowo_agent::by_id("kimi").unwrap().telemetry().unwrap().transcript().unwrap();
    let mut modes: Vec<(String, String)> = Vec::new();
    for line in kimi_wire(home, native).lines() {
        for m in spec.agent_modes_from_line(line) {
            modes.retain(|(d, _)| d != &m.dimension);
            modes.push((m.dimension, m.value));
        }
    }
    modes
}

/// Stop hook / refresh_session_model 读模型的同一入口（telemetry.stop_outputs）。
fn stop_model(provider: &str, native: &str) -> (Option<String>, Option<String>) {
    let out = meowo_agent::by_id(provider)
        .unwrap()
        .telemetry()
        .unwrap()
        .stop_outputs(&meowo_agent::caps::HookContext {
            session_id: native,
            transcript_path: None,
            last_assistant_message: None,
        });
    (out.model, out.effort)
}

// ---------------------------------------------------------------- kimi

const KIMI_CONFIG: &str = r#"default_model = "fake/fake-model"
default_permission_mode = "manual"

[providers.fake]
type = "kimi"
base_url = "http://127.0.0.1:PORT/v1"
api_key = "sk-fake"

[models."fake/fake-model"]
provider = "fake"
model = "fake-model"
max_context_size = 262144
capabilities = ["tool_use"]
display_name = "Fake A"

[models."fake/fake-model-b"]
provider = "fake"
model = "fake-model-b"
max_context_size = 262144
capabilities = ["tool_use", "thinking"]
display_name = "Fake B"
support_efforts = ["low", "high"]
default_effort = "high"
"#;

#[test]
#[ignore = "拉起真实 kimi TUI（假模型）；手动复跑"]
fn tms_probe_kimi() {
    let mut sb = Sb::new("kimi");
    let env: Vec<(String, String)> = vec![
        ("KIMI_CODE_HOME".into(), sb.home.to_string_lossy().into_owned()),
        ("KIMI_CODE_NO_AUTO_UPDATE".into(), "1".into()),
        ("MEOWO_DB".into(), sb.db.to_string_lossy().into_owned()),
    ];
    for (k, v) in &env {
        std::env::set_var(k, v);
    }
    sb.start_fake();
    std::fs::write(sb.home.join("config.toml"), KIMI_CONFIG.replace("PORT", &sb.port.to_string())).unwrap();
    let version = std::process::Command::new(&meowo_agent::by_id("kimi").unwrap().launch_argv()[0])
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    sb.note("cli_version", json!(version));
    wire_hooks("kimi", &sb, "2.6.0", &sb.home.clone());
    let config_before = std::fs::read_to_string(sb.home.join("config.toml")).unwrap();
    let default_line = |c: &str| c.lines().find(|l| l.starts_with("default_model")).map(str::to_owned);
    let kimi = meowo_agent::by_id("kimi").unwrap();
    let catalog = crate::session_settings::catalog_for(kimi, &env, Some(&sb.cwd));
    sb.note("catalog", json!(catalog.iter().map(|c| json!({"id": c.id, "label": c.label, "efforts": c.efforts})).collect::<Vec<_>>()));
    let markers = kimi.awaiting_first_message_markers();

    // ---- K1 未建会话：认作「等第一条消息」；此时切模型 = 带新参数重开，不卡启动；首条消息后 hook 认领。
    let (endpoint, claims) = fake_claim_listener();
    let launch_token = format!("launch-{}", token());
    let mut claim_env = env.clone();
    claim_env.extend([
        ("MEOWO_PTY_ENDPOINT".to_string(), endpoint),
        ("MEOWO_PTY_TOKEN".to_string(), "probe-token".to_string()),
        ("MEOWO_PTY_LAUNCH".to_string(), launch_token.clone()),
        ("MEOWO_PTY_PROTOCOL".to_string(), meowo_protocol::broker::CURRENT_PROTOCOL_VERSION.to_string()),
        ("MEOWO_PTY_DB".to_string(), sb.db.to_string_lossy().into_owned()),
    ]);
    let argv0 = fresh_argv("kimi", &HashMap::new(), &env, &sb.cwd);
    let started = Instant::now();
    let mut tui = Tui::spawn(&argv0, &sb.cwd, &claim_env, 140, 40);
    let ready = kimi_pass_trust(&tui);
    let awaiting = tui.wait("No session yet", 20, |_| screen_shows_any(&tui.lines(), markers));
    sb.save("k1-0-fresh.txt", &tui.screen());
    sb.check("K1.awaiting_marker_140col", ready && awaiting, json!({"ms": started.elapsed().as_millis() as u64, "argv": argv0}));
    let no_session = kimi_session_dirs(&sb.home).is_empty() && sb.sessions().is_empty();
    sb.check("K1.no_session_before_first_message", no_session, json!({"dirs": kimi_session_dirs(&sb.home), "db": sb.sessions().len()}));
    // 未建会话时选模型 B：apply_blocking 的 session_id < 0 分支（校验清单 → reopen_pending_with）。
    let picks = {
        let catalog_ok = meowo_agent::model_id_allowed("fake/fake-model-b", &catalog);
        sb.check("K1.model_in_catalog", catalog_ok, json!("fake/fake-model-b"));
        selections(&[("model", "fake/fake-model-b")])
    };
    let argv1 = fresh_argv("kimi", &picks, &env, &sb.cwd);
    tui.hard_kill();
    // 重开用 broker 拿不到网格时的默认 100×30（reopen_pending_with），顺带验窄屏下原文折行仍认得出。
    let reopen_at = Instant::now();
    let tui = Tui::spawn(&argv1, &sb.cwd, &claim_env, 100, 30);
    let ready = kimi_pass_trust(&tui);
    let awaiting = tui.wait("重开后 No session yet", 20, |_| screen_shows_any(&tui.lines(), markers));
    let reopen_ms = reopen_at.elapsed().as_millis() as u64;
    sb.save("k1-1-reopened.txt", &tui.screen());
    sb.check(
        "K1.reopen_not_stuck",
        ready && awaiting && argv1.windows(2).any(|w| w[0] == "-m" && w[1] == "fake/fake-model-b"),
        json!({"argv": argv1, "reopen_ready_ms": reopen_ms, "screen_shows_model_b": tui.screen().contains("Fake B")}),
    );
    tui.submit("SAY k1first");
    let replied = tui.wait("K1 回复", 60, |s| s.contains("SAID k1first"));
    tui.settle(3, 30);
    sb.save("k1-2-first-reply.txt", &tui.screen());
    let req = sb.request_with("SAY k1first");
    let model1 = req.as_ref().and_then(|r| r.pointer("/body_meta/model").cloned());
    sb.check("K1.first_request_uses_reopen_model", replied && model1 == Some(json!("fake-model-b")), json!({"model": model1}));
    let claimed = wait_until(15, || {
        String::from_utf8_lossy(&claims.lock().unwrap()).contains(&launch_token)
    });
    let rows = sb.sessions();
    let native = rows.first().map(|(_, n)| n.clone()).unwrap_or_default();
    let claim_text = String::from_utf8_lossy(&claims.lock().unwrap()).into_owned();
    sb.check(
        "K1.hook_claims_after_first_message",
        claimed && native.starts_with("session_") && claim_text.contains(&format!("\"session_id\":{}", rows.first().map(|r| r.0).unwrap_or(-1))),
        json!({"db_rows": rows, "claim_bytes": claim_text.len(), "claim_has_launch_token": claimed}),
    );
    let sid = rows.first().map(|r| r.0).unwrap_or(0);
    {
        let store = sb.store();
        store.set_session_provider(sid, "kimi").unwrap();
        store.set_session_cwd(sid, &sb.cwd.to_string_lossy(), 3).unwrap();
        // 生产里 broker 认领时把重开时的选择落库（pending_launch_args → claim）；探针替 broker 写这一笔。
        store.set_session_launch_args(sid, &serde_json::to_string(&picks).unwrap()).unwrap();
    }
    sb.check("K1.config_default_untouched", default_line(&std::fs::read_to_string(sb.home.join("config.toml")).unwrap()) == default_line(&config_before), json!(default_line(&config_before)));
    drop(tui);

    // 重新以真实接回形态起会话（broker 认领后的会话没有 MEOWO_PTY_* 也照常跑）。
    let (argv, cwd) = restart_argv("kimi", sid, &native, &sb.cwd, &HashMap::new());
    sb.note("k2_resume_argv_no_override", json!(argv));
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    assert!(kimi_pass_trust(&tui), "接回失败");
    tui.settle(2, 20);

    // ---- K2 空闲切模型（B → A）：plan → 重启接回；同一会话、历史在、下一轮用新模型；TUI 草稿去向。
    let plan = plan_for("kimi", Some("fake/fake-model"), &[], &env, &sb.cwd, &[]).unwrap();
    sb.note("k2_plan", json!({"restart": plan.restart, "server": plan.server, "needs_new_session": plan.needs_new_session}));
    tui.type_text("DRAFT-K2");
    std::thread::sleep(Duration::from_millis(500));
    let draft_before = tui.screen().contains("DRAFT-K2");
    let mut tui = tui;
    tui.hard_kill();
    let (argv, cwd) = restart_argv("kimi", sid, &native, &sb.cwd, &plan.restart);
    let restart_at = Instant::now();
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    let ready = kimi_pass_trust(&tui);
    let history = tui.wait("历史回放", 20, |s| s.contains("SAID k1first"));
    let restart_ms = restart_at.elapsed().as_millis() as u64;
    tui.settle(2, 20);
    sb.save("k2-0-restarted.txt", &tui.screen());
    let draft_after = tui.screen().contains("DRAFT-K2");
    sb.check(
        "K2.restart_same_session_history",
        ready && history && argv.contains(&native) && argv.windows(2).any(|w| w[0] == "-m" && w[1] == "fake/fake-model"),
        json!({"argv": argv, "restart_ready_ms": restart_ms}),
    );
    sb.check("K2.tui_draft_after_restart_recorded", true, json!({"draft_in_tui_before": draft_before, "draft_in_tui_after": draft_after}));
    tui.submit("SAY k2next");
    let replied = tui.wait("K2 回复", 60, |s| s.contains("SAID k2next"));
    tui.settle(3, 30);
    sb.save("k2-1-next-turn.txt", &tui.screen());
    let model2 = sb.request_with("SAY k2next").and_then(|r| r.pointer("/body_meta/model").cloned());
    let dirs = kimi_session_dirs(&sb.home);
    sb.check(
        "K2.next_turn_new_model_same_id",
        replied && model2 == Some(json!("fake-model")) && sb.sessions().len() == 1 && dirs.contains(&native),
        json!({"model": model2, "session_dirs": dirs, "session_index": std::fs::read_to_string(sb.home.join("session_index.jsonl")).unwrap_or_default(), "db_rows": sb.sessions(), "status_bar_has_fake_a": tui.screen().contains("Fake A")}),
    );
    let (m, e) = stop_model("kimi", &native);
    sb.check("K2.stop_outputs_model", m.as_deref() == Some("Fake A"), json!({"model": m, "effort": e}));
    sb.check("K2.config_default_untouched", default_line(&std::fs::read_to_string(sb.home.join("config.toml")).unwrap()) == default_line(&config_before), json!(null));

    // ---- K4a 有待批审批：manual 档下模型要写文件 → 审批框；Meowo 的判定输入（库里 pending_review）
    // 被 hook 写上 → 前端 mustDefer 不写终端不重启；等着不动，审批不被答掉、文件不落地。
    let target = sb.cwd.join("approval-k.txt");
    sb.plan(json!({"phase":"K4","gate_path": target.to_string_lossy(),"gate_body":"approved-content"}));
    tui.submit("OPEN-GATE");
    let shown = tui.wait("审批框", 60, kimi_approval);
    std::thread::sleep(Duration::from_secs(2));
    let review = sb.store().pending_review_of(sid).ok().flatten();
    sb.save("k4-0-approval.txt", &tui.screen());
    std::thread::sleep(Duration::from_secs(5));
    let still = kimi_approval(&tui.screen());
    sb.check(
        "K4.approval_pending_marks_session",
        shown && review.is_some() && still && !target.exists(),
        json!({"pending_review": review, "approval_still_shown_after_7s": still, "file_written": target.exists()}),
    );
    // 用户自己拒绝（Esc）后回合结束，排着的切换才执行。
    tui.send("\x1b");
    std::thread::sleep(Duration::from_secs(1));
    tui.settle(3, 30);
    let review_after = sb.store().pending_review_of(sid).ok().flatten();
    sb.note("k4_after_decline", json!({"pending_review": review_after, "file_written": target.exists()}));
    sb.save("k4-1-declined.txt", &tui.screen());

    // ---- K4b 运行中：SLOW 回合跑到一半时切换会打断这一轮（这就是要排到回合结束的原因）；回合结束后再切，
    // 这一轮的输出完整在历史里，下一轮用新模型。
    sb.plan(json!({"phase":"K4b","slow": 8}));
    tui.submit("SLOW");
    let running_screen = {
        std::thread::sleep(Duration::from_secs(2));
        tui.screen()
    };
    let done = tui.wait("SLOW 结束", 40, |s| s.contains("SLOW-DONE"));
    tui.settle(3, 30);
    let plan = plan_for("kimi", Some("fake/fake-model-b"), &[], &env, &sb.cwd, &[]).unwrap();
    let mut tui = tui;
    tui.hard_kill();
    let (argv, cwd) = restart_argv("kimi", sid, &native, &sb.cwd, &plan.restart);
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    kimi_pass_trust(&tui);
    let kept = tui.wait("SLOW-DONE 仍在历史", 20, |s| s.contains("SLOW-DONE"));
    tui.submit("SAY k4after");
    tui.wait("K4b 回复", 60, |s| s.contains("SAID k4after"));
    tui.settle(3, 30);
    sb.save("k4-2-after-slow.txt", &tui.screen());
    let model4 = sb.request_with("SAY k4after").and_then(|r| r.pointer("/body_meta/model").cloned());
    sb.check(
        "K4.switch_after_turn_end",
        done && kept && model4 == Some(json!("fake-model-b")),
        json!({"running_screen_tail": running_screen.lines().rev().take(6).collect::<Vec<_>>(), "model": model4}),
    );

    // ---- K7 模式：权限档 manual → auto（Restart，带 --auto 接回）；收紧回 manual 如实报需要新会话；
    // plan 是 Control（前端打 `/plan on|off`），transcript 读到 enter / cancel。
    let plan = plan_for("kimi", None, &[("permission", "auto")], &env, &sb.cwd, &[("permission", "manual")]).unwrap();
    let mut tui = tui;
    tui.hard_kill();
    let (argv, cwd) = restart_argv("kimi", sid, &native, &sb.cwd, &plan.restart);
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    kimi_pass_trust(&tui);
    let never = tui.wait("Never Ask", 20, |s| s.contains("Never Ask"));
    tui.settle(2, 20);
    sb.save("k7-0-auto.txt", &tui.screen());
    let modes = kimi_modes(&sb.home, &native);
    sb.check(
        "K7.permission_auto_via_restart",
        never && argv.iter().any(|a| a == "--auto") && modes.contains(&("permission".into(), "auto".into())),
        json!({"argv": argv, "transcript_modes": modes}),
    );
    let tighten = plan_for("kimi", None, &[("permission", "manual")], &env, &sb.cwd, &[("permission", "auto")]).unwrap();
    sb.check("K7.tighten_needs_new_session", tighten.needs_new_session, json!({"restart": tighten.restart}));
    tui.submit("/plan on");
    let plan_on = tui.wait("plan 开", 15, |s| s.lines().any(|l| l.contains("Never Ask") && l.contains("plan")));
    tui.settle(2, 15);
    let modes_on = kimi_modes(&sb.home, &native);
    tui.submit("/plan off");
    tui.settle(2, 15);
    let modes_off = kimi_modes(&sb.home, &native);
    sb.save("k7-1-plan.txt", &tui.screen());
    sb.check(
        "K7.plan_on_off_read_back",
        plan_on && modes_on.contains(&("work".into(), "plan".into())) && modes_off.contains(&("work".into(), "default".into())),
        json!({"after_on": modes_on, "after_off": modes_off}),
    );

    // ---- K8 TUI 里自己切模型（/model <alias> 预选 + Alt+S 只对本会话），回合结束后读到新值。
    tui.submit("/model fake/fake-model");
    std::thread::sleep(Duration::from_millis(1500));
    sb.save("k8-0-picker.txt", &tui.screen());
    tui.send("\x1bs");
    std::thread::sleep(Duration::from_millis(1500));
    tui.submit("SAY k8");
    tui.wait("K8 回复", 60, |s| s.contains("SAID k8"));
    tui.settle(3, 30);
    sb.save("k8-1-after.txt", &tui.screen());
    let (m, e) = stop_model("kimi", &native);
    let model8 = sb.request_with("SAY k8").and_then(|r| r.pointer("/body_meta/model").cloned());
    sb.check("K8.tui_self_switch_read_back", m.as_deref() == Some("Fake A") && model8 == Some(json!("fake-model")), json!({"stop_outputs": [m, e], "request_model": model8}));
    sb.check("K8.alt_s_does_not_touch_config", default_line(&std::fs::read_to_string(sb.home.join("config.toml")).unwrap()) == default_line(&config_before), json!(null));
    drop(tui);
    // 新建页默认值：改 config.toml 的默认模型与默认权限档，launch_defaults 现读跟上。
    let before = kimi.launch_defaults(&env, Some(&sb.cwd));
    let edited = std::fs::read_to_string(sb.home.join("config.toml"))
        .unwrap()
        .replace("default_model = \"fake/fake-model\"", "default_model = \"fake/fake-model-b\"")
        .replace("default_permission_mode = \"manual\"", "default_permission_mode = \"yolo\"");
    std::fs::write(sb.home.join("config.toml"), edited).unwrap();
    let after = kimi.launch_defaults(&env, Some(&sb.cwd));
    sb.check(
        "K8.launch_defaults_follow_config",
        after.model.as_deref() == Some("fake/fake-model-b") && after.choices.get("permission").map(String::as_str) == Some("yolo"),
        json!({"before": {"model": before.model, "choices": before.choices}, "after": {"model": after.model, "choices": after.choices}}),
    );

    // ---- K7b 新建会话带模式与模型启动参数：Never Ask + plan + Fake B。
    let picks = selections(&[("model", "fake/fake-model-b"), ("permission", "auto"), ("work", "plan")]);
    let argv = fresh_argv("kimi", &picks, &env, &sb.cwd);
    let tui = Tui::spawn(&argv, &sb.cwd, &env, 140, 40);
    kimi_pass_trust(&tui);
    let shown = tui.wait("新建带参", 20, |s| s.lines().any(|l| l.contains("Never Ask") && l.contains("plan") && l.contains("Fake B")));
    sb.save("k7-2-new-with-modes.txt", &tui.screen());
    sb.check("K7.new_session_with_mode_args", shown, json!({"argv": argv}));
    drop(tui);

    let failed: Vec<_> = sb.checks.iter().filter(|c| c["ok"] == false).cloned().collect();
    eprintln!("[tms] kimi 证据：{}", sb.evidence.display());
    assert!(failed.is_empty(), "{failed:#?}");
}

// ---------------------------------------------------------------- gemini

fn gemini_ready(s: &str) -> bool {
    s.contains("Type your message")
}

fn gemini_chats(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for proj in std::fs::read_dir(home.join(".gemini/tmp")).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(proj.path().join("chats")).into_iter().flatten().flatten() {
            out.push(f.path());
        }
    }
    out.sort();
    out
}

fn gemini_model_of(r: &Value) -> Option<String> {
    let path = r["path"].as_str()?;
    let rest = path.split("/models/").nth(1)?;
    Some(rest.split(':').next()?.to_owned())
}

#[test]
#[ignore = "拉起真实 gemini TUI（假模型）；手动复跑"]
fn tms_probe_gemini() {
    let mut sb = Sb::new("gemini");
    sb.start_fake();
    let gemini_dir = sb.home.join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let env: Vec<(String, String)> = vec![
        ("GEMINI_CLI_HOME".into(), sb.home.to_string_lossy().into_owned()),
        ("MEOWO_DB".into(), sb.db.to_string_lossy().into_owned()),
        ("GEMINI_CLI_NO_RELAUNCH".into(), "true".into()),
        ("GEMINI_API_KEY".into(), "fake-key".into()),
        ("GOOGLE_GEMINI_BASE_URL".into(), format!("http://127.0.0.1:{}", sb.port)),
    ];
    for (k, v) in &env {
        std::env::set_var(k, v);
    }
    let settings = json!({
        "general": {"enableAutoUpdate": false, "enableAutoUpdateNotification": false, "defaultApprovalMode": "default"},
        "model": {"name": "gemini-3.5-flash-lite"},
        "ui": {"hideTips": true, "hideBanner": true},
        "privacy": {"usageStatisticsEnabled": false},
        "security": {"auth": {"selectedType": "gemini-api-key"}},
    });
    std::fs::write(gemini_dir.join("settings.json"), serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    std::fs::write(
        gemini_dir.join("trustedFolders.json"),
        json!({ sb.cwd.to_string_lossy(): "TRUST_FOLDER" }).to_string(),
    )
    .unwrap();
    let gemini = meowo_agent::by_id("gemini").unwrap();
    let version = std::process::Command::new("cmd")
        .args(["/c", "gemini", "--version"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    sb.note("cli_version", json!(version));
    wire_hooks("gemini", &sb, "0.68.0", &gemini_dir);
    let catalog = crate::session_settings::catalog_for(gemini, &env, Some(&sb.cwd));
    sb.note("catalog", json!(catalog.iter().map(|c| json!({"id": c.id, "label": c.label, "group": c.group})).collect::<Vec<_>>()));
    let settings_text = || std::fs::read_to_string(gemini_dir.join("settings.json")).unwrap();
    let settings_before = settings_text();

    // ---- G0 新会话：启动即有 uuid、hook 认领（与 Kimi 不同，不会「等第一条消息」）。
    let argv0 = fresh_argv("gemini", &HashMap::new(), &env, &sb.cwd);
    let mut tui = Tui::spawn(&argv0, &sb.cwd, &env, 140, 40);
    let ready = tui.wait("gemini 输入框", 90, gemini_ready);
    let claimed = wait_until(20, || !sb.sessions().is_empty());
    let rows = sb.sessions();
    let (mut sid, mut native) = rows.first().cloned().unwrap_or((0, String::new()));
    sb.check("G0.session_claimed_at_startup", ready && claimed, json!({"argv": argv0, "db_rows": rows}));
    {
        let store = sb.store();
        let _ = store.set_session_provider(sid, "gemini");
        let _ = store.set_session_cwd(sid, &sb.cwd.to_string_lossy(), 3);
    }

    // ---- G1 还没发过消息就切模型：硬杀后会话文件是否还在决定 --resume / --session-id。
    let plan = plan_for("gemini", Some("gemini-2.5-pro"), &[], &env, &sb.cwd, &[]).unwrap();
    sb.note("g1_plan", json!({"restart": plan.restart}));
    let chats_before_kill = gemini_chats(&sb.home);
    tui.hard_kill();
    std::thread::sleep(Duration::from_millis(800));
    let chats_after_kill = gemini_chats(&sb.home);
    let (argv, cwd) = restart_argv("gemini", sid, &native, &sb.cwd, &plan.restart);
    let at = Instant::now();
    let mut tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    let resume_failed = |s: &str| s.contains("Invalid session") || s.contains("Error resuming session");
    let ready = tui.wait("G1 接回", 60, |s| gemini_ready(s) || resume_failed(s));
    let ready_ms = at.elapsed().as_millis() as u64;
    std::thread::sleep(Duration::from_secs(2));
    sb.save("g1-0-unsent-restart.txt", &tui.screen());
    let invalid = resume_failed(&tui.screen()) || !tui.alive();
    let first_argv = argv.clone();
    if invalid {
        // 硬杀（与 broker.stop 同一刀）后 gemini 来不及删空会话文件，swap_unsent_resume 看见文件就仍用
        // --resume，CLI 却认为没有可恢复的会话直接退出。探针按「本该走的」--session-id 继续，好验后面几步。
        sb.check("G1.unsent_restart_after_hard_kill", false, json!({"argv": first_argv, "ready_ms": ready_ms, "chats_after_hard_kill": chats_after_kill, "screen": tui.screen().trim()}));
        // 再试「本该走的」--session-id：实测报 Session ID already exists——这个 id 两条路都接不回了。
        let mut fixed = argv.clone();
        if let Some(i) = fixed.iter().position(|a| a == "--resume") {
            fixed[i] = "--session-id".into();
        }
        tui = Tui::spawn(&fixed, &cwd, &env, 140, 40);
        let alt_ok = tui.wait("G1 --session-id", 40, |s| gemini_ready(s) || s.contains("already exists"));
        std::thread::sleep(Duration::from_secs(2));
        let alt_screen = tui.screen();
        sb.check(
            "G1.session_id_fallback_after_hard_kill",
            alt_ok && gemini_ready(&alt_screen) && !alt_screen.contains("already exists"),
            json!({"argv": fixed, "screen": alt_screen.trim()}),
        );
        // 后面几步要一个有消息的会话：另起新会话（带 -m，等同 reopen 的参数），换用新 id。
        let argv = fresh_argv("gemini", &selections(&[("model", "gemini-2.5-pro")]), &env, &sb.cwd);
        tui = Tui::spawn(&argv, &sb.cwd, &env, 140, 40);
        tui.wait("G1 新会话", 90, gemini_ready);
        wait_until(20, || sb.sessions().len() >= 2);
        if let Some((id, n)) = sb.sessions().last().cloned() {
            sid = id;
            native = n;
            let store = sb.store();
            let _ = store.set_session_provider(sid, "gemini");
            let _ = store.set_session_cwd(sid, &sb.cwd.to_string_lossy(), 3);
        }
    }
    tui.submit("SAY g1first");
    let replied = tui.wait("G1 回复", 60, |s| s.contains("SAID g1first"));
    tui.settle(3, 30);
    sb.save("g1-1-first-reply.txt", &tui.screen());
    let model1 = sb.request_with("SAY g1first").as_ref().and_then(gemini_model_of);
    let rows = sb.sessions();
    sb.check(
        "G1.unsent_switch_keeps_id_and_model",
        !invalid && replied && model1.as_deref() == Some("gemini-2.5-pro") && rows.len() == 1,
        json!({"argv_used": if invalid { "fallback --session-id" } else { "restart_argv" }, "argv": argv, "ready_ms": ready_ms, "chats_before_kill": chats_before_kill, "chats_after_hard_kill": chats_after_kill, "first_attempt_failed": invalid, "ready_ok": ready, "model": model1, "db_rows": rows}),
    );

    // ---- G2 空闲切模型（已有消息）：--resume <uuid> -m；历史在、id 不变、下一轮新模型；TUI 草稿去向。
    let plan = plan_for("gemini", Some("gemini-3.8-flash"), &[], &env, &sb.cwd, &[]).unwrap();
    tui.type_text("DRAFT-G2");
    std::thread::sleep(Duration::from_millis(500));
    let draft_before = tui.screen().contains("DRAFT-G2");
    tui.hard_kill();
    let (argv, cwd) = restart_argv("gemini", sid, &native, &sb.cwd, &plan.restart);
    let at = Instant::now();
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    let ready = tui.wait("G2 接回", 60, gemini_ready);
    let history = tui.wait("G2 历史", 15, |s| s.contains("SAID g1first"));
    let ready_ms = at.elapsed().as_millis() as u64;
    tui.settle(2, 15);
    sb.save("g2-0-restarted.txt", &tui.screen());
    let draft_after = tui.screen().contains("DRAFT-G2");
    sb.check("G2.tui_draft_after_restart_recorded", true, json!({"draft_in_tui_before": draft_before, "draft_in_tui_after": draft_after}));
    tui.submit("SAY g2next");
    // 假模型按末尾几段文本里的第一个 SAY 回话（会回成 SAID g1first），以「请求发出且回合结束」为准。
    let replied = wait_until(60, || sb.request_with("SAY g2next").is_some());
    tui.settle(3, 30);
    sb.save("g2-1-next.txt", &tui.screen());
    let model2 = sb.request_with("SAY g2next").as_ref().and_then(gemini_model_of);
    let rows = sb.sessions();
    sb.check(
        "G2.resume_same_session_new_model",
        ready && history && replied && model2.as_deref() == Some("gemini-3.8-flash") && sb.sessions().iter().filter(|r| r.1 == native).count() == 1 && argv.iter().any(|a| a == "--resume"),
        json!({"argv": argv, "ready_ms": ready_ms, "model": model2, "db_rows": rows, "chats": gemini_chats(&sb.home), "status_bar_has_model": tui.screen().contains("gemini-3.8-flash")}),
    );
    let (m, _) = stop_model("gemini", &native);
    sb.check("G2.stop_outputs_model", m.as_deref() == Some("gemini-3.8-flash"), json!(m));
    sb.check("G2.settings_untouched", settings_text() == settings_before, json!(null));

    // ---- G4 有待批审批：default 档写文件弹审批；库里 pending_review 被 hook 写上（前端据此不写终端
    // 不重启）；等着不动，审批不被答掉。
    let target = sb.cwd.join("approval-g.txt");
    sb.plan(json!({"phase":"G4","gate_path": target.to_string_lossy(),"gate_body":"approved-content"}));
    tui.submit("OPEN-GATE");
    let shown = tui.wait("审批框", 60, gemini_approval);
    std::thread::sleep(Duration::from_secs(2));
    let review = sb.store().pending_review_of(sid).ok().flatten();
    sb.save("g4-0-approval.txt", &tui.screen());
    std::thread::sleep(Duration::from_secs(5));
    let still = gemini_approval(&tui.screen());
    sb.check(
        "G4.approval_pending_marks_session",
        shown && review.is_some() && still && !target.exists(),
        json!({"pending_review": review, "approval_still_shown_after_7s": still, "file_written": target.exists()}),
    );
    tui.send("\x1b");
    std::thread::sleep(Duration::from_secs(1));
    tui.settle(3, 30);
    sb.note("g4_after_decline", json!({"pending_review": sb.store().pending_review_of(sid).ok().flatten(), "file_written": target.exists()}));
    sb.save("g4-1-declined.txt", &tui.screen());

    // ---- G7 模式：default → plan（--approval-mode plan 接回，底栏 plan）；再 → autoEdit；新建带 --yolo。
    let plan = plan_for("gemini", None, &[("approval", "plan")], &env, &sb.cwd, &[("approval", "default")]).unwrap();
    let mut tui = tui;
    tui.hard_kill();
    let (argv, cwd) = restart_argv("gemini", sid, &native, &sb.cwd, &plan.restart);
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    tui.wait("G7 接回", 60, gemini_ready);
    let in_plan = tui.wait("plan 底栏", 15, |s| s.to_lowercase().contains("shift+tab to manual") || s.contains("plan"));
    sb.save("g7-0-plan.txt", &tui.screen());
    let screen_value = |lines: &[String]| -> Option<&'static str> {
        let joined = lines.join("\n").to_lowercase();
        gemini.mode_controls()[0]
            .screen_markers
            .iter()
            .find(|m| joined.contains(m.marker))
            .map(|m| m.value)
    };
    let seen = screen_value(&tui.lines());
    sb.check("G7.approval_plan_via_restart", in_plan && seen == Some("plan"), json!({"argv": argv, "screen_marker": seen}));
    let plan = plan_for("gemini", None, &[("approval", "autoEdit")], &env, &sb.cwd, &[("approval", "plan")]).unwrap();
    let mut tui = tui;
    tui.hard_kill();
    let (argv, cwd) = restart_argv("gemini", sid, &native, &sb.cwd, &plan.restart);
    let tui = Tui::spawn(&argv, &cwd, &env, 140, 40);
    tui.wait("G7b 接回", 60, gemini_ready);
    tui.settle(2, 15);
    let seen = screen_value(&tui.lines());
    sb.save("g7-1-autoedit.txt", &tui.screen());
    sb.check("G7.approval_autoedit_via_restart", seen == Some("autoEdit"), json!({"argv": argv, "screen_marker": seen}));

    // ---- G8 TUI 里自己切：/model set（本会话）→ 回合结束后 stop_outputs 读到；Shift+Tab 切审批档，
    // 只能从底栏原文读（transcript/hook 都不带审批模式）。
    tui.submit("/model set gemini-2.5-pro");
    std::thread::sleep(Duration::from_secs(2));
    let (m_now, _) = stop_model("gemini", &native);
    tui.submit("SAY g8");
    tui.wait("G8 回复", 60, |s| s.contains("SAID g8"));
    tui.settle(3, 30);
    let (m_after, _) = stop_model("gemini", &native);
    let model8 = sb.request_with("SAY g8").as_ref().and_then(gemini_model_of);
    sb.check(
        "G8.tui_model_set_read_back",
        m_now.as_deref() == Some("gemini-2.5-pro") && m_after.as_deref() == Some("gemini-2.5-pro") && model8.as_deref() == Some("gemini-2.5-pro"),
        json!({"right_after_set": m_now, "after_turn": m_after, "request_model": model8}),
    );
    tui.send("\x1b[Z");
    std::thread::sleep(Duration::from_millis(1200));
    let seen = screen_value(&tui.lines());
    sb.save("g8-0-shifttab.txt", &tui.screen());
    sb.check("G8.shift_tab_read_from_footer", seen == Some("plan"), json!({"after_shift_tab_from_autoEdit": seen}));
    sb.check("G8.settings_untouched", settings_text() == settings_before, json!(null));
    drop(tui);
    // 新建页默认值：改 settings.json 的 model.name / defaultApprovalMode，launch_defaults 现读跟上。
    let before = gemini.launch_defaults(&env, Some(&sb.cwd));
    let mut edited: Value = serde_json::from_str(&settings_text()).unwrap();
    edited["model"]["name"] = json!("gemini-2.5-pro");
    edited["general"]["defaultApprovalMode"] = json!("plan");
    std::fs::write(gemini_dir.join("settings.json"), serde_json::to_string_pretty(&edited).unwrap()).unwrap();
    let after = gemini.launch_defaults(&env, Some(&sb.cwd));
    sb.check(
        "G8.launch_defaults_follow_settings",
        after.model.as_deref() == Some("gemini-2.5-pro") && after.choices.get("approval").map(String::as_str) == Some("plan"),
        json!({"before": {"model": before.model, "choices": before.choices}, "after": {"model": after.model, "choices": after.choices}}),
    );
    // 多账号：env 里给另一个 GEMINI_CLI_HOME（profile 的隔离目录），清单与默认值是否跟着 env 走。
    let other = sb.root.join("other-profile");
    std::fs::create_dir_all(other.join(".gemini")).unwrap();
    std::fs::write(
        other.join(".gemini/settings.json"),
        r#"{"model":{"name":"gemini-3-flash-preview"},"general":{"defaultApprovalMode":"auto_edit"},"security":{"auth":{"selectedType":"oauth-personal"}}}"#,
    )
    .unwrap();
    let mut other_env = env.clone();
    other_env.retain(|(k, _)| k != "GEMINI_CLI_HOME");
    other_env.push(("GEMINI_CLI_HOME".into(), other.to_string_lossy().into_owned()));
    let profile_defaults = gemini.launch_defaults(&other_env, Some(&sb.cwd));
    let profile_catalog = crate::session_settings::catalog_for(gemini, &other_env, Some(&sb.cwd));
    // Meowo 不给 gemini 建多账号（profile::env_of 对 gemini 不注入任何变量），env 里不会有别的
    // GEMINI_CLI_HOME；插件读进程自己的数据目录，这里只记录，不算失败。
    sb.note(
        "g8_profile_env_ignored_info",
        json!({"profile_defaults": {"model": profile_defaults.model, "choices": profile_defaults.choices}, "profile_catalog_len": profile_catalog.len(), "default_catalog_len": catalog.len()}),
    );

    // ---- G7c 新建会话带模式与模型：--yolo + -m。
    let picks = selections(&[("model", "gemini-3.8-flash"), ("approval", "yolo")]);
    let argv = fresh_argv("gemini", &picks, &env, &sb.cwd);
    let tui = Tui::spawn(&argv, &sb.cwd, &env, 140, 40);
    tui.wait("G7c", 60, gemini_ready);
    tui.settle(2, 15);
    let seen = screen_value(&tui.lines());
    sb.save("g7-2-new-yolo.txt", &tui.screen());
    sb.check(
        "G7.new_session_with_mode_args",
        seen == Some("yolo") && tui.screen().contains("gemini-3.8-flash"),
        json!({"argv": argv, "screen_marker": seen}),
    );
    drop(tui);

    let failed: Vec<_> = sb.checks.iter().filter(|c| c["ok"] == false).cloned().collect();
    eprintln!("[tms] gemini 证据：{}", sb.evidence.display());
    assert!(failed.is_empty(), "{failed:#?}");
}
