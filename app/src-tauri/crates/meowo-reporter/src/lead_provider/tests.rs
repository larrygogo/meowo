use super::*;

#[test]
fn installed_skill_gate_example_is_read_and_answered_by_the_real_protocol() {
    let e = env();
    let skill = meowo_agent::lead_install::SKILL.replace('\r', "");
    let example = skill
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
        .trim();
    fs::write(e.task.join("gate.json"), example).unwrap();
    let gate = lead::read_gate(&e.task).expect("安装的 skill 示例必须符合 reporter 的真实关卡协议");
    assert_eq!(gate.kind, "A");
    assert_eq!(gate.status, "pending");
    lead::answer_gate(
        &e.task,
        &gate.id,
        &meowo_protocol::ipc::LeadGateAnswerDto {
            choice: "confirm".into(),
            note: String::new(),
        },
    )
    .unwrap();
    assert_eq!(lead::read_gate(&e.task).unwrap().status, "answered");
}

struct Env {
    root: tempfile::TempDir,
    task: PathBuf,
    registry: PathBuf,
}
fn env() -> Env {
    let root = tempfile::tempdir().unwrap();
    let task = root.path().join(".lead/20261112-provider");
    fs::create_dir_all(&task).unwrap();
    fs::write(task.join("SPEC.md"), "# task").unwrap();
    fs::write(task.join("progress.md"), "phase: implement\n").unwrap();
    fs::write(
        task.join("acceptance.json"),
        r#"[{"id":1,"check":"finish","passes":false}]"#,
    )
    .unwrap();
    let registry = root.path().join("sessions.json");
    Env {
        root,
        task,
        registry,
    }
}
// 字段形态逐项取自模块头官方来源；这些是协议 fixture，不冒充真实模型闭环探针。
fn claim(e: &Env, provider: &str, session: &str) -> Value {
    let file = e.task.join("progress.md");
    match provider {
        "codex" => {
            json!({"hook_event_name":"PostToolUse","session_id":session,"cwd":e.root.path(),"turn_id":"t1","tool_name":"apply_patch","tool_input":{"command":format!("*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch",file.display())},"tool_response":"Success. Updated the following files:\nM progress.md"})
        }
        "kimi" => {
            json!({"hook_event_name":"PostToolUse","session_id":session,"cwd":e.root.path(),"client_type":"kimi_code_cli","tool_name":"Write","tool_input":{"path":file,"content":"new"}})
        }
        "gemini" => {
            json!({"hook_event_name":"AfterTool","session_id":session,"cwd":e.root.path(),"tool_name":"write_file","tool_input":{"file_path":file,"content":"new"},"tool_response":{"llmContent":"File written."}})
        }
        "opencode" => {
            json!({"hook_event_name":"tool.execute.after","cwd":e.root.path(),"input":{"sessionID":session,"tool":"write","callID":"c1","args":{"filePath":file,"content":"new"}},"output":{"title":"write","output":"ok","metadata":{}}})
        }
        _ => unreachable!(),
    }
}
fn call(e: &Env, provider: &str, mode: &str, value: Value) -> Output {
    run(provider, mode, &value.to_string(), &e.registry)
}
fn prompt(e: &Env, provider: &str, session: &str, text: &str) -> Value {
    match provider {
        "gemini" => {
            json!({"hook_event_name":"BeforeAgent","session_id":session,"cwd":e.root.path(),"prompt":text})
        }
        "opencode" => {
            json!({"hook_event_name":"chat.message","cwd":e.root.path(),"input":{"sessionID":session},"output":{"parts":[{"type":"text","text":text}]}})
        }
        _ => {
            json!({"hook_event_name":"UserPromptSubmit","session_id":session,"client_type":if provider == "kimi" { "kimi_code_cli" } else { "codex" },"cwd":e.root.path(),"turn_id":"t1","prompt":text})
        }
    }
}
fn stop(e: &Env, provider: &str, session: &str, text: &str) -> Value {
    json!({"hook_event_name":if provider == "gemini" { "AfterAgent" } else { "Stop" },"session_id":session,"client_type":if provider == "kimi" { "kimi_code_cli" } else { "other" },"cwd":e.root.path(),"turn_id":"t1","prompt":text,"stop_hook_active":false})
}

#[test]
fn native_write_fixtures_claim_only_the_native_session() {
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        let mut raw = claim(&e, provider, "owner-123");
        if provider == "opencode" {
            raw["input"]["args"]["sessionID"] = json!("victim");
        } else {
            raw["tool_input"]["session_id"] = json!("victim");
        }
        assert_eq!(call(&e, provider, "claim", raw), Output::default());
        assert_eq!(
            fs::read_to_string(e.task.join(".owner")).unwrap(),
            "owner-123",
            "{provider}"
        );
    }
}

#[test]
fn reads_failed_unknown_and_mcp_identity_do_not_claim() {
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        let mut raw = claim(&e, provider, "owner");
        if provider == "opencode" {
            raw["input"]["tool"] = json!("read");
        } else {
            raw["tool_name"] = json!("Read");
        }
        call(&e, provider, "claim", raw);
        let mut raw = claim(&e, provider, "owner");
        if provider == "opencode" {
            raw["output"]["error"] = json!("failed");
        } else {
            raw["tool_response"] = json!({"error":"failed","exit_code":1});
        }
        call(&e, provider, "claim", raw);
        call(
            &e,
            provider,
            "claim",
            json!({"hook_event_name":"PostToolUse","tool_input":{"session_id":"owner","path":e.task.join("progress.md")}}),
        );
        assert!(!e.task.join(".owner").exists(), "{provider}");
    }
    let e = env();
    assert_eq!(
        run("unknown", "claim", "not json", &e.registry),
        Output::default()
    );
    assert_eq!(
        run("kimi", "claim", r#"{"tool_name":"Read"}"#, &e.registry),
        Output::default()
    );
    assert!(!e.registry.exists());
}

#[test]
fn another_sessions_owner_is_not_stolen() {
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        fs::write(e.task.join(".owner"), "original-owner").unwrap();
        call(&e, provider, "claim", claim(&e, provider, "new-owner"));
        assert_eq!(
            fs::read_to_string(e.task.join(".owner")).unwrap(),
            "original-owner"
        );
    }
}

#[test]
fn parallel_native_claims_reserve_one_owner() {
    let e = env();
    std::thread::scope(|scope| {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
        for index in 0..6 {
            let barrier = barrier.clone();
            let e = &e;
            scope.spawn(move || {
                barrier.wait();
                call(
                    e,
                    "kimi",
                    "claim",
                    claim(e, "kimi", &format!("owner-{index}")),
                );
            });
        }
    });
    let owner = fs::read_to_string(e.task.join(".owner")).unwrap();
    let registry: Value = serde_json::from_slice(&fs::read(&e.registry).unwrap()).unwrap();
    assert_eq!(registry.as_object().unwrap().len(), 1);
    assert!(registry.get(&owner).is_some());
}

#[test]
fn malformed_success_responses_and_unverified_kimi_variants_are_noops() {
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        let mut raw = claim(&e, provider, "owner");
        match provider {
            "codex" => raw["tool_response"] = json!("unknown output"),
            "kimi" => raw["client_type"] = json!("kimi_cli"),
            "gemini" => raw["tool_response"] = json!({}),
            _ => raw["output"] = json!({}),
        }
        call(&e, provider, "claim", raw);
        assert!(!e.task.join(".owner").exists(), "{provider}");
    }
}

#[test]
fn new_runtime_and_new_turn_drop_previous_write_facts() {
    let e = env();
    call(&e, "codex", "claim", claim(&e, "codex", "owner"));
    call(
        &e,
        "codex",
        "session",
        prompt(&e, "codex", "owner", "implement"),
    );
    call(&e, "codex", "claim", claim(&e, "codex", "owner"));
    let mut changed = stop(&e, "codex", "owner", "implement");
    changed["turn_id"] = json!("t2");
    assert_eq!(call(&e, "codex", "stop", changed), Output::default());
    call(
        &e,
        "codex",
        "session",
        json!({"hook_event_name":"SessionStart","session_id":"owner","cwd":e.root.path(),"source":"resume"}),
    );
    assert_eq!(
        call(&e, "codex", "stop", stop(&e, "codex", "owner", "implement")),
        Output::default()
    );
}

#[test]
fn native_stop_protocols_reuse_rules_and_respect_pause_unrelated_and_retry_bound() {
    for provider in ["codex", "kimi", "gemini"] {
        let e = env();
        call(&e, provider, "claim", claim(&e, provider, "owner"));
        call(
            &e,
            provider,
            "session",
            prompt(&e, provider, "owner", "implement"),
        );
        call(&e, provider, "claim", claim(&e, provider, "owner"));
        let mut blocked = 0;
        for _ in 0..6 {
            let out = call(
                &e,
                provider,
                "stop",
                stop(&e, provider, "owner", "implement"),
            );
            if out != Output::default() {
                blocked += 1;
                match provider {
                    "kimi" => {
                        assert_eq!(out.exit, 2);
                        assert!(out.stderr.unwrap().contains("[lead]"));
                    }
                    "gemini" => assert_eq!(
                        serde_json::from_str::<Value>(&out.stdout.unwrap()).unwrap()["decision"],
                        "deny"
                    ),
                    _ => assert_eq!(
                        serde_json::from_str::<Value>(&out.stdout.unwrap()).unwrap()["decision"],
                        "block"
                    ),
                }
            }
        }
        assert_eq!(blocked, 5, "{provider}");
        call(
            &e,
            provider,
            "session",
            prompt(&e, provider, "owner", "what is README?"),
        );
        assert_eq!(
            call(
                &e,
                provider,
                "stop",
                stop(&e, provider, "owner", "what is README?")
            ),
            Output::default()
        );
        call(
            &e,
            provider,
            "session",
            prompt(&e, provider, "owner", "pause"),
        );
        call(&e, provider, "claim", claim(&e, provider, "owner"));
        assert_eq!(
            call(&e, provider, "stop", stop(&e, provider, "owner", "pause")),
            Output::default()
        );
    }
}

#[test]
fn context_uses_native_output_and_does_not_inject_kimi_observation_events() {
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        call(&e, provider, "claim", claim(&e, provider, "owner"));
        let out = call(
            &e,
            provider,
            "session",
            prompt(&e, provider, "owner", "implement"),
        );
        assert!(
            out.stdout
                .as_ref()
                .is_some_and(|text| text.contains("SPEC.md")),
            "{provider}"
        );
        if provider == "kimi" {
            for event in ["SessionStart", "PostCompact"] {
                assert_eq!(
                    call(
                        &e,
                        provider,
                        "session",
                        json!({"hook_event_name":event,"session_id":"owner","cwd":e.root.path()})
                    ),
                    Output::default()
                );
            }
        }
        assert_eq!(
            call(
                &e,
                provider,
                "stop",
                stop(&e, provider, "different-owner", "implement")
            ),
            Output::default()
        );
    }
}

#[test]
fn gemini_before_model_preserves_request_and_replaces_only_owned_context() {
    let e = env();
    call(&e, "gemini", "claim", claim(&e, "gemini", "owner"));
    let raw = json!({"hook_event_name":"BeforeModel","session_id":"owner","cwd":e.root.path(),"llm_request":{"model":"native-model","messages":[{"role":"user","content":"hello"},{"role":"system","content":"[meowo-lead-context]\nstale"}],"config":{"temperature":0.1}}});
    let out = call(&e, "gemini", "session", raw);
    let value: Value = serde_json::from_str(&out.stdout.unwrap()).unwrap();
    let request = &value["hookSpecificOutput"]["llm_request"];
    assert_eq!(request["model"], "native-model");
    assert_eq!(request["config"]["temperature"], 0.1);
    assert_eq!(request["messages"].as_array().unwrap().len(), 2);
    assert!(request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("SPEC.md"));
    // Gemini API 不收 system 角色（真模型 400）：注入的上下文一律用 user。
    assert_eq!(request["messages"][1]["role"], "user");
    assert_eq!(request["messages"][0]["content"], "hello");
}

#[test]
fn compression_does_not_reset_turn_facts_and_opencode_idle_is_not_a_stop_control() {
    let e = env();
    call(&e, "codex", "claim", claim(&e, "codex", "owner"));
    call(
        &e,
        "codex",
        "session",
        prompt(&e, "codex", "owner", "implement"),
    );
    call(&e, "codex", "claim", claim(&e, "codex", "owner"));
    call(
        &e,
        "codex",
        "session",
        json!({"hook_event_name":"SessionStart","session_id":"owner","cwd":e.root.path(),"source":"compact"}),
    );
    assert!(
        call(&e, "codex", "stop", stop(&e, "codex", "owner", "implement"))
            .stdout
            .is_some()
    );
    let e = env();
    call(&e, "opencode", "claim", claim(&e, "opencode", "owner"));
    assert_eq!(
        call(
            &e,
            "opencode",
            "stop",
            json!({"hook_event_name":"session.idle","event":{"type":"session.idle","properties":{"sessionID":"owner"}},"cwd":e.root.path()})
        ),
        Output::default()
    );
}

/// 验收 #3「同项目两个会话与多个任务互不串线」：同一项目下两个任务各由一个会话认领，
/// 认领、SessionStart 注入、Stop 拦截都只作用于各自的任务；作答一个任务的关卡，另一个会话不被唤醒。
/// 兄弟场景逐家覆盖四个 Provider。
#[test]
fn two_sessions_on_two_tasks_in_one_project_never_cross() {
    let mut injected = 0;
    for provider in ["codex", "kimi", "gemini", "opencode"] {
        let e = env();
        let other = e.root.path().join(".lead/20261112-other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("SPEC.md"), "# other").unwrap();
        fs::write(other.join("progress.md"), "phase: implement\n").unwrap();
        fs::write(other.join("acceptance.json"), r#"[{"id":1,"check":"other","passes":false}]"#).unwrap();
        // A 写本任务，B 写另一个任务（同一 fixture，换掉目标路径）。
        call(&e, provider, "claim", claim(&e, provider, "session-a"));
        let first = e.task.join("progress.md").display().to_string();
        let second = other.join("progress.md").display().to_string();
        let raw = claim(&e, provider, "session-b").to_string().replace(
            &serde_json::to_string(&first).unwrap()[1..serde_json::to_string(&first).unwrap().len() - 1],
            &serde_json::to_string(&second).unwrap()[1..serde_json::to_string(&second).unwrap().len() - 1],
        );
        call(&e, provider, "claim", serde_json::from_str(&raw).unwrap());
        assert_eq!(fs::read_to_string(e.task.join(".owner")).unwrap(), "session-a", "{provider}");
        assert_eq!(fs::read_to_string(other.join(".owner")).unwrap(), "session-b", "{provider}");
        let registry: Value = serde_json::from_slice(&fs::read(&e.registry).unwrap()).unwrap();
        let task_of = |sid: &str| registry[sid].as_str().map(|s| s.replace('\\', "/"));
        assert!(task_of("session-a").unwrap().ends_with("20261112-provider"), "{provider}");
        assert!(task_of("session-b").unwrap().ends_with("20261112-other"), "{provider}");
        // 两个会话的上下文注入只提各自的任务。
        for (sid, mine, theirs) in [("session-a", "20261112-provider", "20261112-other"), ("session-b", "20261112-other", "20261112-provider")] {
            let out = call(&e, provider, "session", prompt(&e, provider, sid, "implement"));
            let text = format!("{}{}", out.stdout.unwrap_or_default(), out.stderr.unwrap_or_default());
            if !text.is_empty() {
                injected += 1;
                assert!(text.contains(mine), "{provider} {sid} 注入缺自己的任务: {text}");
                assert!(!text.contains(theirs), "{provider} {sid} 注入串到别的任务: {text}");
            }
        }
        // 另一个任务挂关卡并作答：只认领本任务的会话 A 不因此被唤醒或换任务。
        fs::write(other.join("gate.json"), r#"{"v":1,"id":"Q-other","type":"Q","title":"t","summary":"s","options":[{"id":"1","label":"a"}],"status":"pending","answer":null}"#).unwrap();
        lead::answer_gate(&other, "Q-other", &meowo_protocol::ipc::LeadGateAnswerDto { choice: "1".into(), note: String::new() }).unwrap();
        let out = call(&e, provider, "session", prompt(&e, provider, "session-a", "implement"));
        let text = format!("{}{}", out.stdout.unwrap_or_default(), out.stderr.unwrap_or_default());
        assert!(!text.contains("Q-other") && !text.contains("20261112-other"), "{provider} A 收到了别的任务的关卡: {text}");
        assert_eq!(fs::read_to_string(e.task.join(".owner")).unwrap(), "session-a", "{provider}");
    }
    // 防止注入检查空过：至少要有真实注入被核对过。
    assert!(injected > 0, "没有任何 Provider 产生任务注入，串线检查未生效");
    eprintln!("核对了 {injected} 次任务注入");
}

// ---------------- 原生 TUI 等待器（任务 20261113-lead-tui） ----------------

fn open_gate(e: &Env, provider: &str, session: &str) {
    call(e, provider, "session", prompt(e, provider, session, "开始"));
    call(e, provider, "claim", claim(e, provider, session));
    fs::write(
        e.task.join("gate.json"),
        r#"{"id":"Q-wait","type":"Q","title":"t","status":"pending","options":[{"id":"go","label":"Go"}]}"#,
    )
    .unwrap();
}

#[test]
fn kimi_stop_asks_for_the_waiter_once_gate_is_open_and_hands_over_an_early_answer() {
    let e = env();
    open_gate(&e, "kimi", "session_k1");
    let out = call(&e, "kimi", "stop", stop(&e, "kimi", "session_k1", "开始"));
    assert_eq!(out.exit, 2);
    let reason = out.stderr.unwrap();
    assert!(reason.contains("run_in_background=true") && reason.contains("disable_timeout=true"), "{reason}");
    assert!(reason.contains("lead wait --provider kimi --session session_k1`"), "{reason}");
    // 别的会话停下不受影响。
    assert_eq!(call(&e, "kimi", "stop", stop(&e, "kimi", "session_other", "开始")), Output::default());
    // 答复在本回合结束前就到了：Stop 直接把答复交给模型，只送一次。
    lead::answer_gate(&e.task, "Q-wait", &meowo_protocol::ipc::LeadGateAnswerDto { choice: "go".into(), note: String::new() }).unwrap();
    let out = call(&e, "kimi", "stop", stop(&e, "kimi", "session_k1", "开始"));
    assert!(out.stderr.unwrap().contains("leader 已在 Meowo 里回答关卡 Q"));
    let again = call(&e, "kimi", "stop", stop(&e, "kimi", "session_k1", "开始"));
    assert!(again.stderr.is_none_or(|text| !text.contains("leader 已在 Meowo 里回答")));
}

#[test]
fn gemini_gate_write_starts_waiter_by_tail_call_and_explains_it_to_the_model() {
    let e = env();
    call(&e, "gemini", "session", prompt(&e, "gemini", "g-1", "开始"));
    call(&e, "gemini", "claim", claim(&e, "gemini", "g-1"));
    fs::write(
        e.task.join("gate.json"),
        r#"{"id":"Q-wait","type":"Q","title":"t","status":"pending","options":[{"id":"go","label":"Go"}]}"#,
    )
    .unwrap();
    let write = json!({"hook_event_name":"AfterTool","session_id":"g-1","cwd":e.root.path(),"tool_name":"write_file","tool_input":{"file_path":e.task.join("gate.json"),"content":"{}"},"tool_response":{"llmContent":"File written."}});
    let out: Value = serde_json::from_str(&call(&e, "gemini", "claim", write).stdout.unwrap()).unwrap();
    let tail = &out["hookSpecificOutput"]["tailToolCallRequest"];
    assert_eq!(tail["name"], "run_shell_command");
    assert_eq!(tail["args"]["is_background"], true);
    let command = tail["args"]["command"].as_str().unwrap().to_owned();
    assert!(command.ends_with(" lead wait --provider gemini --session g-1"), "{command}");
    // 尾调用自己的 AfterTool：补一句说明，叫模型结束本轮。
    let started = json!({"hook_event_name":"AfterTool","session_id":"g-1","cwd":e.root.path(),"tool_name":"run_shell_command","tool_input":{"command":command,"is_background":true},"tool_response":{"llmContent":"Command moved to background (PID: 1)."}});
    let out: Value = serde_json::from_str(&call(&e, "gemini", "claim", started).stdout.unwrap()).unwrap();
    assert!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("直接结束本轮"));
    // 写别的任务文件不触发尾调用。
    assert_eq!(call(&e, "gemini", "claim", claim(&e, "gemini", "g-1")), Output::default());
    // AfterAgent 兜底只问一次，避免无限续跑。
    let first = call(&e, "gemini", "stop", stop(&e, "gemini", "g-1", "开始"));
    assert!(first.stdout.unwrap().contains("is_background=true"));
    let second = call(&e, "gemini", "stop", stop(&e, "gemini", "g-1", "开始"));
    assert!(second.stdout.is_none_or(|text| !text.contains("is_background=true")));
}

/// kimi 2.6.0 的 Stop hook 不触发：写挂起关卡前在 PreToolUse 拦一次，给出原样命令；
/// 已有等待器、消费关卡、别人的任务、无关文件都放行。
#[test]
fn kimi_pre_tool_use_requires_a_waiter_before_writing_a_pending_gate() {
    let e = env();
    call(&e, "kimi", "session", prompt(&e, "kimi", "session_k2", "开始"));
    call(&e, "kimi", "claim", claim(&e, "kimi", "session_k2"));
    let gate = e.task.join("gate.json");
    let pre = |content: &str| json!({"hook_event_name":"PreToolUse","session_id":"session_k2","cwd":e.root.path(),"client_type":"kimi_code_cli","tool_name":"Write","tool_input":{"path":gate,"content":content}});
    let pending = r#"{"id":"Q-9","type":"Q","title":"t","status":"pending"}"#;
    let out = call(&e, "kimi", "gate", pre(pending));
    assert_eq!(out.exit, 2);
    let reason = out.stderr.unwrap();
    assert!(reason.contains("lead wait --provider kimi --session session_k2`") && reason.contains("disable_timeout=true"), "{reason}");
    // 消费关卡不拦。
    assert_eq!(call(&e, "kimi", "gate", pre(&pending.replace("pending", "consumed"))), Output::default());
    // 别的会话写这个任务的关卡不拦（任务已归 session_k2）。
    let mut other = pre(pending);
    other["session_id"] = json!("session_other");
    assert_eq!(call(&e, "kimi", "gate", other), Output::default());
    // 等待器已在等（先启动、等下一个关卡）：放行。
    let record = crate::lead_waiter::WaiterRecord {
        pid: std::process::id(),
        started: crate::lead_waiter::process_started(std::process::id()).unwrap(),
        gate: String::new(),
        session: "session_k2".into(),
        provider: "kimi".into(),
        cli_pid: None,
        cli_started: None,
    };
    fs::write(crate::lead_waiter::waiter_path(&e.task), serde_json::to_string(&record).unwrap()).unwrap();
    assert_eq!(call(&e, "kimi", "gate", pre(pending)), Output::default());
    // 不碰 gate.json 的调用走快速路径。
    assert_eq!(call(&e, "kimi", "gate", json!({"hook_event_name":"PreToolUse","session_id":"session_k2","cwd":e.root.path(),"client_type":"kimi_code_cli","tool_name":"Bash","tool_input":{"command":"ls"}})), Output::default());
}

/// 第二轮真机 run5 D：kimi 2.6 的子代理写挂起关卡时，hook 载荷与主 agent 一模一样（没有 agent 身份）。
/// 认出是子代理（主 wire：前台子代理在跑）就直接拒绝，原因里不给等待命令，也不认领任务。
#[test]
fn kimi_subagent_writing_a_pending_gate_is_denied_without_a_waiter_command() {
    let e = env();
    let gate = e.task.join("gate.json");
    let raw = json!({"hook_event_name":"PreToolUse","session_id":"session_k3","cwd":e.root.path(),"client_type":"kimi_code_cli","tool_name":"Write","tool_input":{"path":gate,"content":r#"{"id":"Q-1","type":"Q","title":"t","status":"pending"}"#}});
    let native = normalize(Provider::Kimi, &raw).unwrap();
    let out = kimi_gate_guard_with(&native, &raw, "session_k3", &e.registry, |_| true).unwrap();
    assert_eq!(out.exit, 2);
    let reason = out.stderr.unwrap();
    assert!(reason.contains("关卡只由主会话发"), "{reason}");
    assert!(!reason.contains("lead wait"), "不能把等待命令给子代理：{reason}");
    assert!(!e.task.join(".owner").exists(), "子代理不能借写关卡认领任务");
    // 主 agent 写：照旧要求先起等待器。
    let out = kimi_gate_guard_with(&native, &raw, "session_k3", &e.registry, |_| false).unwrap();
    assert!(out.stderr.unwrap().contains("lead wait --provider kimi --session session_k3"));
}

/// gemini 0.68 子代理与主 agent 共用 hook 载荷：按主 agent 的 invoke_agent 起止认。期间写挂起关卡被
/// BeforeTool 拒绝、AfterTool 也不起等待器；invoke_agent 结束（或主回合起止）后主 agent 照常。
#[test]
fn gemini_subagent_cannot_write_a_pending_gate_while_invoke_agent_runs() {
    let e = env();
    call(&e, "gemini", "session", prompt(&e, "gemini", "g-2", "开始"));
    call(&e, "gemini", "claim", claim(&e, "gemini", "g-2"));
    let gate = e.task.join("gate.json");
    let agent = |event: &str| json!({"hook_event_name":event,"session_id":"g-2","cwd":e.root.path(),"tool_name":"invoke_agent","tool_input":{"agent_name":"generalist","prompt":"写关卡"},"tool_response":{"llmContent":"done"}});
    let write = json!({"hook_event_name":"BeforeTool","session_id":"g-2","cwd":e.root.path(),"tool_name":"write_file","tool_input":{"file_path":gate,"content":r#"{"id":"Q-1","type":"Q","title":"t","status":"pending"}"#}});
    // 没有子代理：BeforeTool 放行。
    assert_eq!(call(&e, "gemini", "gate", write.clone()), Output::default());
    // 主 agent 派出子代理：期间写挂起关卡被拒，原因交给模型。
    assert_eq!(call(&e, "gemini", "gate", agent("BeforeTool")), Output::default());
    let out: Value = serde_json::from_str(&call(&e, "gemini", "gate", write.clone()).stdout.unwrap()).unwrap();
    assert_eq!(out["decision"], "deny");
    assert!(out["reason"].as_str().unwrap().contains("关卡只由主会话发"));
    // 消费关卡、写别的文件不拦。
    let mut consume = write.clone();
    consume["tool_input"]["content"] = json!(r#"{"id":"Q-1","status":"consumed"}"#);
    assert_eq!(call(&e, "gemini", "gate", consume), Output::default());
    // 子代理以别的写法写进去了：AfterTool 不给它起等待器。
    fs::write(&gate, r#"{"id":"Q-1","type":"Q","title":"t","status":"pending","options":[{"id":"go","label":"Go"}]}"#).unwrap();
    let after = json!({"hook_event_name":"AfterTool","session_id":"g-2","cwd":e.root.path(),"tool_name":"write_file","tool_input":{"file_path":gate,"content":"{}"},"tool_response":{"llmContent":"File written."}});
    let out = call(&e, "gemini", "claim", after.clone());
    assert!(out.stdout.is_none_or(|text| !text.contains("tailToolCallRequest")));
    // invoke_agent 结束：主 agent 写关卡照常（BeforeTool 放行、AfterTool 起等待器）。
    assert_eq!(call(&e, "gemini", "claim", agent("AfterTool")), Output::default());
    assert_eq!(call(&e, "gemini", "gate", write.clone()), Output::default());
    assert!(call(&e, "gemini", "claim", after).stdout.unwrap().contains("tailToolCallRequest"));
    // invoke_agent 被取消、没有 AfterTool：主回合起止清掉残留标记，不误拦主 agent。
    call(&e, "gemini", "gate", agent("BeforeTool"));
    assert_ne!(call(&e, "gemini", "gate", write.clone()), Output::default());
    call(&e, "gemini", "stop", stop(&e, "gemini", "g-2", "开始"));
    assert_eq!(call(&e, "gemini", "gate", write), Output::default());
}
