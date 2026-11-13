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
