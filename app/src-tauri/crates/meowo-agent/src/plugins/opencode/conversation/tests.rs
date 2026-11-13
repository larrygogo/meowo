use super::*;

fn http(actions: &[ConversationAction]) -> (&str, &str, &str, Option<&Value>) {
    actions
        .iter()
        .find_map(|a| {
            if let ConversationAction::Http {
                tag,
                method,
                path,
                body,
            } = a
            {
                Some((tag.as_str(), method.as_str(), path.as_str(), body.as_ref()))
            } else {
                None
            }
        })
        .unwrap()
}
fn events(actions: &[ConversationAction]) -> Vec<&ConversationEvent> {
    actions
        .iter()
        .filter_map(|a| {
            if let ConversationAction::Emit(e) = a {
                Some(e)
            } else {
                None
            }
        })
        .collect()
}
fn response(c: &mut Codec, actions: &[ConversationAction], body: Value) -> Vec<ConversationAction> {
    c.receive(json!({"_meowo_http":{"tag":http(actions).0,"status":200,"body":body}}))
        .unwrap()
}
fn bound() -> Codec {
    let mut c = Codec::new(ConversationInit {
        cwd: "C:/isolated space".into(),
        ..Default::default()
    });
    let init = c.initialize().unwrap();
    assert_eq!(http(&init).1, "POST");
    let bind = response(&mut c, &init, json!({"id":"ses_root"}));
    assert!(http(&bind)
        .2
        .contains("limit=4097&directory=C%3A%2Fisolated%20space"));
    let ready = response(&mut c, &bind, json!([]));
    assert!(matches!(
        events(&ready).last().unwrap(),
        ConversationEvent::Ready
    ));
    c
}
fn prompt(c: &mut Codec, id: &str) -> Vec<ConversationAction> {
    c.input(ConversationInput::Prompt {
        request_id: id.into(),
        text: "hello".into(),
        attachments: vec![],
    })
    .unwrap()
}

#[test]
fn http_204_is_acceptance_only_native_user_is_consumption() {
    let mut c = bound();
    let send = prompt(&mut c, "stable-id");
    assert_eq!(http(&send).3.unwrap()["messageID"], message_id("stable-id"));
    assert!(http(&send).3.unwrap()["parts"][0]["id"]
        .as_str()
        .unwrap()
        .starts_with("prt_meowo_"));
    let accepted = c
        .receive(json!({"_meowo_http":{"tag":http(&send).0,"status":204,"body":null}}))
        .unwrap();
    assert!(matches!(
        events(&accepted)[0],
        ConversationEvent::Accepted { .. }
    ));
    assert!(c
        .receive(json!({"type":"session.idle","properties":{"sessionID":"ses_root"}}))
        .unwrap()
        .is_empty());
    let consumed=c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"sessionID":"ses_root","id":message_id("stable-id"),"role":"user"}}})).unwrap();
    assert!(
        matches!(events(&consumed)[0],ConversationEvent::Consumed {request_id,..}if request_id=="stable-id")
    );
    let done = c
        .receive(json!({"type":"session.idle","properties":{"sessionID":"ses_root"}}))
        .unwrap();
    assert!(matches!(
        events(&done)[0],
        ConversationEvent::Completed { .. }
    ));
    assert!(prompt(&mut c, "stable-id")
        .iter()
        .all(|a| !matches!(a, ConversationAction::Http { .. })));
}

#[test]
fn resume_reconciles_native_message_id_and_rejects_child_root() {
    let mut c = Codec::new(ConversationInit {
        native_session_id: Some("ses_root".into()),
        ..Default::default()
    });
    let init = c.initialize().unwrap();
    assert!(http(&init).2.starts_with("/session/ses_root?"));
    let bind = response(&mut c, &init, json!({"id":"ses_root"}));
    let history = response(
        &mut c,
        &bind,
        json!([{"info":{"id":message_id("delivery"),"sessionID":"ses_root","role":"user"},"parts":[{"id":"prt_saved","type":"text","text":"saved"}]}]),
    );
    assert!(events(&history)
        .iter()
        .any(|e| matches!(e,ConversationEvent::Consumed {request_id,..}if request_id=="delivery")));
    assert_eq!(
        request_id(&message_id("中文 request")),
        Some("中文 request".into())
    );
    let mut c = Codec::new(ConversationInit::default());
    let init = c.initialize().unwrap();
    assert!(c.receive(json!({"_meowo_http":{"tag":http(&init).0,"status":200,"body":{"id":"ses_child","parentID":"ses_root"}}})).is_err());
}

#[test]
fn root_filters_child_and_stream_delta_snapshot_do_not_duplicate() {
    let mut c = bound();
    prompt(&mut c, "d");
    assert!(c.receive(json!({"type":"permission.asked","properties":{"sessionID":"ses_child","id":"per_child"}})).unwrap().is_empty());
    c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":"msg_assistant","role":"assistant"}}})).unwrap();
    let first=c.receive(json!({"type":"message.part.updated","properties":{"sessionID":"ses_root","part":{"id":"prt_a","messageID":"msg_assistant","type":"text","text":"hi"}}})).unwrap();
    assert!(matches!(events(&first)[0],ConversationEvent::Output {text,..}if text=="hi"));
    let delta=c.receive(json!({"type":"message.part.delta","properties":{"sessionID":"ses_root","messageID":"msg_assistant","partID":"prt_a","field":"text","delta":" there"}})).unwrap();
    assert!(matches!(events(&delta)[0],ConversationEvent::Output {text,..}if text==" there"));
    assert!(c.receive(json!({"type":"message.part.updated","properties":{"sessionID":"ses_root","part":{"id":"prt_a","messageID":"msg_assistant","type":"text","text":"hi there"}}})).unwrap().is_empty());
}

#[test]
fn native_sse_failure_records_only_a_bounded_classification() {
    let mut c = bound();
    prompt(&mut c, "delivery");
    let secret = "Bearer private-token-that-must-not-persist";
    let actions = c
        .receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":"msg_assistant","role":"assistant","error":{"status":401,"message":format!("unauthorized {secret}")}}}}))
        .unwrap();
    let diagnostic = events(&actions)
        .into_iter()
        .find(|event| matches!(event, ConversationEvent::NativeDiagnostic { .. }))
        .unwrap();
    let ConversationEvent::NativeDiagnostic {
        provider,
        transport,
        signal,
        category,
        error_shape,
        status,
    } = diagnostic
    else {
        unreachable!()
    };
    assert_eq!(provider, "opencode");
    assert_eq!(transport, "sse");
    assert_eq!(signal, "message.updated.info.error");
    assert_eq!(category, "auth");
    assert_eq!(error_shape, "object");
    assert_eq!(*status, Some(401));
    let serialized = serde_json::to_string(diagnostic).unwrap();
    assert!(!serialized.contains(secret));
    assert!(events(&actions).iter().any(
        |event| matches!(event, ConversationEvent::Failed { reason, .. } if reason == "conversation/native_turn_failed")
    ));
}

#[test]
fn nested_native_failure_is_classified_without_persisting_its_text() {
    let mut c = bound();
    prompt(&mut c, "delivery");
    let secret = "Bearer private-token-that-must-not-persist";
    let actions = c
        .receive(json!({"type":"session.error","properties":{"sessionID":"ses_root","error":{"data":{"message":format!("forbidden {secret}")}}}}))
        .unwrap();
    let diagnostic = events(&actions)
        .into_iter()
        .find(|event| matches!(event, ConversationEvent::NativeDiagnostic { .. }))
        .unwrap();
    assert!(matches!(diagnostic, ConversationEvent::NativeDiagnostic { category, .. } if category == "auth"));
    assert!(!serde_json::to_string(diagnostic).unwrap().contains(secret));
}

#[test]
fn approval_and_questions_cannot_reply_to_another_request() {
    let mut c = bound();
    c.receive(json!({"type":"permission.v2.asked","properties":{"sessionID":"ses_root","id":"per_a","action":"bash"}})).unwrap();
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "per_other".into(),
            option_id: Some("once".into()),
            answers: Value::Null
        })
        .is_err());
    let resolve = c
        .input(ConversationInput::Resolve {
            interaction_id: "per_a".into(),
            option_id: Some("reject".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert!(http(&resolve)
        .2
        .starts_with("/api/session/ses_root/permission/per_a/reply?"));
    assert_eq!(http(&resolve).3.unwrap()["reply"], "reject");
    c.receive(json!({"type":"question.asked","properties":{"sessionID":"ses_root","id":"que_a","questions":[{"header":"one","question":"pick","options":[]}]}})).unwrap();
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "que_a".into(),
            option_id: None,
            answers: json!({"0":["answer"]}),
        })
        .unwrap();
    assert_eq!(http(&reply).3.unwrap()["answers"], json!([["answer"]]));
}

#[test]
fn cancel_is_scoped_and_does_not_pretend_process_has_stopped() {
    let mut c = bound();
    prompt(&mut c, "d");
    let abort = c.input(ConversationInput::Cancel).unwrap();
    assert!(http(&abort).2.starts_with("/session/ses_root/abort?"));
    let response = response(&mut c, &abort, json!(true));
    assert!(response.is_empty());
    assert!(c.active.is_some());
}

#[test]
fn catalogs_only_offer_connected_provider_models_and_primary_agents() {
    let mut c = bound();
    let models = c.http("GET", "/provider".into(), None, Pending::Models);
    let actions = vec![models];
    let out = response(
        &mut c,
        &actions,
        json!({"connected":["usable"],"all":[{"id":"unauthed","models":{"ghost":{"name":"No credential"}}},{"id":"usable","models":{"real":{"name":"Real model"}}}]}),
    );
    assert!(
        matches!(events(&out)[0],ConversationEvent::Catalog {models,..}if models.len()==1&&models[0].id=="usable/real")
    );
    let agents = vec![c.http("GET", "/agent".into(), None, Pending::Agents)];
    response(
        &mut c,
        &agents,
        json!([{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"},{"name":"child","mode":"subagent"},{"name":"secret","mode":"primary","hidden":true}]),
    );
    assert_eq!(
        c.modes.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["build", "plan"]
    );
    assert!(c
        .input(ConversationInput::SetMode {
            dimension: "approval".into(),
            mode: "never".into()
        })
        .is_err());
    let set = c
        .input(ConversationInput::SetMode {
            dimension: "agent".into(),
            mode: "plan".into(),
        })
        .unwrap();
    assert!(http(&set).2.starts_with("/api/session/ses_root/agent?"));
    assert_eq!(http(&set).3.unwrap()["agent"], "plan");
    assert!(c.agent_mode.is_none());
    response(&mut c, &set, json!({}));
    assert_eq!(c.agent_mode.as_deref(), Some("plan"));
    let send = prompt(&mut c, "d");
    assert_eq!(http(&send).3.unwrap()["agent"], "plan");
}

#[test]
fn multiple_text_parts_in_same_delivery_keep_separate_segment_ids() {
    let mut c = bound();
    prompt(&mut c, "d");
    c.roles.insert("msg_assistant".into(), "assistant".into());
    let first=c.stream(&json!({"id":"prt_before","messageID":"msg_assistant","type":"text","text":"before tool"}),None).unwrap();
    let second=c.stream(&json!({"id":"prt_after","messageID":"msg_assistant","type":"text","text":"after tool"}),None).unwrap();
    let segment = |a: &[ConversationAction]| match events(a)[0] {
        ConversationEvent::Output { segment_id, .. } => segment_id.clone(),
        _ => panic!(),
    };
    assert_ne!(segment(&first), segment(&second));
}

#[test]
fn history_splits_parts_and_native_full_message_id_is_the_only_user_request_evidence() {
    let mut c = bound();
    let native = message_id("stable-delivery");
    let out=c.history(&json!([
        {"info":{"id":native,"sessionID":"ses_root","role":"user"},"parts":[{"id":"prt_user","type":"text","text":"same text"}]},
        {"info":{"id":"msg_assistant","sessionID":"ses_root","role":"assistant"},"parts":[{"id":"prt_before","type":"text","text":"before tool"},{"id":"prt_after","type":"text","text":"after tool"},{"id":"prt_thought","type":"reasoning","text":"thinking"}]},
        {"info":{"id":"msg_ordinary","sessionID":"ses_root","role":"user"},"parts":[{"id":"prt_unmapped","type":"text","text":"same text"}]}
    ])).unwrap();
    let messages = events(&out)
        .into_iter()
        .find_map(|e| {
            if let ConversationEvent::History { messages } = e {
                Some(messages)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0].request_id.as_deref(), Some("stable-delivery"));
    assert_eq!(messages[0].segment_id.as_deref(), Some("prt_user"));
    assert_eq!(messages[1].native_message_id, messages[2].native_message_id);
    assert_ne!(messages[1].segment_id, messages[2].segment_id);
    assert_eq!(messages[3].role, "reasoning");
    assert_eq!(messages[3].segment_id.as_deref(), Some("prt_thought"));
    assert_eq!(messages[4].text, messages[0].text);
    assert!(messages[4].request_id.is_none());
    assert!(request_id("msg_meowo_68656C6C6F").is_none());
}

#[test]
fn only_root_current_assistant_completed_writes_and_exit_zero_bash_are_facts() {
    let mut c = bound();
    prompt(&mut c, "d");
    c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":"msg_current","role":"assistant","parentID":message_id("d")}}})).unwrap();
    let event = |session: &str, mid: &str, tool: &str, status: &str, exit: Value, input: Value| json!({"type":"message.part.updated","properties":{"sessionID":session,"part":{"id":"prt_tool","messageID":mid,"type":"tool","tool":tool,"callID":"call-current","state":{"status":status,"input":input,"metadata":{"exit":exit}}}}});
    for (session, mid, tool, status, exit, writes) in [
        (
            "ses_root",
            "msg_current",
            "write",
            "completed",
            json!(0),
            true,
        ),
        (
            "ses_child",
            "msg_current",
            "write",
            "completed",
            json!(0),
            false,
        ),
        ("ses_root", "msg_old", "write", "completed", json!(0), false),
        (
            "ses_root",
            "msg_current",
            "read",
            "completed",
            json!(0),
            false,
        ),
        ("ses_root", "msg_current", "write", "error", json!(0), false),
        (
            "ses_root",
            "msg_current",
            "write",
            "running",
            json!(0),
            false,
        ),
        (
            "ses_root",
            "msg_current",
            "bash",
            "completed",
            json!(1),
            false,
        ),
        (
            "ses_root",
            "msg_current",
            "bash",
            "completed",
            Value::Null,
            false,
        ),
        (
            "ses_root",
            "msg_current",
            "bash",
            "completed",
            json!(0),
            true,
        ),
    ] {
        let out=c.receive(event(session,mid,tool,status,exit,json!({"filePath":".lead/actual/SPEC.md","command":"echo text > .lead/actual/SPEC.md"}))).unwrap();
        assert_eq!(
            events(&out)
                .iter()
                .any(|e| matches!(e, ConversationEvent::TaskWrite { .. })),
            writes,
            "{session}/{mid}/{tool}/{status}"
        );
    }
    let read = c
        .receive(event(
            "ses_root",
            "msg_current",
            "bash",
            "completed",
            json!(0),
            json!({"command":"cp .lead/read/SPEC.md /tmp/out"}),
        ))
        .unwrap();
    assert!(!events(&read)
        .iter()
        .any(|e| matches!(e, ConversationEvent::TaskWrite { .. })));
}

#[test]
fn native_running_tool_at_idle_defers_auto_continue() {
    let mut c = bound();
    prompt(&mut c, "d");
    c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":message_id("d"),"role":"user"}}})).unwrap();
    c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":"msg_current","role":"assistant","parentID":message_id("d")}}})).unwrap();
    c.receive(json!({"type":"message.part.updated","properties":{"sessionID":"ses_root","part":{"type":"tool","messageID":"msg_current","tool":"bash","callID":"running-native","state":{"status":"running"}}}})).unwrap();
    let out = c
        .receive(json!({"type":"session.idle","properties":{"sessionID":"ses_root"}}))
        .unwrap();
    assert!(events(&out).iter().any(|e|matches!(e,ConversationEvent::Completed {stop_reason,..}if stop_reason=="awaiting_background")));
}

#[test]
fn native_session_created_parent_chain_allows_only_child_interactions_with_original_scope() {
    let mut c = bound();
    prompt(&mut c, "d");
    for (id, parent) in [
        ("ses_child", "ses_root"),
        ("ses_grandchild", "ses_child"),
        ("ses_foreign", "ses_other"),
    ] {
        c.receive(
            json!({"type":"session.created","properties":{"info":{"id":id,"parentID":parent}}}),
        )
        .unwrap();
    }
    let question = |session: &str, id: &str| json!({"type":"question.v2.asked","properties":{"sessionID":session,"id":id,"questions":[{"header":"test","question":"Run local tests?","options":[]}]}});
    let out = c.receive(question("ses_grandchild", "que_child")).unwrap();
    assert!(
        matches!(events(&out)[0],ConversationEvent::Interaction {title,..}if title.starts_with("Subtask"))
    );
    assert!(c
        .receive(question("ses_foreign", "que_foreign"))
        .unwrap()
        .is_empty());
    assert!(c
        .receive(question("ses_unknown", "que_unknown"))
        .unwrap()
        .is_empty());
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "que_child".into(),
            option_id: None,
            answers: json!({"0":["approved local tests"]}),
        })
        .unwrap();
    assert!(http(&reply)
        .2
        .starts_with("/api/session/ses_grandchild/question/que_child/reply?"));
    assert!(c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_child","info":{"id":message_id("d"),"role":"user"}}})).unwrap().is_empty());
    assert!(!c.active.as_ref().unwrap().consumed);
    assert!(c.receive(json!({"type":"message.part.updated","properties":{"sessionID":"ses_child","part":{"type":"tool","tool":"write","messageID":"msg_child","callID":"call_child","state":{"status":"completed","input":{"filePath":".lead/child/SPEC.md"}}}}})).unwrap().is_empty());
    assert_eq!(c.root.as_deref(), Some("ses_root"));
}

#[test]
fn native_child_approval_survives_root_idle_and_resolution_cannot_cross_sessions() {
    let mut c = bound();
    prompt(&mut c, "d");
    c.receive(json!({"type":"message.updated","properties":{"sessionID":"ses_root","info":{"id":message_id("d"),"role":"user"}}})).unwrap();
    c.receive(json!({"type":"session.created","properties":{"info":{"id":"ses_child","parentID":"ses_root"}}})).unwrap();
    c.receive(json!({"type":"permission.asked","properties":{"sessionID":"ses_child","id":"per_child","permission":"bash"}})).unwrap();
    c.receive(json!({"type":"session.idle","properties":{"sessionID":"ses_root"}}))
        .unwrap();
    assert!(c.interactions.contains_key("per_child"));
    assert!(c.receive(json!({"type":"permission.replied","properties":{"sessionID":"ses_root","requestID":"per_child","reply":"once"}})).unwrap().is_empty());
    assert!(c.interactions.contains_key("per_child"));
    let resolved=c.receive(json!({"type":"permission.replied","properties":{"sessionID":"ses_child","requestID":"per_child","reply":"once"}})).unwrap();
    assert!(
        matches!(events(&resolved)[0],ConversationEvent::InteractionResolved {interaction_id}if interaction_id=="per_child")
    );
}
