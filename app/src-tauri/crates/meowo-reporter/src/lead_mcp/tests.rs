use super::*;
use std::fs;

struct Fixture {
    _temp: tempfile::TempDir,
    ctx: Context,
    task: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let task = root.join(".lead/demo");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join(".owner"), "native-thread").unwrap();
        fs::write(task.join("progress.md"), "phase: implement").unwrap();
        let fixture = Self {
            ctx: Context {
                session_id: "native-thread".into(),
                cwd: root.to_string_lossy().into_owned(),
                provider: "codex".into(),
            },
            _temp: temp,
            task,
        };
        fixture.gate("g1", "pending");
        fixture
    }
    fn gate(&self, id: &str, status: &str) {
        fs::write(self.task.join("gate.json"), json!({"id":id,"type":"Q","status":status,"title":"继续吗", "answer":{"choice":"yes","note":"保留答案"}}).to_string()).unwrap();
    }
    fn call(&self, id: u32) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"lead_wait","arguments":{},"_meta":{"threadId":"native-thread"}}})
    }
    fn authenticate(&self, thread: Option<&str>, catalog: bool) -> Result<Context, String> {
        if !catalog && thread != Some("native-thread") {
            return Err("lead/context_thread_mismatch".into());
        }
        Ok(self.ctx.clone())
    }
}

#[test]
fn pending_wait_answers_ping_and_cancellation_without_changing_gate() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    assert!(s.request(f.call(1), &auth).is_none());
    assert_eq!(
        s.request(json!({"id":2,"method":"ping"}), &auth).unwrap()["result"],
        json!({})
    );
    let cancelled = s
        .request(
            json!({"method":"notifications/cancelled","params":{"requestId":1}}),
            &auth,
        )
        .unwrap();
    assert_eq!(cancelled["error"]["code"], -32800);
    assert!(s.active.is_none());
    assert_eq!(gate(&f.task).unwrap().status, "pending");
    assert!(!f.task.join(".delivered").exists());
}

#[test]
fn delivery_is_repeatable_and_only_disk_consumed_means_consumed() {
    let f = Fixture::new();
    f.gate("g1", "answered");
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    let first = s.request(f.call(1), &auth).unwrap();
    assert_eq!(first["result"]["isError"], false);
    assert!(first.to_string().contains("保留答案"));
    assert_eq!(gate(&f.task).unwrap().status, "answered");
    assert!(!f.task.join(".delivered").exists());
    let second = s.request(f.call(2), &auth).unwrap();
    assert_eq!(first["result"], second["result"]);
    f.gate("g1", "consumed");
    assert_eq!(
        s.request(f.call(3), &auth).unwrap()["result"]["content"][0]["text"],
        "lead/gate_consumed"
    );
}

#[test]
fn changed_gate_and_changed_owner_cannot_deliver_another_answer() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    s.request(f.call(1), &auth);
    f.gate("g2", "answered");
    assert!(s.tick().unwrap().to_string().contains("lead/gate_changed"));
    f.gate("g1", "pending");
    s.request(f.call(2), &auth);
    fs::write(f.task.join(".owner"), "another-native-thread").unwrap();
    f.gate("g1", "answered");
    assert!(s
        .tick()
        .unwrap()
        .to_string()
        .contains("lead/task_owner_changed"));
}

#[test]
fn duplicate_id_keeps_one_wait_and_different_call_conflicts() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    assert!(s.request(f.call(1), &auth).is_none());
    assert!(s.request(f.call(1), &auth).is_none());
    assert!(s
        .request(f.call(2), &auth)
        .unwrap()
        .to_string()
        .contains("lead/wait_busy"));
    f.gate("g1", "answered");
    assert_eq!(s.tick().unwrap()["id"], 1);
    assert!(s.tick().is_none());
}

#[test]
fn timeout_is_explicit_and_leaves_answer_persistent() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    s.request(f.call(1), &auth);
    s.active.as_mut().unwrap().deadline = Some(Instant::now());
    assert!(s.tick().unwrap().to_string().contains("lead/wait_timeout"));
    assert!(s.active.is_none());
    assert_eq!(gate(&f.task).unwrap().status, "pending");
}

#[test]
fn missing_or_spoofed_metadata_and_session_arguments_are_rejected() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    let mut call = f.call(1);
    call["params"]["_meta"] =
        json!({"sessionId":"native-thread","x-codex-turn-metadata":{"threadId":"native-thread"}});
    assert!(s
        .request(call, &auth)
        .unwrap()
        .to_string()
        .contains("lead/context_thread_mismatch"));
    let mut call = f.call(2);
    call["params"]["arguments"] = json!({"sessionId":"native-thread"});
    assert!(s
        .request(call, &auth)
        .unwrap()
        .to_string()
        .contains("lead/invalid_arguments"));
    let mut call = f.call(3);
    call["params"]["arguments"] = json!({"taskSlug":"../demo"});
    assert!(s
        .request(call, &auth)
        .unwrap()
        .to_string()
        .contains("lead/invalid_arguments"));
}

#[test]
fn catalog_does_not_expose_tools_without_host_authentication() {
    let mut s = Server::default();
    let denied = |_: Option<&str>, _: bool| Err("lead/context_unbound".into());
    assert_eq!(
        s.request(json!({"id":1,"method":"tools/list"}), &denied)
            .unwrap()["result"]["tools"],
        json!([])
    );
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    assert_eq!(
        s.request(json!({"id":2,"method":"tools/list"}), &auth)
            .unwrap()["result"]["tools"][0]["name"],
        "lead_wait"
    );
}

#[test]
fn unowned_task_is_not_implicitly_claimed() {
    let f = Fixture::new();
    fs::remove_file(f.task.join(".owner")).unwrap();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    assert!(Server::default()
        .request(f.call(1), &auth)
        .unwrap()
        .to_string()
        .contains("lead/owned_gate_missing"));
}

#[test]
fn oversized_gate_and_request_are_bounded() {
    let f = Fixture::new();
    fs::write(f.task.join("gate.json"), vec![b' '; MAX_FILE + 1]).unwrap();
    assert!(gate(&f.task).is_none());
    let (send, receive) = mpsc::sync_channel(2);
    read_input(std::io::Cursor::new(vec![b'x'; MAX_LINE + 2]), send);
    assert!(matches!(receive.recv().unwrap(), Input::Oversized));
    let denied = |_: Option<&str>, _: bool| Err("lead/context_unbound".into());
    let mut out = Vec::new();
    let (send, receive) = mpsc::sync_channel(2);
    send.send(Input::Oversized).unwrap();
    serve(receive, &mut out, &denied).unwrap();
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("lead/request_too_large"));
}

#[test]
fn eof_and_output_disconnect_end_an_active_wait() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let (send, receive) = mpsc::sync_channel(2);
    send.send(Input::Request(f.call(1))).unwrap();
    send.send(Input::Eof).unwrap();
    let mut out = Vec::new();
    serve(receive, &mut out, &auth).unwrap();
    assert!(out.is_empty());
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (send, receive) = mpsc::sync_channel(2);
    send.send(Input::Request(json!({"id":1,"method":"ping"})))
        .unwrap();
    assert_eq!(
        serve(receive, Broken, &auth).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(gate(&f.task).unwrap().status, "pending");
}

#[test]
fn revoked_binding_ends_wait_and_late_validation_cannot_cancel_a_new_call() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    s.request(f.call(1), &auth);
    let old = s.active.as_ref().unwrap().generation;
    let revoked = s.validated(old, Err("lead/context_auth".into())).unwrap();
    assert!(revoked.to_string().contains("lead/context_auth"));
    s.request(f.call(2), &auth);
    assert!(s.validated(old, Err("lead/context_auth".into())).is_none());
    let generation = s.active.as_ref().unwrap().generation;
    let mut other = f.ctx.clone();
    other.session_id = "different-native-thread".into();
    assert!(s
        .validated(generation, Ok(other))
        .unwrap()
        .to_string()
        .contains("lead/context_changed"));
    assert_eq!(gate(&f.task).unwrap().status, "pending");
}

#[test]
fn matching_cancellation_only_and_invalid_json_can_recover() {
    let f = Fixture::new();
    let auth = |t: Option<&str>, c| f.authenticate(t, c);
    let mut s = Server::default();
    s.request(f.call(1), &auth);
    assert!(s
        .request(
            json!({"method":"notifications/cancelled","params":{"requestId":99}}),
            &auth
        )
        .is_none());
    assert!(s.active.is_some());
    let (send, receive) = mpsc::sync_channel(4);
    read_input(
        std::io::Cursor::new(b"invalid\n{\"id\":3,\"method\":\"ping\"}\n"),
        send,
    );
    let mut out = Vec::new();
    serve(receive, &mut out, &auth).unwrap();
    let text = String::from_utf8(out).unwrap();
    let replies: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 3);
}
