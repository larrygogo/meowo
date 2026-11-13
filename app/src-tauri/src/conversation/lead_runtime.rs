//! 原生hook输出在各CLI上的控制力不同；托管worker只用协议确认的root事实执行同一套Lead规则。
use meowo_store::{managed::ManagedDelivery, Store};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) struct LeadRuntime {
    enabled: bool,
    cwd: String,
    registry: PathBuf,
    native: Option<String>,
    claimed_native: Option<String>,
    request: Option<String>,
    calls: BTreeMap<String, Value>,
}

impl LeadRuntime {
    pub(super) fn new(enabled: bool, db: &Path, cwd: &str) -> Self {
        Self {
            enabled,
            cwd: cwd.into(),
            registry: db.with_file_name("lead-sessions.json"),
            native: None,
            claimed_native: None,
            request: None,
            calls: BTreeMap::new(),
        }
    }

    pub(super) fn bound(&mut self, native: &str) {
        self.native = Some(native.into());
    }

    pub(super) fn load_binding(&mut self, store: &Store, session: i64) -> Result<(), String> {
        if self.enabled {
            if let Some(fact) = store
                .managed_latest_fact(session, "lead_claimed")
                .map_err(|e| e.to_string())?
            {
                let fact: Value = serde_json::from_str(&fact.payload_json)
                    .map_err(|_| "conversation/persisted_payload_invalid")?;
                self.claimed_native = fact
                    .get("native_session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        }
        Ok(())
    }

    pub(super) fn prompt(&mut self, delivery: &ManagedDelivery) -> String {
        self.request = Some(delivery.delivery_id.clone());
        self.calls.clear();
        // 安装技能不代表此会话在做Lead任务；未认领时完全不访问任务文件。
        if self.enabled
            && (self.native.is_some() && self.claimed_native == self.native
                || delivery.gate_ref_json.is_some())
        {
            if let Some(context) = self.native.as_deref().and_then(|native| {
                meowo_reporter::lead::managed_context(&self.cwd, native, &self.registry)
            }) {
                return format!("{context}\n\n{}", delivery.text);
            }
        }
        delivery.text.clone()
    }

    pub(super) fn wrote(
        &mut self,
        store: &Store,
        session: i64,
        request: &str,
        tool_id: &str,
        call: &Value,
    ) -> Result<(), String> {
        if !self.enabled || self.request.as_deref() != Some(request) {
            return Ok(());
        }
        let raw = call.to_string();
        // 普通工具没有任务文件IO，只有插件已验证的写工具且路径涉及.lead才进入认领。
        if !raw.contains(".lead") {
            return Ok(());
        }
        if raw.len() > 64 * 1024 || self.calls.len() >= 256 {
            return Err("conversation/lead_evidence_limit".into());
        }
        let Some(native) = self.native.as_deref() else {
            return Ok(());
        };
        if self
            .calls
            .get(tool_id)
            .is_some_and(|previous| previous == call)
        {
            return Ok(());
        }
        if meowo_reporter::lead::managed_claim(&self.cwd, native, call, &self.registry) {
            if self.claimed_native.as_deref() != Some(native) {
                store
                    .managed_append_event(
                        session,
                        &serde_json::json!({"kind":"lead_claimed","native_session_id":native})
                            .to_string(),
                        crate::now_ms(),
                    )
                    .map_err(|e| e.to_string())?;
                self.claimed_native = Some(native.into());
            }
            self.calls.insert(tool_id.into(), call.clone());
        }
        Ok(())
    }

    pub(super) fn completed(
        &self,
        store: &Store,
        session: i64,
        delivery: &ManagedDelivery,
        stop_reason: &str,
    ) -> Result<(), String> {
        if !self.enabled
            || matches!(stop_reason, "cancelled" | "awaiting_background")
            || self.calls.is_empty()
            || self.request.as_deref() != Some(&delivery.delivery_id)
        {
            return Ok(());
        }
        let Some(native) = self.native.as_deref() else {
            return Ok(());
        };
        let human = store
            .managed_latest_user_delivery(session)
            .map_err(|e| e.to_string())?;
        let user_text = human.as_ref().map(|item| item.text.as_str()).unwrap_or("");
        let calls = self.calls.values().cloned().collect::<Vec<_>>();
        if let Some(reason) = meowo_reporter::lead::managed_stop(
            &self.cwd,
            native,
            user_text,
            &calls,
            delivery.origin == "lead",
            &self.registry,
        ) {
            use sha2::{Digest, Sha256};
            let key = format!(
                "lead-auto-{:x}",
                Sha256::digest(delivery.delivery_id.as_bytes())
            );
            store
                .managed_enqueue_lead(session, &key, &reason, "[]", None, crate::now_ms())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
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
                "meowo-managed-lead-{}",
                crate::remote::generate_token().unwrap()
            ));
            let task = root.join(".lead/task");
            std::fs::create_dir_all(&task).unwrap();
            std::fs::write(task.join("progress.md"), "phase: implement").unwrap();
            std::fs::write(task.join("SPEC.md"), "Only the owned task context").unwrap();
            std::fs::write(
                task.join("acceptance.json"),
                r#"[{"id":1,"check":"real validation","passes":false}]"#,
            )
            .unwrap();
            let db = root.join("board.db");
            let store = crate::open_store(&db).unwrap();
            let id = store
                .managed_create("fixture", &root.to_string_lossy(), None, 1)
                .unwrap();
            Self { root, task, db, id }
        }
        fn store(&self) -> Store {
            crate::open_store(&self.db).unwrap()
        }
        fn runtime(&self, enabled: bool) -> LeadRuntime {
            let mut lead = LeadRuntime::new(enabled, &self.db, &self.root.to_string_lossy());
            lead.bound("root-session");
            lead
        }
        fn user(&self, text: &str) -> ManagedDelivery {
            self.store()
                .managed_enqueue(self.id, "user-prompt", text, "[]", None, crate::now_ms())
                .unwrap()
        }
        fn call(&self) -> Value {
            json!({"name":"Write","input":{"file_path":self.task.join("progress.md")}})
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn disabled_unrelated_read_and_child_facts_do_not_claim_or_queue() {
        let f = Fixture::new();
        let delivery = f.user("implement");
        let mut disabled = f.runtime(false);
        assert_eq!(disabled.prompt(&delivery), delivery.text);
        disabled
            .wrote(&f.store(), f.id, &delivery.delivery_id, "write", &f.call())
            .unwrap();
        disabled
            .completed(&f.store(), f.id, &delivery, "end_turn")
            .unwrap();
        let mut enabled = f.runtime(true);
        enabled.prompt(&delivery);
        enabled
            .wrote(&f.store(), f.id, "child-prompt", "write", &f.call())
            .unwrap();
        enabled
            .wrote(
                &f.store(),
                f.id,
                &delivery.delivery_id,
                "read",
                &json!({"name":"Read","input":{"file_path":f.task.join("progress.md")}}),
            )
            .unwrap();
        enabled
            .completed(&f.store(), f.id, &delivery, "end_turn")
            .unwrap();
        assert!(!f.task.join(".owner").exists());
        assert_eq!(f.store().managed_deliveries(f.id).unwrap().len(), 1);
    }

    #[test]
    fn successful_root_write_claims_once_and_another_session_cannot_steal() {
        let f = Fixture::new();
        let delivery = f.user("implement");
        let mut lead = f.runtime(true);
        lead.prompt(&delivery);
        lead.wrote(&f.store(), f.id, &delivery.delivery_id, "write", &f.call())
            .unwrap();
        let mut other = f.runtime(true);
        other.bound("other-root");
        other.prompt(&delivery);
        other
            .wrote(&f.store(), f.id, &delivery.delivery_id, "write", &f.call())
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(f.task.join(".owner")).unwrap(),
            "root-session"
        );
        assert!(lead
            .prompt(&delivery)
            .contains("Only the owned task context"));
        assert!(!other
            .prompt(&delivery)
            .contains("Only the owned task context"));
    }

    #[test]
    fn installed_but_unclaimed_prompts_ignore_task_files_until_a_verified_write() {
        let f = Fixture::new();
        let delivery = f.user("ordinary question");
        std::fs::write(f.task.join(".owner"), "root-session").unwrap();
        let mut lead = f.runtime(true);
        lead.load_binding(&f.store(), f.id).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            assert_eq!(lead.prompt(&delivery), delivery.text);
        }
        let elapsed = started.elapsed();
        // 非Lead快路径不能因任务目录数量或磁盘状态退化成轮询热点；预算留足Windows调试构建余量，
        // 同时能抓住误把managed_context接回未认领会话的数量级回归。
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "10,000次未认领prompt超过1秒：{elapsed:?}"
        );
        eprintln!(
            "installed unclaimed prompt: {:.3} us per call",
            elapsed.as_secs_f64() * 100_000.0
        );
        lead.wrote(
            &f.store(),
            f.id,
            &delivery.delivery_id,
            "verified-write",
            &f.call(),
        )
        .unwrap();
        assert!(lead
            .prompt(&delivery)
            .contains("Only the owned task context"));
        let mut restored = f.runtime(true);
        restored.load_binding(&f.store(), f.id).unwrap();
        assert!(restored
            .prompt(&delivery)
            .contains("Only the owned task context"));
    }

    #[test]
    fn continuation_is_persistent_separate_from_human_input_and_bounded_to_five() {
        let f = Fixture::new();
        let mut delivery = f.user("implement");
        let mut lead = f.runtime(true);
        for attempt in 0..6 {
            lead.prompt(&delivery);
            lead.wrote(&f.store(), f.id, &delivery.delivery_id, "write", &f.call())
                .unwrap();
            lead.completed(&f.store(), f.id, &delivery, "end_turn")
                .unwrap();
            let deliveries = f.store().managed_deliveries(f.id).unwrap();
            assert_eq!(deliveries.len(), (attempt + 2).min(6));
            delivery = deliveries[0].clone();
        }
        assert_eq!(
            f.store()
                .managed_latest_user_delivery(f.id)
                .unwrap()
                .unwrap()
                .text,
            "implement"
        );
        assert_eq!(delivery.origin, "lead");
    }

    #[test]
    fn pause_pending_gate_cancellation_and_background_allow_stopping() {
        for boundary in ["pause", "gate", "cancelled", "awaiting_background"] {
            let f = Fixture::new();
            let delivery = f.user(if boundary == "pause" {
                "暂停"
            } else {
                "implement"
            });
            let mut lead = f.runtime(true);
            lead.prompt(&delivery);
            lead.wrote(&f.store(), f.id, &delivery.delivery_id, "write", &f.call())
                .unwrap();
            if boundary == "gate" {
                std::fs::write(f.task.join("gate.json"),json!({"id":"Q-one","type":"Q","title":"Decision","status":"pending","options":[]}).to_string()).unwrap();
            }
            lead.completed(&f.store(), f.id, &delivery, boundary)
                .unwrap();
            assert_eq!(
                f.store().managed_deliveries(f.id).unwrap().len(),
                1,
                "{boundary}"
            );
        }
    }
}
