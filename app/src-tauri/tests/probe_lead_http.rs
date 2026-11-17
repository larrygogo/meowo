#![cfg(windows)]

use meowo_store::Store;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Cleanup {
    root: PathBuf,
    child: Option<Child>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn call(port: u16, command: &str, body: &Value, token: &str) -> (u16, String) {
    let result = ureq::post(&format!("http://127.0.0.1:{port}/rpc/{command}"))
        .set("x-meowo-token", token)
        .send_string(&body.to_string());
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return (0, error.to_string()),
    };
    (response.status(), response.into_string().unwrap())
}

/// 真实 Wry 应用与生产 HTTP 路由；必须显式提供隔离构建的 MEOWO_APP_BIN。
#[test]
#[ignore = "真实隔离应用探针：MEOWO_APP_BIN=<隔离构建> cargo test --test probe_lead_http -- --ignored --nocapture"]
fn production_http_answer_snapshot_and_retry() {
    let binary = std::env::var_os("MEOWO_APP_BIN").expect("MEOWO_APP_BIN must point to an isolated app build");
    let root = std::env::temp_dir().join(format!("meowo-http-probe-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    let task = project.join(".lead/http-probe");
    std::fs::create_dir_all(&task).unwrap();
    let data = home.join(".meowo");
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("board.db");
    let store = Store::open(&db).unwrap();
    let project_id = store.upsert_project_by_root(&project.to_string_lossy(), "fixture", 1).unwrap();
    let (session, _) = store.start_session(project_id, "http-owner", 1).unwrap();
    store.set_session_provider(session, "fixture").unwrap();
    store.set_session_cwd(session, &project.to_string_lossy(), 1).unwrap();
    drop(store);
    std::fs::write(task.join(".owner"), "http-owner").unwrap();
    std::fs::write(task.join("acceptance.json"), "[]").unwrap();
    std::fs::write(project.join(".lead/active"), "http-probe").unwrap();
    std::fs::write(task.join("gate.json"), json!({"id":"Q-http","type":"Q","status":"pending","options":[{"id":"go","label":"Continue"}]}).to_string()).unwrap();
    meowo_reporter::lead::publication::initialize(&task).unwrap();
    let gate = meowo_reporter::lead::publication::publish(&task, None).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let token = "a".repeat(64);
    std::fs::write(data.join("settings.json"), json!({
        "auto_update_enabled":false,"notifications_enabled":false,"onboarding_seen":true,
        "remote_access_enabled":true,"remote_access_port":port,"remote_access_bind":"loopback","remote_access_token":token
    }).to_string()).unwrap();
    // 真实启动会消费安装器种子；探针只在种子不存在时运行，不能把隔离验证变成全局设置变更。
    if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\larrygogo\Meowo", winreg::enums::KEY_READ)
    {
        assert!(key.get_value::<u32, _>("ChatEnabled").is_err(), "安装器 ChatEnabled 种子仍存在，不能运行真实应用探针");
    }
    let app_log = std::fs::File::create(root.join("app.log")).unwrap();
    let child = Command::new(binary)
        .env_remove("MEOWO_TEST_EXTERNAL_RESUME")
        .env_remove("MEOWO_DEV_WIRE")
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("MEOWO_DB", &db)
        .stdout(app_log.try_clone().unwrap())
        .stderr(app_log)
        .spawn()
        .unwrap();
    let mut cleanup = Cleanup { root: root.clone(), child: Some(child) };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if call(port, "lead_task_snapshot", &json!({"cwd":project,"session":"http-owner"}), &token).0 == 200 {
            break;
        }
        if Instant::now() >= deadline {
            panic!("isolated HTTP server did not start: {}", std::fs::read_to_string(root.join("app.log")).unwrap_or_default());
        }
        assert!(cleanup.child.as_mut().unwrap().try_wait().unwrap().is_none(), "isolated app exited");
        std::thread::sleep(Duration::from_millis(200));
    }
    let payload = json!({"cwd":project,"gateId":gate.id,"choice":"go","note":"real http","version":gate.version});
    assert_eq!(call(port, "answer_lead_gate", &payload, "wrong").0, 401);
    assert_eq!(call(port, "answer_lead_gate", &payload, &token), (200, "null".into()));
    assert_eq!(call(port, "answer_lead_gate", &payload, &token), (200, "null".into()));
    let stale = json!({"cwd":project,"gateId":gate.id,"choice":"go","note":"stale","version":"publication-0"});
    assert_eq!(call(port, "answer_lead_gate", &stale, &token), (400, "\"lead/gate_changed\"".into()));
    let unknown = json!({"cwd":root.join("other"),"gateId":gate.id,"choice":"go","note":"","version":gate.version});
    assert_eq!(call(port, "answer_lead_gate", &unknown, &token).1, "\"remote/not_session_dir\"");
    let mut extra = payload.as_object().unwrap().clone();
    extra.insert("extra".into(), json!(true));
    assert!(call(port, "answer_lead_gate", &Value::Object(extra), &token).1.contains("remote/bad_args"));
    let (status, snapshot) = call(port, "lead_task_snapshot", &json!({"cwd":project,"session":"http-owner"}), &token);
    assert_eq!(status, 200);
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(snapshot["gate"]["status"], "answered");
    assert_eq!(snapshot["gate"]["answer"]["note"], "real http");
    println!("production HTTP: auth=401, answer=200, duplicate=200, stale=400, scope=400, extra=400, snapshot=answered");
}
