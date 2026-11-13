use super::*;
use crate::registry::AgentPlugin;

fn sent(actions: &[ConversationAction]) -> Value {
    actions
        .iter()
        .find_map(|action| {
            if let ConversationAction::Send(value) = action {
                Some(value.clone())
            } else {
                None
            }
        })
        .unwrap()
}
fn events(actions: Vec<ConversationAction>) -> Vec<ConversationEvent> {
    actions
        .into_iter()
        .filter_map(|action| {
            if let ConversationAction::Emit(event) = action {
                Some(event)
            } else {
                None
            }
        })
        .collect()
}
fn init_response(id: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1,"agentInfo":{"name":"Kimi Code CLI","version":"2.5.3"},"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true}}}})
}
fn ready(resume: bool) -> AcpCodec {
    let mut codec = AcpCodec::new(
        ConversationInit {
            cwd: "C:/isolated".into(),
            native_session_id: resume.then(|| "root".into()),
            ..Default::default()
        },
        "Kimi Code CLI",
        (2, 5),
    );
    let request = sent(&codec.initialize().unwrap());
    let next = sent(&codec.receive(init_response(request["id"].clone())).unwrap());
    assert_eq!(
        next["method"],
        if resume {
            "session/load"
        } else {
            "session/new"
        }
    );
    codec
        .receive(json!({"jsonrpc":"2.0","id":next["id"],"result":{"sessionId":"root"}}))
        .unwrap();
    codec
}
fn prompt(codec: &mut AcpCodec) -> Value {
    sent(
        &codec
            .input(ConversationInput::Prompt {
                request_id: "delivery-1".into(),
                text: "message".into(),
                attachments: vec![],
            })
            .unwrap(),
    )
}
fn update(sid: &str, kind: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,"update":{"sessionUpdate":kind,"content":{"type":"text","text":"answer"}}}})
}

#[test]
fn current_root_execution_is_consumed_once_and_child_or_idle_events_are_not() {
    let mut codec = ready(false);
    assert!(codec
        .receive(update("root", "agent_message_chunk"))
        .unwrap()
        .is_empty());
    let request = prompt(&mut codec);
    assert!(codec
        .receive(update("child", "agent_message_chunk"))
        .unwrap()
        .is_empty());
    let first = events(
        codec
            .receive(update("root", "agent_message_chunk"))
            .unwrap(),
    );
    assert!(
        matches!(&first[0],ConversationEvent::Consumed{request_id,..}if request_id=="delivery-1")
    );
    assert_eq!(
        first
            .iter()
            .filter(|event| matches!(event, ConversationEvent::Consumed { .. }))
            .count(),
        1
    );
    assert_eq!(
        events(
            codec
                .receive(update("root", "agent_thought_chunk"))
                .unwrap()
        )
        .iter()
        .filter(|event| matches!(event, ConversationEvent::Consumed { .. }))
        .count(),
        0
    );
    let complete = events(
        codec
            .receive(json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}}))
            .unwrap(),
    );
    assert!(complete.iter().any(|event|matches!(event,ConversationEvent::Completed{request_id,..}if request_id=="delivery-1")));
}

#[test]
fn load_replay_is_history_and_never_consumes_a_delivery() {
    let mut codec = AcpCodec::new(
        ConversationInit {
            cwd: "C:/isolated".into(),
            native_session_id: Some("root".into()),
            ..Default::default()
        },
        "Kimi Code CLI",
        (2, 5),
    );
    let req = sent(&codec.initialize().unwrap());
    let load = sent(&codec.receive(init_response(req["id"].clone())).unwrap());
    assert!(codec
        .receive(update("root", "user_message_chunk"))
        .unwrap()
        .is_empty());
    assert!(codec
        .receive(update("root", "agent_message_chunk"))
        .unwrap()
        .is_empty());
    assert!(codec
        .input(ConversationInput::Prompt {
            request_id: "early".into(),
            text: "not yet".into(),
            attachments: vec![]
        })
        .is_err());
    let replay = events(
        codec
            .receive(json!({"jsonrpc":"2.0","id":load["id"],"result":{}}))
            .unwrap(),
    );
    assert!(replay
        .iter()
        .any(|event| matches!(event,ConversationEvent::History{messages}if messages.len()==2)));
    assert!(!replay
        .iter()
        .any(|event| matches!(event, ConversationEvent::Consumed { .. })));
    prompt(&mut codec);
    assert!(codec
        .receive(update("other-root", "user_message_chunk"))
        .unwrap()
        .is_empty());
}

fn permission(codec: &mut AcpCodec, id: Value, sid: &str) -> Vec<ConversationAction> {
    codec.receive(json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{"sessionId":sid,"toolCall":{"title":"Run command","toolCallId":"tool-1"},"options":[{"optionId":"native-once","name":"Allow once","kind":"allow_once"},{"optionId":"native-no","name":"Deny","kind":"reject_once"}]}})).unwrap()
}

#[test]
fn permissions_wait_for_explicit_native_option_and_keep_exact_rpc_identity() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let foreign = permission(&mut codec, json!(19), "child");
    assert_eq!(sent(&foreign)["error"]["code"], -32602);
    let actions = permission(&mut codec, json!("native-request"), "root");
    assert!(actions
        .iter()
        .all(|action| !matches!(action, ConversationAction::Send(_))));
    let interaction = events(actions)
        .into_iter()
        .find_map(|event| {
            if let ConversationEvent::Interaction { interaction_id, .. } = event {
                Some(interaction_id)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        codec
            .input(ConversationInput::Resolve {
                interaction_id: interaction.clone(),
                option_id: Some("allow".into()),
                answers: Value::Null
            })
            .unwrap_err(),
        "conversation/option_invalid"
    );
    let resolved = codec
        .input(ConversationInput::Resolve {
            interaction_id: interaction.clone(),
            option_id: Some("native-no".into()),
            answers: Value::Null,
        })
        .unwrap();
    let response = sent(&resolved);
    assert_eq!(response["id"], "native-request");
    assert_eq!(response["result"]["outcome"]["optionId"], "native-no");
    assert!(codec
        .input(ConversationInput::Resolve {
            interaction_id: interaction,
            option_id: None,
            answers: Value::Null
        })
        .is_err());
}

#[test]
fn cancellation_does_not_complete_until_native_prompt_response_and_unknown_rpc_is_rejected() {
    let mut codec = ready(false);
    let request = prompt(&mut codec);
    permission(&mut codec, json!(7), "root");
    let cancelled = codec.input(ConversationInput::Cancel).unwrap();
    assert!(cancelled.iter().any(|action|matches!(action,ConversationAction::Send(value)if value["method"]=="session/cancel")));
    assert!(!events(cancelled)
        .iter()
        .any(|event| matches!(event, ConversationEvent::Completed { .. })));
    assert!(codec
        .input(ConversationInput::Prompt {
            request_id: "second".into(),
            text: "next".into(),
            attachments: vec![]
        })
        .is_err());
    let done = events(
        codec
            .receive(
                json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"cancelled"}}),
            )
            .unwrap(),
    );
    assert!(done.iter().any(|event|matches!(event,ConversationEvent::Completed{stop_reason,..}if stop_reason=="cancelled")));
    let unknown=codec.receive(json!({"jsonrpc":"2.0","id":"fs-1","method":"fs/write_text_file","params":{"sessionId":"root","path":"C:/outside","content":"no"}})).unwrap();
    assert_eq!(sent(&unknown)["error"]["code"], -32601);
}

#[test]
fn unknown_protocol_and_unadvertised_resume_fail_without_pty_fallback() {
    let mut codec = ready(false);
    assert!(codec.initialize().is_err());
    let mut codec = AcpCodec::new(ConversationInit::default(), "Kimi Code CLI", (2, 5));
    let req = sent(&codec.initialize().unwrap());
    let mut bad = init_response(req["id"].clone());
    bad["result"]["protocolVersion"] = json!(2);
    assert_eq!(
        codec.receive(bad).unwrap_err(),
        "conversation/protocol_unsupported"
    );
    let mut codec = AcpCodec::new(
        ConversationInit {
            native_session_id: Some("root".into()),
            ..Default::default()
        },
        "Kimi Code CLI",
        (2, 5),
    );
    let req = sent(&codec.initialize().unwrap());
    let mut bad = init_response(req["id"].clone());
    bad["result"]["agentCapabilities"]["loadSession"] = json!(false);
    assert_eq!(
        codec.receive(bad).unwrap_err(),
        "conversation/resume_unsupported"
    );
}

#[test]
fn actual_isolated_handshakes_drive_each_native_plugin_and_mcp_forwarding() {
    let providers: [(&dyn AgentPlugin, &str, &str, &str); 2] = [
        (
            &crate::plugins::kimi::Kimi,
            "2.5.3",
            "acp",
            include_str!("fixtures/kimi-initialize.json"),
        ),
        (
            &crate::plugins::gemini::Gemini,
            "0.65.0",
            "--acp",
            include_str!("fixtures/gemini-initialize.json"),
        ),
    ];
    for (plugin, version, arg, fixture) in providers {
        let installation = plugin.variants()[0].installation_at(
            plugin.id(),
            std::path::PathBuf::from("isolated-fixture-directory"),
            None,
        );
        let cap = plugin.conversation().unwrap();
        assert_eq!(
            cap.spec(&installation, Some(version)).unwrap().args,
            vec![arg]
        );
        if plugin.id() == crate::id::GEMINI {
            assert!(cap.spec(&installation, Some("0.66.0")).is_err());
        }
        assert!(cap.spec(&installation, None).is_err());
        assert!(cap.spec(&installation, Some("999.999.0")).is_err());
        let mut codec = cap.codec(ConversationInit {
            cwd: "C:/isolated".into(),
            mcp_servers: vec![
                json!({"name":"lead","command":"isolated-reporter","args":["lead-mcp"],"env":[]}),
            ],
            ..Default::default()
        });
        let request = sent(&codec.initialize().unwrap());
        assert_eq!(request["params"]["clientCapabilities"]["terminal"], false);
        let mut response = serde_json::from_str::<Value>(fixture).unwrap();
        response["id"] = request["id"].clone();
        let create = sent(&codec.receive(response).unwrap());
        assert_eq!(create["method"], "session/new");
        assert_eq!(create["params"]["mcpServers"][0]["name"], "lead");
        let bound = events(
            codec
                .receive(json!({"jsonrpc":"2.0","id":create["id"],"result":{"sessionId":"root"}}))
                .unwrap(),
        );
        assert!(bound.iter().any(|event|matches!(event,ConversationEvent::Bound{native_session_id}if native_session_id=="root")));
    }
}

#[test]
fn current_tools_preserve_native_input_across_partial_updates() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let tool = json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"tool_call","toolCallId":"t-1","title":"Write task","rawInput":{"path":".lead/task/SPEC.md"},"status":"pending"}}});
    codec.receive(tool).unwrap();
    let patch = json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"written"}}]}}});
    let output = events(codec.receive(patch).unwrap());
    assert!(output.iter().any(|event|matches!(event,ConversationEvent::Tool{tool_id,name,input,output:Some(output),status:ConversationToolStatus::Completed,..}if tool_id=="t-1"&&name=="Write task"&&input["path"]==".lead/task/SPEC.md"&&output=="written")));
}

#[test]
fn cancel_before_any_execution_and_shutdown_do_not_fabricate_consumption_or_readiness() {
    let mut codec = ready(false);
    let request = prompt(&mut codec);
    codec.input(ConversationInput::Shutdown).unwrap();
    let done = events(
        codec
            .receive(
                json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"cancelled"}}),
            )
            .unwrap(),
    );
    assert!(!done.iter().any(|event| matches!(
        event,
        ConversationEvent::Consumed { .. } | ConversationEvent::Ready
    )));
    assert_eq!(
        codec
            .input(ConversationInput::Prompt {
                request_id: "after-stop".into(),
                text: "no".into(),
                attachments: vec![]
            })
            .unwrap_err(),
        "conversation/closed"
    );
}

#[test]
fn initial_model_and_mode_are_acknowledged_before_ready() {
    let mut codec = AcpCodec::new(
        ConversationInit {
            cwd: "C:/isolated".into(),
            model: Some("native-model".into()),
            approval_mode: Some("plan".into()),
            ..Default::default()
        },
        "Kimi Code CLI",
        (2, 5),
    );
    let init = sent(&codec.initialize().unwrap());
    let create = sent(&codec.receive(init_response(init["id"].clone())).unwrap());
    let model = codec
        .receive(json!({"jsonrpc":"2.0","id":create["id"],"result":{"sessionId":"root"}}))
        .unwrap();
    assert!(!model
        .iter()
        .any(|action| matches!(action, ConversationAction::Emit(ConversationEvent::Ready))));
    let model = sent(&model);
    assert_eq!(model["method"], "session/set_model");
    assert_eq!(model["params"]["modelId"], "native-model");
    let mode = sent(
        &codec
            .receive(json!({"jsonrpc":"2.0","id":model["id"],"result":{}}))
            .unwrap(),
    );
    assert_eq!(mode["method"], "session/set_mode");
    assert!(events(
        codec
            .receive(json!({"jsonrpc":"2.0","id":mode["id"],"result":{}}))
            .unwrap()
    )
    .iter()
    .any(|event| matches!(event, ConversationEvent::Ready)));
}

fn form_request() -> Value {
    // 来自本机 Kimi 2.5.3 嵌入源码 questionRequestToElicitationParams；不是模型在线提问实测。
    json!({"jsonrpc":"2.0","id":"question-native","method":"elicitation/create","params":{"sessionId":"root","toolCallId":"ask-1","mode":"form","message":"Pick two fields","requestedSchema":{"type":"object","required":["q0","q1"],"properties":{"q0":{"type":"string","title":"Theme","oneOf":[{"const":"dark","title":"Dark"},{"const":"light","title":"Light"}]},"q1":{"type":"array","title":"Features","minItems":1,"items":{"anyOf":[{"const":"a","title":"A"},{"const":"b","title":"B"}]}}}}}})
}

#[test]
fn native_question_form_retains_field_keys_choices_and_multiselect_answers() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let actions = codec.receive(form_request()).unwrap();
    let interaction = events(actions)
        .into_iter()
        .find_map(|event| {
            if let ConversationEvent::Interaction {
                interaction_id,
                questions,
                ..
            } = event
            {
                assert_eq!(questions.len(), 2);
                assert!(questions.iter().any(|q| q.id == "q1" && q.multiple));
                Some(interaction_id)
            } else {
                None
            }
        })
        .unwrap();
    let invalid = codec.input(ConversationInput::Resolve {
        interaction_id: interaction.clone(),
        option_id: Some("accept".into()),
        answers: json!({"q0":"dark","q1":["invented"]}),
    });
    assert_eq!(invalid.unwrap_err(), "conversation/answer_invalid");
    let answers = json!({"q0":"dark","q1":["b","a"]});
    let response = sent(
        &codec
            .input(ConversationInput::Resolve {
                interaction_id: interaction,
                option_id: Some("accept".into()),
                answers: answers.clone(),
            })
            .unwrap(),
    );
    assert_eq!(response["id"], "question-native");
    assert_eq!(
        response["result"],
        json!({"action":"accept","content":answers})
    );
}

#[test]
fn malformed_forms_and_native_requests_are_rejected_without_auto_allow() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let mut unsupported = form_request();
    unsupported["params"]["requestedSchema"]["allOf"] = json!([]);
    assert_eq!(
        sent(&codec.receive(unsupported).unwrap())["error"]["code"],
        -32602
    );
    let malformed = json!({"jsonrpc":"2.0","id":91,"method":"session/request_permission","params":{"sessionId":"root","options":[{"optionId":"x","kind":"allow_once"}]}});
    assert_eq!(
        sent(&codec.receive(malformed).unwrap())["error"]["code"],
        -32602
    );
}

#[test]
fn mode_dimensions_cannot_be_silently_substituted() {
    let mut codec = ready(false);
    for dimension in ["approval", "collaboration", "agent"] {
        assert_eq!(
            codec
                .input(ConversationInput::SetMode {
                    dimension: dimension.into(),
                    mode: "plan".into()
                })
                .unwrap_err(),
            "conversation/mode_dimension_unsupported"
        );
    }
    let request = sent(
        &codec
            .input(ConversationInput::SetMode {
                dimension: "mode".into(),
                mode: "plan".into(),
            })
            .unwrap(),
    );
    assert_eq!(request["method"], "session/set_mode");
    assert_eq!(request["params"]["modeId"], "plan");
}

#[test]
fn streamed_segments_are_stable_within_chunks_and_split_at_tools_and_reasoning() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let output = |codec: &mut AcpCodec, kind: &str| {
        events(codec.receive(update("root", kind)).unwrap())
            .into_iter()
            .find_map(|event| {
                if let ConversationEvent::Output { segment_id, .. } = event {
                    Some(segment_id)
                } else {
                    None
                }
            })
            .unwrap()
    };
    let first = output(&mut codec, "agent_message_chunk");
    assert_eq!(first, output(&mut codec, "agent_message_chunk"));
    let thought = output(&mut codec, "agent_thought_chunk");
    assert_ne!(first, thought);
    assert_eq!(thought, output(&mut codec, "agent_thought_chunk"));
    let second = output(&mut codec, "agent_message_chunk");
    assert_ne!(thought, second);
    assert_ne!(first, second);
    codec.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"tool_call","toolCallId":"tool-between","title":"Read","status":"pending"}}})).unwrap();
    let third = output(&mut codec, "agent_message_chunk");
    assert_ne!(second, third);
    assert_eq!(third, output(&mut codec, "agent_message_chunk"));
}

#[test]
fn kimi_empty_end_turn_does_not_hide_native_non_auth_failure_or_fabricate_consumption() {
    let mut codec = ready(false);
    let request = prompt(&mut codec);
    let actions = events(
        codec
            .receive(json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}}))
            .unwrap(),
    );
    assert!(actions.iter().any(|event|matches!(event,ConversationEvent::Failed{reason,..}if reason=="conversation/no_execution_evidence")));
    assert!(!actions.iter().any(|event| matches!(
        event,
        ConversationEvent::Consumed { .. } | ConversationEvent::Completed { .. }
    )));
}

#[test]
fn native_catalogs_are_exposed_without_static_menu_substitution() {
    let event=catalog(&json!({"models":{"availableModels":[{"modelId":"native-fast","name":"Fast"}]},"modes":{"availableModes":[{"id":"plan","name":"Plan"}]}})).unwrap();
    assert!(
        matches!(event,ConversationEvent::Catalog{models,modes,mode_dimension:Some(dimension)}if models[0].id=="native-fast"&&modes[0].id=="plan"&&dimension=="mode")
    );
    let event=catalog(&json!({"configOptions":[{"id":"model","type":"select","currentValue":"kimi-code","options":[{"value":"kimi-code","name":"Kimi Code"}]}]})).unwrap();
    assert!(
        matches!(event,ConversationEvent::Catalog{models,modes,..}if models[0].id=="kimi-code"&&modes.is_empty())
    );
    assert!(catalog(&json!({})).is_none());
}

fn native_tool(codec: &mut AcpCodec, update: Value) -> Vec<ConversationEvent> {
    events(codec.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":update}})).unwrap())
}

#[test]
fn kimi_successful_native_write_emits_one_canonical_fact_only_after_the_result() {
    let mut codec = ready(false);
    prompt(&mut codec);
    let started = native_tool(
        &mut codec,
        json!({"sessionUpdate":"tool_call","toolCallId":"1:file","title":"Write task specification","kind":"edit","status":"in_progress","rawInput":{"path":".lead/task/SPEC.md","content":"task"}}),
    );
    assert!(!started
        .iter()
        .any(|event| matches!(event, ConversationEvent::TaskWrite { .. })));
    let result = json!({"sessionUpdate":"tool_call_update","toolCallId":"1:file","status":"completed","rawOutput":"File written"});
    let finished = native_tool(&mut codec, result.clone());
    assert!(finished.iter().any(|event|matches!(event,ConversationEvent::TaskWrite{request_id,call,..}if request_id=="delivery-1"&&call==&json!({"name":"Write","input":{"file_path":".lead/task/SPEC.md"}}))));
    assert!(!native_tool(&mut codec, result)
        .iter()
        .any(|event| matches!(event, ConversationEvent::TaskWrite { .. })));
}

#[test]
fn failed_read_background_and_unseen_calls_never_become_write_facts() {
    for (kind, status, input) in [
        ("read", "completed", json!({"path":".lead/task/SPEC.md"})),
        ("edit", "failed", json!({"path":".lead/task/SPEC.md"})),
        (
            "edit",
            "completed",
            json!({"path":".lead/task/SPEC.md","background":true}),
        ),
        (
            "execute",
            "completed",
            json!({"command":"echo x > .lead/task/SPEC.md"}),
        ),
    ] {
        let mut codec = ready(false);
        prompt(&mut codec);
        native_tool(
            &mut codec,
            json!({"sessionUpdate":"tool_call","toolCallId":"x","kind":kind,"status":"in_progress","rawInput":input}),
        );
        let result = native_tool(
            &mut codec,
            json!({"sessionUpdate":"tool_call_update","toolCallId":"x","status":status,"rawOutput":{"exit_code":0}}),
        );
        assert!(!result
            .iter()
            .any(|event| matches!(event, ConversationEvent::TaskWrite { .. })));
    }
    let mut codec = ready(false);
    prompt(&mut codec);
    assert!(!native_tool(&mut codec,json!({"sessionUpdate":"tool_call_update","toolCallId":"unseen-old","kind":"edit","status":"completed","rawInput":{"path":".lead/task/SPEC.md"},"rawOutput":"written"})).iter().any(|event|matches!(event,ConversationEvent::TaskWrite{..})));
}

#[test]
fn gemini_requires_a_current_success_result_diff_not_a_cached_permission_preview() {
    let mut codec = AcpCodec::new(ConversationInit::default(), "gemini-cli", (0, 65));
    let init = sent(&codec.initialize().unwrap());
    let mut fixture =
        serde_json::from_str::<Value>(include_str!("fixtures/gemini-initialize.json")).unwrap();
    fixture["id"] = init["id"].clone();
    let bind = sent(&codec.receive(fixture).unwrap());
    codec
        .receive(json!({"jsonrpc":"2.0","id":bind["id"],"result":{"sessionId":"root"}}))
        .unwrap();
    prompt(&mut codec);
    let diff =
        json!({"type":"diff","path":".lead/task/progress.md","oldText":"before","newText":"after"});
    native_tool(
        &mut codec,
        json!({"sessionUpdate":"tool_call","toolCallId":"gemini-file","kind":"edit","status":"pending","content":[diff.clone()]}),
    );
    assert!(!native_tool(&mut codec,json!({"sessionUpdate":"tool_call_update","toolCallId":"gemini-file","kind":"edit","status":"completed","content":[{"type":"content","content":{"type":"text","text":"no diff"}}]})).iter().any(|event|matches!(event,ConversationEvent::TaskWrite{..})));
    let good = native_tool(
        &mut codec,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"gemini-file","kind":"edit","status":"completed","content":[diff]}),
    );
    assert!(good.iter().any(|event|matches!(event,ConversationEvent::TaskWrite{call,..}if call["input"]["file_path"]==".lead/task/progress.md")));
}

fn replay_codec() -> (AcpCodec, Value) {
    let mut codec = AcpCodec::new(
        ConversationInit {
            cwd: "C:/isolated".into(),
            native_session_id: Some("root".into()),
            ..Default::default()
        },
        "Kimi Code CLI",
        (2, 5),
    );
    let init = sent(&codec.initialize().unwrap());
    let load = sent(&codec.receive(init_response(init["id"].clone())).unwrap());
    (codec, load)
}

#[test]
fn delivery_marker_is_last_and_only_its_suffix_is_removed_from_native_user_history() {
    let original = format!("original{}", delivery_marker("user_supplied_fake"));
    let mut live = ready(false);
    let wire = sent(
        &live
            .input(ConversationInput::Prompt {
                request_id: "real_root_123".into(),
                text: original.clone(),
                attachments: vec![],
            })
            .unwrap(),
    );
    let parts = wire["params"]["prompt"].as_array().unwrap();
    assert_eq!(parts[0]["text"], original);
    assert_eq!(
        parts.last().unwrap()["text"],
        delivery_marker("real_root_123")
    );
    let (mut replay, load) = replay_codec();
    // Kimi按content part分开回放，Gemini把文本parts直接拼接；两种都只在完整用户段结束后strip。
    for part in parts {
        replay.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"user_message_chunk","content":part}}})).unwrap();
    }
    replay
        .receive(update("root", "agent_message_chunk"))
        .unwrap();
    let history = events(
        replay
            .receive(json!({"jsonrpc":"2.0","id":load["id"],"result":{}}))
            .unwrap(),
    )
    .into_iter()
    .find_map(|event| {
        if let ConversationEvent::History { messages } = event {
            Some(messages)
        } else {
            None
        }
    })
    .unwrap();
    assert_eq!(history[0].text, original);
    assert_eq!(history[0].request_id.as_deref(), Some("real_root_123"));
    assert_eq!(history[1].request_id.as_deref(), Some("real_root_123"));
    assert_eq!(history[1].segment_id, None);
}

#[test]
fn marker_validation_is_exact_and_never_guesses_request_ids_from_ordinary_text() {
    for bad in ["", "not an id", "id:colon", "中文", "id\n"] {
        assert!(!valid_delivery_id(bad));
    }
    assert!(!valid_delivery_id(&"a".repeat(161)));
    assert!(valid_delivery_id(&"a".repeat(160)));
    let legacy = "ordinary user message real_root_123";
    assert_eq!(strip_delivery_marker(legacy), (legacy, None));
    let malformed = "user\n\n<!-- meowo:delivery=bad:identifier -->";
    assert_eq!(strip_delivery_marker(malformed), (malformed, None));
    let trailing = "user\n\n<!-- meowo:delivery=real_root_123 --> trailing";
    assert_eq!(strip_delivery_marker(trailing), (trailing, None));
    let mut codec = ready(false);
    assert_eq!(
        codec
            .input(ConversationInput::Prompt {
                request_id: "unsafe:id".into(),
                text: "keep".into(),
                attachments: vec![]
            })
            .unwrap_err(),
        "conversation/delivery_id_invalid"
    );
    prompt(&mut codec);
}

#[test]
fn unmarked_users_reset_replay_identity_and_history_tools_are_never_live_write_facts() {
    let (mut replay, load) = replay_codec();
    let user = format!("root text{}", delivery_marker("known_1"));
    replay.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":user}}}})).unwrap();
    replay
        .receive(update("root", "agent_message_chunk"))
        .unwrap();
    let tool=replay.receive(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"root","update":{"sessionUpdate":"tool_call","toolCallId":"old","kind":"edit","rawInput":{"path":".lead/task/SPEC.md"},"status":"completed"}}})).unwrap();
    assert!(tool.is_empty());
    replay
        .receive(update("root", "user_message_chunk"))
        .unwrap();
    replay
        .receive(update("root", "agent_message_chunk"))
        .unwrap();
    let history = events(
        replay
            .receive(json!({"jsonrpc":"2.0","id":load["id"],"result":{}}))
            .unwrap(),
    )
    .into_iter()
    .find_map(|event| {
        if let ConversationEvent::History { messages } = event {
            Some(messages)
        } else {
            None
        }
    })
    .unwrap();
    assert!(history
        .iter()
        .any(|message| message.role == "tool" && message.text.contains("old")));
    assert_eq!(history.last().unwrap().request_id, None);
}

#[test]
fn live_segment_ids_do_not_collide_across_restored_codec_instances() {
    let mut first = ready(false);
    prompt(&mut first);
    let one = events(
        first
            .receive(update("root", "agent_message_chunk"))
            .unwrap(),
    )
    .into_iter()
    .find_map(|event| {
        if let ConversationEvent::Output { segment_id, .. } = event {
            Some(segment_id)
        } else {
            None
        }
    })
    .unwrap();
    let mut restored = ready(true);
    restored
        .input(ConversationInput::Prompt {
            request_id: "new_delivery_2".into(),
            text: "next".into(),
            attachments: vec![],
        })
        .unwrap();
    let two = events(
        restored
            .receive(update("root", "agent_message_chunk"))
            .unwrap(),
    )
    .into_iter()
    .find_map(|event| {
        if let ConversationEvent::Output { segment_id, .. } = event {
            Some(segment_id)
        } else {
            None
        }
    })
    .unwrap();
    assert_ne!(one, two);
    assert!(two.contains("new_delivery_2"));
}
