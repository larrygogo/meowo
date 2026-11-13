use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex as StdMutex};

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "meowo-oc-http-{tag}-{}-{}",
        std::process::id(),
        now_ms()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[derive(Default)]
struct Seen {
    requests: Vec<(String, String, String)>,
}

/// 最小的 OpenCode 服务替身：只实现投递用到的四个端点，记录收到的请求。
fn fake_server(password: &str, last_user: Value) -> (u16, Arc<StdMutex<Seen>>) {
    fake_server_with(password, move |route| {
        if route == "/session/ses_abc" {
            (200, r#"{"id":"ses_abc","directory":"C:\\proj"}"#.to_owned())
        } else if route == "/session/ses_abc/message" {
            (
                200,
                json!([{"info":{"role":"assistant"}}, last_user, {"info":{"role":"assistant"}}])
                    .to_string(),
            )
        } else if route == "/session/ses_abc/prompt_async" {
            (204, String::new())
        } else {
            (404, String::new())
        }
    })
}

/// 带路由的服务替身：鉴权与 /global/health 统一处理，其余路径交给 `route`。
/// 期望的 Basic Auth 用户名取自插件声明（LeadCap::server_auth），顺带钉住这条取值链路。
fn fake_server_with(
    password: &str,
    route_of: impl Fn(&str) -> (u16, String) + Send + 'static,
) -> (u16, Arc<StdMutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(StdMutex::new(Seen::default()));
    let username = server_auth().expect("插件声明了服务鉴权").username;
    let expected = {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
        )
    };
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_owned();
            let path = parts.next().unwrap_or("").to_owned();
            let (mut length, mut auth) = (0usize, String::new());
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                    break;
                }
                let lower = header.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if lower.starts_with("authorization:") {
                    auth = header["authorization:".len()..].trim().to_owned();
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            log.lock().unwrap().requests.push((
                method.clone(),
                path.clone(),
                String::from_utf8_lossy(&body).into_owned(),
            ));
            let route = path.split('?').next().unwrap_or("");
            let (status, payload) = if auth != expected {
                (401, String::new())
            } else if route == "/global/health" {
                (200, r#"{"healthy":true,"version":"1.19.5"}"#.to_owned())
            } else {
                route_of(route)
            };
            let reason = match status {
                200 => "OK",
                204 => "No Content",
                401 => "Unauthorized",
                _ => "Not Found",
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    (port, seen)
}

fn target<'a>(db: &'a Path, task: &'a Path, force: bool) -> NativeTarget<'a> {
    NativeTarget {
        db,
        session_id: 7,
        provider: "opencode",
        native_session: "ses_abc",
        cwd: Some("C:\\proj"),
        task,
        gate_id: "gate-1",
        message: "关卡已答复：继续",
        force,
    }
}

fn register_tui(db: &Path, pid: u32, port: u16, password: &str) {
    let path = registry_path(db);
    let mut reg = Registry::load(&path);
    reg.endpoints.push(Endpoint {
        kind: Kind::Tui,
        pid,
        port,
        password: password.into(),
        native_session: None,
        cwd: None,
        attach_pid: None,
        attach_pending_ms: None,
        aliases: Vec::new(),
        started_ms: now_ms(),
    });
    reg.save(&path).unwrap();
}

fn prompts(seen: &Arc<StdMutex<Seen>>) -> Vec<Value> {
    seen.lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(m, p, _)| m == "POST" && p.contains("/prompt_async"))
        .map(|(_, _, body)| serde_json::from_str(body).unwrap())
        .collect()
}

#[test]
fn online_tui_receives_answer_once_with_its_own_agent_and_model() {
    let root = temp_root("online");
    let db = root.join("board.db");
    let task = root.join("task");
    let last_user = json!({"info":{"role":"user","agent":"plan","model":{"providerID":"opencode","modelID":"m1"},"variant":"high"}});
    let (port, seen) = fake_server("pw-secret-1", last_user);
    register_tui(&db, 4242, port, "pw-secret-1");

    let outcome = deliver_with(&target(&db, &task, false), Presence::Managed(Some(4242))).unwrap();
    assert_eq!(outcome, NativeOutcome::Delivered);
    // 关卡巡检会反复调 deliver：已被服务接收的答复不能再送第二次。
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(4242))).unwrap();
    deliver_with(&target(&db, &task, true), Presence::Managed(Some(4242))).unwrap();
    let sent = prompts(&seen);
    assert_eq!(sent.len(), 1, "同一答复只投递一次");
    let body = &sent[0];
    // 沿用 TUI 当前的 agent/model/variant：plan 模式下注入的回合不能退回默认 build 拿到写权限。
    assert_eq!(body["agent"], "plan");
    assert_eq!(
        body["model"],
        json!({"providerID":"opencode","modelID":"m1"})
    );
    assert_eq!(body["variant"], "high");
    assert_eq!(body["parts"][0]["text"], "关卡已答复：继续");
    assert!(body["messageID"].as_str().unwrap().starts_with("msg_"));
    assert!(body["parts"][0]["id"].as_str().unwrap().starts_with("prt_"));
    // 会话目录随请求带上，服务端按它选项目实例。
    let paths: Vec<String> = seen
        .lock()
        .unwrap()
        .requests
        .iter()
        .map(|r| r.1.clone())
        .collect();
    assert!(paths
        .iter()
        .any(|p| p.starts_with("/session/ses_abc/prompt_async?directory=C%3A%5Cproj")));
    // 只经输入框以外的通道投递：从不调用会改输入框的 /tui/* 端点。
    assert!(paths.iter().all(|p| !p.starts_with("/tui/")));
    assert_eq!(status(&db, &task, "gate-1", "ses_abc").state, "accepted");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn running_tui_without_port_is_left_alone_and_reported_as_manual() {
    let root = temp_root("noport");
    let db = root.join("board.db");
    let task = root.join("task");
    // TUI 在跑但不是带端口启动的（Lead 安装前开的、外部终端里的）：不能再起第二个驱动者。
    let outcome = deliver_with(&target(&db, &task, false), Presence::Elsewhere(999_999)).unwrap();
    assert_eq!(outcome, NativeOutcome::Unsupported);
    assert_eq!(
        status(&db, &task, "gate-1", "ses_abc").state,
        "native_manual"
    );
    let reg = Registry::load(&registry_path(&db));
    assert!(
        reg.endpoints.iter().all(|e| e.kind != Kind::Serve),
        "没有起 serve"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn wrong_password_fails_retryably_without_resending_blindly() {
    let root = temp_root("auth");
    let db = root.join("board.db");
    let task = root.join("task");
    let (port, seen) = fake_server("right-password", json!({}));
    register_tui(&db, 5151, port, "stale-password");
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(5151))).unwrap();
    let state = status(&db, &task, "gate-1", "ses_abc");
    assert_eq!(state.state, "failed");
    assert!(state.can_retry);
    // 退避期内巡检不重试；面板「重试」（force）立即重试，且沿用同一 messageID（服务端按 id 去重）。
    let first = Registry::load(&registry_path(&db))
        .deliveries
        .values()
        .next()
        .unwrap()
        .clone();
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(5151))).unwrap();
    assert_eq!(
        Registry::load(&registry_path(&db))
            .deliveries
            .values()
            .next()
            .unwrap()
            .attempts,
        1
    );
    deliver_with(&target(&db, &task, true), Presence::Managed(Some(5151))).unwrap();
    let second = Registry::load(&registry_path(&db))
        .deliveries
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(second.attempts, 2);
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.part_id, first.part_id);
    assert!(prompts(&seen).is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ascending_ids_match_opencode_shape_and_sort_after_older_messages() {
    let older = "msg_1d3d91395001mv5f0vn5ht1GOx"; // 1.19.5 实拍 TUI 消息 id
    let id = ascending_id("msg", 1_794_555_999_000, "lead-key");
    assert_eq!(id.len(), older.len());
    assert!(id.as_str() > older, "新消息必须排在既有消息之后：{id}");
    assert_eq!(
        id,
        ascending_id("msg", 1_794_555_999_000, "lead-key"),
        "同一投递 id 稳定"
    );
    assert_ne!(id, ascending_id("msg", 1_794_555_999_000, "other-key"));
    assert!(ascending_id("prt", 1, "k").starts_with("prt_"));
}

#[test]
fn resume_argv_is_recognized_but_fork_is_not() {
    let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        resumed_session(&argv(&["opencode", "--session", "ses_x1"])),
        Some("ses_x1")
    );
    assert_eq!(
        resumed_session(&argv(&["opencode", "--session", "ses_x1", "--fork"])),
        None
    );
    assert_eq!(resumed_session(&argv(&["opencode"])), None);
    assert_eq!(
        resumed_session(&argv(&["opencode", "--session", "a b"])),
        None
    );
}

#[test]
fn launch_plan_opens_password_protected_port_and_attaches_to_live_serve() {
    let root = temp_root("plan");
    let db = root.join("board.db");
    let argv = vec![
        "opencode.exe".to_string(),
        "--session".into(),
        "ses_abc".into(),
    ];
    let plan = plan_for(&db, &argv).unwrap();
    assert!(plan.argv.is_none());
    assert_eq!(plan.args[..3], ["--hostname", "127.0.0.1", "--port"]);
    assert_eq!(plan.env[0].0, "OPENCODE_SERVER_PASSWORD");
    assert!(plan.env[0].1.len() >= 32, "随机密码");
    assert!(
        !plan.args.iter().any(|a| a == &plan.env[0].1),
        "密码不进命令行"
    );
    plan.launched(Some(31337));
    assert!(Registry::load(&registry_path(&db))
        .endpoints
        .iter()
        .any(|e| e.kind == Kind::Tui && e.pid == 31337));

    // serve 正驱动该会话时，打开会话改为 attach 连上去，而不是再起一个 TUI 服务。
    let (port, _seen) = fake_server("serve-pw", json!({}));
    let path = registry_path(&db);
    let mut reg = Registry::load(&path);
    reg.endpoints.push(Endpoint {
        kind: Kind::Serve,
        pid: 777,
        port,
        password: "serve-pw".into(),
        native_session: Some("ses_abc".into()),
        cwd: None,
        attach_pid: None,
        attach_pending_ms: None,
        aliases: Vec::new(),
        started_ms: now_ms(),
    });
    reg.save(&path).unwrap();
    let plan = plan_for(&db, &argv).unwrap();
    let attach = plan.argv.clone().unwrap();
    assert_eq!(
        attach[1..],
        [
            "attach".to_string(),
            format!("http://127.0.0.1:{port}"),
            "--session".into(),
            "ses_abc".into()
        ]
    );
    assert_eq!(
        plan.env,
        vec![(
            "OPENCODE_SERVER_PASSWORD".to_string(),
            "serve-pw".to_string()
        )]
    );
    plan.launched(Some(888));
    let reg = Registry::load(&path);
    assert_eq!(
        reg.endpoints
            .iter()
            .find(|e| e.pid == 777)
            .unwrap()
            .attach_pid,
        Some(888)
    );
    // 有活着的 serve 时投递走它（单一驱动者），即便会话库里记着 serve 的 pid。
    let outcome = deliver_with(
        &target(&db, &root.join("task"), false),
        Presence::Elsewhere(777),
    )
    .unwrap();
    assert_eq!(outcome, NativeOutcome::Delivered);
    assert_eq!(
        status(&db, &root.join("task"), "gate-1", "ses_abc").state,
        "accepted"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn last_user_choice_ignores_missing_fields() {
    assert_eq!(last_user_choice(&Value::Null), json!({}));
    let only_agent = json!([{"info":{"role":"user","agent":"build"}}]);
    assert_eq!(last_user_choice(&only_agent), json!({"agent":"build"}));
}

/// 父链替身：`sessions` 里每项是（id，parentID），`last_user` 只挂在主会话的消息列表上。
fn chain_server(
    password: &str,
    sessions: &'static [(&'static str, Option<&'static str>)],
    root: &'static str,
    last_user: Value,
) -> (u16, Arc<StdMutex<Seen>>) {
    fake_server_with(password, move |route| {
        let rest = route.strip_prefix("/session/").unwrap_or("");
        let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let Some((_, parent)) = sessions.iter().find(|(s, _)| *s == id) else {
            return (404, String::new());
        };
        match tail {
            "" => {
                let mut info = json!({"id": id, "directory": "C:\\proj"});
                if let Some(parent) = parent {
                    info["parentID"] = json!(parent);
                }
                (200, info.to_string())
            }
            "message" if id == root => (200, json!([last_user]).to_string()),
            "message" => (
                200,
                json!([{"info":{"role":"user","agent":"explore"}}]).to_string(),
            ),
            "prompt_async" => (204, String::new()),
            _ => (404, String::new()),
        }
    })
}

fn sub_target<'a>(db: &'a Path, task: &'a Path, native: &'a str) -> NativeTarget<'a> {
    NativeTarget {
        native_session: native,
        ..target(db, task, false)
    }
}

fn posted_paths(seen: &Arc<StdMutex<Seen>>) -> Vec<String> {
    seen.lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(m, _, _)| m == "POST")
        .map(|(_, p, _)| p.split('?').next().unwrap_or("").to_owned())
        .collect()
}

fn serve_endpoint(pid: u32, port: u16, password: &str, native: &str) -> Endpoint {
    Endpoint {
        kind: Kind::Serve,
        pid,
        port,
        password: password.into(),
        native_session: Some(native.into()),
        cwd: None,
        attach_pid: None,
        attach_pending_ms: None,
        aliases: Vec::new(),
        started_ms: now_ms(),
    }
}

fn resume_argv(native: &str) -> Vec<String> {
    vec!["opencode.exe".into(), "--session".into(), native.into()]
}

#[test]
fn subagent_owner_answer_is_delivered_to_root_session() {
    // H3：.owner 记的是子代理会话（task 工具派出、parentID 非空）。答复投进子会话 TUI 主视图看不到，
    // 必须沿父链投给主会话，并沿用主会话（而不是子代理）的 agent。
    static CHAIN: &[(&str, Option<&str>)] = &[
        ("ses_child", Some("ses_mid")),
        ("ses_mid", Some("ses_root")),
        ("ses_root", None),
    ];
    let root = temp_root("subagent");
    let db = root.join("board.db");
    let task = root.join("task");
    let last_user = json!({"info":{"role":"user","agent":"plan"}});
    let (port, seen) = chain_server("pw-sub", CHAIN, "ses_root", last_user);
    register_tui(&db, 4343, port, "pw-sub");

    let outcome = deliver_with(
        &sub_target(&db, &task, "ses_child"),
        Presence::Managed(Some(4343)),
    )
    .unwrap();
    assert_eq!(outcome, NativeOutcome::Delivered);
    assert_eq!(posted_paths(&seen), ["/session/ses_root/prompt_async"]);
    assert_eq!(prompts(&seen)[0]["agent"], "plan");
    // 投递记录仍按 .owner 的会话记账，面板按它查状态。
    assert_eq!(status(&db, &task, "gate-1", "ses_child").state, "accepted");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn broken_or_cyclic_parent_chain_fails_without_posting() {
    static CYCLE: &[(&str, Option<&str>)] = &[("ses_a", Some("ses_b")), ("ses_b", Some("ses_a"))];
    static ORPHAN: &[(&str, Option<&str>)] = &[("ses_c", Some("ses_gone"))];
    for (tag, chain, owner) in [("cycle", CYCLE, "ses_a"), ("orphan", ORPHAN, "ses_c")] {
        let root = temp_root(tag);
        let db = root.join("board.db");
        let task = root.join("task");
        let (port, seen) = chain_server("pw-chain", chain, "ses_none", json!({}));
        register_tui(&db, 4444, port, "pw-chain");
        deliver_with(
            &sub_target(&db, &task, owner),
            Presence::Managed(Some(4444)),
        )
        .unwrap();
        let state = status(&db, &task, "gate-1", owner);
        assert_eq!(state.state, "failed", "{tag}");
        assert_eq!(
            state.reason.as_deref(),
            Some("lead/opencode_parent_unresolved"),
            "{tag}"
        );
        assert!(posted_paths(&seen).is_empty(), "{tag}：父链不通时不猜着投");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn serve_started_for_subagent_rebinds_to_root_so_opening_main_session_attaches() {
    static CHAIN: &[(&str, Option<&str>)] = &[("ses_child", Some("ses_root")), ("ses_root", None)];
    let root = temp_root("serve-root");
    let db = root.join("board.db");
    let task = root.join("task");
    let (port, seen) = chain_server("serve-pw", CHAIN, "ses_root", json!({}));
    let path = registry_path(&db);
    let mut reg = Registry::load(&path);
    reg.endpoints
        .push(serve_endpoint(777, port, "serve-pw", "ses_child"));
    reg.save(&path).unwrap();

    deliver_with(&sub_target(&db, &task, "ses_child"), Presence::Gone).unwrap();
    assert_eq!(posted_paths(&seen), ["/session/ses_root/prompt_async"]);
    let reg = Registry::load(&path);
    let serve = reg.endpoints.iter().find(|e| e.pid == 777).unwrap();
    assert_eq!(serve.native_session.as_deref(), Some("ses_root"));
    assert_eq!(serve.aliases, ["ses_child"]);
    // 用户打开的是主会话：要 attach 到这个 serve，而不是另起一个普通 TUI 抢着驱动。
    let plan = plan_for(&db, &resume_argv("ses_root")).unwrap();
    assert_eq!(plan.argv.as_ref().unwrap()[1], "attach");
    // 子会话 id 的后续投递仍认得同一个 serve（不再起第二个）。
    assert!(live_serve(&Registry::load(&path), "ses_child").is_some());
    drop(plan);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn delivery_only_touches_session_endpoints_never_tui_input_or_permissions() {
    // 草稿与审批的不变量（1.19.5 源码核对，见 post_prompt 注释）：投递只用会话三端点。
    // 替身同时挂着一个待批 permission 和任意 /tui/* 端点，任何访问都会被记下来。
    let root = temp_root("surface");
    let db = root.join("board.db");
    let task = root.join("task");
    let (port, seen) = fake_server_with("pw-surface", |route| match route {
        "/session/ses_abc" => (200, r#"{"id":"ses_abc","directory":"C:\\proj"}"#.to_owned()),
        "/session/ses_abc/message" => (200, "[]".to_owned()),
        "/session/ses_abc/prompt_async" => (204, String::new()),
        "/permission" => (
            200,
            r#"[{"id":"per_1","sessionID":"ses_abc","permission":"bash"}]"#.to_owned(),
        ),
        _ => (200, "true".to_owned()),
    });
    register_tui(&db, 4545, port, "pw-surface");
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(4545))).unwrap();
    let routes: Vec<(String, String)> = seen
        .lock()
        .unwrap()
        .requests
        .iter()
        .map(|(m, p, _)| (m.clone(), p.split('?').next().unwrap_or("").to_owned()))
        .filter(|(_, p)| p != "/global/health")
        .collect();
    let expected = [
        ("GET", "/session/ses_abc"),
        ("GET", "/session/ses_abc/message"),
        ("POST", "/session/ses_abc/prompt_async"),
    ]
    .map(|(m, p)| (m.to_owned(), p.to_owned()));
    assert_eq!(routes, expected);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn attach_placeholder_stops_reaper_from_killing_serve_until_spawn_settles() {
    // M5：plan_for 决定 attach 后、attach 进程 spawn 出来前，回收线程判定空闲也不能结束 serve。
    let root = temp_root("attach-race");
    let db = root.join("board.db");
    let path = registry_path(&db);
    let (port, _seen) = fake_server("serve-pw", json!({}));
    let mut reg = Registry::load(&path);
    reg.endpoints
        .push(serve_endpoint(777, port, "serve-pw", "ses_abc"));
    reg.save(&path).unwrap();
    let killed = StdMutex::new(Vec::new());
    let killer = |pid: u32| killed.lock().unwrap().push(pid);

    // 1. 占位在：回收线程的结束请求被拒，serve 条目保留，没有结束任何进程。
    let plan = plan_for(&db, &resume_argv("ses_abc")).unwrap();
    assert!(plan.argv.is_some(), "serve 活着时应走 attach");
    assert!(!stop_serve(&path, 777, true, killer));
    assert!(killed.lock().unwrap().is_empty());
    // 2. spawn 失败（方案没走到 launched 就被丢弃）：占位撤掉，serve 回到按空闲回收。
    drop(plan);
    assert!(Registry::load(&path).endpoints[0]
        .attach_pending_ms
        .is_none());
    assert!(stop_serve(&path, 777, true, killer));
    assert_eq!(*killed.lock().unwrap(), [777]);
    // 3. serve 先被结束：同一把锁保证之后的打开看不到它，改起普通 TUI，而不是 attach 到死服务。
    let plan = plan_for(&db, &resume_argv("ses_abc")).unwrap();
    assert!(plan.argv.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn launched_attach_replaces_placeholder_with_pid() {
    let root = temp_root("attach-launched");
    let db = root.join("board.db");
    let path = registry_path(&db);
    let (port, _seen) = fake_server("serve-pw", json!({}));
    let mut reg = Registry::load(&path);
    reg.endpoints
        .push(serve_endpoint(777, port, "serve-pw", "ses_abc"));
    reg.save(&path).unwrap();
    let plan = plan_for(&db, &resume_argv("ses_abc")).unwrap();
    assert!(Registry::load(&path).endpoints[0]
        .attach_pending_ms
        .is_some());
    plan.launched(Some(9898));
    let entry = Registry::load(&path).endpoints[0].clone();
    assert_eq!(entry.attach_pid, Some(9898));
    assert!(entry.attach_pending_ms.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn startup_adopt_keeps_serves_and_clears_dead_tuis_and_stale_placeholders() {
    // Meowo 不在时 serve 没有别的时限（opencode serve 无空闲自退参数），启动时必须全部接回回收。
    let root = temp_root("adopt");
    let db = root.join("board.db");
    let path = registry_path(&db);
    let mut reg = Registry::load(&path);
    let mut serve = serve_endpoint(777, 1, "pw", "ses_abc");
    serve.attach_pending_ms = Some(now_ms());
    reg.endpoints.push(serve);
    reg.save(&path).unwrap();
    register_tui(&db, 4_000_000_001, 2, "pw-tui");

    assert_eq!(adopt_scan(&db), [777]);
    let reg = Registry::load(&path);
    assert_eq!(
        reg.endpoints.len(),
        1,
        "已退出的 TUI 条目（含其密码）被清掉"
    );
    assert!(
        reg.endpoints[0].attach_pending_ms.is_none(),
        "上个进程的 attach 占位作废"
    );
    let _ = std::fs::remove_dir_all(root);
}
