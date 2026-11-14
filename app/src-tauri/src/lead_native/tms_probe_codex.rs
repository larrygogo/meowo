//! 真机探针：Codex 原生 TUI 会话里切模型 / 模式（任务 .lead/20261114-tui-model-switch，验收 #3 #4 #7 #8）。
//!
//! 走 Meowo 后端同一入口：`session_settings::plan` 拆计划 → `codex_app_server::update_settings`
//! （即 apply_session_settings 的 Server 分支）；清单用 `session_settings::catalog_for`，当前值用
//! `codex_app_server::live_settings`（session_live_settings 的 ServerSettings 分支），新建页默认值用
//! `plugin.launch_defaults`（agent_launch_defaults 背后）。新会话的启动参数用生产同一套
//! `session_settings::args_for_selections` + `lead_runtime::launch_args` + `prepare_launch` 拼。
//!
//! 默认进程内假 Responses 服务（不花额度），隔离 CODEX_HOME 与 MEOWO_DB，不装 Lead（模拟未装 Lead 的账号）。
//! 证据写 `MEOWO_TMS_EVIDENCE`（缺省 `$CARGO_TARGET_DIR/tms-probe/codex/<时间戳>`）。
//!
//! ```text
//! cargo test -p meowo-app --lib tms_probe_codex -- --ignored --nocapture --test-threads=1
//! ```
use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    fn submit(&self, text: &str) {
        self.send(text);
        std::thread::sleep(Duration::from_millis(500));
        self.send("\r");
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

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// assistant 行以「• 」开头：口令出现在 assistant 行才算模型输出了它。
fn assistant_says(screen: &str, nonce: &str) -> bool {
    screen
        .lines()
        .any(|l| l.trim_start().starts_with('•') && l.contains(nonce))
}

fn nonce(tag: &str) -> String {
    format!(
        "{tag}_{}",
        crate::remote::generate_token().unwrap()[..10]
            .to_uppercase()
            .replace(['-', '_'], "X")
    )
}

/// 假 Responses 服务：回最后一条用户消息里最后一个 `T<n>_口令`。SLOWLY → 先睡 10 秒（运行中切换）；
/// APPROVE_ME → 调一条要提权的命令（停在审批框）。每个请求记模型、思考档、originator、UA、回合元数据。
fn fake_model(log: Arc<Mutex<Vec<Value>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let nonce_re = regex::Regex::new(r"\bT[0-9]+_[A-Z0-9]{10}\b").unwrap();
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
                let found = nonce_re
                    .find_iter(&last_user)
                    .last()
                    .map(|m| m.as_str().to_owned());
                let (reply, item) = if last_is_tool_output {
                    ("TOOL_DONE".to_owned(), None)
                } else if last_user.contains("APPROVE_ME") {
                    (
                        "<exec_command>".to_owned(),
                        Some(json!({"type":"function_call","id":"fc","call_id":format!("call_{}", &crate::remote::generate_token().unwrap()[..8]),"name":"exec_command","arguments":json!({"cmd":"New-Item -ItemType File probe-approval.txt","sandbox_permissions":"require_escalated","justification":"probe approval"}).to_string()})),
                    )
                } else {
                    (found.clone().unwrap_or_else(|| "OK".into()), None)
                };
                let header = |k: &str| headers.iter().find(|(h, _)| h == k).map(|(_, v)| v.clone());
                let meta: Value = header("x-codex-turn-metadata")
                    .and_then(|m| serde_json::from_str(&m).ok())
                    .unwrap_or(Value::Null);
                let slow = !last_is_tool_output && last_user.contains("SLOWLY");
                log.lock().unwrap().push(json!({
                    "at": crate::now_ms(),
                    "reply": reply,
                    "nonceInLastUser": found,
                    "model": request["model"],
                    "reasoning": request["reasoning"],
                    "originator": header("originator"),
                    "userAgent": header("user-agent"),
                    "turnMetadata": meta,
                    "slow": slow,
                }));
                if slow {
                    std::thread::sleep(Duration::from_secs(10));
                }
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
    log: Arc<Mutex<Vec<Value>>>,
    out: PathBuf,
}

impl Drop for Probe {
    fn drop(&mut self) {
        for (dir, _) in states(&self.db) {
            retire(&dir);
        }
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

struct Launched {
    tui: Tui,
    server_pid: u32,
    tui_argv: Vec<String>,
    lead_args: Vec<String>,
}

impl Probe {
    fn plugin() -> &'static dyn meowo_agent::AgentPlugin {
        meowo_agent::by_id("codex").unwrap()
    }

    /// 与 pty.rs 新建会话同一顺序拼 argv：launch_argv + 启动选项参数 → Lead MCP 参数（按安装状态）
    /// → prepare_launch 拆成 app-server + `--remote` TUI。
    fn launch(&self, session_id: i64, selections: &HashMap<String, String>) -> Launched {
        let plugin = Self::plugin();
        let mut argv = plugin.launch_argv();
        argv.extend(crate::session_settings::args_for_selections(
            plugin,
            selections,
            &self.env,
            Some(&self.work),
            false,
        ));
        // 关启动更新检查（探针曾按到「立即更新」把用户 codex 升级，见 codex_app_server_probe）。
        argv.extend(["-c".into(), "check_for_update_on_startup=false".into()]);
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        let lead_args = crate::lead_runtime::launch_args(
            "codex",
            &self.env,
            Some(crate::browser_agent::McpSessionEnv {
                endpoint: "127.0.0.1:9",
                token: "probe-token",
                protocol: 2,
            }),
        );
        command.args(&lead_args);
        command.cwd(&self.work);
        for (key, _) in std::env::vars() {
            if key.starts_with("MEOWO_") || key.starts_with("CODEX_") || key.starts_with("CLAUDE") {
                command.env_remove(key);
            }
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let launch = prepare_launch(session_id, "codex", &self.env, &mut command)
            .expect("未装 Lead 时服务通道也应启用（prepare_launch 返回 Some）");
        let tui_argv: Vec<String> = command
            .get_argv()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(tui_argv.iter().any(|a| a == "--remote"), "{tui_argv:?}");
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 140,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        bind_tui(launch, child.process_id());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 140, 0)));
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
        let server_pid = states(&self.db)
            .into_iter()
            .filter(|(_, s)| s.launch_session == session_id)
            .map(|(_, s)| s.server_pid)
            .max()
            .unwrap();
        Launched {
            tui,
            server_pid,
            tui_argv,
            lead_args,
        }
    }

    /// 宿主库登记会话（生产由 hook 落库，pid = app-server）。
    fn register(&self, thread: &str, server_pid: u32) -> i64 {
        let store = crate::open_store(&self.db).unwrap();
        let project = store
            .upsert_project_by_root(&self.work.to_string_lossy(), "probe", 1)
            .unwrap();
        let (sid, _) = store.start_session(project, thread, 2).unwrap();
        store.set_session_provider(sid, "codex").unwrap();
        store
            .set_session_shared_pid(sid, i64::from(server_pid), 3)
            .unwrap();
        sid
    }

    /// apply_session_settings 的 Server 分支：清单校验 + 拆计划 → update_settings。
    fn apply(
        &self,
        sid: i64,
        thread: &str,
        model: Option<&str>,
        effort: Option<&str>,
        modes: &[(&str, &str)],
    ) -> Result<u128, String> {
        let started = Instant::now();
        let plugin = Self::plugin();
        let modes: HashMap<String, String> = modes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let catalog = if model.is_some() {
            crate::session_settings::catalog_for(plugin, &self.env, Some(&self.work))
        } else {
            Vec::new()
        };
        let plan = crate::session_settings::plan(
            plugin,
            model,
            effort,
            &modes,
            &catalog,
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )?;
        assert!(plan.restart.is_empty() && plan.dialog_model.is_none() && !plan.needs_new_session);
        let update: CodexSettingsUpdate =
            serde_json::from_value(Value::Object(plan.server)).map_err(|e| e.to_string())?;
        update_settings(&self.db, sid, thread, &update).map_err(|e| format!("{e:?}"))?;
        Ok(started.elapsed().as_millis())
    }

    fn save(&self, name: &str, text: &str) {
        std::fs::write(self.out.join(name), text).unwrap();
    }

    fn requests_for(&self, nonce: &str) -> Vec<Value> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["nonceInLastUser"].as_str() == Some(nonce))
            // 每回合还有一条不带 turn_trigger 的旁路请求（同模型、effort=low，后台摘要类），
            // 不是这一轮对话本身，按回合请求判模型。
            .filter(|r| !r["turnMetadata"]["turn_trigger"].is_null())
            .cloned()
            .collect()
    }

    fn turn(&self, tui: &Tui, nonce: &str, text: &str) -> (bool, u128) {
        let started = Instant::now();
        tui.submit(text);
        let ok = tui.wait(120, |s| assistant_says(s, nonce));
        // 等回合收尾（rollout 写完 turn_context / 结束事件）。
        std::thread::sleep(Duration::from_millis(1500));
        (ok, started.elapsed().as_millis())
    }

    fn rollouts(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![self.home.join("sessions")];
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

    /// 主会话 thread id：rollout 的 session_meta（不 initialize 连服务，免得 UA 后缀提前变）。
    fn thread_with_user(&self, nonce: &str) -> Option<String> {
        self.rollouts().iter().find_map(|p| {
            let body = std::fs::read_to_string(p).ok()?;
            if !body
                .lines()
                .any(|l| l.contains("\"role\":\"user\"") && l.contains(nonce))
            {
                return None;
            }
            body.lines().find_map(|line| {
                let v: Value = serde_json::from_str(line).ok()?;
                (v["type"] == "session_meta")
                    .then(|| v.pointer("/payload/id")?.as_str().map(str::to_owned))?
            })
        })
    }

    fn live(&self, thread: &str) -> Value {
        match live_settings(thread) {
            Some((model, effort, modes)) => json!({
                "model": model, "effort": effort,
                "modes": modes.into_iter().map(|(d, v)| json!({"dimension": d, "value": v})).collect::<Vec<_>>(),
            }),
            None => Value::Null,
        }
    }
}

fn mode_of(live: &Value, dimension: &str) -> Option<String> {
    live["modes"]
        .as_array()?
        .iter()
        .find(|m| m["dimension"] == dimension)
        .and_then(|m| m["value"].as_str().map(str::to_owned))
}

fn approval_shown(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("would you like to run")
        || lower.contains("yes, proceed")
        || lower.contains("allow command")
        || lower.contains("approve")
}

fn server_cmdline(pid: u32) -> String {
    super::super::probe_util::snapshot()
        .get(&pid)
        .map(|p| p.cmd.clone())
        .unwrap_or_default()
}

#[test]
#[ignore = "拉起真实 codex（假模型），手动复跑"]
fn tms_probe_codex() {
    let root = std::env::temp_dir().join(format!(
        "mtms-codex-{}",
        &crate::remote::generate_token().unwrap()[..8]
    ));
    std::fs::create_dir_all(root.join("h")).unwrap();
    std::fs::create_dir_all(root.join("w")).unwrap();
    let root = dunce::canonicalize(root).unwrap();
    let (home, work) = (root.join("h"), root.join("w"));
    let db = root.join("board.db");
    std::env::set_var("MEOWO_DB", &db);
    std::env::set_var("CODEX_HOME", &home);
    let log = Arc::new(Mutex::new(Vec::new()));
    let port = fake_model(log.clone());
    let mut config = toml_edit::DocumentMut::new();
    config["check_for_update_on_startup"] = toml_edit::value(false);
    config["model"] = toml_edit::value("fake-model");
    config["model_provider"] = toml_edit::value("fake");
    config["model_providers"]["fake"]["name"] = toml_edit::value("fake");
    config["model_providers"]["fake"]["base_url"] =
        toml_edit::value(format!("http://127.0.0.1:{port}/v1"));
    config["model_providers"]["fake"]["wire_api"] = toml_edit::value("responses");
    config["model_providers"]["fake"]["env_key"] = toml_edit::value("FAKE_KEY");
    config["projects"][work.to_string_lossy().as_ref()]["trust_level"] =
        toml_edit::value("trusted");
    std::fs::write(home.join("config.toml"), config.to_string()).unwrap();
    let env = vec![
        ("CODEX_HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("FAKE_KEY".to_owned(), "x".to_owned()),
        ("MEOWO_DB".to_owned(), db.to_string_lossy().into_owned()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
    ];
    let out = std::env::var("MEOWO_TMS_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
                .join("tms-probe/codex")
                .join(crate::now_ms().to_string())
        });
    std::fs::create_dir_all(&out).unwrap();
    let probe = Probe {
        root: root.clone(),
        home,
        work,
        db,
        env,
        log,
        out,
    };
    let plugin = Probe::plugin();
    let exe = plugin.launch_argv();
    let version = std::process::Command::new(&exe[0])
        .args(&exe[1..])
        .arg("--version")
        .output()
        .unwrap();
    let mut ev = json!({ "codex": String::from_utf8_lossy(&version.stdout).trim() });
    let save = |ev: &Value| probe.save("result.json", &serde_json::to_string_pretty(ev).unwrap());
    let mut failures: Vec<String> = Vec::new();
    let mut check = |ok: bool, what: &str| {
        if !ok {
            failures.push(what.to_owned());
        }
    };

    // ---- 清单：会话里的模型清单（catalog_for：无 models_cache 时 `codex debug models --bundled`）。
    let started = Instant::now();
    let catalog = crate::session_settings::catalog_for(plugin, &probe.env, Some(&probe.work));
    let catalog_ms = started.elapsed().as_millis();
    let ids: Vec<String> = catalog.iter().map(|e| e.id.clone()).collect();
    ev["catalog"] = json!({"ms": catalog_ms, "ids": ids,
        "efforts": catalog.iter().map(|e| json!({"id": e.id, "efforts": e.efforts})).collect::<Vec<_>>()});
    let with_high: Vec<&meowo_agent::ModelCatalogEntry> = catalog
        .iter()
        .filter(|e| e.efforts.iter().any(|x| x == "high"))
        .collect();
    assert!(with_high.len() >= 3, "清单里带 high 档的模型不足 3 个：{ids:?}");
    let (model_a, model_b, model_c) = (
        with_high[0].id.clone(),
        with_high[1].id.clone(),
        with_high[2].id.clone(),
    );

    // ---- 前提：未装 Lead。服务通道照开，argv 不带 Lead MCP。新建会话带模式参数（只读）。
    check(
        !crate::lead_runtime::enabled("codex", &probe.env),
        "未装 Lead 时 lead_runtime::enabled 应为 false",
    );
    let started = Instant::now();
    let selections: HashMap<String, String> =
        HashMap::from([("approval".to_owned(), "readOnly".to_owned())]);
    let Launched {
        tui,
        server_pid,
        tui_argv,
        lead_args,
    } = probe.launch(-1, &selections);
    let launch_ms = started.elapsed().as_millis();
    let server_cmd = server_cmdline(server_pid);
    let no_lead_mcp = !tui_argv.iter().any(|a| a.contains("meowo_lead"))
        && !server_cmd.contains("meowo_lead")
        && lead_args.is_empty();
    ev["pre_noLead"] = json!({
        "leadEnabled": false, "leadArgs": lead_args, "tuiArgv": tui_argv,
        "serverCmdline": server_cmd, "noLeadMcp": no_lead_mcp, "launchMs": launch_ms,
    });
    check(no_lead_mcp, "未装 Lead 时 argv 不应含 meowo_lead");
    check(
        tui_argv.iter().any(|a| a == "read-only"),
        "新建会话的只读档应作为 TUI 启动参数",
    );
    save(&ev);

    // ---- 首轮
    let t1 = nonce("T1");
    let (ok, ms) = probe.turn(&tui, &t1, &format!("只回复 {t1}"));
    assert!(ok, "首轮无回复：\n{}", tui.text());
    let thread = probe.thread_with_user(&t1).expect("rollout 里找不到主 thread");
    let sid = probe.register(&thread, server_pid);
    let live1 = probe.live(&thread);
    ev["first"] = json!({"nonce": t1, "ms": ms, "thread": thread, "live": live1,
        "request": probe.requests_for(&t1)});
    check(
        mode_of(&live1, "approval").as_deref() == Some("readOnly"),
        "#7 新建会话带只读参数应生效（turn_context sandbox read-only）",
    );
    probe.save("01-first.txt", &tui.text());
    save(&ev);

    // ---- #3 空闲切模型 + 思考档；草稿保留；下一轮用新模型；同一 thread。
    let draft = format!("draft{}", &nonce("D")[2..8]);
    tui.send(&draft);
    std::thread::sleep(Duration::from_millis(600));
    let applied = probe.apply(sid, &thread, Some(&model_a), Some("high"), &[]);
    // 底栏写显示名（实拍 `GPT-6-Astra high`），大小写不敏感比对。
    let footer = format!("{model_a} high").to_lowercase();
    let shown = tui.wait(10, |s| s.to_lowercase().contains(&footer));
    let screen = tui.text();
    let draft_kept = screen.contains(&draft);
    probe.save("02-idle-switched.txt", &screen);
    tui.send("\x15");
    std::thread::sleep(Duration::from_millis(500));
    let t2 = nonce("T2");
    let (ok, ms) = probe.turn(&tui, &t2, &format!("只回复 {t2}"));
    let req2 = probe.requests_for(&t2);
    let model_ok = !req2.is_empty()
        && req2
            .iter()
            .all(|r| r["model"] == model_a.as_str() && r["reasoning"]["effort"] == "high");
    let same_thread = probe.thread_with_user(&t2).as_deref() == Some(thread.as_str());
    let history = tui.text().contains(&t1) || {
        // 首轮可能已滚出屏幕：以 rollout 里同一 thread 同时有 T1、T2 为准。
        same_thread
    };
    let rollout_count = probe
        .rollouts()
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .count();
    ev["s3_idle"] = json!({
        "applied": applied, "model": model_a, "effort": "high", "footerShown": shown,
        "draft": draft, "draftKept": draft_kept, "nextTurn": {"nonce": t2, "ok": ok, "ms": ms, "requests": req2},
        "modelInRequest": model_ok, "sameThread": same_thread, "historyKept": history,
        "rolloutFiles": rollout_count, "live": probe.live(&thread),
    });
    check(applied.is_ok(), "#3 切换应成功");
    check(shown, "#3 TUI 底栏应显示新模型");
    check(draft_kept, "#4 空闲切换草稿应保留");
    check(ok && model_ok, "#3 下一轮请求应用新模型与思考档");
    check(same_thread, "#3 会话 id 不变");
    probe.save("03-after-idle-turn.txt", &tui.text());
    save(&ev);

    // ---- #4 运行中切换：这一轮仍旧模型，下一轮新模型；运行中草稿保留。
    let t3 = nonce("T3");
    tui.submit(&format!("SLOWLY 只回复 {t3}"));
    let in_flight = (0..60).any(|_| {
        std::thread::sleep(Duration::from_millis(250));
        !probe.requests_for(&t3).is_empty()
    });
    let applied = probe.apply(sid, &thread, Some(&model_b), Some("high"), &[]);
    let during = tui.text();
    let draft3 = format!("run{}", &nonce("D")[2..8]);
    tui.send(&draft3);
    let done = tui.wait(60, |s| assistant_says(s, &t3));
    std::thread::sleep(Duration::from_millis(1500));
    let after = tui.text();
    let draft3_kept = after.contains(&draft3);
    tui.send("\x15");
    std::thread::sleep(Duration::from_millis(500));
    let t4 = nonce("T4");
    let (ok4, _) = probe.turn(&tui, &t4, &format!("只回复 {t4}"));
    let req3 = probe.requests_for(&t3);
    let req4 = probe.requests_for(&t4);
    let current_kept_old = req3.iter().all(|r| r["model"] == model_a.as_str());
    let next_new = !req4.is_empty() && req4.iter().all(|r| r["model"] == model_b.as_str());
    ev["s4_running"] = json!({
        "inFlightWhenApplied": in_flight, "applied": applied, "screenDuring": during,
        "slowTurnDone": done, "slowRequests": req3, "nextRequests": req4,
        "currentTurnKeptOldModel": current_kept_old, "nextTurnNewModel": next_new && ok4,
        "draft": draft3, "draftKept": draft3_kept,
    });
    check(in_flight && applied.is_ok(), "#4 运行中切换应被接受");
    check(current_kept_old, "#4 运行中这一轮应仍是旧模型");
    check(next_new && ok4, "#4 下一轮应换成新模型");
    check(draft3_kept, "#4 运行中草稿应保留");
    probe.save("04-running.txt", &after);
    save(&ev);

    // ---- #4 待批审批：切换不碰审批框，审批不被答掉，文件不建。
    tui.submit("APPROVE_ME");
    let pending = tui.wait(60, approval_shown);
    let before = tui.text();
    let applied = probe.apply(sid, &thread, None, None, &[("collaboration", "default")]);
    let applied_model = probe.apply(sid, &thread, Some(&model_a), Some("high"), &[]);
    std::thread::sleep(Duration::from_secs(4));
    let during = tui.text();
    let still = approval_shown(&during);
    let file_made = probe.work.join("probe-approval.txt").exists();
    tui.send("\x1b");
    std::thread::sleep(Duration::from_secs(4));
    let file_after_decline = probe.work.join("probe-approval.txt").exists();
    ev["s4_approval"] = json!({
        "approvalShown": pending, "appliedMode": applied, "appliedModel": applied_model,
        "approvalStillPending4s": still, "fileCreatedWhilePending": file_made,
        "fileCreatedAfterDecline": file_after_decline,
        "screenBefore": before, "screenDuring": during, "screenAfterDecline": tui.text(),
    });
    check(pending, "#4 应出现审批框");
    check(applied_model.is_ok(), "#4 有审批时经服务切换应成功（不经 PTY）");
    check(still && !file_made, "#4 审批不应被答掉");
    probe.save("05-approval.txt", &during);
    save(&ev);
    // 审批拒绝后回合可能还在收尾。
    std::thread::sleep(Duration::from_secs(3));

    // ---- #7 模式：Full Access + Plan；再切 Approve for me + Default。
    let applied = probe.apply(
        sid,
        &thread,
        None,
        None,
        &[("approval", "yolo"), ("collaboration", "plan")],
    );
    let plan_shown = tui.wait(10, |s| s.contains("Plan mode"));
    probe.save("06-plan-yolo.txt", &tui.text());
    let t5 = nonce("T5");
    let (ok5, _) = probe.turn(&tui, &t5, &format!("只回复 {t5}"));
    let live5 = probe.live(&thread);
    let req5 = probe.requests_for(&t5);
    let applied2 = probe.apply(
        sid,
        &thread,
        None,
        None,
        &[("approval", "approveForMe"), ("collaboration", "default")],
    );
    let plan_gone = tui.wait(10, |s| !s.contains("Plan mode"));
    let t6 = nonce("T6");
    let (ok6, _) = probe.turn(&tui, &t6, &format!("只回复 {t6}"));
    let live6 = probe.live(&thread);
    let req6 = probe.requests_for(&t6);
    ev["s7_modes"] = json!({
        "yoloPlan": {"applied": applied, "planModeShown": plan_shown, "turnOk": ok5, "live": live5, "requests": req5},
        "approveForMeDefault": {"applied": applied2, "planModeGone": plan_gone, "turnOk": ok6, "live": live6, "requests": req6},
    });
    check(applied.is_ok() && plan_shown, "#7 Plan 应在 TUI 显示");
    check(
        mode_of(&live5, "approval").as_deref() == Some("yolo")
            && mode_of(&live5, "collaboration").as_deref() == Some("plan"),
        "#7 下一轮应为 Full Access + Plan",
    );
    check(
        mode_of(&live6, "approval").as_deref() == Some("approveForMe")
            && mode_of(&live6, "collaboration").as_deref() == Some("default"),
        "#7 下一轮应为 Approve for me + Default",
    );
    probe.save("07-modes.txt", &tui.text());
    save(&ev);

    // ---- #8 TUI 里自己切：/model <C> --reasoning=high，Shift+Tab 进 Plan；回合结束后 live_settings 跟上。
    tui.submit(&format!("/model {model_c} --reasoning=high"));
    let changed = tui.wait(15, |s| s.contains("Model changed"));
    std::thread::sleep(Duration::from_millis(800));
    tui.send("\x1b[Z");
    let plan_on = tui.wait(10, |s| s.contains("Plan mode"));
    let live_before_turn = probe.live(&thread);
    probe.save("08-tui-self-switch.txt", &tui.text());
    let t7 = nonce("T7");
    let (ok7, _) = probe.turn(&tui, &t7, &format!("只回复 {t7}"));
    let live7 = probe.live(&thread);
    let req7 = probe.requests_for(&t7);
    ev["s8_tui_self"] = json!({
        "inlineModelChanged": changed, "shiftTabPlan": plan_on,
        "liveBeforeNextTurn": live_before_turn, "turnOk": ok7, "liveAfterTurn": live7,
        "requests": req7,
    });
    check(
        live7["model"] == model_c.as_str()
            && mode_of(&live7, "collaboration").as_deref() == Some("plan"),
        "#8 回合结束后 live_settings 应读到 TUI 里自切的模型与 Plan",
    );
    probe.save("09-after-self-turn.txt", &tui.text());
    save(&ev);

    // ---- 计费：所有请求 originator 仍是 codex-tui；记 UA 后缀变化。
    let all = probe.log.lock().unwrap().clone();
    let originators: std::collections::BTreeSet<String> = all
        .iter()
        .map(|r| r["originator"].as_str().unwrap_or("?").to_owned())
        .collect();
    let uas: std::collections::BTreeSet<String> = all
        .iter()
        .map(|r| r["userAgent"].as_str().unwrap_or("?").to_owned())
        .collect();
    ev["billing"] = json!({"originators": originators, "userAgents": uas, "requestCount": all.len()});
    check(
        originators.len() == 1 && originators.contains("codex-tui"),
        "切换后 originator 应仍是 codex-tui",
    );
    let mut tui = tui;
    tui.quit();
    save(&ev);

    // ---- #7 新建会话带 Approve for me 启动参数（PERMISSION_FLAGS 进 TUI 侧）。
    let selections: HashMap<String, String> =
        HashMap::from([("approval".to_owned(), "approveForMe".to_owned())]);
    let Launched {
        tui: mut tui2,
        tui_argv: argv2,
        ..
    } = probe.launch(-2, &selections);
    let t8 = nonce("T8");
    let (ok8, _) = probe.turn(&tui2, &t8, &format!("只回复 {t8}"));
    let thread2 = probe.thread_with_user(&t8);
    let live8 = thread2.as_deref().map(|t| probe.live(t)).unwrap_or(Value::Null);
    ev["s7_new_session_approveForMe"] = json!({"tuiArgv": argv2, "turnOk": ok8, "thread": thread2, "live": live8,
        "requests": probe.requests_for(&t8), "screen": tui2.text()});
    // 2026-11-14 起不再用 `--approve-for-me`（Windows 上沙箱落成 read-only），改为与会话内同一组值：
    // workspace-write + on-request + approvals_reviewer=auto_review。
    check(
        argv2.windows(2).any(|w| w[0] == "--sandbox" && w[1] == "workspace-write"),
        "#7 新建会话启动参数应带 workspace-write 与自动审查（-c 交给 app-server）",
    );
    // 0.167.2 Windows 实测：`--approve-for-me` 单独启动时沙箱落成 read-only（内嵌与 --remote 一样，
    // 与 `--sandbox` 互斥），自动审查是开的。所以按请求元数据的 auto_review 判「启动参数生效」，
    // 沙箱如实记下（会话里显示成 Read Only，与会话内切到 Approve for me 的 workspace-write 不一致）。
    let review_on = probe
        .requests_for(&t8)
        .iter()
        .any(|r| r["turnMetadata"]["auto_review_enabled"] == true);
    ev["s7_new_session_approveForMe"]["autoReviewEnabled"] = json!(review_on);
    ev["s7_new_session_approveForMe"]["tierShownInSession"] = json!(mode_of(&live8, "approval"));
    // 认领后宿主按存的这一档经服务补设（pty 认领 → session_settings::apply_launch_server_settings 同一份片段），
    // 下一轮看会话里显示的档。
    let patch = crate::session_settings::launch_server_update(plugin, &selections).expect("服务端片段");
    let update: CodexSettingsUpdate = serde_json::from_value(Value::Object(patch)).unwrap();
    let applied8 = thread2
        .as_deref()
        .map(|thread| crate::lead_native::codex_app_server::update_settings(&probe.db, -2, thread, &update));
    let t9 = nonce("T9");
    let (ok9, _) = probe.turn(&tui2, &t9, &format!("只回复 {t9}"));
    let live8 = thread2.as_deref().map(|t| probe.live(t)).unwrap_or(Value::Null);
    ev["s7_new_session_approveForMe"]["afterLaunchPatch"] = json!({"applied": format!("{applied8:?}"), "turnOk": ok9, "live": live8,
        "sandbox": probe.requests_for(&t9).iter().map(|r| r["turnMetadata"]["sandbox_mode"].clone()).collect::<Vec<_>>()});
    check(review_on, "#7 新建会话 Approve for me 应开自动审查");
    check(
        mode_of(&live8, "approval").as_deref() == Some("approveForMe"),
        "#7 新建会话 Approve for me 在会话里应显示成同一档（沙箱 workspace-write）",
    );
    probe.save("10-new-approveForMe.txt", &tui2.text());
    tui2.quit();
    save(&ev);

    // ---- #8 改 CLI 配置文件默认模型 → 新建页默认值跟上（launch_defaults 现读）。
    let before = plugin.launch_defaults(&probe.env, Some(&probe.work));
    let text = std::fs::read_to_string(probe.home.join("config.toml")).unwrap();
    let edited = text.replacen(
        "model = \"fake-model\"",
        &format!("model = \"{model_b}\"\nsandbox_mode = \"workspace-write\"\napprovals_reviewer = \"auto_review\""),
        1,
    );
    std::fs::write(probe.home.join("config.toml"), &edited).unwrap();
    let after = plugin.launch_defaults(&probe.env, Some(&probe.work));
    ev["s8_launch_defaults"] = json!({
        "before": {"model": before.model, "choices": before.choices},
        "after": {"model": after.model, "choices": after.choices},
    });
    check(
        after.model.as_deref() == Some(model_b.as_str())
            && after.choices.get("approval").map(String::as_str) == Some("approveForMe"),
        "#8 改配置后新建页默认值应跟上",
    );

    // ---- 对照：装了 Lead 才注入（只为证明上面的「未注入」来自安装状态门，不是恰好缺 reporter）。
    let install = |installed: bool| {
        crate::lead_runtime::update_installation(&meowo_protocol::ipc::LeadProviderInstallationDto {
            provider: "codex".into(),
            profile_id: None,
            profile_name: None,
            name: "Codex".into(),
            version: None,
            configured: true,
            supported: true,
            managed: true,
            installed,
            assets_installed: installed,
            hooks_installed: installed,
            automatic_continue: installed,
            notes: Vec::new(),
            reason: None,
        })
    };
    install(true);
    let enabled_when_installed = crate::lead_runtime::enabled("codex", &probe.env);
    install(false);
    ev["control_leadInstalledGate"] = json!({"enabledWhenInstalled": enabled_when_installed});
    ev["fakeModelRequests"] = json!(*probe.log.lock().unwrap());
    ev["failures"] = json!(failures);
    save(&ev);
    println!("证据：{}", probe.out.display());
    assert!(failures.is_empty(), "未过：{failures:?}");
    let _ = root;
}
