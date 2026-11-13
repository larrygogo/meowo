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
        pid,
        port,
        password: password.into(),
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
    assert!(Registry::load(&registry_path(&db)).endpoints.is_empty());
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
fn launch_plan_opens_password_protected_port() {
    let root = temp_root("plan");
    let db = root.join("board.db");
    let plan = plan_for(&db).unwrap();
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
        .any(|e| e.pid == 31337));
    let _ = std::fs::remove_dir_all(root);
}

/// leader 2026-11-13「不需要离线」：TUI 不在时不另起服务、不投递，记为「等会话打开」（不计次）；
/// 用户重新打开会话（TUI 带端口起来）后，下一次巡检经 prompt_async 送达。
#[test]
fn answer_waits_for_the_tui_to_reopen_then_patrol_delivers_it() {
    let root = temp_root("reopen");
    let db = root.join("board.db");
    let task = root.join("task");
    for _ in 0..3 {
        let outcome = deliver_with(&target(&db, &task, false), Presence::Gone).unwrap();
        assert_eq!(outcome, NativeOutcome::Saved);
    }
    let state = status(&db, &task, "gate-1", "ses_abc");
    assert_eq!((state.state, state.can_retry), ("native_on_open", false));
    let reg = Registry::load(&registry_path(&db));
    assert!(reg.endpoints.is_empty(), "没有另起任何服务");
    assert_eq!(reg.deliveries.values().next().unwrap().attempts, 0, "等待不计投递次数");

    // TUI 刚起、服务还没监听：仍是等待，不进失败退避。
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = closed.local_addr().unwrap().port();
    drop(closed);
    register_tui(&db, 6060, dead_port, "pw-reopen");
    let outcome = deliver_with(&target(&db, &task, false), Presence::Managed(Some(6060))).unwrap();
    assert_eq!(outcome, NativeOutcome::Saved);
    assert_eq!(status(&db, &task, "gate-1", "ses_abc").state, "native_on_open");

    // 服务起来了：巡检送达，只送一次。
    let (port, seen) = fake_server("pw-reopen", json!({}));
    register_tui(&db, 6161, port, "pw-reopen");
    let outcome = deliver_with(&target(&db, &task, false), Presence::Managed(Some(6161))).unwrap();
    assert_eq!(outcome, NativeOutcome::Delivered);
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(6161))).unwrap();
    assert_eq!(prompts(&seen).len(), 1);
    assert_eq!(status(&db, &task, "gate-1", "ses_abc").state, "accepted");
    let _ = std::fs::remove_dir_all(root);
}

/// 来者不拒的服务替身：任何鉴权都回 200 健康（模拟抢到我们端口的别的服务，或没设密码的服务）。
fn impostor_server() -> (u16, Arc<StdMutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(StdMutex::new(Seen::default()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                    break;
                }
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_owned();
            let path = parts.next().unwrap_or("").to_owned();
            log.lock()
                .unwrap()
                .requests
                .push((method, path, String::new()));
            let payload = r#"{"healthy":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    (port, seen)
}

/// 做空 L4：free_port 放开后端口被别的服务抢了——带我们的密码也 200，但诱饵密码同样 200，
/// 认不出是自己的 TUI，不投递、记失败（可重试）。自己的服务：本密码 200、诱饵 401，照常投递。
#[test]
fn port_taken_by_another_service_is_never_delivered_to() {
    let root = temp_root("l4");
    let db = root.join("board.db");
    let task = root.join("task");
    let (port, seen) = impostor_server();
    register_tui(&db, 7070, port, "pw-mine");
    deliver_with(&target(&db, &task, false), Presence::Managed(Some(7070))).unwrap();
    let state = status(&db, &task, "gate-1", "ses_abc");
    assert_eq!(state.state, "failed");
    assert_eq!(state.reason.as_deref(), Some("lead/opencode_http_identity"));
    assert!(state.can_retry);
    assert!(
        seen.lock()
            .unwrap()
            .requests
            .iter()
            .all(|(m, p, _)| m == "GET" && p.starts_with("/global/health")),
        "只做了身份核对，没碰会话"
    );

    let (port, _) = fake_server("pw-mine", json!({}));
    let client = Client::new(port, "pw-mine");
    assert_eq!(client.verify_identity(Duration::from_secs(3)), Ok(()));
    let wrong = Client::new(port, "not-mine");
    assert_eq!(
        wrong.verify_identity(Duration::from_secs(3)),
        Err("lead/opencode_http_unauthorized".into())
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

/// 第二轮 P2：用户按 Esc 拒绝审批后，排着的答复留在会话里不跑，面板原来一直「已接收」。
/// 1.19.5 没有只让会话接着跑的接口，巡检认出「会话空闲、我们那条是最后一条用户消息、没有回复」
/// 就如实改成 queued_stalled；会话忙、已有回复、用户之后又发过话都不算卡住。不发任何 POST。
#[test]
fn stalled_answer_is_reported_without_posting_anything() {
    let root = temp_root("stall");
    let db = root.join("board.db");
    let task = root.join("task");
    let key = delivery_key(&task, "gate-1", "ses_abc");
    let ours = "msg_ours";
    let scenario = Arc::new(StdMutex::new((json!({}), json!([]))));
    let routes = scenario.clone();
    let (port, seen) = fake_server_with("pw-stall", move |route| {
        let (status, messages) = routes.lock().unwrap().clone();
        match route {
            "/session/ses_abc" => (200, r#"{"id":"ses_abc","directory":"C:\\proj"}"#.to_owned()),
            "/session/status" => (200, status.to_string()),
            "/session/ses_abc/message" => (200, messages.to_string()),
            _ => (404, String::new()),
        }
    });
    register_tui(&db, 4646, port, "pw-stall");
    let accepted = |at_ms: i64| {
        let path = registry_path(&db);
        let mut reg = Registry::load(&path);
        reg.deliveries.insert(
            key.clone(),
            Delivery {
                state: "accepted".into(),
                message_id: ours.into(),
                part_id: "prt_ours".into(),
                attempts: 1,
                at_ms,
                reason: None,
                stalled: false,
            },
        );
        reg.save(&path).unwrap();
    };
    let state = || status(&db, &task, "gate-1", "ses_abc").state;
    let patrol = || deliver_with(&target(&db, &task, false), Presence::Managed(Some(4646))).unwrap();
    let user = |id: &str| json!({"info":{"id":id,"role":"user"}});
    let reply_to = |id: &str| json!({"info":{"id":"msg_r","role":"assistant","parentID":id}});
    let stalled = json!([user("msg_old"), reply_to("msg_old"), user(ours)]);

    // 刚接收（宽限期内）：不查。
    accepted(now_ms());
    *scenario.lock().unwrap() = (json!({}), stalled.clone());
    patrol();
    assert_eq!(state(), "accepted");
    // 过了宽限期、会话空闲、我们那条没有回复：卡住。
    accepted(now_ms() - STALL_GRACE_MS - 1);
    patrol();
    assert_eq!(state(), crate::lead_native::QUEUED_STALLED);
    // 用户又发了一句、那一轮带上了它：不再算卡住。
    let resumed = json!([user(ours), user("msg_user"), reply_to("msg_user")]);
    *scenario.lock().unwrap() = (json!({}), resumed);
    patrol();
    assert_eq!(state(), "accepted");
    // 会话忙（在 /session/status 里）或已有回复：都不算。
    *scenario.lock().unwrap() = (json!({"ses_abc":{"type":"busy"}}), stalled);
    patrol();
    assert_eq!(state(), "accepted");
    *scenario.lock().unwrap() = (json!({}), json!([user(ours), reply_to(ours)]));
    patrol();
    assert_eq!(state(), "accepted");
    assert!(prompts(&seen).is_empty(), "核对卡住不能再投一遍");
    let _ = std::fs::remove_dir_all(root);
}

/// 第二轮 P4：TUI 退出后它的端口条目（带密码）不再留在登记表里；本次指着的那个保留。
#[test]
fn patrol_prunes_endpoints_of_exited_tuis() {
    let root = temp_root("prune");
    let db = root.join("board.db");
    let task = root.join("task");
    register_tui(&db, 4_294_000, 1, "gone"); // 不存在的进程
    register_tui(&db, 4747, 2, "here");
    let _ = deliver_with(&target(&db, &task, false), Presence::Managed(Some(4747)));
    let pids: Vec<u32> = Registry::load(&registry_path(&db))
        .endpoints
        .iter()
        .map(|e| e.pid)
        .collect();
    assert_eq!(pids, [4747]);
    let _ = std::fs::remove_dir_all(root);
}

// ---------------------------------------------------------------------------------------------
// 会话设置（任务 20261114-tui-model-switch）

/// 假 TUI 的可见状态：开着的对话框、搜索框内容、底栏。服务替身的 /tui/* 路由会改它，
/// 宿主的写入回调（PTY）也改它，读屏回调按它画出画面（标题行与底栏取 oc4/oc5 画面原文）。
#[derive(Default)]
struct FakeTui {
    dialog: Option<&'static str>,
    search: String,
    footer: String,
    /// 回车能选中的名字（对话框里列着的项）。
    choices: Vec<&'static str>,
    /// 模拟对话框弹不出来（TUI 卡住、焦点被别的层抢走）。
    stuck: bool,
}

type SharedTui = Arc<StdMutex<FakeTui>>;

fn fake_tui(footer: &str, choices: Vec<&'static str>) -> SharedTui {
    Arc::new(StdMutex::new(FakeTui {
        footer: footer.into(),
        choices,
        ..Default::default()
    }))
}

fn screen_of(tui: &SharedTui) -> String {
    let tui = tui.lock().unwrap();
    let mut screen = String::from("┃  hello one\n");
    if let Some(title) = tui.dialog {
        screen.push_str(&format!(
            "                                            {title}                                     esc\n"
        ));
        screen.push_str(&format!("                                            {}\n", tui.search));
    }
    screen.push_str(&format!("┃  {}\n", tui.footer));
    screen
}

/// 宿主的终端通道替身：记下每次写入，并像真 TUI 一样处理按键。
fn io_for(tui: &SharedTui, writes: &Arc<StdMutex<Vec<String>>>) -> impl TuiIo {
    let (t1, t2, log) = (tui.clone(), tui.clone(), writes.clone());
    (
        move |text: &str| -> Result<(), String> {
            log.lock().unwrap().push(text.to_owned());
            let mut tui = t1.lock().unwrap();
            match (tui.dialog, text) {
                (Some(_), "\x1b") => tui.dialog = None,
                (Some(title), "\r") => {
                    let search = tui.search.clone();
                    let hit = tui
                        .choices
                        .iter()
                        .find(|c| c.eq_ignore_ascii_case(&search))
                        .copied();
                    if let Some(hit) = hit {
                        tui.footer = if title == "Select model" {
                            format!("Build · {hit} Fake")
                        } else {
                            let mut name = hit.to_owned();
                            name[..1].make_ascii_uppercase();
                            format!("{name} · Fake One Fake")
                        };
                        tui.dialog = None;
                    }
                }
                (Some(_), typed) => tui.search.push_str(typed),
                // 对话框不在时字会落进输入框——测试里出现即说明守卫失效。
                (None, typed) => panic!("对话框不在时往终端写了 {typed:?}"),
            }
            Ok(())
        },
        move || Some(screen_of(&t2)),
    )
}

/// 服务替身：会话端点 + 待批列表 + 会话状态 + 两个开对话框的 /tui 端点（开框即改假 TUI）。
fn control_server(
    password: &str,
    tui: SharedTui,
    permissions: &'static str,
    status: &'static str,
) -> (u16, Arc<StdMutex<Seen>>) {
    fake_server_with(password, move |route| match route {
        "/permission" => (200, permissions.to_owned()),
        "/question" => (200, "[]".to_owned()),
        "/session/ses_abc" => (200, r#"{"id":"ses_abc","directory":"C:\\proj"}"#.to_owned()),
        "/session/status" => (200, status.to_owned()),
        "/session/ses_abc/message" => (
            200,
            json!([
                {"info":{"role":"user","agent":"plan","model":{"providerID":"fake","modelID":"fake-model-2"},"variant":"high"}},
                {"info":{"role":"assistant"}}
            ])
            .to_string(),
        ),
        // oc1 实测形状（截短）：只含已连接 provider，models 是对象，variants 是对象。
        "/config/providers" => (
            200,
            json!({"providers":[{"id":"fake","name":"Fake","models":{
                "fake-model":{"id":"fake-model","name":"Fake One"},
                "fake-model-3":{"id":"fake-model-3","name":"Fake Three","variants":{"high":{"reasoningEffort":"high"}}},
                "fake-model-2":{"id":"fake-model-2","name":"Fake Two"}}}],
                "default":{"fake":"fake-model"}})
            .to_string(),
        ),
        "/agent" => (
            200,
            json!([
                {"name":"build","description":"The default agent.","mode":"primary","native":true,"permission":[]},
                {"name":"plan","description":"Plan mode. Disallows all edit tools.","mode":"primary","native":true},
                {"name":"explore","mode":"subagent","native":true},
                {"name":"title","mode":"primary","native":true,"hidden":true},
                {"name":"reviewer","mode":"all","native":false}
            ])
            .to_string(),
        ),
        "/tui/open-models" | "/tui/publish" => {
            let mut tui = tui.lock().unwrap();
            if !tui.stuck {
                tui.dialog = Some(if route == "/tui/open-models" {
                    "Select model"
                } else {
                    "Select agent"
                });
                tui.search.clear();
            }
            (200, "true".to_owned())
        }
        _ => (404, String::new()),
    })
}

const READY: TuiState = TuiState {
    idle: true,
    composer_empty: true,
    approval_pending: false,
};

fn switch_req(db: &Path, pid: u32, state: TuiState) -> DialogSwitch<'_> {
    DialogSwitch {
        db,
        presence: Presence::Managed(Some(pid)),
        native_session: "ses_abc",
        cwd: Some("C:\\proj"),
        state,
    }
}

fn no_sleep(_: Duration) {}

fn run_switch(
    db: &Path,
    pid: u32,
    state: TuiState,
    dialog: Dialog,
    name: &str,
    io: &mut dyn TuiIo,
) -> Result<SwitchOutcome, ControlError> {
    switch_via_dialog(&switch_req(db, pid, state), dialog, name, io, &no_sleep)
}

fn posts(seen: &Arc<StdMutex<Seen>>) -> Vec<(String, String)> {
    seen.lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(m, _, _)| m == "POST")
        .map(|(_, p, b)| (p.split('?').next().unwrap_or("").to_owned(), b.clone()))
        .collect()
}

#[test]
fn catalog_agents_and_current_choice_come_from_the_tui_server() {
    let root = temp_root("catalog");
    let db = root.join("board.db");
    let tui = fake_tui("Build · Fake One Fake", vec![]);
    let (port, seen) = control_server("pw-cat", tui, "[]", "{}");
    register_tui(&db, 5252, port, "pw-cat");
    let here = Presence::Managed(Some(5252));
    let catalog = list_models(&db, here, Some("C:\\proj")).unwrap();
    assert_eq!(
        catalog.defaults.get("fake").map(String::as_str),
        Some("fake-model")
    );
    let fake = &catalog.providers[0];
    assert_eq!((fake.id.as_str(), fake.name.as_str()), ("fake", "Fake"));
    let names: Vec<&str> = fake.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Fake One", "Fake Three", "Fake Two"]);
    assert_eq!(fake.models[1].variants, ["high"]);
    let agents: Vec<String> = list_agents(&db, here, None)
        .unwrap()
        .into_iter()
        .map(|a| format!("{}:{}", a.name, a.mode))
        .collect();
    // subagent 与 hidden 的不进菜单；mode=all 的自定义 agent 可选。
    assert_eq!(agents, ["build:primary", "plan:primary", "reviewer:all"]);
    assert!(seen
        .lock()
        .unwrap()
        .requests
        .iter()
        .any(|(_, p, _)| p == "/config/providers?directory=C%3A%5Cproj"));
    // 会话当前值：主会话最后一条用户消息。
    let choice = current_choice(&db, here, "ses_abc", Some("C:\\proj"))
        .unwrap()
        .unwrap();
    assert_eq!(
        choice,
        OcChoice {
            agent: Some("plan".into()),
            provider_id: Some("fake".into()),
            model_id: Some("fake-model-2".into()),
            variant: Some("high".into()),
        }
    );
    assert_eq!(choice_of(&json!({})), None);
    assert!(posts(&seen).is_empty(), "列清单、读当前都只读");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn session_without_port_is_unsupported_and_closed_tui_is_not_ready() {
    let root = temp_root("ctl-route");
    let db = root.join("board.db");
    assert_eq!(
        list_models(&db, Presence::Elsewhere(999_999), None),
        Err(ControlError::Unsupported)
    );
    assert_eq!(
        list_agents(&db, Presence::Gone, None),
        Err(ControlError::NotReady("lead/opencode_tui_not_ready".into()))
    );
    let _ = std::fs::remove_dir_all(root);
}

/// 切模型：服务端开模型对话框（/tui/open-models）→ 确认弹出后写显示名 → 回车 → 框关、底栏变。
/// 不用服务端那几个 TUI 不认的「设置」接口，也不发消息。
#[test]
fn switch_model_opens_dialog_then_types_name_and_enter() {
    let root = temp_root("sw-model");
    let db = root.join("board.db");
    let tui = fake_tui("Build · Fake One Fake", vec!["Fake One", "Fake Two"]);
    let (port, seen) = control_server("pw-sw", tui.clone(), "[]", "{}");
    register_tui(&db, 5353, port, "pw-sw");
    let writes = Arc::new(StdMutex::new(Vec::new()));
    let mut io = io_for(&tui, &writes);
    // 走公开入口（真实等待节奏）。
    let outcome = switch_model(&switch_req(&db, 5353, READY), "Fake Two", &mut io);
    assert_eq!(outcome, Ok(SwitchOutcome { confirmed: true }));
    assert_eq!(*writes.lock().unwrap(), ["Fake Two", "\r"]);
    assert_eq!(tui.lock().unwrap().footer, "Build · Fake Two Fake");
    let posted: Vec<String> = posts(&seen).into_iter().map(|(p, _)| p).collect();
    assert_eq!(posted, ["/tui/open-models"]);
    let _ = std::fs::remove_dir_all(root);
}

/// 切 agent：`/tui/publish` 发 tui.command.execute agent.list 打开「Select agent」（oc5/oc6 实测；
/// 走 /tui/execute-command 不弹框）。
#[test]
fn switch_agent_opens_agent_dialog_via_publish() {
    let root = temp_root("sw-agent");
    let db = root.join("board.db");
    let tui = fake_tui("Build · Fake One Fake", vec!["build", "plan"]);
    let (port, seen) = control_server("pw-ag", tui.clone(), "[]", "{}");
    register_tui(&db, 5454, port, "pw-ag");
    let writes = Arc::new(StdMutex::new(Vec::new()));
    let mut io = io_for(&tui, &writes);
    let outcome = switch_agent(&switch_req(&db, 5454, READY), "plan", &mut io);
    assert_eq!(outcome, Ok(SwitchOutcome { confirmed: true }));
    assert_eq!(tui.lock().unwrap().footer, "Plan · Fake One Fake");
    let sent = posts(&seen);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "/tui/publish");
    let body: Value = serde_json::from_str(&sent[0].1).unwrap();
    assert_eq!(
        body,
        json!({"type":"tui.command.execute","properties":{"command":"agent.list"}})
    );
    let _ = std::fs::remove_dir_all(root);
}

/// 调用约定：运行中、输入框有草稿、有待批审批——宿主说的或服务端查到的——一律 NotNow，不写终端、
/// 不开对话框。
#[test]
fn switch_refuses_when_tui_is_busy_drafting_or_awaiting_approval() {
    let root = temp_root("sw-guard");
    let db = root.join("board.db");
    let tui = fake_tui("Build · Fake One Fake", vec!["Fake Two"]);
    let (port, seen) = control_server("pw-guard", tui.clone(), "[]", "{}");
    register_tui(&db, 5555, port, "pw-guard");
    let writes = Arc::new(StdMutex::new(Vec::new()));
    for (state, reason) in [
        (TuiState { idle: false, ..READY }, "lead/opencode_busy"),
        (
            TuiState {
                composer_empty: false,
                ..READY
            },
            "lead/opencode_composer_not_empty",
        ),
        (
            TuiState {
                approval_pending: true,
                ..READY
            },
            "lead/opencode_approval_pending",
        ),
    ] {
        let mut io = io_for(&tui, &writes);
        assert_eq!(
            run_switch(&db, 5555, state, Dialog::Models, "Fake Two", &mut io),
            Err(ControlError::NotNow(reason.into()))
        );
    }
    assert!(
        seen.lock().unwrap().requests.is_empty(),
        "约定不满足时连服务都不碰"
    );

    // 宿主以为没有审批，服务端却有待批权限：同样不动手。
    let (port, seen) = control_server(
        "pw-perm",
        tui.clone(),
        r#"[{"id":"per_1","sessionID":"ses_abc","permission":"bash"}]"#,
        "{}",
    );
    register_tui(&db, 5656, port, "pw-perm");
    let mut io = io_for(&tui, &writes);
    assert_eq!(
        run_switch(&db, 5656, READY, Dialog::Models, "Fake Two", &mut io),
        Err(ControlError::NotNow("lead/opencode_approval_pending".into()))
    );
    assert!(posts(&seen).is_empty());
    // 宿主以为空闲，服务端说主会话在跑。
    let (port, seen) = control_server(
        "pw-busy",
        tui.clone(),
        "[]",
        r#"{"ses_abc":{"type":"busy"}}"#,
    );
    register_tui(&db, 5757, port, "pw-busy");
    let mut io = io_for(&tui, &writes);
    assert_eq!(
        run_switch(&db, 5757, READY, Dialog::Agents, "plan", &mut io),
        Err(ControlError::NotNow("lead/opencode_busy".into()))
    );
    assert!(posts(&seen).is_empty());
    assert!(writes.lock().unwrap().is_empty(), "不满足时一个键都没写");
    let _ = std::fs::remove_dir_all(root);
}

/// 对话框没弹出：一个字都不写（否则名字落进输入框、回车就成了一条消息）。
/// 搜不到：回车后框还在，按 Esc 关掉，如实失败。名字里的控制字符：拒绝。
#[test]
fn switch_never_types_into_the_composer_and_cleans_up_misses() {
    let root = temp_root("sw-miss");
    let db = root.join("board.db");
    let tui = fake_tui("Build · Fake One Fake", vec!["Fake Two"]);
    tui.lock().unwrap().stuck = true;
    let (port, _) = control_server("pw-miss", tui.clone(), "[]", "{}");
    register_tui(&db, 5858, port, "pw-miss");
    let writes = Arc::new(StdMutex::new(Vec::new()));
    let mut io = io_for(&tui, &writes);
    assert_eq!(
        run_switch(&db, 5858, READY, Dialog::Models, "Fake Two", &mut io),
        Err(ControlError::Failed("lead/opencode_dialog_not_open".into()))
    );
    assert!(writes.lock().unwrap().is_empty());

    tui.lock().unwrap().stuck = false;
    let mut io = io_for(&tui, &writes);
    assert_eq!(
        run_switch(&db, 5858, READY, Dialog::Models, "Fake Nine", &mut io),
        Err(ControlError::Failed("lead/opencode_choice_not_found".into()))
    );
    assert_eq!(*writes.lock().unwrap(), ["Fake Nine", "\r", "\x1b"]);
    assert_eq!(tui.lock().unwrap().dialog, None, "没搜到也要把框关掉");
    assert_eq!(tui.lock().unwrap().footer, "Build · Fake One Fake");

    writes.lock().unwrap().clear();
    for bad in ["", "  ", "Fake\rTwo", "x\x1b[A"] {
        let mut io = io_for(&tui, &writes);
        assert_eq!(
            run_switch(&db, 5858, READY, Dialog::Models, bad, &mut io),
            Err(ControlError::Failed("lead/opencode_switch_invalid".into())),
            "{bad:?}"
        );
    }
    // 用户自己开着对话框：不叠一层、不往里写。
    tui.lock().unwrap().dialog = Some("Select model");
    let mut io = io_for(&tui, &writes);
    assert_eq!(
        run_switch(&db, 5858, READY, Dialog::Models, "Fake Two", &mut io),
        Err(ControlError::NotNow("lead/opencode_dialog_open".into()))
    );
    assert!(writes.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

/// 画面判定用 1.19.5 原文：对话框标题行、底栏（oc4 dialog_tail / f_picked、oc6 f_agent）。
#[test]
fn dialog_and_footer_match_real_screens() {
    let dialog = "LSPs are disabled\n                                            Select model                                     esc\nSearch\n";
    assert!(dialog_visible(Some(dialog), Dialog::Models));
    assert!(!dialog_visible(Some(dialog), Dialog::Agents));
    assert!(!dialog_visible(None, Dialog::Models));
    // 对话里出现这几个字不算（没有右侧的 esc）。
    assert!(!dialog_visible(
        Some("┃  how do I Select model in opencode?"),
        Dialog::Models
    ));
    assert!(footer_shows("┃  Build · Fake Two Fake", Dialog::Models, "Fake Two"));
    assert!(!footer_shows("┃  Build · Fake One Fake", Dialog::Models, "Fake Two"));
    assert!(footer_shows("┃  Plan · Fake One Fake", Dialog::Agents, "plan"));
    assert!(!footer_shows("┃  Build · Fake One Fake", Dialog::Agents, "plan"));
}
