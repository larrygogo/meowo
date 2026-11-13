use super::*;

fn send(actions: &[ConversationAction]) -> &Value {
    actions
        .iter()
        .find_map(|a| {
            if let ConversationAction::Send(v) = a {
                Some(v)
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
fn bound() -> Codec {
    let mut c = Codec::new(ConversationInit {
        cwd: "C:/isolated".into(),
        ..Default::default()
    });
    let init = c.initialize().unwrap();
    let next = c
        .receive(json!({"id":send(&init)["id"],"result":{"userAgent":"codex"}}))
        .unwrap();
    let bind = next
        .iter()
        .find_map(|a| {
            if let ConversationAction::Send(v) = a {
                (v["method"] == "thread/start").then_some(v)
            } else {
                None
            }
        })
        .unwrap();
    let result=c.receive(json!({"id":bind["id"],"result":{"thread":{"id":"root","parentThreadId":null,"turns":[]},"model":"gpt","approvalPolicy":"on-request"}})).unwrap();
    assert!(events(&result).iter().any(
        |e| matches!(e,ConversationEvent::Bound {native_session_id}if native_session_id=="root")
    ));
    assert!(events(&result)
        .iter()
        .any(|e| matches!(e, ConversationEvent::Ready)));
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
fn initialize_create_accept_consume_complete_are_distinct() {
    let mut c = bound();
    let actions = prompt(&mut c, "delivery-1");
    assert_eq!(
        send(&actions)["params"]["clientUserMessageId"],
        "delivery-1"
    );
    let accepted = c
        .receive(json!({"id":send(&actions)["id"],"result":{"turn":{"id":"turn1"}}}))
        .unwrap();
    assert!(matches!(
        events(&accepted)[0],
        ConversationEvent::Accepted { .. }
    ));
    assert!(!events(&accepted)
        .iter()
        .any(|e| matches!(e, ConversationEvent::Consumed { .. })));
    let consumed = c
        .receive(
            json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"turn1"}}}),
        )
        .unwrap();
    assert!(matches!(
        events(&consumed)[0],
        ConversationEvent::Consumed { .. }
    ));
    let done=c.receive(json!({"method":"turn/completed","params":{"threadId":"root","turn":{"id":"turn1","status":"completed"}}})).unwrap();
    assert!(matches!(
        events(&done)[0],
        ConversationEvent::Completed { .. }
    ));
    let duplicate = prompt(&mut c, "delivery-1");
    assert!(matches!(
        events(&duplicate)[0],
        ConversationEvent::Consumed { .. }
    ));
    assert!(!duplicate
        .iter()
        .any(|a| matches!(a, ConversationAction::Send(_))));
}

#[test]
fn subagent_events_and_stale_turn_completion_cannot_rebind_or_finish_root() {
    let mut c = bound();
    prompt(&mut c, "delivery");
    assert!(c
        .receive(json!({"method":"turn/started","params":{"threadId":"child","turn":{"id":"bad"}}}))
        .unwrap()
        .is_empty());
    c.receive(
        json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"current"}}}),
    )
    .unwrap();
    assert!(c.receive(json!({"method":"turn/completed","params":{"threadId":"root","turn":{"id":"stale","status":"completed"}}})).unwrap().is_empty());
    assert_eq!(c.root.as_deref(), Some("root"));
    assert!(c.active.is_some());
    let cancel = c.input(ConversationInput::Cancel).unwrap();
    assert_eq!(send(&cancel)["params"]["turnId"], "current");
}

#[test]
fn native_user_message_enriches_idempotency_mapping() {
    let mut c = bound();
    prompt(&mut c, "delivery");
    c.receive(json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"t"}}}))
        .unwrap();
    let consumed=c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"t","item":{"id":"native-user","type":"userMessage","clientId":"delivery","content":[]}}})).unwrap();
    assert!(
        matches!(events(&consumed)[0],ConversationEvent::Consumed {native_message_id:Some(id),..}if id=="native-user")
    );
    assert_eq!(
        c.seen.get("delivery").map(String::as_str),
        Some("native-user")
    );
}

#[test]
fn resume_checks_root_and_reconciles_client_ids_before_ready() {
    let mut c = Codec::new(ConversationInit {
        native_session_id: Some("root".into()),
        ..Default::default()
    });
    let out=c.bind(&json!({"thread":{"id":"root","parentThreadId":null,"turns":[{"status":"completed","items":[{"type":"userMessage","id":"u1","clientId":"stable-delivery","content":[{"type":"text","text":"saved"}]}]}]}})).unwrap();
    assert!(events(&out).iter().any(
        |e| matches!(e,ConversationEvent::Consumed {request_id,..}if request_id=="stable-delivery")
    ));
    assert!(prompt(&mut c, "stable-delivery")
        .iter()
        .all(|a| !matches!(a, ConversationAction::Send(_))));
    assert!(c
        .bind(&json!({"thread":{"id":"child","parentThreadId":"root","turns":[]}}))
        .is_err());
    assert!(c
        .bind(&json!({"thread":{"id":"root","historyMode":"paginated","turns":[]}}))
        .is_ok());
}

#[test]
fn approval_and_question_reply_only_to_original_native_id() {
    let mut c = bound();
    let request = json!({"id":17,"method":"item/commandExecution/requestApproval","params":{"threadId":"root","kind":"humanCommand","command":"git status"}});
    let out = c.receive(request).unwrap();
    assert!(
        matches!(events(&out)[0],ConversationEvent::Interaction {choices,..}if !choices.iter().any(|c|c.id=="acceptForSession"))
    );
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "17".into(),
            option_id: Some("invented".into()),
            answers: Value::Null
        })
        .is_err());
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "17".into(),
            option_id: Some("decline".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert_eq!(send(&reply)["id"], 17);
    assert_eq!(send(&reply)["result"]["decision"], "decline");
    c.receive(json!({"id":"q","method":"item/tool/requestUserInput","params":{"threadId":"root","questions":[{"id":"why","header":"h","question":"q"}]}})).unwrap();
    let answer = c
        .input(ConversationInput::Resolve {
            interaction_id: "\"q\"".into(),
            option_id: None,
            answers: json!({"why":["answer"]}),
        })
        .unwrap();
    assert_eq!(
        send(&answer)["result"]["answers"]["why"]["answers"],
        json!(["answer"])
    );
}

#[test]
fn unknown_server_request_is_rejected_not_allowed() {
    let mut c = bound();
    let out = c
        .receive(json!({"id":9,"method":"unknown/danger","params":{"threadId":"root"}}))
        .unwrap();
    assert_eq!(send(&out)["error"]["code"], -32601);
}

#[test]
fn native_item_segments_keep_distinct_assistant_messages_in_one_delivery() {
    let mut c = bound();
    prompt(&mut c, "same-delivery");
    let delta = |item: &str, body: &str| json!({"method":"item/agentMessage/delta","params":{"threadId":"root","itemId":item,"delta":body}});
    let first = c.receive(delta("first-item", "before tool")).unwrap();
    let chunk = c.receive(delta("first-item", " more")).unwrap();
    let second = c.receive(delta("second-item", "after tool")).unwrap();
    let segment = |a: &[ConversationAction]| match events(a)[0] {
        ConversationEvent::Output { segment_id, .. } => segment_id.clone(),
        _ => panic!(),
    };
    assert_eq!(segment(&first), segment(&chunk));
    assert_eq!(segment(&first), "first-item");
    assert_ne!(segment(&first), segment(&second));
    assert!(c
        .input(ConversationInput::SetMode {
            dimension: "collaboration".into(),
            mode: "plan".into()
        })
        .is_err());
}

#[test]
fn model_catalog_uses_native_names_and_does_not_invent_collaboration_modes() {
    let mut c = bound();
    let action = c.rpc("model/list", json!({}), Pending::Models);
    let value = if let ConversationAction::Send(value) = action {
        value
    } else {
        panic!()
    };
    let out=c.receive(json!({"id":value["id"],"result":{"data":[{"id":"native","model":"actual-model","displayName":"Native model"},{"id":"hidden","hidden":true}],"nextCursor":null}})).unwrap();
    assert!(
        matches!(events(&out)[0],ConversationEvent::Catalog {models,modes,mode_dimension:None}if models.len()==1&&models[0].id=="actual-model"&&modes.is_empty())
    );
}

#[test]
fn verified_native_child_tree_can_ask_approval_without_rebinding_or_consuming_root() {
    let mut c = bound();
    let root_prompt = prompt(&mut c, "root-delivery");
    assert_eq!(send(&root_prompt)["params"]["threadId"], "root");
    for (child, parent) in [
        ("child", "root"),
        ("grandchild", "child"),
        ("foreign", "foreign-root"),
    ] {
        c.receive(json!({"method":"thread/started","params":{"thread":{"id":child,"parentThreadId":parent}}})).unwrap();
    }
    let approval = |id: u64, thread: &str| json!({"id":id,"method":"item/commandExecution/requestApproval","params":{"threadId":thread,"command":"cargo test"}});
    let request = c.receive(approval(41, "grandchild")).unwrap();
    assert!(
        matches!(events(&request)[0],ConversationEvent::Interaction {title,..}if title.starts_with("Subtask"))
    );
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "41".into(),
            option_id: Some("accept".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert_eq!(send(&reply)["id"], 41);
    assert_eq!(send(&reply)["result"]["decision"], "accept");
    assert_eq!(
        send(&c.receive(approval(42, "foreign")).unwrap())["error"]["code"],
        -32602
    );
    assert_eq!(
        send(&c.receive(approval(43, "unknown-child")).unwrap())["error"]["code"],
        -32602
    );
    assert!(c.receive(json!({"method":"turn/started","params":{"threadId":"child","turn":{"id":"child-turn"}}})).unwrap().is_empty());
    assert!(!c.active.as_ref().unwrap().consumed);
    assert!(c.active.as_ref().unwrap().turn.is_none());
    assert_eq!(c.root.as_deref(), Some("root"));
}

#[test]
fn child_notification_before_bind_is_not_authority_until_root_is_verified() {
    let mut c = Codec::new(ConversationInit {
        native_session_id: Some("root".into()),
        ..Default::default()
    });
    c.receive(json!({"method":"thread/started","params":{"thread":{"id":"child","parentThreadId":"root"}}})).unwrap();
    assert!(!c.owns_child("child"));
    c.bind(&json!({"thread":{"id":"root","parentThreadId":null,"turns":[]}}))
        .unwrap();
    assert!(c.owns_child("child"));
    c.thread_parents.insert("cycle-a".into(), "cycle-b".into());
    c.thread_parents.insert("cycle-b".into(), "cycle-a".into());
    assert!(!c.owns_child("cycle-a"));
}

#[test]
fn persisted_paginated_full_hydration_is_supported_but_summary_is_not_empty_history() {
    let mut c = Codec::new(ConversationInit {
        native_session_id: Some("root".into()),
        ..Default::default()
    });
    let native = json!({"thread":{"id":"root","parentThreadId":null,"historyMode":"paginated","turns":[{"id":"saved-turn","status":"completed","itemsView":"full","items":[{"id":"saved-user","type":"userMessage","clientId":"saved-delivery","content":[{"type":"text","text":"saved prompt"}]},{"id":"saved-assistant","type":"agentMessage","text":"saved answer"}]}]}});
    let out = c.bind(&native).unwrap();
    assert!(events(&out)
        .iter()
        .any(|e| matches!(e,ConversationEvent::History {messages}if messages.len()==2)));
    assert!(prompt(&mut c, "saved-delivery")
        .iter()
        .all(|a| !matches!(a, ConversationAction::Send(_))));
    let mut partial = native;
    partial["thread"]["turns"][0]["itemsView"] = json!("summary");
    assert_eq!(
        c.bind(&partial).unwrap_err(),
        "conversation/history_incomplete"
    );
    assert!(!c.ready);
}

#[test]
fn parent_completion_does_not_lose_live_child_approval_and_native_resolution_is_scoped() {
    let mut c = bound();
    prompt(&mut c, "root-delivery");
    c.receive(
        json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"root-turn"}}}),
    )
    .unwrap();
    c.receive(json!({"method":"thread/started","params":{"thread":{"id":"child","parentThreadId":"root"}}})).unwrap();
    c.receive(json!({"id":71,"method":"item/commandExecution/requestApproval","params":{"threadId":"child","command":"cargo test"}})).unwrap();
    c.receive(json!({"method":"turn/completed","params":{"threadId":"root","turn":{"id":"root-turn","status":"completed"}}})).unwrap();
    assert!(c.interactions.contains_key("71"));
    assert!(c.receive(json!({"method":"serverRequest/resolved","params":{"threadId":"foreign","requestId":71}})).unwrap().is_empty());
    assert!(c.interactions.contains_key("71"));
    let resolved = c
        .receive(
            json!({"method":"serverRequest/resolved","params":{"threadId":"child","requestId":71}}),
        )
        .unwrap();
    assert!(
        matches!(events(&resolved)[0],ConversationEvent::InteractionResolved {interaction_id}if interaction_id=="71")
    );
    assert!(!c.interactions.contains_key("71"));
}

#[test]
fn history_uses_native_item_ids_and_only_native_client_id_correlates_a_user_request() {
    let mut c = bound();
    let out=c.history(&json!({"turns":[{"itemsView":"full","items":[
        {"type":"userMessage","id":"user-native","clientId":"stable-delivery","content":[{"text":"same text"}]},
        {"type":"agentMessage","id":"assistant-native","text":"answer"},
        {"type":"reasoning","id":"reasoning-native","summary":["thinking"],"content":[]},
        {"type":"userMessage","id":"another-user","content":[{"text":"same text"}]}
    ]}]})).unwrap();
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
    assert_eq!(messages[0].segment_id.as_deref(), Some("user-native"));
    assert_eq!(messages[0].request_id.as_deref(), Some("stable-delivery"));
    assert_eq!(messages[1].segment_id.as_deref(), Some("assistant-native"));
    assert!(messages[1].request_id.is_none());
    assert_eq!(messages[2].segment_id.as_deref(), Some("reasoning-native"));
    assert_eq!(messages[2].role, "reasoning");
    assert_eq!(messages[3].text, messages[0].text);
    assert!(messages[3].request_id.is_none());
}

#[test]
fn only_successful_current_root_native_writes_emit_task_facts() {
    let mut c = bound();
    prompt(&mut c, "current-request");
    c.receive(
        json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"current-turn"}}}),
    )
    .unwrap();
    let event = |thread: &str, turn: &str, item: Value| json!({"method":"item/completed","params":{"threadId":thread,"turnId":turn,"item":item}});
    let patch = json!({"id":"patch","type":"fileChange","status":"completed","changes":[{"path":".lead/actual/SPEC.md"}]});
    let out = c
        .receive(event("root", "current-turn", patch.clone()))
        .unwrap();
    assert!(events(&out).iter().any(|e|matches!(e,ConversationEvent::TaskWrite {request_id,call,..}if request_id=="current-request"&&call["input"]["file_path"]==".lead/actual/SPEC.md")));
    for (thread, turn, status) in [
        ("child", "current-turn", "completed"),
        ("root", "stale-turn", "completed"),
        ("root", "current-turn", "failed"),
        ("root", "current-turn", "inProgress"),
    ] {
        let mut item = patch.clone();
        item["status"] = json!(status);
        assert!(!events(&c.receive(event(thread, turn, item)).unwrap())
            .iter()
            .any(|e| matches!(e, ConversationEvent::TaskWrite { .. })));
    }
    for (command, exit, writes) in [
        ("cat .lead/read/SPEC.md", 0, false),
        ("cp .lead/read/SPEC.md /tmp/out", 0, false),
        ("echo '> .lead/fake/SPEC.md'", 0, false),
        ("echo text > .lead/real/progress.md", 1, false),
        ("echo text > .lead/real/progress.md", 0, true),
    ] {
        let out=c.receive(event("root","current-turn",json!({"id":command,"type":"commandExecution","status":"completed","exitCode":exit,"command":command}))).unwrap();
        assert_eq!(
            events(&out)
                .iter()
                .any(|e| matches!(e, ConversationEvent::TaskWrite { .. })),
            writes,
            "{command}"
        );
    }
}

#[test]
fn known_native_in_progress_work_defers_auto_stop_without_guessing_from_process_ids() {
    let mut c = bound();
    prompt(&mut c, "d");
    c.receive(json!({"method":"turn/started","params":{"threadId":"root","turn":{"id":"t"}}}))
        .unwrap();
    c.receive(json!({"method":"item/completed","params":{"threadId":"root","turnId":"t","item":{"id":"background-command","type":"commandExecution","status":"inProgress","processId":"123","exitCode":null}}})).unwrap();
    let out=c.receive(json!({"method":"turn/completed","params":{"threadId":"root","turn":{"id":"t","status":"completed"}}})).unwrap();
    assert!(events(&out).iter().any(|e|matches!(e,ConversationEvent::Completed {stop_reason,..}if stop_reason=="awaiting_background")));
}

#[test]
fn available_decisions_are_exact_and_structured_policy_is_roundtripped_to_original_rpc() {
    let mut c = bound();
    let native = json!(["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["Get-Content","-LiteralPath"]}},"cancel"]);
    let out=c.receive(json!({"id":91,"method":"item/commandExecution/requestApproval","params":{"threadId":"root","command":"Get-Content -LiteralPath probe","availableDecisions":native}})).unwrap();
    let choices = events(&out)
        .into_iter()
        .find_map(|e| {
            if let ConversationEvent::Interaction { choices, .. } = e {
                Some(choices)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["accept", "native-decision-1", "cancel"]
    );
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "91".into(),
            option_id: Some("acceptForSession".into()),
            answers: Value::Null
        })
        .is_err());
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "91".into(),
            option_id: Some("decline".into()),
            answers: Value::Null
        })
        .is_err());
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "91".into(),
            option_id: Some("native-decision-1".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert_eq!(send(&reply)["id"], 91);
    assert_eq!(send(&reply)["result"]["decision"], native[1]);
    assert_eq!(c.receive(json!({"id":92,"method":"item/commandExecution/requestApproval","params":{"threadId":"root","availableDecisions":["accept",{"unknownPolicy":{}}]}})).unwrap_err(),"conversation/approval_decision_unsupported");
}

fn patch_changes() -> Value {
    json!([
        {"path":"C:/private/.lead/probe/decisions.md","kind":{"type":"add"},"diff":"+accepted decision"},
        {"path":"C:/private/.lead/probe/gate.json","kind":{"type":"update","move_path":null},"diff":"-answered\n+consumed"}
    ])
}

fn file_approval_detail(actions: &[ConversationAction]) -> (&str, &Vec<ConversationChoice>) {
    events(actions)
        .into_iter()
        .find_map(|event| match event {
            ConversationEvent::Interaction {
                detail, choices, ..
            } => Some((detail.as_str(), choices)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn file_approval_joins_prior_native_item_using_all_three_identifiers() {
    let mut c = bound();
    let changes = patch_changes();
    c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"turn","item":{"id":"patch","type":"fileChange","status":"inProgress","changes":changes}}})).unwrap();
    let out=c.receive(json!({"id":101,"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":"turn","itemId":"patch","grantRoot":null,"startedAtMs":123}})).unwrap();
    let (detail, choices) = file_approval_detail(&out);
    let detail: Value = serde_json::from_str(detail).unwrap();
    assert_eq!(detail["changes"], changes);
    assert_eq!(detail["request"]["itemId"], "patch");
    assert!(choices.iter().any(|choice| choice.id == "accept"));
    for (turn, item) in [("different-turn", "patch"), ("turn", "different-item")] {
        let out=c.receive(json!({"id":format!("{turn}-{item}"),"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":turn,"itemId":item,"grantRoot":"C:/private"}})).unwrap();
        let (detail, choices) = file_approval_detail(&out);
        assert!(detail.contains("conversation/approval_details_missing"));
        assert_eq!(
            choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["cancel"]
        );
    }
    assert!(c.receive(json!({"method":"item/started","params":{"threadId":"unknown","turnId":"turn","item":{"id":"foreign","type":"fileChange","changes":changes}}})).unwrap().is_empty());
    assert!(!c
        .file_changes
        .keys()
        .any(|(thread, _, _)| thread == "unknown"));
}

#[test]
fn verified_child_file_approval_has_its_own_diff_without_root_tool_or_consumption() {
    let mut c = bound();
    c.receive(json!({"method":"thread/started","params":{"thread":{"id":"child","parentThreadId":"root"}}})).unwrap();
    let out=c.receive(json!({"method":"item/started","params":{"threadId":"child","turnId":"same-turn","item":{"id":"same-item","type":"fileChange","changes":patch_changes()}}})).unwrap();
    assert!(out.is_empty());
    let out=c.receive(json!({"id":"child-rpc","method":"item/fileChange/requestApproval","params":{"threadId":"child","turnId":"same-turn","itemId":"same-item"}})).unwrap();
    let (detail, choices) = file_approval_detail(&out);
    assert!(detail.contains("+consumed"));
    assert!(choices.iter().any(|c| c.id == "accept"));
    assert!(
        matches!(events(&out)[0],ConversationEvent::Interaction {title,..} if title.starts_with("Subtask"))
    );
    let root=c.receive(json!({"id":104,"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":"same-turn","itemId":"same-item"}})).unwrap();
    assert!(file_approval_detail(&root)
        .0
        .contains("approval_details_missing"));
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "\"child-rpc\"".into(),
            option_id: Some("accept".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert_eq!(send(&reply)["id"], "child-rpc");
    assert_eq!(send(&reply)["result"]["decision"], "accept");
}

#[test]
fn oversized_or_unknown_file_details_disable_grants_and_cache_is_bounded() {
    let mut c = bound();
    let mut changes = patch_changes();
    changes[0]["diff"] = json!("x".repeat(FILE_CHANGE_ITEM_BYTES));
    c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"t","item":{"id":"large","type":"fileChange","changes":changes}}})).unwrap();
    let out=c.receive(json!({"id":105,"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":"t","itemId":"large"}})).unwrap();
    let (detail, choices) = file_approval_detail(&out);
    assert!(detail.contains("approval_details_too_large"));
    assert_eq!(
        choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["cancel"]
    );
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "105".into(),
            option_id: Some("accept".into()),
            answers: Value::Null
        })
        .is_err());
    let reply = c
        .input(ConversationInput::Resolve {
            interaction_id: "105".into(),
            option_id: Some("cancel".into()),
            answers: Value::Null,
        })
        .unwrap();
    assert_eq!(send(&reply)["result"]["decision"], "cancel");
    for index in 0..80 {
        c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"t","item":{"id":format!("item-{index}"),"type":"fileChange","changes":patch_changes()}}})).unwrap();
    }
    assert!(c.file_changes.len() <= FILE_CHANGE_CACHE_ITEMS);
    assert!(c.file_change_order.len() <= FILE_CHANGE_CACHE_ITEMS);
    assert!(c.file_change_bytes <= FILE_CHANGE_CACHE_BYTES);
    c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"t","item":{"id":"unknown","type":"fileChange","changes":[{"path":"somewhere","kind":{"type":"unknown"},"diff":"+text"}]}}})).unwrap();
    let out=c.receive(json!({"id":106,"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":"t","itemId":"unknown"}})).unwrap();
    assert!(file_approval_detail(&out)
        .0
        .contains("approval_details_invalid"));
    assert_eq!(file_approval_detail(&out).1.len(), 1);
}

#[test]
fn native_patch_updates_refresh_a_pending_review_and_revoke_grants_if_unreviewable() {
    let mut c = bound();
    c.receive(json!({"id":107,"method":"item/fileChange/requestApproval","params":{"threadId":"root","turnId":"t","itemId":"p"}})).unwrap();
    let out=c.receive(json!({"method":"item/started","params":{"threadId":"root","turnId":"t","item":{"id":"p","type":"fileChange","changes":patch_changes()}}})).unwrap();
    assert!(file_approval_detail(&out)
        .1
        .iter()
        .any(|c| c.id == "accept"));
    let out=c.receive(json!({"method":"item/fileChange/patchUpdated","params":{"threadId":"root","turnId":"t","itemId":"p","changes":[]}})).unwrap();
    assert!(file_approval_detail(&out)
        .0
        .contains("approval_details_invalid"));
    assert_eq!(
        file_approval_detail(&out)
            .1
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["cancel"]
    );
    assert!(c
        .input(ConversationInput::Resolve {
            interaction_id: "107".into(),
            option_id: Some("accept".into()),
            answers: Value::Null
        })
        .is_err());
}
