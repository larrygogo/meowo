//! 桌面与手机共用同一套扁平命令；所有数据库、目录和会话控制工作都放在 blocking 池。
use meowo_agent::conversation::ConversationEvent as Event;
use meowo_protocol::conversation::*;
use serde_json::Value;
use std::path::Path;
use tauri::State;

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "conversation/worker_join".to_owned())?
}

pub(crate) fn session_cwd(db: &Path, id: i64) -> Result<String, String> {
    if id <= 0 {
        return Err("conversation/session_missing".into());
    }
    let store = crate::open_store(db)?;
    if let Some(state) = store.managed_state(id).map_err(|e| e.to_string())? {
        return Ok(state.cwd);
    }
    let session = store
        .get_session(id)
        .map_err(|_| "conversation/session_missing")?;
    if let Some(cwd) = store
        .session_cwd(id)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
    {
        return Ok(cwd);
    }
    store
        .list_projects()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.id == session.project_id)
        .map(|p| p.root_path)
        .ok_or_else(|| "conversation/cwd_invalid".into())
}

fn validate_model(catalog: Option<&Event>, model: &str) -> Result<(), String> {
    let Some(Event::Catalog { models, .. }) = catalog else {
        return Err("conversation/catalog_unavailable".into());
    };
    if models
        .iter()
        .any(|item| item.id == model && !model.is_empty())
    {
        Ok(())
    } else {
        Err("conversation/model_unknown".into())
    }
}

fn validate_mode(catalog: Option<&Event>, dimension: &str, mode: &str) -> Result<(), String> {
    let Some(Event::Catalog {
        modes,
        mode_dimension,
        ..
    }) = catalog
    else {
        return Err("conversation/catalog_unavailable".into());
    };
    if !dimension.is_empty()
        && mode_dimension.as_deref() == Some(dimension)
        && modes.iter().any(|item| item.id == mode && !mode.is_empty())
    {
        Ok(())
    } else {
        Err("conversation/mode_unknown".into())
    }
}

fn validate_resolution(event: &Event, option: Option<&str>, answers: &Value) -> Result<(), String> {
    let Event::Interaction {
        choices, questions, ..
    } = event
    else {
        return Err("conversation/interaction_stale".into());
    };
    if let Some(option) = option {
        let choice = choices
            .iter()
            .find(|choice| choice.id == option)
            .ok_or("conversation/option_invalid")?;
        let form = !questions.is_empty()
            && choices.iter().all(|choice| {
                ["accept", "decline", "reject", "cancel"].contains(&choice.kind.as_str())
            });
        if form {
            if matches!(choice.kind.as_str(), "decline" | "reject" | "cancel") {
                return Ok(());
            }
            if choice.kind != "accept" {
                return Err("conversation/option_unsupported".into());
            }
        } else {
            if !questions.is_empty()
                || !["allow_once", "allow_always", "reject_once", "reject_always"]
                    .contains(&choice.kind.as_str())
            {
                return Err("conversation/option_unsupported".into());
            }
            return Ok(());
        }
    } else if questions.is_empty() || !choices.is_empty() {
        return Err("conversation/option_missing".into());
    }
    let map = answers.as_object().ok_or("conversation/answers_invalid")?;
    if map.len() != questions.len() {
        return Err("conversation/answers_invalid".into());
    }
    for question in questions {
        let answer = map
            .get(&question.id)
            .ok_or("conversation/answers_invalid")?;
        let values = if question.multiple {
            answer
                .as_array()
                .ok_or("conversation/answers_invalid")?
                .iter()
                .map(|value| value.as_str().ok_or("conversation/answers_invalid"))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![answer.as_str().ok_or("conversation/answers_invalid")?]
        };
        if values.is_empty()
            || values.iter().any(|value| {
                value.is_empty()
                    || !question.options.is_empty()
                        && !question.options.iter().any(|option| option.id == *value)
            })
            || values
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != values.len()
        {
            return Err("conversation/answers_invalid".into());
        }
    }
    Ok(())
}

fn validate_gate(db: &Path, id: i64, gate: &ManagedGateRefDto) -> Result<(), String> {
    let cwd = session_cwd(db, id)?;
    let expected = meowo_reporter::lead::task_of_gate(&cwd, &gate.gate_id)
        .ok_or("conversation/gate_missing")?;
    let actual = dunce::canonicalize(&gate.task_dir).map_err(|_| "conversation/gate_missing")?;
    let expected = dunce::canonicalize(expected).map_err(|_| "conversation/gate_missing")?;
    if actual != expected {
        return Err("conversation/gate_owner_mismatch".into());
    }
    let owner = std::fs::read_to_string(expected.join(".owner"))
        .map_err(|_| "conversation/gate_owner_mismatch")?;
    let header = crate::open_store(db)?
        .session_header(id)
        .map_err(|e| e.to_string())?;
    if owner.trim() != header.cc_session_id {
        return Err("conversation/gate_owner_mismatch".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn managed_conversation_create(
    app: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    provider: String,
    cwd: String,
    profile_id: Option<String>,
) -> Result<ManagedConversationDto, String> {
    let db = state.db_path.clone();
    blocking(move || super::create(&db, &provider, &cwd, profile_id.as_deref(), Some(app))).await
}

#[tauri::command]
pub(crate) async fn managed_conversation_adopt(
    app: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    session_id: i64,
) -> Result<ManagedConversationDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::adopt(&db, session_id, Some(app))
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_state(
    state: State<'_, crate::AppState>,
    session_id: i64,
) -> Result<Option<ManagedConversationDto>, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::state(&db, session_id)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn managed_conversation_send(
    app: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    session_id: i64,
    delivery_id: String,
    text: String,
    attachments: Vec<ManagedAttachmentDto>,
    gate_ref: Option<ManagedGateRefDto>,
) -> Result<ManagedDeliveryDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        if let Some(gate) = &gate_ref {
            validate_gate(&db, session_id, gate)?;
        }
        super::send(
            &db,
            session_id,
            &delivery_id,
            &text,
            &attachments,
            gate_ref.as_ref(),
            Some(app),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_deliveries(
    state: State<'_, crate::AppState>,
    session_id: i64,
) -> Result<Vec<ManagedDeliveryDto>, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::deliveries(&db, session_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_events(
    state: State<'_, crate::AppState>,
    session_id: i64,
    after: i64,
    limit: usize,
) -> Result<ManagedEventPageDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::events(&db, session_id, after, limit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_control(
    app: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    session_id: i64,
    action: String,
) -> Result<ManagedConversationDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::control(&db, session_id, &action, Some(app))
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_resolve(
    state: State<'_, crate::AppState>,
    session_id: i64,
    interaction_id: String,
    option_id: Option<String>,
    answers: Value,
) -> Result<(), String> {
    let db = state.db_path.clone();
    blocking(move||{
        session_cwd(&db,session_id)?;
        let pending=crate::open_store(&db)?.managed_pending_interactions(session_id).map_err(|e|e.to_string())?;
        let event=pending.into_iter().filter_map(|row|serde_json::from_str::<Event>(&row.payload_json).ok()).find(|event|matches!(event,Event::Interaction{interaction_id:pending,..} if pending==&interaction_id)).ok_or("conversation/interaction_stale")?;
        validate_resolution(&event,option_id.as_deref(),&answers)?;
        super::resolve(&db,session_id,&interaction_id,option_id.as_deref(),answers)
    }).await
}

#[tauri::command]
pub(crate) async fn managed_conversation_set_model(
    state: State<'_, crate::AppState>,
    session_id: i64,
    model: String,
) -> Result<(), String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        validate_model(super::catalog(&db, session_id)?.as_ref(), &model)?;
        super::set_model(&db, session_id, &model)
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_set_mode(
    state: State<'_, crate::AppState>,
    session_id: i64,
    dimension: String,
    mode: String,
) -> Result<(), String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        validate_mode(super::catalog(&db, session_id)?.as_ref(), &dimension, &mode)?;
        super::set_mode(&db, session_id, &dimension, &mode)
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_delivery_action(
    state: State<'_, crate::AppState>,
    session_id: i64,
    delivery_id: String,
    action: String,
    replacement_delivery_id: Option<String>,
) -> Result<ManagedDeliveryDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::delivery_action(
            &db,
            session_id,
            &delivery_id,
            &action,
            replacement_delivery_id.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn managed_conversation_recoveries(
    state: State<'_, crate::AppState>,
    session_id: i64,
    cursor: Option<ManagedNativeRecoveryCursorDto>,
    limit: Option<usize>,
) -> Result<ManagedNativeRecoveryPageDto, String> {
    let db = state.db_path.clone();
    blocking(move || {
        session_cwd(&db, session_id)?;
        super::recoveries(&db, session_id, cursor.as_ref(), limit.unwrap_or(50))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use meowo_agent::conversation::{ConversationChoice, ConversationQuestion};
    use serde_json::json;
    fn choice(id: &str, kind: &str) -> ConversationChoice {
        ConversationChoice {
            id: id.into(),
            label: id.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn absent_or_unregistered_catalog_values_are_not_default_modes() {
        let catalog = Event::Catalog {
            models: vec![choice("known", "model")],
            modes: vec![choice("safe", "permission")],
            mode_dimension: Some("permission".into()),
        };
        assert!(validate_model(Some(&catalog), "known").is_ok());
        assert!(validate_model(Some(&catalog), "unknown").is_err());
        assert!(validate_model(None, "known").is_err());
        assert!(validate_mode(Some(&catalog), "permission", "safe").is_ok());
        assert!(validate_mode(Some(&catalog), "unknown", "safe").is_err());
        assert!(validate_mode(Some(&catalog), "permission", "unregistered").is_err());
    }

    #[test]
    fn unknown_permission_option_or_kind_cannot_become_allow() {
        let mut event = Event::Interaction {
            interaction_id: "request-a".into(),
            title: String::new(),
            detail: String::new(),
            choices: vec![choice("reject", "reject_once")],
            questions: vec![],
        };
        assert!(validate_resolution(&event, Some("reject"), &Value::Null).is_ok());
        assert!(
            validate_resolution(&event, Some("allow-from-other-request"), &Value::Null).is_err()
        );
        assert!(validate_resolution(&event, None, &Value::Null).is_err());
        if let Event::Interaction { choices, .. } = &mut event {
            choices[0].kind = "unknown-default-allow".into();
        }
        assert!(validate_resolution(&event, Some("reject"), &Value::Null).is_err());
    }

    #[test]
    fn question_answers_require_the_actual_question_ids_and_cardinality() {
        let event = Event::Interaction {
            interaction_id: "q".into(),
            title: String::new(),
            detail: String::new(),
            choices: vec![],
            questions: vec![ConversationQuestion {
                id: "actual".into(),
                title: String::new(),
                text: String::new(),
                multiple: false,
                options: vec![],
            }],
        };
        assert!(validate_resolution(&event, None, &json!({"actual":"answer"})).is_ok());
        assert!(validate_resolution(&event, None, &json!({"other":["answer"]})).is_err());
        assert!(validate_resolution(&event, None, &json!({"actual":["one","two"]})).is_err());
    }

    fn actual_kimi_form() -> (Box<dyn meowo_agent::conversation::ConversationCodec>, Event) {
        use meowo_agent::conversation::{
            ConversationAction as A, ConversationInit, ConversationInput,
        };
        fn sent(actions: Vec<A>) -> Value {
            actions
                .into_iter()
                .find_map(|action| {
                    if let A::Send(value) = action {
                        Some(value)
                    } else {
                        None
                    }
                })
                .unwrap()
        }
        let plugin = meowo_agent::by_id("kimi").unwrap();
        let mut codec = plugin.conversation().unwrap().codec(ConversationInit {
            cwd: "C:/isolated".into(),
            ..Default::default()
        });
        let init = sent(codec.initialize().unwrap());
        let new=sent(codec.receive(json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentInfo":{"name":"Kimi Code CLI","version":"2.5.3"},"agentCapabilities":{"promptCapabilities":{"image":false}}}})).unwrap());
        codec
            .receive(json!({"jsonrpc":"2.0","id":new["id"],"result":{"sessionId":"root"}}))
            .unwrap();
        codec
            .input(ConversationInput::Prompt {
                request_id: "form-delivery".into(),
                text: "question".into(),
                attachments: vec![],
            })
            .unwrap();
        // 本机Kimi原生form样本与crate codec回归同构；串起真实codec而非手造通用Interaction。
        let actions=codec.receive(json!({"jsonrpc":"2.0","id":"native-form","method":"elicitation/create","params":{"sessionId":"root","mode":"form","requestedSchema":{"type":"object","required":["q0","q1"],"properties":{"q0":{"type":"string","oneOf":[{"const":"dark","title":"Dark"}]},"q1":{"type":"array","minItems":1,"items":{"anyOf":[{"const":"a","title":"A"},{"const":"b","title":"B"}]}}}}}})).unwrap();
        let interaction = actions
            .into_iter()
            .find_map(|action| {
                if let A::Emit(event @ Event::Interaction { .. }) = action {
                    Some(event)
                } else {
                    None
                }
            })
            .unwrap();
        (codec, interaction)
    }

    #[test]
    fn actual_acp_form_flows_through_wrapper_without_rewriting_native_answers_or_actions() {
        use meowo_agent::conversation::{ConversationAction, ConversationInput};
        for action in ["accept", "decline", "cancel"] {
            let (mut codec, event) = actual_kimi_form();
            let answers = json!({"q0":"dark","q1":["a","b"]});
            assert!(validate_resolution(&event, Some(action), &answers).is_ok());
            assert!(validate_resolution(&event, Some("allow_once"), &answers).is_err());
            assert!(validate_resolution(
                &event,
                Some("accept"),
                &json!({"q0":["dark"],"q1":["a"]})
            )
            .is_err());
            assert!(
                validate_resolution(&event, Some("accept"), &json!({"q0":"dark","q1":"a"}))
                    .is_err()
            );
            let Event::Interaction { interaction_id, .. } = event else {
                unreachable!()
            };
            let result = codec
                .input(ConversationInput::Resolve {
                    interaction_id,
                    option_id: Some(action.into()),
                    answers: answers.clone(),
                })
                .unwrap();
            let response = result
                .into_iter()
                .find_map(|action| {
                    if let ConversationAction::Send(value) = action {
                        Some(value)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(response["id"], "native-form");
            assert_eq!(response["result"]["action"], action);
            if action == "accept" {
                assert_eq!(response["result"]["content"], answers);
            }
        }
    }
}
