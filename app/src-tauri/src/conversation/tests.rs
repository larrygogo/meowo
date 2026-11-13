use super::*;
use meowo_agent::conversation::{ConversationChoice, ConversationSpec, ConversationTransport};
use serde_json::json;

struct Fixture {
    home: PathBuf,
    db: PathBuf,
    id: i64,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "meowo-actor-{}",
            &crate::remote::generate_token().unwrap()[..16]
        ));
        std::fs::create_dir(&home).unwrap();
        let db = home.join("board.db");
        let id = open(&db)
            .unwrap()
            .managed_create("codex", &home.to_string_lossy(), None, now())
            .unwrap();
        Self { home, db, id }
    }
    fn store(&self) -> Store {
        open(&self.db).unwrap()
    }
    fn wait(&self, predicate: impl Fn(&Store) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if predicate(&self.store()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "actor timeout: {:?}",
                self.store().managed_state(self.id).unwrap()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn start(&self, mode: &'static str) {
        self.start_history(mode, vec![]);
    }
    fn start_history(
        &self,
        mode: &'static str,
        history: Vec<meowo_agent::conversation::ConversationHistoryMessage>,
    ) {
        let script = "require('readline').createInterface({input:process.stdin}).on('line',line=>{for(const event of JSON.parse(line))process.stdout.write(JSON.stringify(event)+'\\n')})";
        let launch = Launch {
            lead_enabled: false,
            argv: vec!["node".into(), "-e".into(), script.into()],
            env: vec![
                ("HOME".into(), self.home.to_string_lossy().into()),
                ("MEOWO_DB".into(), self.db.to_string_lossy().into()),
            ],
            spec: ConversationSpec {
                args: vec![],
                environment: vec![],
                transport: ConversationTransport::JsonLines,
                ready_timeout_ms: 3000,
            },
            codec: Box::new(FakeCodec {
                home: self.home.clone(),
                mode,
                active: None,
                history,
            }),
        };
        spawn_worker(
            &self.db,
            self.id,
            launch,
            None,
            &mut WORKERS.lock().unwrap(),
        )
        .unwrap();
    }
    fn enqueue(&self, id: &str) {
        self.store()
            .managed_enqueue(self.id, id, &format!("body-{id}"), "[]", None, now())
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(worker) = handle(&self.db, self.id) {
            let _ = worker.sender.send(Message::Input(Input::Shutdown));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle(&self.db, self.id).is_ok() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        // 测试只删本次新建的临时根，绝不使用共享 home 或生产库。
        if self
            .home
            .canonicalize()
            .ok()
            .is_some_and(|p| p.parent() == std::env::temp_dir().canonicalize().ok().as_deref())
        {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }
}

struct FakeCodec {
    home: PathBuf,
    mode: &'static str,
    active: Option<String>,
    history: Vec<meowo_agent::conversation::ConversationHistoryMessage>,
}
fn emit(event: Event) -> Action {
    Action::Emit(event)
}
fn wire(events: Vec<Event>) -> Action {
    Action::Send(serde_json::to_value(events).unwrap())
}
impl ConversationCodec for FakeCodec {
    fn initialize(&mut self) -> Result<Vec<Action>, String> {
        Ok(vec![
            emit(Event::Bound {
                native_session_id: "actor-root".into(),
            }),
            emit(Event::Metadata {
                model: Some("model-a".into()),
                mode: Some("safe".into()),
            }),
            wire(vec![
                Event::History {
                    messages: std::mem::take(&mut self.history),
                },
                Event::Ready,
            ]),
        ])
    }
    fn input(&mut self, input: Input) -> Result<Vec<Action>, String> {
        match input {
            Input::Prompt { request_id, .. } => {
                if self.mode == "gate-race" && request_id == "gate-delivery" {
                    std::fs::write(self.home.join(".lead/task/.owner"), "changed-root").unwrap();
                }
                self.active = Some(request_id.clone());
                let native = Some(format!("native-{request_id}"));
                let mut events = vec![Event::Accepted {
                    request_id: request_id.clone(),
                    native_message_id: native.clone(),
                }];
                if self.mode == "fail" {
                    events.push(Event::Failed {
                        request_id: Some(request_id),
                        reason: "fake/native_failed".into(),
                    });
                } else {
                    events.extend([
                        Event::Consumed {
                            request_id: request_id.clone(),
                            native_message_id: native,
                        },
                        Event::Ready,
                        Event::Completed {
                            request_id: "another-request".into(),
                            stop_reason: "done".into(),
                        },
                        Event::Output {
                            request_id: Some(request_id.clone()),
                            segment_id: format!("segment-{request_id}"),
                            text: "hello ".into(),
                            reasoning: false,
                        },
                        Event::Output {
                            request_id: Some(request_id.clone()),
                            segment_id: format!("segment-{request_id}"),
                            text: "world".into(),
                            reasoning: false,
                        },
                        Event::Metadata {
                            model: None,
                            mode: Some("careful".into()),
                        },
                    ]);
                    if matches!(self.mode, "approval" | "child") {
                        events.push(Event::Interaction {
                            interaction_id: "reused-native-id".into(),
                            title: "approve".into(),
                            detail: String::new(),
                            choices: vec![ConversationChoice {
                                id: "no".into(),
                                label: "no".into(),
                                kind: "reject_once".into(),
                            }],
                            questions: vec![],
                        });
                        if self.mode == "child" {
                            events.push(Event::Completed {
                                request_id: request_id.clone(),
                                stop_reason: "done".into(),
                            });
                            events.push(Event::Ready);
                        }
                    }
                    if self.mode == "parallel" {
                        for key in ["a", "b"] {
                            events.push(Event::Interaction {
                                interaction_id: format!("{key}-{request_id}"),
                                title: key.into(),
                                detail: String::new(),
                                choices: vec![ConversationChoice {
                                    id: "no".into(),
                                    label: "no".into(),
                                    kind: "reject_once".into(),
                                }],
                                questions: vec![],
                            });
                        }
                        events.push(Event::Completed {
                            request_id: request_id.clone(),
                            stop_reason: "done".into(),
                        });
                        events.push(Event::Ready);
                    }
                }
                Ok(vec![wire(events)])
            }
            Input::Resolve { interaction_id, .. } => {
                if self.mode == "parallel" {
                    return Ok(vec![wire(vec![Event::InteractionResolved {
                        interaction_id,
                    }])]);
                }
                assert_eq!(
                    interaction_id, "reused-native-id",
                    "public epoch must be stripped before codec"
                );
                self.mode = "hold";
                Ok(vec![wire(vec![
                    Event::InteractionResolved { interaction_id },
                    Event::Completed {
                        request_id: self.active.take().unwrap(),
                        stop_reason: "done".into(),
                    },
                ])])
            }
            Input::Shutdown => Ok(vec![]),
            _ => Ok(vec![]),
        }
    }
    fn receive(&mut self, message: Value) -> Result<Vec<Action>, String> {
        Ok(vec![emit(serde_json::from_value(message).map_err(error)?)])
    }
}

#[test]
fn native_stdio_actor_serializes_queue_receipts_epochs_and_projection() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.enqueue("two");
    fixture.start("approval");
    fixture.wait(|store| {
        store.managed_state(fixture.id).unwrap().unwrap().phase == "blocked"
            && !store
                .managed_pending_interactions(fixture.id)
                .unwrap()
                .is_empty()
    });
    let store = fixture.store();
    assert_eq!(
        store
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "consumed"
    );
    assert_eq!(
        store
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state,
        "queued",
        "duplicate Ready and unrelated Completed must not start another delivery"
    );
    let snapshot = store.managed_chat_window(fixture.id, None, 30).unwrap();
    let items: Vec<Value> = snapshot
        .items_json
        .iter()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["text"], "body-one");
    assert_eq!(items[1]["text"], "hello world");
    assert!(
        matches!(metadata(&fixture.db, fixture.id).unwrap(), Some(Event::Metadata { model: Some(model), mode: Some(mode) }) if model == "model-a" && mode == "careful")
    );
    let Some(Event::Interaction {
        interaction_id: first,
        ..
    }) = interaction(&fixture.db, fixture.id).unwrap()
    else {
        panic!("missing approval")
    };
    assert_ne!(first, "reused-native-id");
    assert!(resolve(
        &fixture.db,
        fixture.id,
        "reused-native-id",
        Some("no"),
        Value::Null
    )
    .is_err());
    resolve(&fixture.db, fixture.id, &first, Some("no"), Value::Null).unwrap();
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "completed"
    );
    assert!(
        resolve(&fixture.db, fixture.id, &first, Some("no"), Value::Null).is_err(),
        "retired public request must not answer the next pending approval"
    );
}

#[test]
fn native_failure_reaps_worker_before_uncertain_and_stop_without_worker_is_idempotent() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.enqueue("two");
    fixture.start("fail");
    fixture.wait(|store| {
        store
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .runtime_owner
            .is_none()
            && store.managed_state(fixture.id).unwrap().unwrap().phase == "failed"
    });
    fixture.wait(|_| handle(&fixture.db, fixture.id).is_err());
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "uncertain"
    );
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
    assert!(fixture
        .store()
        .managed_next_queued(fixture.id)
        .unwrap()
        .is_none());
    assert!(interaction(&fixture.db, fixture.id).unwrap().is_none());
    assert_eq!(
        control(&fixture.db, fixture.id, "stop", None)
            .unwrap()
            .phase,
        "stopped"
    );
    assert_eq!(
        control(&fixture.db, fixture.id, "stop", None)
            .unwrap()
            .phase,
        "stopped"
    );
}

#[test]
fn native_request_id_reuse_after_restart_cannot_accept_an_old_public_approval() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.start("approval");
    fixture.wait(|store| {
        store.managed_state(fixture.id).unwrap().unwrap().phase == "blocked"
            && !store
                .managed_pending_interactions(fixture.id)
                .unwrap()
                .is_empty()
    });
    let Some(Event::Interaction {
        interaction_id: old,
        ..
    }) = interaction(&fixture.db, fixture.id).unwrap()
    else {
        panic!("missing old approval")
    };
    control(&fixture.db, fixture.id, "stop", None).unwrap();
    fixture.wait(|store| {
        store
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .runtime_owner
            .is_none()
    });
    fixture.wait(|_| handle(&fixture.db, fixture.id).is_err());
    delivery_action(&fixture.db, fixture.id, "one", "dismiss", None).unwrap();
    fixture.enqueue("two");
    fixture.start("approval");
    fixture.wait(|store| {
        store.managed_state(fixture.id).unwrap().unwrap().phase == "blocked"
            && !store
                .managed_pending_interactions(fixture.id)
                .unwrap()
                .is_empty()
    });
    let Some(Event::Interaction {
        interaction_id: new,
        ..
    }) = interaction(&fixture.db, fixture.id).unwrap()
    else {
        panic!("missing new approval")
    };
    assert_ne!(old, new);
    assert!(resolve(&fixture.db, fixture.id, &old, Some("no"), Value::Null).is_err());
    assert!(resolve(&fixture.db, fixture.id, &new, Some("no"), Value::Null).is_ok());
}

#[test]
fn durable_enqueue_receipt_survives_startup_failure_and_legacy_adopt_is_closed() {
    let mut fixture = Fixture::new();
    // 已知持久化会话可直接测试启动失败，不解析/启动任何真实 CLI。
    fixture.id = fixture
        .store()
        .managed_create(
            "missing-provider",
            &fixture.home.to_string_lossy(),
            None,
            now(),
        )
        .unwrap();
    let first = send(
        &fixture.db,
        fixture.id,
        "stable",
        "original",
        &[],
        None,
        None,
    )
    .unwrap();
    assert_eq!(first.state, "queued");
    assert_eq!(
        send(
            &fixture.db,
            fixture.id,
            "stable",
            "original",
            &[],
            None,
            None
        )
        .unwrap()
        .delivery_id,
        "stable"
    );
    assert_eq!(fixture.store().managed_queued_count(fixture.id).unwrap(), 1);
    assert_eq!(
        state(&fixture.db, fixture.id)
            .unwrap()
            .unwrap()
            .last_error
            .as_deref(),
        Some("conversation/provider_unknown")
    );
    let store = fixture.store();
    let project = store
        .upsert_project_by_root(&fixture.home.to_string_lossy(), "legacy", now())
        .unwrap();
    let (legacy, _) = store
        .start_session(project, "external-root", now())
        .unwrap();
    assert_eq!(
        adopt(&fixture.db, legacy, None).unwrap_err(),
        "conversation/adoption_unverified"
    );
    assert!(store.managed_state(legacy).unwrap().is_none());
}

#[test]
fn idle_managed_worker_consumes_a_woken_delivery_within_latency_budget() {
    let fixture = Fixture::new();
    fixture.start("hold");
    fixture.wait(|store| store.managed_state(fixture.id).unwrap().unwrap().phase == "ready");

    let started = Instant::now();
    send(
        &fixture.db,
        fixture.id,
        "latency-budget",
        "leader answer delivery",
        &[],
        None,
        None,
    )
    .unwrap();
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "latency-budget")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
    let elapsed = started.elapsed();
    // Lead 作答和普通托管投递共用 send→Wake→worker 管道。250ms recv_timeout 是漏 Wake 时的
    // 兜底，不应让已唤醒的空闲会话拖到秒级；500ms 同时覆盖 Windows 的调度与一次持久化写入。
    assert!(
        elapsed < Duration::from_millis(500),
        "idle worker consumed delivery too slowly: {elapsed:?}"
    );
}

#[test]
fn parent_completion_does_not_retire_child_approval_or_start_the_next_prompt() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.enqueue("two");
    fixture.start("child");
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state
            == "completed"
            && store.managed_state(fixture.id).unwrap().unwrap().phase == "blocked"
    });
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
    let Some(Event::Interaction { interaction_id, .. }) =
        interaction(&fixture.db, fixture.id).unwrap()
    else {
        panic!("parent completion lost child approval")
    };
    resolve(
        &fixture.db,
        fixture.id,
        &interaction_id,
        Some("no"),
        Value::Null,
    )
    .unwrap();
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
}

#[test]
fn unverified_exit_keeps_runtime_slot_and_inflight_receipt_and_old_owner_cannot_close_new_epoch() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    let store = fixture.store();
    store
        .managed_acquire_runtime(fixture.id, "owner", now())
        .unwrap();
    store
        .managed_set_delivery_state(fixture.id, "one", "queued", "sending", None, None, now())
        .unwrap();
    finish_runtime(
        &fixture.db,
        fixture.id,
        "owner",
        Some("transport/error"),
        false,
    )
    .unwrap();
    let state = store.managed_state(fixture.id).unwrap().unwrap();
    assert_eq!(state.runtime_owner.as_deref(), Some("owner"));
    assert_eq!(
        state.last_error.as_deref(),
        Some("conversation/exit_unverified")
    );
    assert_eq!(
        store
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "sending"
    );
    assert_eq!(
        ensure_worker(&fixture.db, fixture.id, None).unwrap_err(),
        "conversation/previous_process_unverified"
    );
    assert!(finish_runtime(
        &fixture.db,
        fixture.id,
        "old-owner",
        Some("transport/error"),
        true
    )
    .is_err());
    assert_eq!(
        store
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .runtime_owner
            .as_deref(),
        Some("owner")
    );
}

#[test]
fn snapshot_and_incremental_error_ids_do_not_depend_on_a_precommit_sequence_guess() {
    let event = Event::Failed {
        request_id: Some("request".into()),
        reason: "native/failure".into(),
    };
    assert_eq!(chat_item(&event, 0), chat_item(&event, 91));
}

#[test]
fn managed_launch_keeps_exact_profile_wrapper_and_uses_the_native_relay_capability() {
    let plugin = meowo_agent::by_id("codex").unwrap();
    let cap = plugin.relay().unwrap();
    let selected = vec![
        "C:/private-profile/node.exe".into(),
        "C:/private-profile/codex.js".into(),
        "--profile".into(),
        "chosen".into(),
    ];
    let argv = managed_launch_argv(
        plugin.id(),
        Some(selected.clone()),
        || panic!("exact profile cannot fall back to global executable"),
        |id, argv| {
            assert_eq!(id, plugin.id());
            cap.augment_argv(
                meowo_agent::relay::RelayConfig {
                    base_url: "https://relay.invalid/v1",
                    model: "relay-model",
                    protocol: "responses",
                    auth: "bearer",
                    env_options: &[],
                },
                true,
                argv,
            )
        },
    );
    assert_eq!(&argv[..selected.len()], selected.as_slice());
    assert!(argv
        .iter()
        .any(|arg| arg == "model_provider=\"meowo-relay\""));
    assert!(argv
        .iter()
        .any(|arg| arg == "model_providers.meowo-relay.wire_api=\"responses\""));
    assert!(argv.iter().any(|arg| arg == "model=\"relay-model\""));
    assert!(
        !argv.iter().any(|arg| arg == "app-server"),
        "协议spec.args仍由Transport追加，不能换掉profile原始wrapper"
    );
}

fn queued_gate(fixture: &Fixture) -> PathBuf {
    let task = fixture.home.join(".lead/task");
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(task.join(".owner"), "actor-root").unwrap();
    std::fs::write(task.join("progress.md"), "phase: blocked\n").unwrap();
    std::fs::write(task.join("gate.json"),json!({"id":"gate","type":"B","title":"approve","status":"answered","answer":{"choice":"go","note":"approved"}}).to_string()).unwrap();
    let gate = meowo_reporter::lead::read_gate(&task).unwrap();
    let body = meowo_reporter::lead::answer_message(&task, &gate);
    fixture
        .store()
        .managed_enqueue(
            fixture.id,
            "gate-delivery",
            &body,
            "[]",
            Some(
                &serde_json::to_string(&ManagedGateRefDto {
                    task_dir: task.to_string_lossy().into(),
                    gate_id: "gate".into(),
                })
                .unwrap(),
            ),
            now(),
        )
        .unwrap();
    task
}

#[test]
fn queued_gate_is_revalidated_after_waiting_and_invalid_answers_never_reach_native() {
    for change in ["consumed", "new-id", "owner", "done", "answer", "malformed"] {
        let fixture = Fixture::new();
        fixture.enqueue("one");
        fixture.start("hold");
        fixture.wait(|store| {
            store
                .managed_delivery(fixture.id, "one")
                .unwrap()
                .unwrap()
                .state
                == "consumed"
        });
        let task = queued_gate(&fixture);
        fixture.enqueue("after");
        match change {
            "owner" => std::fs::write(task.join(".owner"), "other-root").unwrap(),
            "done" => std::fs::write(task.join("progress.md"), "phase: done\n").unwrap(),
            "malformed" => std::fs::write(task.join("gate.json"), "{").unwrap(),
            _ => {
                let mut gate: Value =
                    serde_json::from_str(&std::fs::read_to_string(task.join("gate.json")).unwrap())
                        .unwrap();
                match change {
                    "consumed" => gate["status"] = json!("consumed"),
                    "new-id" => gate["id"] = json!("new-gate"),
                    "answer" => gate["answer"]["choice"] = json!("different"),
                    _ => unreachable!(),
                };
                std::fs::write(task.join("gate.json"), gate.to_string()).unwrap();
            }
        }
        handle(&fixture.db, fixture.id)
            .unwrap()
            .sender
            .send(Message::Native(TransportEvent::Wire(
                serde_json::to_value(Event::Completed {
                    request_id: "one".into(),
                    stop_reason: "done".into(),
                })
                .unwrap(),
            )))
            .unwrap();
        fixture.wait(|store| {
            store
                .managed_delivery(fixture.id, "after")
                .unwrap()
                .unwrap()
                .state
                == "consumed"
        });
        let receipt = fixture
            .store()
            .managed_delivery(fixture.id, "gate-delivery")
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.state,
            if change == "malformed" {
                "failed"
            } else {
                "cancelled"
            },
            "{change}"
        );
        assert!(receipt.accepted_at.is_none() && receipt.consumed_at.is_none());
        assert!(fixture
            .store()
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .runtime_owner
            .is_some());
    }
}

#[test]
fn gate_owner_changed_during_codec_preparation_is_checked_again_before_the_first_write() {
    let fixture = Fixture::new();
    fixture
        .store()
        .managed_bind(fixture.id, "actor-root", now())
        .unwrap();
    queued_gate(&fixture);
    fixture.start("gate-race");
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "gate-delivery")
            .unwrap()
            .unwrap()
            .state
            == "cancelled"
    });
    assert!(fixture
        .store()
        .managed_state(fixture.id)
        .unwrap()
        .unwrap()
        .runtime_owner
        .is_some());
    assert!(fixture
        .store()
        .managed_chat_window(fixture.id, None, 30)
        .unwrap()
        .items_json
        .is_empty());
    fixture.wait(|store| store.managed_state(fixture.id).unwrap().unwrap().phase == "failed");
    let old_owner = handle(&fixture.db, fixture.id).unwrap().owner.clone();
    stop_failed_worker_for_resume(&fixture.db, fixture.id).unwrap();
    assert!(handle(&fixture.db, fixture.id).is_err());
    assert!(fixture
        .store()
        .managed_state(fixture.id)
        .unwrap()
        .unwrap()
        .runtime_owner
        .is_none());
    fixture.start("hold");
    fixture.wait(|store| store.managed_state(fixture.id).unwrap().unwrap().phase == "ready");
    assert_ne!(handle(&fixture.db, fixture.id).unwrap().owner, old_owner);
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "gate-delivery")
            .unwrap()
            .unwrap()
            .state,
        "cancelled"
    );
}

#[test]
fn local_file_references_and_failed_image_preflight_leave_the_same_engine_usable() {
    let fixture = Fixture::new();
    let path = fixture.home.join("note-$()&.pdf");
    std::fs::write(&path, b"%PDF fixture").unwrap();
    let raw = serde_json::to_string(&vec![ManagedAttachmentDto {
        path: path.to_string_lossy().into(),
        mime_type: Some("application/pdf".into()),
        data_base64: None,
    }])
    .unwrap();
    let (text, images) = preflight::prompt("inspect".into(), &raw).unwrap();
    assert!(images.is_empty());
    assert!(text.contains("本机文件路径引用"));
    assert!(text.contains("note-$()&.pdf"));
    let store = fixture.store();
    store
        .managed_enqueue(
            fixture.id,
            "image",
            "image",
            "[{\"path\":\"inline.png\",\"mimeType\":\"image/png\",\"dataBase64\":\"eA==\"}]",
            None,
            now(),
        )
        .unwrap();
    store
        .managed_enqueue(fixture.id, "file", "inspect", &raw, None, now())
        .unwrap();
    fixture.start("hold");
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "file")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
    let failed = store
        .managed_delivery(fixture.id, "image")
        .unwrap()
        .unwrap();
    assert_eq!(failed.state, "failed");
    assert!(failed.accepted_at.is_none());
    assert!(store
        .managed_state(fixture.id)
        .unwrap()
        .unwrap()
        .runtime_owner
        .is_some());
}

#[test]
fn valid_answered_gate_in_blocked_task_is_delivered_and_changed_files_fail_before_sending() {
    let fixture = Fixture::new();
    fixture
        .store()
        .managed_bind(fixture.id, "actor-root", now())
        .unwrap();
    queued_gate(&fixture);
    fixture.start("hold");
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "gate-delivery")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
    for disappeared in [false, true] {
        let fixture = Fixture::new();
        let path = fixture
            .home
            .join(if disappeared { "note.txt" } else { "image.png" });
        std::fs::write(&path, b"small initial file").unwrap();
        fixture.enqueue("one");
        fixture.start("hold");
        fixture.wait(|store| {
            store
                .managed_delivery(fixture.id, "one")
                .unwrap()
                .unwrap()
                .state
                == "consumed"
        });
        fixture
            .store()
            .managed_enqueue(
                fixture.id,
                "changed-file",
                "inspect",
                &serde_json::to_string(&vec![ManagedAttachmentDto {
                    path: path.to_string_lossy().into(),
                    mime_type: Some(
                        if disappeared {
                            "text/plain"
                        } else {
                            "image/png"
                        }
                        .into(),
                    ),
                    data_base64: None,
                }])
                .unwrap(),
                None,
                now(),
            )
            .unwrap();
        if disappeared {
            std::fs::remove_file(&path).unwrap();
        } else {
            std::fs::write(&path, vec![0; 2 * 1024 * 1024 + 1]).unwrap();
        }
        fixture.enqueue("after");
        handle(&fixture.db, fixture.id)
            .unwrap()
            .sender
            .send(Message::Native(TransportEvent::Wire(
                serde_json::to_value(Event::Completed {
                    request_id: "one".into(),
                    stop_reason: "done".into(),
                })
                .unwrap(),
            )))
            .unwrap();
        fixture.wait(|store| {
            store
                .managed_delivery(fixture.id, "after")
                .unwrap()
                .unwrap()
                .state
                == "consumed"
        });
        let receipt = fixture
            .store()
            .managed_delivery(fixture.id, "changed-file")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.state, "failed");
        assert!(receipt.consumed_at.is_none());
        assert!(fixture
            .store()
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .runtime_owner
            .is_some());
    }
}

#[test]
fn weak_native_history_is_durable_and_does_not_block_ready_or_overwrite_live_projection() {
    use meowo_agent::conversation::ConversationHistoryMessage as History;
    let fixture = Fixture::new();
    let store = fixture.store();
    let output = Event::Output {
        request_id: None,
        segment_id: "local-stream".into(),
        text: "local live text".into(),
        reasoning: false,
    };
    store
        .managed_record_event(
            fixture.id,
            &serde_json::to_string(&output).unwrap(),
            Some(&serde_json::to_string(&chat_item(&output, 0).unwrap()).unwrap()),
            now(),
        )
        .unwrap();
    fixture.start_history(
        "hold",
        vec![History {
            native_message_id: None,
            segment_id: None,
            request_id: None,
            role: "assistant".into(),
            text: "native reordered or compacted snapshot tail".into(),
        }],
    );
    fixture.wait(|store| {
        matches!(
            store
                .managed_state(fixture.id)
                .unwrap()
                .unwrap()
                .phase
                .as_str(),
            "ready" | "failed"
        )
    });
    assert!(
        store
            .managed_events_since(fixture.id, 0, 200)
            .unwrap()
            .iter()
            .any(
                |event| serde_json::from_str::<Value>(&event.payload_json).unwrap()["kind"]
                    == "history"
            ),
        "raw History must be durable before reconciliation"
    );
    assert_eq!(
        store.managed_state(fixture.id).unwrap().unwrap().phase,
        "ready"
    );
    let main = store.managed_chat_window(fixture.id, None, 30).unwrap();
    assert_eq!(main.items_json.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&main.items_json[0]).unwrap()["text"],
        "local live text"
    );
    let recovered = store
        .managed_native_recovery_page(fixture.id, None, 50)
        .unwrap();
    assert_eq!(recovered.chunks.len(), 1);
    assert_eq!(
        recovered.chunks[0].text,
        "native reordered or compacted snapshot tail"
    );
    fixture.enqueue("after-restore");
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "after-restore")
            .unwrap()
            .unwrap()
            .state
            == "consumed"
    });
}

#[test]
fn internal_lead_consumption_and_history_never_project_a_fake_human_message() {
    use meowo_agent::conversation::ConversationHistoryMessage as History;
    let fixture = Fixture::new();
    let store = fixture.store();
    store
        .managed_enqueue_lead(
            fixture.id,
            "auto",
            "internal instruction",
            "[]",
            None,
            now(),
        )
        .unwrap();
    store
        .managed_set_delivery_state(fixture.id, "auto", "queued", "sending", None, None, now())
        .unwrap();
    record_consumption(&store, fixture.id, "auto", Some("native-auto"), true).unwrap();
    reconcile_history(
        &store,
        fixture.id,
        &[History {
            native_message_id: Some("native-auto".into()),
            segment_id: None,
            request_id: Some("auto".into()),
            role: "user".into(),
            text: "context plus internal instruction".into(),
        }],
    )
    .unwrap();
    assert!(store
        .managed_chat_window(fixture.id, None, 30)
        .unwrap()
        .items_json
        .is_empty());
    assert!(store
        .managed_latest_user_delivery(fixture.id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .managed_delivery(fixture.id, "auto")
            .unwrap()
            .unwrap()
            .state,
        "consumed"
    );
}

#[test]
fn independent_native_approvals_survive_parent_completion_and_close_only_their_own_ids() {
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.enqueue("two");
    fixture.start("parallel");
    fixture.wait(|store| {
        store
            .managed_pending_interactions(fixture.id)
            .unwrap()
            .len()
            == 2
            && store
                .managed_delivery(fixture.id, "one")
                .unwrap()
                .unwrap()
                .state
                == "completed"
    });
    let pending = fixture
        .store()
        .managed_pending_interactions(fixture.id)
        .unwrap();
    let ids: Vec<String> = pending
        .iter()
        .map(
            |event| match serde_json::from_str::<Event>(&event.payload_json).unwrap() {
                Event::Interaction { interaction_id, .. } => interaction_id,
                _ => panic!("wrong fact"),
            },
        )
        .collect();
    resolve(&fixture.db, fixture.id, &ids[0], Some("no"), Value::Null).unwrap();
    fixture.wait(|store| {
        store
            .managed_pending_interactions(fixture.id)
            .unwrap()
            .len()
            == 1
    });
    assert_eq!(
        fixture
            .store()
            .managed_state(fixture.id)
            .unwrap()
            .unwrap()
            .phase,
        "blocked"
    );
    assert_eq!(
        fixture
            .store()
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
    resolve(&fixture.db, fixture.id, &ids[1], Some("no"), Value::Null).unwrap();
    fixture.wait(|store| {
        store
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state
            != "queued"
    });
}

#[test]
fn history_from_native_stdio_repairs_the_tail_but_does_not_release_unfinished_receipts() {
    use meowo_agent::conversation::ConversationHistoryMessage as History;
    let fixture = Fixture::new();
    fixture.enqueue("one");
    fixture.enqueue("two");
    let store = fixture.store();
    store
        .managed_set_delivery_state(fixture.id, "one", "queued", "sending", None, None, now())
        .unwrap();
    record_consumption(&store, fixture.id, "one", Some("native-one"), false).unwrap();
    store
        .managed_set_delivery_state(
            fixture.id,
            "one",
            "consumed",
            "uncertain",
            None,
            None,
            now(),
        )
        .unwrap();
    let output = Event::Output {
        request_id: Some("one".into()),
        segment_id: "tail".into(),
        text: "partial".into(),
        reasoning: false,
    };
    store
        .managed_record_event(
            fixture.id,
            &serde_json::to_string(&output).unwrap(),
            Some(&serde_json::to_string(&chat_item(&output, 0).unwrap()).unwrap()),
            now(),
        )
        .unwrap();
    fixture.start_history(
        "hold",
        vec![
            History {
                native_message_id: Some("native-one".into()),
                segment_id: None,
                request_id: Some("one".into()),
                role: "user".into(),
                text: "body-one".into(),
            },
            History {
                native_message_id: Some("answer".into()),
                segment_id: Some("tail".into()),
                request_id: None,
                role: "assistant".into(),
                text: "partial with native tail".into(),
            },
        ],
    );
    fixture.wait(|store| store.managed_state(fixture.id).unwrap().unwrap().phase == "ready");
    let window = store.managed_chat_window(fixture.id, None, 30).unwrap();
    assert_eq!(window.items_json.len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(&window.items_json[1]).unwrap()["text"],
        "partial with native tail"
    );
    assert_eq!(
        store
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "consumed"
    );
    assert_eq!(
        store
            .managed_delivery(fixture.id, "two")
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
}

#[test]
fn human_retry_is_atomic_idempotent_keeps_old_receipts_and_requires_an_exited_engine() {
    let fixture = Fixture::new();
    fixture.enqueue("old");
    let store = fixture.store();
    store
        .managed_acquire_runtime(fixture.id, "owner", now())
        .unwrap();
    store
        .managed_set_delivery_state(fixture.id, "old", "queued", "sending", None, None, now())
        .unwrap();
    store
        .managed_set_delivery_state(
            fixture.id,
            "old",
            "sending",
            "accepted",
            Some("native-old"),
            None,
            now(),
        )
        .unwrap();
    store
        .managed_set_delivery_state(
            fixture.id,
            "old",
            "accepted",
            "uncertain",
            None,
            None,
            now(),
        )
        .unwrap();
    assert!(
        delivery_action(&fixture.db, fixture.id, "old", "retry", Some("new"))
            .unwrap_err()
            .contains("delivery_engine_live")
    );
    assert!(store.managed_delivery(fixture.id, "new").unwrap().is_none());
    store
        .managed_release_runtime(fixture.id, "owner", now())
        .unwrap();
    assert_eq!(
        delivery_action(&fixture.db, fixture.id, "old", "retry", Some("new"))
            .unwrap()
            .state,
        "queued"
    );
    assert_eq!(
        delivery_action(&fixture.db, fixture.id, "old", "retry", Some("new"))
            .unwrap()
            .delivery_id,
        "new"
    );
    assert!(delivery_action(&fixture.db, fixture.id, "old", "retry", Some("another")).is_err());
    assert!(delivery_action(&fixture.db, fixture.id, "old", "dismiss", None).is_err());
    let old = store.managed_delivery(fixture.id, "old").unwrap().unwrap();
    assert_eq!(old.state, "cancelled");
    assert_eq!(old.native_message_id.as_deref(), Some("native-old"));
    assert!(old.accepted_at.is_some());
    assert_eq!(store.managed_queued_count(fixture.id).unwrap(), 1);
    assert_eq!(
        delivery_action(&fixture.db, fixture.id, "new", "dismiss", None)
            .unwrap()
            .state,
        "cancelled"
    );
    assert_eq!(
        delivery_action(&fixture.db, fixture.id, "new", "dismiss", None)
            .unwrap()
            .state,
        "cancelled"
    );
}

#[test]
fn strong_native_history_extends_partial_projection_without_duplicates_and_never_guesses_completion(
) {
    use meowo_agent::conversation::ConversationHistoryMessage as History;
    let fixture = Fixture::new();
    fixture.enqueue("one");
    let store = fixture.store();
    store
        .managed_set_delivery_state(fixture.id, "one", "queued", "sending", None, None, now())
        .unwrap();
    store
        .managed_set_delivery_state(fixture.id, "one", "sending", "uncertain", None, None, now())
        .unwrap();
    record_consumption(&store, fixture.id, "one", Some("native-one"), false).unwrap();
    let partial = Event::Output {
        request_id: Some("one".into()),
        segment_id: "answer-one".into(),
        text: "partial".into(),
        reasoning: false,
    };
    store
        .managed_record_event(
            fixture.id,
            &serde_json::to_string(&partial).unwrap(),
            Some(&serde_json::to_string(&chat_item(&partial, 0).unwrap()).unwrap()),
            now(),
        )
        .unwrap();
    store
        .managed_set_delivery_state(
            fixture.id,
            "one",
            "consumed",
            "uncertain",
            None,
            None,
            now(),
        )
        .unwrap();
    let history = vec![
        History {
            native_message_id: Some("native-one".into()),
            segment_id: None,
            request_id: Some("one".into()),
            role: "user".into(),
            text: "body-one".into(),
        },
        History {
            native_message_id: Some("native-answer".into()),
            segment_id: Some("answer-one".into()),
            request_id: None,
            role: "assistant".into(),
            text: "partial plus persisted tail".into(),
        },
    ];
    reconcile_history(&store, fixture.id, &history).unwrap();
    reconcile_history(&store, fixture.id, &history).unwrap();
    let window = store.managed_chat_window(fixture.id, None, 30).unwrap();
    assert_eq!(window.items_json.len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(&window.items_json[1]).unwrap()["text"],
        "partial plus persisted tail"
    );
    assert_eq!(
        store
            .managed_delivery(fixture.id, "one")
            .unwrap()
            .unwrap()
            .state,
        "consumed"
    );
    assert!(store.managed_next_queued(fixture.id).unwrap().is_none());
    let unknown = vec![History {
        native_message_id: None,
        segment_id: None,
        request_id: None,
        role: "assistant".into(),
        text: "partial plus persisted tail".into(),
    }];
    assert!(reconcile_history(&store, fixture.id, &unknown).unwrap());
    assert_eq!(
        store
            .managed_chat_window(fixture.id, None, 30)
            .unwrap()
            .items_json
            .len(),
        2
    );
}
