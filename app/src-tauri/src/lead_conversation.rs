//! 手机与桌面的Lead答复共用持久投递；只有任务文件的consumed才算业务接手，原生回执不能代签。

use meowo_protocol::conversation::ManagedGateRefDto;
use meowo_protocol::ipc::{LeadDeliveryDto, LeadGateAnswerDto};
use std::io::Read;
use std::path::{Path, PathBuf};

fn owner(task: &Path) -> Result<String, String> {
    let mut raw = String::new();
    std::fs::File::open(task.join(".owner"))
        .map_err(|_| "lead/owner_missing")?
        .take(257)
        .read_to_string(&mut raw)
        .map_err(|_| "lead/owner_invalid")?;
    let value = raw.trim_start_matches('\u{feff}').trim();
    if value.is_empty() || raw.len() > 256 || !crate::session_command::is_safe_id(value) {
        return Err("lead/owner_invalid".into());
    }
    Ok(value.to_owned())
}

fn task_for(cwd: &str, gate_id: &str) -> Result<PathBuf, String> {
    let mut matches = meowo_reporter::lead::project_gates(cwd)
        .into_iter()
        .filter(|(_, gate)| gate.id == gate_id);
    let task = matches.next().ok_or("lead/gate_missing")?.0;
    if matches.next().is_some() {
        return Err("lead/gate_ambiguous".into());
    }
    dunce::canonicalize(task).map_err(|_| "lead/gate_missing".into())
}

fn delivery_id(task: &Path, gate_id: &str, native_owner: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(task.to_string_lossy().as_bytes());
    hash.update([0]);
    hash.update(gate_id.as_bytes());
    hash.update([0]);
    hash.update(native_owner.as_bytes());
    format!("lead-{:x}", hash.finalize())
}

/// true表示答复归协议托管会话；false只供旧Claude路径维持原有asyncRewake，不自动接管原生CLI。
pub(crate) fn answer_and_deliver(
    db: &Path,
    cwd: &str,
    gate_id: &str,
    answer: &LeadGateAnswerDto,
    app: Option<tauri::AppHandle>,
) -> Result<bool, String> {
    let task = task_for(cwd, gate_id)?;
    if let Err(reason) = meowo_reporter::lead::answer_gate(&task, gate_id, answer) {
        // 手机超时重试可能晚于模型的业务接手；仅原关卡同一答案可幂等成功，不重启旧终端。
        let consumed_same = meowo_reporter::lead::read_gate(&task).is_some_and(|gate| {
            gate.id == gate_id && gate.status == "consumed" && gate.answer.as_ref() == Some(answer)
        });
        if consumed_same && deliver(db, &task, app, false)? {
            return Ok(true);
        }
        return Err(reason);
    }
    deliver(db, &task, app, false)
}

pub(crate) fn retry(
    db: &Path,
    cwd: &str,
    gate_id: &str,
    app: Option<tauri::AppHandle>,
) -> Result<(), String> {
    let task = task_for(cwd, gate_id)?;
    if !deliver(db, &task, app, true)? {
        return Err("lead/owner_not_managed".into());
    }
    Ok(())
}

fn paused(store: &meowo_store::Store, id: i64) -> Result<bool, String> {
    Ok(store
        .managed_latest_user_delivery(id)
        .map_err(|e| e.to_string())?
        .is_some_and(|item| meowo_reporter::lead::contains_stop_word(&item.text)))
}

fn current_delivery_key(
    store: &meowo_store::Store,
    id: i64,
    initial: String,
) -> Result<String, String> {
    let mut key = initial;
    for _ in 0..32 {
        let Some(next) = store
            .managed_delivery_replacement(id, &key)
            .map_err(|e| e.to_string())?
        else {
            return Ok(key);
        };
        key = next;
    }
    Err("lead/delivery_chain_limit".into())
}

fn deliver(
    db: &Path,
    task: &Path,
    app: Option<tauri::AppHandle>,
    force: bool,
) -> Result<bool, String> {
    let store = crate::open_store(db)?;
    if !task.join(".owner").exists() {
        return Ok(false);
    }
    let native = owner(task)?;
    let Some(id) = store
        .find_session_id_pub(&native)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let Some(state) = store.managed_state(id).map_err(|e| e.to_string())? else {
        // 原生 TUI 会话：按插件声明的机制自动续跑（lead_native）；做不到的返回 false，面板提示手动继续。
        let gate = meowo_reporter::lead::read_gate(task).ok_or("lead/gate_missing")?;
        if gate.status != "answered" {
            return Ok(gate.status == "consumed");
        }
        let provider = store.session_provider(id).map_err(|e| e.to_string())?;
        let cwd = store.session_cwd(id).map_err(|e| e.to_string())?;
        let message = meowo_reporter::lead::answer_message(task, &gate);
        let target = crate::lead_native::NativeTarget {
            db,
            session_id: id,
            provider: &provider,
            native_session: &native,
            cwd: cwd.as_deref(),
            task,
            gate_id: &gate.id,
            message: &message,
        };
        let _ = force;
        return match crate::lead_native::deliver(&target, app.as_ref())? {
            crate::lead_native::NativeOutcome::Unsupported => Ok(false),
            _ => Ok(true),
        };
    };
    if state.native_session_id.as_deref() != Some(native.as_str()) {
        return Err("lead/owner_mismatch".into());
    }
    let gate = meowo_reporter::lead::read_gate(task).ok_or("lead/gate_missing")?;
    if gate.status == "consumed" {
        return Ok(true);
    }
    if gate.status != "answered" {
        return Err("lead/gate_not_answered".into());
    }
    if !force && paused(&store, id)? {
        return Err("lead/session_paused".into());
    }
    let key = current_delivery_key(&store, id, delivery_id(task, &gate.id, &native))?;
    let mut message = meowo_reporter::lead::answer_message(task, &gate);
    if let Some(existing) = store
        .managed_delivery(id, &key)
        .map_err(|e| e.to_string())?
    {
        match existing.state.as_str() {
            "uncertain" => return Err("lead/delivery_uncertain".into()),
            "failed" | "cancelled" => return Err("lead/delivery_failed".into()),
            "completed" => return Ok(true),
            _ => {}
        }
        message = existing.text;
    }
    let gate_ref = ManagedGateRefDto {
        task_dir: task.to_string_lossy().into_owned(),
        gate_id: gate.id.clone(),
    };
    crate::conversation::send(db, id, &key, &message, &[], Some(&gate_ref), app)?;
    Ok(true)
}

pub(crate) fn delivery_status(db: &Path, cwd: &str, gate_id: &str) -> Option<LeadDeliveryDto> {
    let task = task_for(cwd, gate_id).ok()?;
    let store = crate::open_store(db).ok()?;
    let native = owner(&task).ok()?;
    let id = store.find_session_id_pub(&native).ok()??;
    let Some(state) = store.managed_state(id).ok()? else {
        return native_session_status(&store, id, &task, gate_id, cwd);
    };
    let key = current_delivery_key(&store, id, delivery_id(&task, gate_id, &native)).ok()?;
    let delivery = store.managed_delivery(id, &key).ok()?;
    let (status, reason) = match delivery {
        Some(item)
            if matches!(state.phase.as_str(), "failed" | "stopped") && item.state == "queued" =>
        {
            ("failed".to_owned(), state.last_error.clone())
        }
        Some(item) => (item.state, item.last_error),
        None if paused(&store, id).ok()? => {
            ("paused".to_owned(), Some("lead/session_paused".into()))
        }
        None => ("saved".to_owned(), state.last_error.clone()),
    };
    let can_retry = matches!(status.as_str(), "saved" | "paused")
        || (status == "failed"
            && store
                .managed_delivery(id, &key)
                .ok()
                .flatten()
                .is_some_and(|item| item.state == "queued"));
    Some(LeadDeliveryDto {
        session_id: id,
        cwd: state.cwd,
        delivery_id: key,
        can_retry,
        state: status,
        reason,
    })
}

/// 关卡归原生终端会话时的送达状态。答复只保存、不代投（不猜按键、不接管）；
/// claude 的原生 wait hook 会自己唤醒模型，照旧不显示投递条。其余 provider 的原生 hook
/// 不能自行开新回合（codex/kimi/gemini/opencode 实测），此前这里返回 None、面板什么都不说，
/// 手机上答完的人以为模型会自己接着做（做空指出的静默降级）。如实说下一步：在会话里发一句话。
fn native_session_status(
    store: &meowo_store::Store,
    id: i64,
    task: &Path,
    gate_id: &str,
    project_cwd: &str,
) -> Option<LeadDeliveryDto> {
    let provider = store.session_provider(id).ok()?;
    // 按插件能力而非 provider 身份分（SPEC 约束）：声明了 LeadCap 且原生路径不能自动继续才提示
    // 手动下一步。没有 LeadCap 的 provider 由宿主自带的 Lead 流程负责原生唤醒（claude 的 wait hook）。
    let cap = meowo_agent::by_id(&provider)?.lead()?;
    if cap.native_auto_continue() {
        return None;
    }
    let gate = meowo_reporter::lead::read_gate(task)?;
    if gate.id != gate_id || gate.status != "answered" {
        return None;
    }
    Some(LeadDeliveryDto {
        session_id: id,
        cwd: store
            .session_cwd(id)
            .ok()
            .flatten()
            .unwrap_or_else(|| project_cwd.to_owned()),
        delivery_id: String::new(),
        can_retry: false,
        state: "native_manual".into(),
        reason: None,
    })
}

/// 复用既有关卡巡检，不为普通会话另开任务目录轮询。已存在的稳定投递ID不会再次入队。
pub(crate) fn reconcile_task(db: &Path, task: &Path, app: Option<tauri::AppHandle>) {
    let _ = deliver(db, task, app, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        root: PathBuf,
        task: PathBuf,
        db: PathBuf,
        id: i64,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "meowo-lead-delivery-{}",
                crate::remote::generate_token().unwrap()
            ));
            let task = root.join(".lead/probe");
            std::fs::create_dir_all(&task).unwrap();
            let db = root.join("board.db");
            let store = crate::open_store(&db).unwrap();
            // 未注册的测试provider保证覆盖启动失败而绝不拉起真实CLI。
            let id = store
                .managed_create("fixture", &root.to_string_lossy(), None, 1)
                .unwrap();
            store.managed_bind(id, "native-root", 2).unwrap();
            std::fs::write(task.join(".owner"), "native-root").unwrap();
            std::fs::write(task.join("gate.json"), json!({"id":"Q-probe","type":"Q","title":"probe","status":"pending","options":[{"id":"go","label":"Continue"}]}).to_string()).unwrap();
            Self { root, task, db, id }
        }
        fn answer(&self) -> Result<bool, String> {
            answer_and_deliver(
                &self.db,
                &self.root.to_string_lossy(),
                "Q-probe",
                &LeadGateAnswerDto {
                    choice: "go".into(),
                    note: "Continue safely".into(),
                },
                None,
            )
        }
        fn store(&self) -> meowo_store::Store {
            crate::open_store(&self.db).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 验收 #4「已作答的关卡只交给归属会话」：同一项目里还有另一个托管会话（同 cwd、刚建、
    /// 甚至更新），答复只进 .owner 指向的那个会话；改成指向另一个会话后，原会话也不再收到。
    #[test]
    fn answer_reaches_only_the_owner_among_sessions_in_the_same_project() {
        let f = Fixture::new();
        let store = f.store();
        let other = store
            .managed_create("fixture", &f.root.to_string_lossy(), None, 3)
            .unwrap();
        store.managed_bind(other, "native-other", 4).unwrap();
        assert_eq!(f.answer(), Ok(true));
        assert_eq!(store.managed_deliveries(f.id).unwrap().len(), 1);
        assert!(store.managed_deliveries(other).unwrap().is_empty(), "另一个会话收到了别人的关卡答复");
        // 新关卡归另一个会话：答复只进它，原会话的投递不增加。
        std::fs::write(f.task.join(".owner"), "native-other").unwrap();
        std::fs::write(f.task.join("gate.json"), json!({"id":"Q-next","type":"Q","title":"next","status":"pending","options":[{"id":"go","label":"Continue"}]}).to_string()).unwrap();
        assert_eq!(
            answer_and_deliver(&f.db, &f.root.to_string_lossy(), "Q-next", &LeadGateAnswerDto { choice: "go".into(), note: String::new() }, None),
            Ok(true)
        );
        assert_eq!(store.managed_deliveries(other).unwrap().len(), 1);
        assert_eq!(store.managed_deliveries(f.id).unwrap().len(), 1);
    }

    #[test]
    fn saved_answer_survives_start_failure_and_duplicate_phone_submission() {
        let f = Fixture::new();
        assert_eq!(f.answer(), Ok(true));
        assert_eq!(f.answer(), Ok(true));
        let gate = meowo_reporter::lead::read_gate(&f.task).unwrap();
        assert_eq!(gate.status, "answered");
        assert_eq!(gate.answer.unwrap().note, "Continue safely");
        let deliveries = f.store().managed_deliveries(f.id).unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].state, "queued");
        let status = delivery_status(&f.db, &f.root.to_string_lossy(), "Q-probe").unwrap();
        assert_eq!(status.state, "failed");
        assert!(status.can_retry);
    }

    #[test]
    fn paused_answer_is_saved_before_refusal_and_only_explicit_retry_enqueues() {
        let f = Fixture::new();
        f.store()
            .managed_enqueue(f.id, "pause", "暂停", "[]", None, 3)
            .unwrap();
        assert_eq!(f.answer(), Err("lead/session_paused".into()));
        assert_eq!(
            meowo_reporter::lead::read_gate(&f.task).unwrap().status,
            "answered"
        );
        reconcile_task(&f.db, &f.task, None);
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 1);
        assert_eq!(
            retry(&f.db, &f.root.to_string_lossy(), "Q-probe", None),
            Ok(())
        );
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 2);
    }

    #[test]
    fn uncertain_answer_is_preserved_without_resend_or_fake_gate_consumption() {
        let f = Fixture::new();
        f.answer().unwrap();
        let item = f.store().managed_deliveries(f.id).unwrap().remove(0);
        let now = crate::now_ms() + 1;
        assert!(f
            .store()
            .managed_set_delivery_state(
                f.id,
                &item.delivery_id,
                "queued",
                "sending",
                None,
                None,
                now
            )
            .unwrap());
        assert!(f
            .store()
            .managed_set_delivery_state(
                f.id,
                &item.delivery_id,
                "sending",
                "uncertain",
                None,
                None,
                now + 1
            )
            .unwrap());
        assert_eq!(f.answer(), Err("lead/delivery_uncertain".into()));
        assert_eq!(
            retry(&f.db, &f.root.to_string_lossy(), "Q-probe", None),
            Err("lead/delivery_uncertain".into())
        );
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 1);
        assert_eq!(
            meowo_reporter::lead::read_gate(&f.task).unwrap().status,
            "answered"
        );
        assert!(
            !delivery_status(&f.db, &f.root.to_string_lossy(), "Q-probe")
                .unwrap()
                .can_retry
        );
    }

    #[test]
    fn late_duplicate_after_business_consumption_never_enqueues_again() {
        let f = Fixture::new();
        f.answer().unwrap();
        let mut gate: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(f.task.join("gate.json")).unwrap())
                .unwrap();
        gate["status"] = json!("consumed");
        std::fs::write(f.task.join("gate.json"), gate.to_string()).unwrap();
        assert_eq!(f.answer(), Ok(true));
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 1);
        assert_eq!(
            answer_and_deliver(
                &f.db,
                &f.root.to_string_lossy(),
                "Q-probe",
                &LeadGateAnswerDto {
                    choice: "go".into(),
                    note: "Different decision".into()
                },
                None
            ),
            Err("lead/gate_closed".into())
        );
    }

    #[test]
    fn status_follows_an_explicit_human_retry_without_another_delivery() {
        let f = Fixture::new();
        f.answer().unwrap();
        let original = f.store().managed_deliveries(f.id).unwrap().remove(0);
        f.store()
            .managed_delivery_action(
                f.id,
                &original.delivery_id,
                "retry",
                Some("human-retry"),
                crate::now_ms() + 1,
            )
            .unwrap();
        let status = delivery_status(&f.db, &f.root.to_string_lossy(), "Q-probe").unwrap();
        assert_eq!(status.delivery_id, "human-retry");
        assert_eq!(status.state, "failed");
        assert!(status.can_retry);
        assert_eq!(f.answer(), Ok(true));
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 2);
    }

    #[test]
    fn duplicate_gate_ids_fail_before_writing_either_answer() {
        let f = Fixture::new();
        let second = f.root.join(".lead/other");
        std::fs::create_dir_all(&second).unwrap();
        std::fs::copy(f.task.join("gate.json"), second.join("gate.json")).unwrap();
        assert_eq!(f.answer(), Err("lead/gate_ambiguous".into()));
        assert_eq!(
            meowo_reporter::lead::read_gate(&f.task).unwrap().status,
            "pending"
        );
        assert!(f.store().managed_deliveries(f.id).unwrap().is_empty());
    }

    #[test]
    fn external_owner_answer_is_saved_but_never_auto_delivered_to_a_pty_session() {
        let f = Fixture::new();
        let store = f.store();
        let project = store
            .upsert_project_by_root(&f.root.to_string_lossy(), "external", 3)
            .unwrap();
        let (legacy, _) = store.start_session(project, "external-root", 4).unwrap();
        assert!(store.managed_state(legacy).unwrap().is_none());
        std::fs::write(f.task.join(".owner"), "external-root").unwrap();

        // 外部 TUI 没有可证明的协议投递语义；答复可安全落盘，但不能猜按键或偷偷接管它。
        assert_eq!(f.answer(), Ok(false));
        assert_eq!(
            meowo_reporter::lead::read_gate(&f.task).unwrap().status,
            "answered"
        );
        assert!(store.managed_deliveries(f.id).unwrap().is_empty());
        assert!(store.managed_state(legacy).unwrap().is_none());
        // claude 原生会话有自己的 wait hook 唤醒，面板不另挂投递条。
        assert!(delivery_status(&f.db, &f.root.to_string_lossy(), "Q-probe").is_none());
        // 其余 provider 的原生会话不会自行开新回合：如实告诉用户下一步，而不是什么都不显示（做空指出）。
        for provider in ["codex", "kimi", "gemini", "opencode"] {
            store.set_session_provider(legacy, provider).unwrap();
            let status = delivery_status(&f.db, &f.root.to_string_lossy(), "Q-probe").unwrap();
            assert_eq!(status.state, "native_manual", "{provider}");
            assert!(!status.can_retry, "{provider}");
            assert_eq!(status.session_id, legacy);
        }
    }
}
