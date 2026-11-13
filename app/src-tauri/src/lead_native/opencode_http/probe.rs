//! OpenCode 原生 TUI 的 Lead 续跑真机探针（任务 20261113-lead-tui，B 路）。默认不运行：会调用真实模型。
//!
//! ```text
//! set MEOWO_RUN_OPENCODE_TUI_PROBE=1
//! cargo test -p meowo-app --lib opencode_tui_lead_continuation_probe -- --ignored --exact --test-threads=1 --nocapture
//! ```
//! 全程隔离：HOME/USERPROFILE/XDG_*/OPENCODE_CONFIG_DIR/MEOWO_DB 都指向一次性目录，不读写用户的
//! ~/.config/opencode、~/.local/share/opencode 与 ~/.meowo。模型默认用 OpenCode 自带的免费模型
//! （`MEOWO_PROBE_OPENCODE_MODEL` 可换），此时不复制任何凭据；需要付费 provider 时设
//! `MEOWO_PROBE_OPENCODE_AUTH_PROVIDER=<id>`，只复制 auth.json 里这一个条目，结束即删。
//! 证据（屏幕文本、送达状态、口令）留在 `target/lead-provider-probes/opencode-tui/<随机>/evidence`。
//!
//! 覆盖：在线 O1（运行中作答 → TUI 显示 QUEUED 排队 → 同一会话新回合输出口令；输入框草稿保留；
//! 关卡被消费）、TUI 不在时作答（只保存、不起服务；重新打开会话后巡检经 prompt_async 送达，这一轮可见）。
//! 答复一律经 `answer_and_deliver`，与手机作答同一后端路径。

use super::*;
use meowo_protocol::ipc::{LeadGateAnswerDto, LeadProviderInstallationDto};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::sync::Arc;
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
    fn spawn(argv: &[String], plan: Option<&LaunchPlan>, cwd: &Path) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        if let Some(plan) = plan {
            command.args(&plan.args);
            for (k, v) in &plan.env {
                command.env(k, v);
            }
        }
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        drop(pair.slave);
        let pid = child.process_id();
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(|e| e.to_string())?,
        ));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let (answer, parser) = (writer.clone(), screen.clone());
        std::thread::spawn(move || {
            let mut bytes = [0; 16_384];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                let chunk = &bytes[..count];
                // OpenConsole 启动握手：生产里 CPR 由后端、DA1 由 xterm 应答（probe_opencode_input 同款）。
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
        Ok(Self {
            child,
            _master: pair.master,
            writer,
            screen,
            pid,
        })
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
    fn wait(&self, timeout: Duration, want: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if want(&self.text()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }
    fn kill(mut self) {
        if let Some(pid) = self.pid {
            crate::proc::kill_descendants(pid);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

fn token() -> String {
    crate::remote::generate_token().unwrap()[..10].to_uppercase()
}

fn write_gate(task: &Path, native: &str, gate_id: &str) {
    std::fs::create_dir_all(task).unwrap();
    for (name, body) in [
        (".owner", native.to_owned()),
        ("SPEC.md", "Isolated Meowo Lead continuation probe. Only touch files inside this task directory.".into()),
        ("progress.md", "phase: blocked\n".into()),
        ("acceptance.json", "[]".into()),
        ("gate.json", json!({"id":gate_id,"type":"Q","status":"pending","title":"TUI continuation probe","summary":"Follow the leader note exactly.","options":[{"id":"go","label":"Continue"}]}).to_string()),
    ] {
        std::fs::write(task.join(name), body).unwrap();
    }
}

fn assistant_text(client: &Client, native: &str) -> String {
    client
        .get(
            &format!("/session/{native}/message"),
            Duration::from_secs(10),
        )
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|m| m["info"]["role"] == "assistant")
        .flat_map(|m| m["parts"].as_array().cloned().unwrap_or_default())
        .filter_map(|p| p["text"].as_str().map(str::to_owned))
        .collect::<Vec<_>>()
        .join("\n")
}

fn wait_idle(client: &Client, native: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    std::thread::sleep(Duration::from_secs(2));
    while Instant::now() < deadline {
        if client
            .get("/session/status", Duration::from_secs(3))
            .is_ok_and(|s| s.get(native).is_none())
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

/// 口令单独成一行才算模型的输出——答复消息正文里本来就含口令，`contains` 会把用户消息误判成回复。
fn reply_line(screen: &str, nonce: &str) -> bool {
    // 右侧栏与正文同行（实测 "ONLINE… <空格> Claude, GPT…"），只看正文起首；用户消息行以 ┃ 开头，不会误命中。
    screen
        .lines()
        .any(|line| line.split_whitespace().next() == Some(nonce))
}

fn remove_dir_retry(dir: &Path) {
    // 刚结束的 opencode 进程句柄要过一会儿才释放（首跑实测删除失败、稍后即可删）。
    for _ in 0..20 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn gate_status(task: &Path) -> String {
    meowo_reporter::lead::read_gate(task)
        .map(|g| g.status)
        .unwrap_or_default()
}

fn tui_endpoint(db: &Path, pid: u32) -> Endpoint {
    Registry::load(&registry_path(db))
        .endpoints
        .into_iter()
        .find(|e| e.pid == pid)
        .expect("TUI 启动后应登记端口与密码")
}

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

fn session_row(db: &Path, native: &str) -> Value {
    let Ok(store) = crate::open_store(db) else {
        return Value::Null;
    };
    match store.find_session_id_pub(native).ok().flatten() {
        Some(id) => json!({"id": id, "pid": store.session_pid(id).ok().flatten(),
                           "provider": store.session_provider(id).ok()}),
        None => Value::Null,
    }
}

/// 某会话里含口令的用户消息条数（答复投进了哪个会话）。
fn user_says(client: &Client, session: &str, nonce: &str) -> usize {
    client
        .get(
            &format!("/session/{session}/message"),
            Duration::from_secs(10),
        )
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|m| m["info"]["role"] == "user" && m.to_string().contains(nonce))
        .count()
}

fn permission_shown(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("permission required")
        || lower.contains("allow once")
        || lower.contains("allow always")
}

fn clear_input(tui: &Tui, text: &str) {
    for _ in 0..text.chars().count() + 2 {
        tui.send("\x7f");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "真机：启动真实 OpenCode TUI 并调用模型；显式 MEOWO_RUN_OPENCODE_TUI_PROBE=1"]
fn opencode_tui_lead_continuation_probe() {
    assert_eq!(
        std::env::var("MEOWO_RUN_OPENCODE_TUI_PROBE").as_deref(),
        Ok("1"),
        "会调用真实模型，需显式开启"
    );
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.iter().any(|a| a == "--test-threads=1"),
        "探针改进程环境变量，必须单线程独占运行"
    );
    use_bundled_conpty();
    // 下面会摘掉进程里所有 MEOWO_ 变量（防探针认领外层会话）：探针自己的开关先读出来。
    let keep_dirs = std::env::var("MEOWO_PROBE_KEEP").as_deref() == Ok("1");
    let with_plugin = std::env::var("MEOWO_PROBE_OC_PLUGIN").as_deref() != Ok("0");
    let reporter = reporter_path();
    let model = std::env::var("MEOWO_PROBE_OPENCODE_MODEL")
        .unwrap_or_else(|_| "opencode/deepseek-v4.1-flash-free".into());
    let source_home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap();
    let source_data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_home.join(".local/share"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lead-provider-probes/opencode-tui")
        .join(token().to_lowercase());
    let home = root.join("private-home");
    let cwd = root.join("cwd");
    let evidence = root.join("evidence");
    for dir in [&home, &cwd, &evidence] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let root = dunce::canonicalize(&root).unwrap();
    let (home, cwd, evidence) = (
        root.join("private-home"),
        root.join("cwd"),
        root.join("evidence"),
    );
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
    // bash 只放 echo 为 ask（C 步要一个挂着的审批，探针最后拒绝它），其余命令一律拒绝：
    // 全设 ask 时模型消费关卡会顺手 ls，审批挂住回合（第二轮首跑实测）。
    let config = json!({"$schema":"https://opencode.ai/config.json","model":model,"small_model":model,"autoupdate":false,"share":"disabled","permission":{"edit":"allow","read":"allow","bash":{"*":"deny","echo *":"ask"},"webfetch":"deny"}});
    std::fs::write(
        home.join("opencode-config/opencode.json"),
        config.to_string(),
    )
    .unwrap();
    let mut copied = vec![];
    if let Ok(provider) = std::env::var("MEOWO_PROBE_OPENCODE_AUTH_PROVIDER") {
        let auth: Value =
            serde_json::from_slice(&std::fs::read(source_data.join("opencode/auth.json")).unwrap())
                .unwrap();
        let entry = auth
            .get(&provider)
            .expect("auth.json 里没有指定 provider")
            .clone();
        std::fs::create_dir_all(home.join("xdg-data/opencode")).unwrap();
        std::fs::write(
            home.join("xdg-data/opencode/auth.json"),
            json!({provider.clone(): entry}).to_string(),
        )
        .unwrap();
        copied.push(format!("auth.json[{provider}]"));
    }
    let mut log = serde_json::Map::new();
    let mut git = std::process::Command::new("git");
    git.args(["init", "--quiet"]).arg(&cwd);
    assert!(git.status().is_ok_and(|s| s.success()));

    // 装 Meowo 通用上报插件（与生产同一接线）：核实子会话会不会单独入库、有没有进程号（redteam-2 遗留）。
    let meowo_dir = root.join("meowo");
    std::fs::create_dir_all(&meowo_dir).unwrap();
    // MEOWO_PROBE_OC_PLUGIN=0 不装（排查用）。
    if with_plugin {
        let wired =
            meowo_agent::by_id("opencode")
                .unwrap()
                .wire(&meowo_agent::wiring::WiringContext {
                    cli_version: None,
                    fallback_reporter: Some(&reporter),
                    meowo_dir: &meowo_dir,
                });
        log.insert("reporterPluginWired".into(), json!(wired.is_none()));
    }
    crate::lead_runtime::update_installation(&LeadProviderInstallationDto {
        provider: "opencode".into(),
        profile_id: None,
        profile_name: None,
        name: "OpenCode".into(),
        version: None,
        configured: true,
        supported: true,
        managed: false,
        installed: true,
        assets_installed: true,
        hooks_installed: true,
        automatic_continue: true,
        reason: None,
    });
    let agent = meowo_agent::by_id("opencode").unwrap();
    let exe = agent.launch_argv()[0].clone();
    let version = std::process::Command::new(&exe)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();

    let mut tuis: Vec<Tui> = vec![];
    let save = |name: &str, text: &str| {
        let _ = std::fs::write(evidence.join(name), text);
    };
    let cwd_text = cwd.to_string_lossy().into_owned();

    let result = (|| -> Result<(), String> {
        // ---- 0. 建会话：经 Meowo 的启动计划起 TUI（带端口+密码），用它自己的服务建一个空会话。
        let plain = vec![exe.clone()];
        let plan = launch_plan("opencode", &plain, &[]).ok_or("launch plan missing")?;
        if !plan.args.contains(&"--port".to_string()) {
            return Err("新 TUI 应带 --port 启动".into());
        }
        let tui = Tui::spawn(&plain, Some(&plan), &cwd)?;
        plan.launched(tui.pid);
        let endpoint = tui_endpoint(&db, tui.pid.ok_or("pid")?);
        let client = Client::of(&endpoint);
        let deadline = Instant::now() + Duration::from_secs(40);
        while client.verify_identity(Duration::from_secs(1)).is_err() {
            if Instant::now() > deadline {
                return Err("TUI 内置服务未就绪".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        // 无密码必须被拒：不设密码时同机任何进程都能驱动会话。
        let unauth = ureq::get(&format!("http://127.0.0.1:{}/global/health", endpoint.port))
            .call()
            .err()
            .and_then(|e| match e {
                ureq::Error::Status(code, _) => Some(code),
                _ => None,
            });
        log.insert("unauthenticatedStatus".into(), json!(unauth));
        if unauth != Some(401) {
            return Err(format!("无密码请求未被拒：{unauth:?}"));
        }
        let created = Client::agent(Duration::from_secs(10))
            .post(&format!("{}/session", client.base))
            .set("authorization", &client.auth)
            .send_json(json!({}))
            .map_err(http_reason)?
            .into_json::<Value>()
            .map_err(|e| e.to_string())?;
        let native = created["id"].as_str().ok_or("session id")?.to_owned();
        tui.kill();
        log.insert("nativeSession".into(), json!(native));

        let store = crate::open_store(&db)?;
        let project = store
            .upsert_project_by_root(&cwd_text, "probe", now_ms())
            .map_err(|e| e.to_string())?;
        let sid = match store
            .find_session_id_pub(&native)
            .map_err(|e| e.to_string())?
        {
            Some(id) => id,
            None => {
                store
                    .start_session(project, &native, now_ms())
                    .map_err(|e| e.to_string())?
                    .0
            }
        };
        log.insert("mainRowFromPlugin".into(), session_row(&db, &native));
        store
            .set_session_provider(sid, "opencode")
            .map_err(|e| e.to_string())?;
        store
            .set_session_cwd(sid, &cwd_text, now_ms())
            .map_err(|e| e.to_string())?;

        // ---- 1. 在线 O1：恢复打开会话；先让模型跑一个长一点的回合，再在输入框里留草稿，然后作答。
        let resume = vec![exe.clone(), "--session".into(), native.clone()];
        let plan = launch_plan("opencode", &resume, &[]).ok_or("launch plan missing")?;
        let tui = Tui::spawn(&resume, Some(&plan), &cwd)?;
        plan.launched(tui.pid);
        let tui_pid = tui.pid.ok_or("pid")?;
        // 生产里由会话内的 Meowo 插件上报进程号；探针没装插件，直接记一笔（探针不传 AppHandle）。
        store
            .set_session_shared_pid(sid, i64::from(tui_pid), now_ms())
            .map_err(|e| e.to_string())?;
        let client = Client::of(&tui_endpoint(&db, tui_pid));
        tuis.push(tui);
        let tui = tuis.last().unwrap();
        if !tui.wait(Duration::from_secs(40), |s| s.contains("ctrl+p")) {
            return Err("TUI 首屏未出现".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        let warm = format!("WARM{}", token());
        tui.send(&format!("Write the integers from 1 to 400 separated by single spaces, nothing else, then on a final new line write {warm}"));
        std::thread::sleep(Duration::from_millis(500));
        tui.send("\r");
        if !tui.wait(Duration::from_secs(30), |s| s.contains("esc interrupt")) {
            return Err("暖场回合未开始".into());
        }
        let draft = format!("DRAFT{}", token());
        tui.send(&draft);
        if !tui.wait(Duration::from_secs(5), |s| s.contains(&draft)) {
            return Err("草稿未出现在输入框".into());
        }
        let task = cwd.join(".lead/tui-probe");
        let gate1 = format!("online-{}", token().to_lowercase());
        write_gate(&task, &native, &gate1);
        let nonce1 = format!("ONLINE{}", token());
        let started = Instant::now();
        let delivered = crate::lead_conversation::answer_and_deliver(
            &db,
            &cwd_text,
            &gate1,
            &LeadGateAnswerDto {
                choice: "go".into(),
                note: format!("After updating gate.json, reply with exactly {nonce1}"),
            },
            None,
        )?;
        let accepted_ms = started.elapsed().as_millis();
        let status1 = crate::lead_conversation::delivery_status(&db, &cwd_text, &gate1);
        let queued = tui.wait(Duration::from_secs(10), |s| s.contains("QUEUED"));
        save("online-queued.txt", &tui.text());
        let busy_during = client
            .get("/session/status", Duration::from_secs(3))
            .map(|s| s.get(&native).is_some())
            .unwrap_or(false);
        if !wait_idle(&client, &native, Duration::from_secs(240)) {
            return Err("在线回合未结束".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        let screen = tui.text();
        save("online-final.txt", &screen);
        let reply = assistant_text(&client, &native);
        let online = json!({
            "deliveredReturned": delivered,
            "acceptMs": accepted_ms,
            "deliveryState": status1.as_ref().map(|s| s.state.clone()),
            "sessionBusyWhenAnswered": busy_during,
            "queuedShownInTui": queued,
            "nonce": nonce1,
            "nonceInModelReply": reply.contains(&nonce1),
            "nonceOnTuiScreen": reply_line(&screen, &nonce1),
            "draftKept": screen.contains(&draft),
            "gateStatus": gate_status(&task),
        });
        log.insert("online".into(), online.clone());
        if !(delivered
            && queued
            && reply.contains(&nonce1)
            && reply_line(&screen, &nonce1)
            && screen.contains(&draft))
        {
            return Err(format!("在线验证未通过：{online}"));
        }
        // 再次巡检不得重复投递。
        crate::lead_conversation::reconcile_task(&db, &task, None);
        let users_after = client
            .get(
                &format!("/session/{native}/message"),
                Duration::from_secs(10),
            )
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|m| m["info"]["role"] == "user")
            .count();
        log.insert("userMessagesAfterOnline".into(), json!(users_after));
        clear_input(tui, &draft);
        std::thread::sleep(Duration::from_secs(1));
        let answer = |gate: &str, nonce: &str| {
            let started = Instant::now();
            let r = crate::lead_conversation::answer_and_deliver(
                &db,
                &cwd_text,
                gate,
                &LeadGateAnswerDto {
                    choice: "go".into(),
                    note: format!("After updating gate.json, reply with exactly {nonce}"),
                },
                None,
            );
            (r, started)
        };

        // ---- A-idle：模型空闲、停在等关卡时作答（草稿在输入框里）→ 同一会话立即开新回合。
        let draft_idle = format!("DRAFT{}", token());
        tui.send(&draft_idle);
        tui.wait(Duration::from_secs(5), |s| s.contains(&draft_idle));
        let gate_idle = format!("idle-{}", token().to_lowercase());
        write_gate(&task, &native, &gate_idle);
        let nonce_idle = format!("IDLE{}", token());
        let (r, started) = answer(&gate_idle, &nonce_idle);
        let accept_idle = started.elapsed().as_millis();
        let state_idle =
            crate::lead_conversation::delivery_status(&db, &cwd_text, &gate_idle).map(|s| s.state);
        let seen = tui.wait(Duration::from_secs(240), |s| reply_line(s, &nonce_idle));
        let seen_ms = started.elapsed().as_millis();
        wait_idle(&client, &native, Duration::from_secs(120));
        let screen = tui.text();
        save("idle-final.txt", &screen);
        log.insert(
            "A_idle".into(),
            json!({
                "answer": r, "acceptMs": accept_idle, "deliveryState": state_idle,
                "nonce": nonce_idle, "nonceOnTuiScreen": seen, "replyOnScreenMs": seen_ms,
                "draftKept": screen.contains(&draft_idle), "gateStatus": gate_status(&task),
            }),
        );
        clear_input(tui, &draft_idle);
        std::thread::sleep(Duration::from_secs(1));

        // ---- C 待批审批：模型挂着 bash 审批时作答。审批不能被答掉，答复排队；拒绝审批后看去向。
        tui.send(
            "Use the bash tool to run exactly this command: echo PROBE_APPROVAL . Do nothing else.",
        );
        std::thread::sleep(Duration::from_millis(500));
        tui.send("\r");
        let pending = tui.wait(Duration::from_secs(180), permission_shown);
        let before = tui.text();
        save("approval-before.txt", &before);
        let gate_c = format!("approval-{}", token().to_lowercase());
        write_gate(&task, &native, &gate_c);
        let nonce_c = format!("APPROVAL{}", token());
        let (r, started) = answer(&gate_c, &nonce_c);
        let accept_c = started.elapsed().as_millis();
        let state_c =
            crate::lead_conversation::delivery_status(&db, &cwd_text, &gate_c).map(|s| s.state);
        std::thread::sleep(Duration::from_secs(6));
        let during = tui.text();
        save("approval-during.txt", &during);
        let perms = client
            .get("/permission", Duration::from_secs(5))
            .unwrap_or(Value::Null);
        let still = permission_shown(&during);
        let early = reply_line(&during, &nonce_c);
        // 手动拒绝审批：Esc。
        let decided = Instant::now();
        tui.send("\x1b");
        let after_seen = tui.wait(Duration::from_secs(180), |s| reply_line(s, &nonce_c));
        let after_ms = decided.elapsed().as_millis();
        wait_idle(&client, &native, Duration::from_secs(120));
        let after = tui.text();
        save("approval-after.txt", &after);
        log.insert("C_approval".into(), json!({
            "permissionShown": pending, "answer": r, "acceptMs": accept_c,
            "deliveryState": state_c, "nonce": nonce_c, "permissionStillShown6s": still,
            "pendingPermissionsApi": perms, "replyBeforeDecision": early,
            "decision": "Esc(reject)", "replyAfterDecision": after_seen, "replyAfterDecisionMs": after_ms,
            "nonceInModelReply": assistant_text(&client, &native).contains(&nonce_c),
            "gateStatus": gate_status(&task),
        }));

        // ---- D 子代理：task 工具派子会话；.owner 指子会话时作答 → 应投主会话（主 TUI 可见），不进子会话。
        tui.send("Use the task tool to launch a general subagent whose prompt is: reply with exactly SUB_DONE. After it finishes, reply with exactly SPAWNED.");
        std::thread::sleep(Duration::from_millis(500));
        tui.send("\r");
        let child = (0..180).find_map(|_| {
            std::thread::sleep(Duration::from_secs(1));
            client
                .get("/session", Duration::from_secs(5))
                .ok()?
                .as_array()?
                .iter()
                .find(|s| s["parentID"].as_str() == Some(native.as_str()))
                .and_then(|s| s["id"].as_str().map(str::to_owned))
        });
        wait_idle(&client, &native, Duration::from_secs(240));
        save("subagent-spawned.txt", &tui.text());
        let mut d = json!({"childSession": child});
        if let Some(child) = child {
            std::thread::sleep(Duration::from_secs(3));
            d["childRowInDb"] = session_row(&db, &child);
            d["mainRowInDb"] = session_row(&db, &native);
            let gate_d = format!("sub-{}", token().to_lowercase());
            write_gate(&task, &child, &gate_d);
            let nonce_d = format!("SUBOWNER{}", token());
            let child_sid = store
                .find_session_id_pub(&child)
                .map_err(|e| e.to_string())?
                .unwrap_or(-1);
            // 遗留核实：子会话行若没有进程号，presence 会不会判成「TUI 不在」。
            d["childPresenceIsGone"] = json!(matches!(
                presence(
                    &NativeTarget {
                        db: &db,
                        session_id: child_sid,
                        provider: "opencode",
                        native_session: &child,
                        cwd: None,
                        task: &task,
                        gate_id: &gate_d,
                        message: "",
                        force: false,
                    },
                    None
                ),
                Ok(Presence::Gone)
            ));
            let (r, started) = answer(&gate_d, &nonce_d);
            d["answer"] = json!(r);
            d["acceptMs"] = json!(started.elapsed().as_millis());
            let st = crate::lead_conversation::delivery_status(&db, &cwd_text, &gate_d);
            d["deliveryState"] = json!(st.as_ref().map(|s| s.state.clone()));
            d["deliveryReason"] = json!(st.as_ref().and_then(|s| s.reason.clone()));
            let seen = tui.wait(Duration::from_secs(240), |s| reply_line(s, &nonce_d));
            d["nonce"] = json!(nonce_d);
            d["mainTuiShowsReply"] = json!(seen);
            d["replyMs"] = json!(started.elapsed().as_millis());
            wait_idle(&client, &native, Duration::from_secs(120));
            d["userMsgsInMain"] = json!(user_says(&client, &native, &nonce_d));
            d["userMsgsInChild"] = json!(user_says(&client, &child, &nonce_d));
            save("subagent-after.txt", &tui.text());
        }
        log.insert("D_subagent".into(), d);

        // ---- 2. TUI 不在时作答：只保存、不起服务；重新打开会话后巡检送达。
        tuis.pop().unwrap().kill();
        std::thread::sleep(Duration::from_secs(1));
        let gate2 = format!("offline-{}", token().to_lowercase());
        write_gate(&task, &native, &gate2);
        let nonce2 = format!("OFFLINE{}", token());
        let procs_before = crate::lead_native::probe_util::snapshot();
        let offline_started = Instant::now();
        let delivered = crate::lead_conversation::answer_and_deliver(
            &db,
            &cwd_text,
            &gate2,
            &LeadGateAnswerDto {
                choice: "go".into(),
                note: format!("After updating gate.json, reply with exactly {nonce2}"),
            },
            None,
        )?;
        let offline_answer_ms = offline_started.elapsed().as_millis();
        let status_saved = crate::lead_conversation::delivery_status(&db, &cwd_text, &gate2);
        std::thread::sleep(Duration::from_secs(5));
        let procs_after = crate::lead_native::probe_util::snapshot();
        let new_procs = crate::lead_native::probe_util::new_related(
            &procs_before,
            &procs_after,
            &root.to_string_lossy(),
        );
        let endpoints_while_closed = Registry::load(&registry_path(&db)).endpoints.len();
        let plan = launch_plan("opencode", &resume, &[]).ok_or("launch plan missing")?;
        let tui = Tui::spawn(&resume, Some(&plan), &cwd)?;
        plan.launched(tui.pid);
        let tui_pid = tui.pid.ok_or("pid")?;
        store
            .set_session_shared_pid(sid, i64::from(tui_pid), now_ms())
            .map_err(|e| e.to_string())?;
        let started = Instant::now();
        // 巡检每 10s 一次；这里紧凑地重复调用，直到服务接收。
        let mut states_seen: Vec<String> = Vec::new();
        let accepted = (0..60).any(|_| {
            crate::lead_conversation::reconcile_task(&db, &task, None);
            let state =
                crate::lead_conversation::delivery_status(&db, &cwd_text, &gate2).map(|s| s.state);
            let label = format!("{}ms:{state:?}", started.elapsed().as_millis());
            if states_seen.last().map(|l| l.split(':').nth(1)) != Some(label.split(':').nth(1)) {
                states_seen.push(label);
            }
            if state.as_deref() == Some("accepted") {
                return true;
            }
            std::thread::sleep(Duration::from_secs(1));
            false
        });
        let accept_ms = started.elapsed().as_millis();
        let seen = tui.wait(Duration::from_secs(240), |s| reply_line(s, &nonce2));
        save("offline-reopen.txt", &tui.text());
        tuis.push(tui);
        let offline = json!({
            "deliveredReturned": delivered,
            "stateWhileClosed": status_saved.as_ref().map(|s| s.state.clone()),
            "servicesStartedWhileClosed": endpoints_while_closed,
            "answerMs": offline_answer_ms,
            "newProcessesWhileClosed": new_procs,
            "procSnapshotOk": !procs_after.is_empty(),
            "replyOnScreenMsAfterReopen": started.elapsed().as_millis(),
            "acceptedAfterReopen": accepted,
            "acceptMsAfterReopen": accept_ms,
            "statesAfterReopen": states_seen,
            "nonce": nonce2,
            "nonceOnScreen": seen,
            "gateStatus": gate_status(&task),
        });
        log.insert("offline".into(), offline.clone());
        if !(delivered
            && status_saved.as_ref().map(|s| s.state.as_str()) == Some("native_on_open")
            && endpoints_while_closed == 0
            && accepted
            && seen)
        {
            return Err(format!("TUI 不在时作答验证未通过：{offline}"));
        }
        Ok(())
    })();

    for tui in tuis.drain(..) {
        tui.kill();
    }
    // MEOWO_PROBE_KEEP=1 保留隔离目录排查（免费模型不复制凭据；复制了凭据时照删）。
    if !keep_dirs || !copied.is_empty() {
        remove_dir_retry(&home);
        remove_dir_retry(&cwd);
    }
    // 注册表里有本次服务的密码；进程都已结束，一并删掉。
    remove_dir_retry(&root.join("lead-native"));
    log.insert("opencodeVersion".into(), json!(version));
    log.insert("model".into(), json!(model));
    log.insert("copiedCredentials".into(), json!(copied));
    log.insert(
        "result".into(),
        json!(result.as_ref().map(|_| "ok").unwrap_or_else(|e| e.as_str())),
    );
    log.insert("privateHomeRemoved".into(), json!(!home.exists()));
    save(
        "result.json",
        &serde_json::to_string_pretty(&Value::Object(log)).unwrap(),
    );
    drop(env);
    eprintln!("opencode TUI probe evidence: {}", evidence.display());
    assert!(result.is_ok(), "{}", result.unwrap_err());
}
