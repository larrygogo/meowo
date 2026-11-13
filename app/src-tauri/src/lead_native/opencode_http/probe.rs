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
//! 关卡被消费）、离线 O2（TUI 退出后作答 → serve 执行；打开即 attach 看到这一轮；serve 空闲后回收；
//! 再打开普通 TUI 可见且关卡已消费）。答复一律经 `answer_and_deliver`，与手机作答同一后端路径。

use super::*;
use meowo_protocol::ipc::{LeadGateAnswerDto, LeadProviderInstallationDto};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::sync::Arc;

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
        let argv = plan.and_then(|p| p.argv.as_deref()).unwrap_or(argv);
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
        .find(|e| e.kind == Kind::Tui && e.pid == pid)
        .expect("TUI 启动后应登记端口与密码")
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
    // 关掉 bash：探针只允许模型读写任务目录里的文件，不在本机执行命令。
    let config = json!({"$schema":"https://opencode.ai/config.json","model":model,"small_model":model,"autoupdate":false,"share":"disabled","permission":{"edit":"allow","read":"allow","bash":"deny","webfetch":"deny"}});
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
    let mut git = std::process::Command::new("git");
    git.args(["init", "--quiet"]).arg(&cwd);
    assert!(git.status().is_ok_and(|s| s.success()));

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
    let mut serve_pids: Vec<u32> = vec![];
    let mut log = serde_json::Map::new();
    let save = |name: &str, text: &str| {
        let _ = std::fs::write(evidence.join(name), text);
    };
    let cwd_text = cwd.to_string_lossy().into_owned();

    let result = (|| -> Result<(), String> {
        // ---- 0. 建会话：经 Meowo 的启动计划起 TUI（带端口+密码），用它自己的服务建一个空会话。
        let plain = vec![exe.clone()];
        let plan = launch_plan("opencode", &plain, &[]).ok_or("launch plan missing")?;
        if plan.argv.is_some() || !plan.args.contains(&"--port".to_string()) {
            return Err("新 TUI 应带 --port 启动".into());
        }
        let tui = Tui::spawn(&plain, Some(&plan), &cwd)?;
        plan.launched(tui.pid);
        let endpoint = tui_endpoint(&db, tui.pid.ok_or("pid")?);
        let client = Client::of(&endpoint);
        let deadline = Instant::now() + Duration::from_secs(40);
        while !client.healthy(Duration::from_secs(1)) {
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
        let (sid, _) = store
            .start_session(project, &native, now_ms())
            .map_err(|e| e.to_string())?;
        store
            .set_session_provider(sid, "opencode")
            .map_err(|e| e.to_string())?;
        store
            .set_session_cwd(sid, &cwd_text, now_ms())
            .map_err(|e| e.to_string())?;

        // ---- 1. 在线 O1：恢复打开会话；先让模型跑一个长一点的回合，再在输入框里留草稿，然后作答。
        let resume = vec![exe.clone(), "--session".into(), native.clone()];
        let plan = launch_plan("opencode", &resume, &[]).ok_or("launch plan missing")?;
        if plan.argv.is_some() {
            return Err("没有 serve 时应以普通 TUI 打开".into());
        }
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

        // ---- 2. 离线 O2：TUI 退出后作答。
        tuis.pop().unwrap().kill();
        std::thread::sleep(Duration::from_secs(1));
        let gate2 = format!("offline-{}", token().to_lowercase());
        write_gate(&task, &native, &gate2);
        let nonce2 = format!("OFFLINE{}", token());
        let started = Instant::now();
        let delivered = crate::lead_conversation::answer_and_deliver(
            &db,
            &cwd_text,
            &gate2,
            &LeadGateAnswerDto {
                choice: "go".into(),
                note: format!("First write the integers from 1 to 80 separated by spaces, then after updating gate.json reply with exactly {nonce2}"),
            },
            None,
        )?;
        let accept_ms = started.elapsed().as_millis();
        let serve = Registry::load(&registry_path(&db))
            .endpoints
            .into_iter()
            .find(|e| e.kind == Kind::Serve && e.native_session.as_deref() == Some(&native))
            .ok_or("离线投递应起 serve")?;
        serve_pids.push(serve.pid);
        let serve_client = Client::of(&serve);
        let status2 = crate::lead_conversation::delivery_status(&db, &cwd_text, &gate2);
        // 打开会话：应改为 attach 到 serve，而不是再起一个驱动者。
        let plan = launch_plan("opencode", &resume, &[]).ok_or("launch plan missing")?;
        let attach_argv = plan.argv.clone().ok_or("serve 在跑时打开应走 attach")?;
        let attach = Tui::spawn(&resume, Some(&plan), &cwd)?;
        plan.launched(attach.pid);
        let attached_seen = attach.wait(Duration::from_secs(240), |s| reply_line(s, &nonce2));
        save("offline-attach.txt", &attach.text());
        let attach_pid_recorded = Registry::load(&registry_path(&db))
            .endpoints
            .iter()
            .any(|e| e.pid == serve.pid && e.attach_pid == attach.pid);
        attach.kill();
        // 回收：会话空闲且 attach 已退出 → 结束 serve（单元测试里不跑回收线程，这里显式跑一遍）。
        wait_idle(&serve_client, &native, Duration::from_secs(240));
        let offline_reply = assistant_text(&serve_client, &native);
        reap(&db, serve.pid);
        let serve_reaped = !serve_client.healthy(Duration::from_secs(2))
            && !Registry::load(&registry_path(&db))
                .endpoints
                .iter()
                .any(|e| e.kind == Kind::Serve);
        // 再打开：回到普通 TUI（O1），这一轮可见、关卡已消费。
        let plan = launch_plan("opencode", &resume, &[]).ok_or("launch plan missing")?;
        let reopened_plain = plan.argv.is_none();
        let tui = Tui::spawn(&resume, Some(&plan), &cwd)?;
        plan.launched(tui.pid);
        let reopened_seen = tui.wait(Duration::from_secs(40), |s| reply_line(s, &nonce2));
        save("offline-reopen.txt", &tui.text());
        tuis.push(tui);
        let offline = json!({
            "deliveredReturned": delivered,
            "acceptMsIncludingServeStart": accept_ms,
            "deliveryState": status2.as_ref().map(|s| s.state.clone()),
            "attachArgvShape": attach_argv.iter().skip(1).take(1).collect::<Vec<_>>(),
            "attachRecorded": attach_pid_recorded,
            "nonce": nonce2,
            "nonceOnAttachScreen": attached_seen,
            "nonceInModelReply": offline_reply.contains(&nonce2),
            "serveReapedWhenIdle": serve_reaped,
            "reopenedAsPlainTui": reopened_plain,
            "nonceVisibleAfterReopen": reopened_seen,
            "gateStatus": gate_status(&task),
        });
        log.insert("offline".into(), offline.clone());
        if !(delivered
            && attached_seen
            && offline_reply.contains(&nonce2)
            && serve_reaped
            && reopened_plain
            && reopened_seen)
        {
            return Err(format!("离线验证未通过：{offline}"));
        }
        Ok(())
    })();

    for tui in tuis.drain(..) {
        tui.kill();
    }
    for pid in serve_pids {
        stop_serve(&registry_path(&db), pid, true, kill_tree);
    }
    remove_dir_retry(&home);
    remove_dir_retry(&cwd);
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
