//! 真机探针：kimi / gemini 原生 TUI 的 Lead 关卡自动续跑（任务 .lead/20261113-lead-tui，C 路）。
//!
//! 每个探针在隔离目录里（临时 HOME/KIMI_CODE_HOME/GEMINI_CLI_HOME、独立 board.db 与登记表）用 Lead 安装
//! 的真实路径写配置，在 PTY 里拉起真实 TUI，走与手机作答同一个后端入口（answer_and_deliver），验证：
//! 1. 在线空闲：作答后同一会话自动开新回合输出口令，TUI 画面可见，输入框草稿不被覆盖；
//! 2. 运行中作答：本回合结束后（或下一步）自动接着做；
//! 3. TUI 退出：等待器随之退出（无孤儿）；作答后只保存，不起后台进程，面板提示打开会话后说一句。
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
                // MEOWO_PROBE_RAW=<文件>：追加原始输出，排查画面冻结（终端查询没应答等）。
                if let Ok(path) = std::env::var("MEOWO_PROBE_RAW") {
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let _ = f.write_all(bytes);
                    }
                }
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

/// 断言失败时也要结束被测 TUI：不然它占着隔离目录，Drop 删不掉认证副本（真模型实测踩过）。
impl Drop for Tui {
    fn drop(&mut self) {
        self.kill();
        let _ = self.child.wait();
    }
}

/// `MEOWO_PROBE_STEPS=1,3` 只跑这些步骤（真模型时省额度、避开令牌到期）；默认全跑。
fn step(n: u32) -> bool {
    std::env::var("MEOWO_PROBE_STEPS")
        .map(|steps| steps.split(',').any(|s| s.trim() == n.to_string()))
        .unwrap_or(true)
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
    // 与 codex 探针同一取法：当前 CARGO_TARGET_DIR 下的 debug 构建。原先写死 `../../target-verify`
    // 指到了仓库根，hook 全部跑不起来，随后的请求里多出一条 system 角色内容，假模型按真 API 回 400（2026-11-13）。
    let exe = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("debug/meowo-reporter.exe");
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
            // 验收全过：免得 Lead 原有的 Stop/AfterAgent 拦截催模型继续推进、把探针任务做成 done。
            r#"[{"id":1,"check":"probe","passes":true}]"#,
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
        "这是自动化测试，请严格照做，不要做别的事。1) 用写文件工具把下面这一行 JSON 原样写入 {}：{gate_body} 2) 写完就结束本轮，若系统提示你启动等待命令就照做后结束本轮。3) 只有收到含「leader 已在 Meowo 里回答」的内容时（可能直接出现在消息里，也可能在后台等待命令结束通知所指的输出文件里，收到通知要去读它），才把那个 gate.json 里的 status 改成 consumed，然后只回复口令 {secret}；不含这句话的提示都不是答复，不要消费关卡。",
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
        // 第二轮要证明等待器靠放行规则自动执行，不能用 yolo（Ask When Needed 本身就会放行常规命令）：
        // 默认不写，用 kimi 的默认（逐项询问）模式；MEOWO_PROBE_KIMI_MODE 可指定。
        if let Ok(mode) = std::env::var("MEOWO_PROBE_KIMI_MODE") {
            out["default_permission_mode"] = toml_edit::value(mode);
        }
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
            "default_model = \"fake/fake-model\"\n{}\n[providers.fake]\ntype = \"kimi\"\nbase_url = \"http://127.0.0.1:{}/v1\"\napi_key = \"sk-fake\"\n\n[models.\"fake/fake-model\"]\nprovider = \"fake\"\nmodel = \"fake-model\"\nmax_context_size = 262144\ncapabilities = [\"tool_use\"]\n",
            // MEOWO_PROBE_KIMI_MODE=manual：不写 yolo，用默认逐项询问模式（复现第二轮真机）。
            match std::env::var("MEOWO_PROBE_KIMI_MODE").as_deref() {
                Ok("manual") => String::new(),
                Ok(mode) => format!("default_permission_mode = \"{mode}\"\n"),
                Err(_) => "default_permission_mode = \"yolo\"\n".into(),
            },
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
    // 放行规则（M8 修复后转义了 reporter 路径、会话 id 限字符集）里有等待器命令。
    assert!(
        config_after.contains("lead wait --provider kimi --session "),
        "{config_after}"
    );
    let mut report = serde_json::Map::new();

    // ---- 1. 在线空闲：作答后自动开新回合，草稿保留
    let gate_path = sb.task.join("gate.json").to_string_lossy().into_owned();
    let mut tui = Tui::spawn(&kimi_argv(&[]), &sb.cwd, &env);
    pass_trust(&tui);
    let mut native = String::new();
    let mut sid = 0;
    if step(1) {
        let s1 = secret();
        sb.plan(json!({"phase":"Q-1","gate_path":gate_path,"gate_body":sb.gate_body("Q-1","pending"),"consume":true,"consumed_body":sb.gate_body("Q-1","consumed"),"secret":s1}));
        let first = if real() {
            real_prompt(&sb.task, &sb.gate_body("Q-1", "pending"), &s1)
        } else {
            "OPEN-GATE".into()
        };
        submit(&tui, &first);
        let mut approvals = Vec::new();
        let waiter = wait_waiter_approving(
            &sb,
            &tui,
            "Q-1",
            if real() { 240 } else { 60 },
            kimi_approval,
            &mut approvals,
        );
        sb.save("kimi-1-waiting.txt", &tui.screen());
        sb.save(
            "kimi-1-approvals.json",
            &serde_json::to_string_pretty(&approvals).unwrap(),
        );
        let waiter = waiter.expect("关卡挂起后会话里应有等待器");
        native = sb.owner();
        assert!(
            native.starts_with("session_"),
            "kimi hook 会话 id 应带 session_ 前缀：{native}"
        );
        sid = sb.register("kimi", &native);
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
            // 真模型：提示词里也有口令且会滚出屏幕，以「关卡已被消费且画面有口令」为准。
            |s| {
                if real() {
                    s.contains(&s1) && sb.gate()["status"] == "consumed"
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
    }

    // ---- 2. 运行中作答
    if step(2) && !real() {
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

    // ---- 3. TUI 退出：等待器自退；离线作答只保存
    let s3 = secret();
    let gate3 = "Q-3";
    sb.plan(json!({"phase":"Q-3","gate_path":gate_path,"gate_body":sb.gate_body(gate3,"pending"),"secret":s3}));
    let prompt3 = if real() {
        real_prompt(&sb.task, &sb.gate_body(gate3, "pending"), &s3)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &prompt3);
    let waiter = sb
        .wait_waiter(gate3, if real() { 240 } else { 60 })
        .expect("第三个关卡的等待器");
    if native.is_empty() {
        native = sb.owner();
        sid = sb.register("kimi", &native);
    }
    assert!(tui.settle(4, 120), "回合应结束");
    submit(&tui, "/exit");
    assert!(wait_until(20, || !tui.alive()), "TUI 应退出");
    let orphan_gone = wait_until(15, || {
        meowo_reporter::lead_waiter::process_started(waiter.pid).is_none()
    });
    assert!(orphan_gone, "TUI 退出后等待器不能留成孤儿");
    assert_eq!(sb.answer(gate3), Ok(true));
    // TUI 不在：只保存，不起任何后台进程（leader 2026-11-13「不需要离线」），面板提示打开会话后说一句。
    let status =
        crate::lead_conversation::delivery_status(&sb.db, &sb.cwd.to_string_lossy(), gate3);
    let leftovers: Vec<String> = std::fs::read_dir(&sb.task)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".native-offline"))
        .collect();
    tui.kill();
    assert_eq!(
        status.as_ref().map(|s| s.state.as_str()),
        Some("native_offline"),
        "{status:?}"
    );
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(sb.gate()["status"], "answered", "TUI 不在时没人消费关卡");
    report.insert(
        "offline".into(),
        json!({"orphan_waiter_exited":true,"status_after":status.map(|s| s.state),"session":sid}),
    );
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
        // API key 只进子进程环境，不落盘到隔离目录。
        if selected == "gemini-api-key" {
            let dotenv = std::fs::read_to_string(source.join(".env")).unwrap_or_default();
            let key = dotenv
                .lines()
                .find_map(|line| line.trim().strip_prefix("GEMINI_API_KEY="))
                .map(|v| v.trim().trim_matches(['"', '\'']).to_owned())
                .or_else(|| std::env::var("GEMINI_API_KEY").ok())
                .expect("gemini-api-key 认证但找不到 GEMINI_API_KEY");
            env.push(("GEMINI_API_KEY".into(), key));
        }
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

/// 等 gemini 会话里的等待器登记。真模型可能先激活 lead skill（会弹「Activate Skill」确认，与本功能
/// 无关，探针代为允许一次）；等待器自己的 shell 命令若弹审批，说明放行策略没生效，直接判失败。
fn gemini_wait_waiter(
    sb: &Sandbox,
    tui: &Tui,
    gate: &str,
    secs: u64,
) -> Option<meowo_reporter::lead_waiter::WaiterRecord> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Some(record) = meowo_reporter::lead_waiter::live_waiter(&sb.task, gate) {
            return Some(record);
        }
        let screen = tui.screen();
        assert!(
            !(screen.contains("lead wait") && screen.contains("Allow execution")),
            "等待器的 shell 命令不该弹审批：
{screen}"
        );
        if screen.contains("Activate Skill") && screen.contains("Allow once") {
            tui.send(
                "
",
            );
            std::thread::sleep(Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
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
    let mut tui = Tui::spawn(&argv, &sb.cwd, &env);
    assert!(tui.wait("gemini 输入框", 90, gemini_ready));
    let mut native = String::new();
    if step(1) {
        let s1 = secret();
        sb.plan(json!({"phase":"Q-1","gate_path":gate_path,"gate_body":sb.gate_body("Q-1","pending"),"consume":true,"consumed_body":sb.gate_body("Q-1","consumed"),"secret":s1}));
        let first = if real() {
            real_prompt(&sb.task, &sb.gate_body("Q-1", "pending"), &s1)
        } else {
            "OPEN-GATE".into()
        };
        submit(&tui, &first);
        let waiter = gemini_wait_waiter(&sb, &tui, "Q-1", if real() { 240 } else { 60 });
        sb.save("gemini-1-waiting.txt", &tui.screen());
        let waiter = waiter.expect("AfterTool 尾调用应在后台启动等待器（不需要审批）");
        native = sb.owner();
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
            |s| s.contains(&s1) && (!real() || sb.gate()["status"] == "consumed"),
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
    }

    // ---- 2. 运行中作答（插话语义下等待器的注入在下一步或回合后提交）
    if step(2) && !real() {
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

    // ---- 3. 退出 → 等待器自退 → 离线作答只保存
    let s3 = secret();
    let gate3 = "Q-3";
    sb.plan(json!({"phase":"Q-3","gate_path":gate_path,"gate_body":sb.gate_body(gate3,"pending"),"secret":s3}));
    let prompt3 = if real() {
        real_prompt(&sb.task, &sb.gate_body("Q-3", "pending"), &s3)
    } else {
        "OPEN-GATE".into()
    };
    submit(&tui, &prompt3);
    let waiter = gemini_wait_waiter(&sb, &tui, "Q-3", if real() { 240 } else { 60 });
    sb.save("gemini-3-waiting.txt", &tui.screen());
    let waiter = waiter.expect("第三个关卡的等待器");
    if native.is_empty() {
        native = sb.owner();
        sb.register("gemini", &native);
    }
    assert!(tui.settle(4, 120), "回合应结束");
    submit(&tui, "/quit");
    // 第二轮 P4：后台挂着等待器时 /quit 曾 20s 退不掉。记下退出耗时（等待器随启动它的 shell 被杀而退出）。
    let quit_at = Instant::now();
    let quit = wait_until(30, || !tui.alive());
    let quit_ms = quit_at.elapsed().as_millis() as u64;
    sb.save("gemini-3-quit.txt", &format!("quit_ms={quit_ms} exited={quit}\n{}", tui.screen()));
    report.insert("quit".into(), json!({"quit_ms": quit_ms, "exited": quit}));
    assert!(quit, "TUI 应退出");
    assert!(
        wait_until(15, || meowo_reporter::lead_waiter::process_started(
            waiter.pid
        )
        .is_none()),
        "TUI 退出后等待器不能留成孤儿"
    );
    assert_eq!(sb.answer(gate3), Ok(true));
    // TUI 不在：只保存，不起任何后台进程（leader 2026-11-13「不需要离线」），面板提示打开会话后说一句。
    let status =
        crate::lead_conversation::delivery_status(&sb.db, &sb.cwd.to_string_lossy(), gate3);
    let leftovers: Vec<String> = std::fs::read_dir(&sb.task)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".native-offline"))
        .collect();
    tui.kill();
    assert_eq!(
        status.as_ref().map(|s| s.state.as_str()),
        Some("native_offline"),
        "{status:?}"
    );
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(sb.gate()["status"], "answered", "TUI 不在时没人消费关卡");
    report.insert(
        "offline".into(),
        json!({"orphan_waiter_exited":true,"status_after":status.map(|s| s.state)}),
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

// ---------------------------------------------------------------- 第二轮复跑（任务 20261113-lead-tui）

/// 第二轮复跑的步骤开关：`MEOWO_PROBE_R2=A,C,D,E`（默认 A,C,E）。
fn r2(name: &str) -> bool {
    std::env::var("MEOWO_PROBE_R2")
        .unwrap_or_else(|_| "A,C,E".into())
        .split(',')
        .any(|s| s.trim() == name)
}

fn kimi_approval(screen: &str) -> bool {
    let s = screen.to_lowercase();
    s.contains("↵ confirm")
        && (s.contains("run this command?")
            || s.contains("write this file?")
            || s.contains("apply these edits?")
            || s.contains("approve"))
}

fn gemini_approval(screen: &str) -> bool {
    let s = screen.to_lowercase();
    s.contains("allow once")
        || s.contains("allow execution")
        || s.contains("allow for this session")
}

fn snap_procs() -> std::collections::BTreeMap<u32, crate::lead_native::probe_util::Proc> {
    crate::lead_native::probe_util::snapshot()
}

/// 等关卡的等待器登记。期间出现的审批：等待器自己的命令（含 `lead wait`）弹审批 = 放行规则没生效，
/// 记下并判失败；其它审批（模型写 gate.json 等）代为批准一次并记录，与本功能无关。
fn wait_waiter_approving(
    sb: &Sandbox,
    tui: &Tui,
    gate: &str,
    secs: u64,
    is_approval: fn(&str) -> bool,
    approvals: &mut Vec<Value>,
) -> Option<meowo_reporter::lead_waiter::WaiterRecord> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Some(record) = meowo_reporter::lead_waiter::live_waiter(&sb.task, gate) {
            if record.gate == gate && sb.gate()["id"] == gate && sb.gate()["status"] == "pending" {
                return Some(record);
            }
        }
        let screen = tui.screen();
        if is_approval(&screen) {
            // 只看审批框本身（最后一条分隔线之后）：上方滚动区里模型复述过等待命令，整屏匹配会把
            // 「写 gate.json 要批」误判成等待器命令要批（第三轮 kimi D 实拍）。
            let boxed = screen
                .rfind("▶")
                .map_or(screen.as_str(), |i| &screen[screen[..i].rfind("────").unwrap_or(0)..]);
            let waiter = boxed.contains("lead wait") || boxed.contains("lead_wait");
            approvals.push(json!({"gate": gate, "waiterCommand": waiter, "screen": screen}));
            if waiter {
                sb.save(&format!("waiter-prompted-{gate}.txt"), &screen);
                return None;
            }
            tui.send("\r");
            std::thread::sleep(Duration::from_secs(2));
        } else if screen.contains("Activate Skill") && screen.contains("Allow once") {
            tui.send("\r");
            std::thread::sleep(Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
}

/// 等画面满足条件；期间出现的审批（模型消费关卡要改 gate.json 等，默认询问模式下都要批）代批并记录。
fn wait_approving(
    tui: &Tui,
    secs: u64,
    is_approval: fn(&str) -> bool,
    approvals: &mut Vec<Value>,
    what: &str,
    pred: impl Fn(&str) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let screen = tui.screen();
        if pred(&screen) {
            return true;
        }
        if is_approval(&screen) {
            approvals.push(json!({"during": what, "screen": screen}));
            tui.send("\r");
            std::thread::sleep(Duration::from_secs(2));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    eprintln!("[probe] 等不到 {what}，末屏：\n{}", tui.screen());
    false
}

/// 画面上是否挂着审批框（各家文案不同）。
type ApprovalCheck = fn(&str) -> bool;

/// 真模型第二轮（kimi / gemini 共用流程）：A 在线空闲 + B 草稿，C 待批审批，D 子代理，E TUI 不在 + 重开说继续。
/// 等待器是在哪个 agent 的 wire 里以后台任务启动的（按 task.started 的 pid 与等待器父链对）。
fn launcher_of(sb: &Sandbox, waiter_pid: u32) -> Value {
    let mut chain = vec![waiter_pid];
    let sys = sysinfo::System::new_all();
    let mut cur = sysinfo::Pid::from_u32(waiter_pid);
    for _ in 0..8 {
        match sys.process(cur).and_then(|p| p.parent()) {
            Some(parent) => {
                chain.push(parent.as_u32());
                cur = parent;
            }
            None => break,
        }
    }
    json!(meowo_reporter::lead_kimi::launcher_in(&agent_wires(&sb.home), &chain).map(|sub| !sub))
}

/// 隔离 KIMI_CODE_HOME 下所有 `agents/<agentId>/wire.jsonl`：(agentId, 全文)。
fn agent_wires(home: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![home.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "wire.jsonl") {
                let agent = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((agent, text));
                }
            }
        }
    }
    out
}

fn r2_flow(provider: &str) {
    assert!(real(), "第二轮复跑只在真模型下有意义（MEOWO_PROBE_REAL=1）");
    let mut sb = Sandbox::new(provider);
    let (env, argv, is_approval): (Vec<(String, String)>, Vec<String>, ApprovalCheck) =
        if provider == "kimi" {
            let env = vec![
                ("KIMI_CODE_HOME", sb.home.to_string_lossy().into_owned()),
                ("KIMI_CODE_NO_AUTO_UPDATE", "1".into()),
                ("MEOWO_DB", sb.db.to_string_lossy().into_owned()),
            ];
            set_env(&env);
            kimi_config(&mut sb);
            install_lead("kimi", "kimi 2.6.0");
            let env: Vec<(String, String)> = env.into_iter().map(|(k, v)| (k.into(), v)).collect();
            (env, kimi_argv(&[]), kimi_approval)
        } else {
            let env = gemini_setup(&mut sb);
            for (key, value) in &env {
                std::env::set_var(key, value);
            }
            install_lead("gemini", "0.68.0");
            let argv = meowo_agent::by_id("gemini").unwrap().launch_argv();
            (env, argv, gemini_approval)
        };
    let config_text = if provider == "kimi" {
        std::fs::read_to_string(sb.home.join("config.toml")).unwrap_or_default()
    } else {
        std::fs::read_to_string(sb.home.join(".gemini/settings.json")).unwrap_or_default()
    };
    let mut report = serde_json::Map::new();
    report.insert(
        "permissionMode".into(),
        json!(config_text
            .lines()
            .find(|l| l.contains("default_permission_mode") || l.contains("defaultApprovalMode"))),
    );
    let mut approvals: Vec<Value> = Vec::new();
    let marker = sb.root.to_string_lossy().into_owned();
    let start = |tui: &Tui| {
        if provider == "kimi" {
            pass_trust(tui);
        } else {
            assert!(tui.wait("gemini 输入框", 90, gemini_ready));
        }
    };
    let mut tui = Tui::spawn(&argv, &sb.cwd, &env);
    start(&tui);
    let mut native = String::new();
    let wait_secs = 300;

    // 开一个关卡，等等待器起来；返回 (口令, 等待器 pid)。
    let open_gate = |tui: &Tui, gate: &str, approvals: &mut Vec<Value>, native: &mut String| {
        let secret = secret();
        submit(
            tui,
            &real_prompt(&sb.task, &sb.gate_body(gate, "pending"), &secret),
        );
        let waiter = wait_waiter_approving(&sb, tui, gate, wait_secs, is_approval, approvals);
        sb.save(&format!("{provider}-{gate}-waiting.txt"), &tui.screen());
        let waiter = waiter
            .unwrap_or_else(|| panic!("{gate}：等不到等待器（或等待器命令弹了审批），见证据"));
        if native.is_empty() {
            *native = sb.owner();
            sb.register(provider, native);
        }
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            if is_approval(&tui.screen()) {
                approvals.push(json!({"gate": gate, "afterWaiter": true, "screen": tui.screen()}));
                tui.send("\r");
                std::thread::sleep(Duration::from_secs(2));
            } else if tui.settle(5, 10) {
                break;
            }
        }
        (secret, waiter.pid)
    };

    // ---- A/B：在线空闲作答，草稿保留
    if r2("A") {
        let (s1, pid) = open_gate(&tui, "Q-1", &mut approvals, &mut native);
        tui.type_text("DRAFT-KEEP-1");
        std::thread::sleep(Duration::from_secs(1));
        let draft_before = tui.screen();
        sb.save(&format!("{provider}-A-draft-before.txt"), &draft_before);
        let answered_at = Instant::now();
        let r = sb.answer("Q-1");
        let mut samples = Vec::new();
        for _ in 0..10 {
            let record = meowo_reporter::lead_waiter::read_record(&sb.task);
            samples.push(json!({
                "ms": answered_at.elapsed().as_millis() as u64,
                "gate": sb.gate()["status"],
                "record": record.as_ref().map(|r| json!({"pid": r.pid, "gate": r.gate, "cli": r.cli_pid})),
                "waiterAlive": meowo_reporter::lead_waiter::process_started(pid).is_some(),
                "delivered": std::fs::read_to_string(sb.task.join(".delivered")).ok(),
            }));
            if sb.gate()["status"] == "consumed" {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let woke = wait_approving(
            &tui,
            wait_secs,
            is_approval,
            &mut approvals,
            "在线空闲自动续跑",
            |s| s.contains(&s1) && sb.gate()["status"] == "consumed",
        );
        let latency = answered_at.elapsed().as_millis() as u64;
        tui.settle(4, 120);
        let screen = tui.screen();
        sb.save(&format!("{provider}-A-woke.txt"), &screen);
        report.insert("A".into(), json!({"secret": s1, "answer": r, "waiter_pid": pid, "samples": samples,
            "woke": woke, "latency_ms": latency, "draft_kept": screen.contains("DRAFT-KEEP-1"), "draft_visible_before": draft_before.contains("DRAFT-KEEP-1"), "native": native}));
        sb.save(
            "report.json",
            &serde_json::to_string_pretty(&Value::Object(report.clone())).unwrap(),
        );
        for _ in 0.."DRAFT-KEEP-1".len() {
            tui.send("\x7f");
        }
        tui.settle(5, 60);
    }

    // ---- C：待批审批时作答。审批框不被答掉、答复不丢；拒绝审批后看去向。
    if r2("C") {
        let (s2, _) = open_gate(&tui, "Q-2", &mut approvals, &mut native);
        submit(
            &tui,
            "请用 shell 工具执行这条命令：echo PROBE_APPROVAL > approval.txt 。只做这一件事。",
        );
        let pending = tui.wait("审批框", 180, is_approval);
        let before = tui.screen();
        sb.save(&format!("{provider}-C-before.txt"), &before);
        let answered_at = Instant::now();
        let r = sb.answer("Q-2");
        std::thread::sleep(Duration::from_secs(8));
        let during = tui.screen();
        sb.save(&format!("{provider}-C-during.txt"), &during);
        let still = is_approval(&during);
        let early = during.contains(&s2) && sb.gate()["status"] == "consumed";
        let gate_during = sb.gate()["status"].clone();
        let file_before = sb.cwd.join("approval.txt").exists();
        // 手动拒绝审批：Esc。
        let decided = Instant::now();
        tui.send("\x1b");
        let after = wait_approving(
            &tui,
            wait_secs,
            is_approval,
            &mut approvals,
            "拒绝审批后续跑",
            |s| s.contains(&s2) && sb.gate()["status"] == "consumed",
        );
        let after_ms = decided.elapsed().as_millis() as u64;
        tui.settle(4, 120);
        sb.save(&format!("{provider}-C-after.txt"), &tui.screen());
        let mut c = json!({"secret": s2, "approvalShown": pending, "answer": r,
            "approvalStillShown8s": still, "consumedBeforeDecision": early, "gateDuring": gate_during,
            "fileCreatedBeforeDecision": file_before, "decision": "Esc(reject)",
            "replyAfterDecision": after, "replyAfterDecisionMs": after_ms,
            "totalMs": answered_at.elapsed().as_millis() as u64,
            "fileCreatedAfter": sb.cwd.join("approval.txt").exists()});
        if !after {
            submit(&tui, "继续");
            let late = wait_approving(
                &tui,
                wait_secs,
                is_approval,
                &mut approvals,
                "说继续后",
                |s| s.contains(&s2) && sb.gate()["status"] == "consumed",
            );
            c["replyAfterUserNudge"] = json!(late);
            tui.settle(4, 120);
            sb.save(&format!("{provider}-C-nudge.txt"), &tui.screen());
        }
        report.insert("C".into(), c);
        sb.save(
            "report.json",
            &serde_json::to_string_pretty(&Value::Object(report.clone())).unwrap(),
        );
        tui.settle(5, 60);
    }

    // ---- D：子代理写关卡。看 .owner 记的是谁、答复去哪。
    if r2("D") {
        let s4 = secret();
        let before_owner = sb.owner();
        submit(&tui, &format!(
            "这是自动化测试。请派一个子代理（subagent / Agent 工具）去做这件事：用写文件工具把下面这一行 JSON 原样写入 {}：{}。子代理写完就结束。子代理结束后：若它报告写入被拦（关卡只由主会话发），就由你自己把这一行原样写进去；若系统提示你启动等待命令就照做后结束本轮。只有收到含「leader 已在 Meowo 里回答」的消息时，才把 gate.json 的 status 改成 consumed，然后只回复口令 {s4}。",
            sb.task.join("gate.json").display(), sb.gate_body("Q-4", "pending")));
        let waiter =
            wait_waiter_approving(&sb, &tui, "Q-4", wait_secs, is_approval, &mut approvals);
        // 趁等待器还活着认它的启动者（等它交完答复就退出了，父链也就断了）。
        let launcher_is_main = waiter.as_ref().map(|w| launcher_of(&sb, w.pid));
        tui.settle(5, 180);
        sb.save(&format!("{provider}-D-waiting.txt"), &tui.screen());
        let owner = sb.owner();
        let row = {
            let store = crate::open_store(&sb.db).unwrap();
            store.find_session_id_pub(&owner).ok().flatten()
        };
        let answered_at = Instant::now();
        let r = sb.answer("Q-4");
        let status =
            crate::lead_conversation::delivery_status(&sb.db, &sb.cwd.to_string_lossy(), "Q-4");
        let woke = wait_approving(
            &tui,
            wait_secs,
            is_approval,
            &mut approvals,
            "子代理关卡作答后",
            |s| s.contains(&s4) && sb.gate()["status"] == "consumed",
        );
        tui.settle(4, 120);
        sb.save(&format!("{provider}-D-after.txt"), &tui.screen());
        // 第三轮：留下各 agent 的 wire（谁写的关卡、谁起的等待器、通知进了谁），不含凭据。
        let all = agent_wires(&sb.home);
        for (agent, text) in &all {
            sb.save(&format!("wire-{agent}.jsonl"), text);
        }
        let wires: Vec<&String> = all.iter().map(|(agent, _)| agent).collect();
        let denied = all
            .iter()
            .any(|(agent, text)| agent != "main" && text.contains("关卡只由主会话发"));
        report.insert(
            "D".into(),
            json!({"secret": s4, "ownerBefore": before_owner, "ownerAfter": owner, "wires": wires,
            "subagentDeniedInItsWire": denied, "waiterLauncherIsMain": launcher_is_main,
            "ownerIsMain": owner == native, "ownerRowInDb": row, "waiterStarted": waiter.is_some(),
            "answer": r, "status": status.map(|s| s.state), "woke": woke,
            "latency_ms": answered_at.elapsed().as_millis() as u64}),
        );
        sb.save(
            "report.json",
            &serde_json::to_string_pretty(&Value::Object(report.clone())).unwrap(),
        );
        tui.settle(5, 60);
    }

    // ---- E：TUI 不在时作答：只保存、不起进程、面板 native_offline；重开说一句继续能接上。
    if r2("E") {
        let (s3, waiter_pid) = open_gate(&tui, "Q-3", &mut approvals, &mut native);
        // 先等回合真正空闲（gemini 运行中输入会变成插话），再正常退出；退不掉就 Ctrl-C 两下，最后强杀
        // （等同用户直接关掉 Meowo 里的终端标签）。
        tui.settle(5, 120);
        submit(&tui, if provider == "kimi" { "/exit" } else { "/quit" });
        let mut quit_by = "command";
        if !wait_until(20, || !tui.alive()) {
            quit_by = "ctrl-c";
            tui.send("\x03");
            std::thread::sleep(Duration::from_millis(500));
            tui.send("\x03");
            if !wait_until(15, || !tui.alive()) {
                quit_by = "kill";
                tui.kill();
            }
        }
        assert!(wait_until(10, || !tui.alive()), "TUI 应退出");
        let orphan_gone = wait_until(15, || {
            meowo_reporter::lead_waiter::process_started(waiter_pid).is_none()
        });
        tui.kill();
        let before = snap_procs();
        let answered_at = Instant::now();
        let r = sb.answer("Q-3");
        let answer_ms = answered_at.elapsed().as_millis() as u64;
        let status =
            crate::lead_conversation::delivery_status(&sb.db, &sb.cwd.to_string_lossy(), "Q-3");
        std::thread::sleep(Duration::from_secs(5));
        let after = snap_procs();
        let spawned = crate::lead_native::probe_util::new_related(&before, &after, &marker);
        let gate_closed = sb.gate()["status"].clone();
        // 重开同一会话，说一句继续。
        let reopen: Vec<String> = if provider == "kimi" {
            kimi_argv(&["-S", &native])
        } else {
            let mut a = argv.clone();
            a.extend(["--resume".to_owned(), native.clone()]);
            a
        };
        let tui2 = Tui::spawn(&reopen, &sb.cwd, &env);
        if provider == "kimi" {
            tui2.wait("kimi 恢复会话", 60, |s| {
                s.contains("Trust this folder") || s.contains("context:")
            });
            if tui2.screen().contains("Trust this folder") {
                tui2.send("\r");
            }
            assert!(tui2.wait("kimi 恢复会话输入框", 60, |s| s.contains("context:")));
        } else {
            start(&tui2);
        }
        std::thread::sleep(Duration::from_secs(2));
        let said = Instant::now();
        submit(&tui2, "继续");
        let resumed = wait_approving(
            &tui2,
            wait_secs,
            is_approval,
            &mut approvals,
            "重开说继续",
            |s| s.contains(&s3) && sb.gate()["status"] == "consumed",
        );
        let resume_ms = said.elapsed().as_millis() as u64;
        tui2.settle(4, 120);
        sb.save(&format!("{provider}-E-reopened.txt"), &tui2.screen());
        report.insert(
            "E".into(),
            json!({"secret": s3, "orphanWaiterExited": orphan_gone,
            "answer": r, "answerMs": answer_ms, "statusWhileClosed": status.map(|s| s.state),
            "gateWhileClosed": gate_closed, "newProcessesWhileClosed": spawned,
            "procSnapshotOk": !after.is_empty(), "resumedAfterContinue": resumed, "quitBy": quit_by,
            "resumeMs": resume_ms, "reopenArgv": reopen[1..].to_vec()}),
        );
        drop(tui2);
    }
    report.insert("approvalsSeen".into(), json!(approvals));
    sb.save(
        "report.json",
        &serde_json::to_string_pretty(&Value::Object(report)).unwrap(),
    );
    eprintln!("[probe] {provider} 第二轮证据：{}", sb.evidence.display());
}

#[test]
#[ignore = "第二轮：真实 kimi TUI + 真模型（MEOWO_PROBE_REAL=1）；手动复跑"]
fn probe_kimi_round2() {
    r2_flow("kimi");
}

#[test]
#[ignore = "第二轮：真实 gemini TUI + 真模型（MEOWO_PROBE_REAL=1）；手动复跑"]
fn probe_gemini_round2() {
    r2_flow("gemini");
}
