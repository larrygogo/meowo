use super::*;
use serde_json::json;
use std::fs;
use std::sync::{Arc, Barrier};

fn draft(task: &Path, id: &str) {
    fs::write(task.join("gate.json"), json!({"id":id,"type":"A","title":"spec","summary":"original","options":[],"status":"pending"}).to_string()).unwrap();
}

fn choice(choice: &str) -> LeadGateAnswerDto {
    LeadGateAnswerDto { choice: choice.into(), note: String::new() }
}

#[test]
fn draft_edits_never_replace_publication_or_answer() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let gate = publish(task, None).unwrap();
    draft(task, "two");
    answer(task, "one", gate.version.as_deref().unwrap(), &choice("confirm")).unwrap();
    assert_eq!(read(task).unwrap().unwrap().id, "one");
    assert!(fs::read_to_string(task.join("gate.json")).unwrap().contains("two"));
    assert_eq!(publish(task, gate.version.as_deref()).unwrap_err(), "lead/decision_unconsumed");
    consume(task, "one", gate.version.as_deref().unwrap()).unwrap();
    let next = publish(task, gate.version.as_deref()).unwrap();
    assert_eq!(next.id, "two");
    assert_ne!(next.version, gate.version);
    assert_eq!(load(task).unwrap().unwrap().records[0].gate.answer, Some(choice("confirm")));
}

#[test]
fn reissue_rejects_old_answers_and_never_reuses_ids() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let first = publish(task, None).unwrap();
    draft(task, "two");
    assert_eq!(publish(task, None).unwrap_err(), "lead/gate_changed");
    let second = publish(task, first.version.as_deref()).unwrap();
    assert_eq!(answer(task, "one", first.version.as_deref().unwrap(), &choice("confirm")).unwrap_err(), "lead/gate_changed");
    draft(task, "one");
    assert_eq!(publish(task, second.version.as_deref()).unwrap_err(), "lead/gate_id_reused");
}

#[test]
fn withdrawal_keeps_history_and_disables_answering() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let first = publish(task, None).unwrap();
    withdraw(task, "one", first.version.as_deref().unwrap()).unwrap();
    assert!(read(task).unwrap().is_none());
    assert!(exists(task));
    assert_eq!(answer(task, "one", first.version.as_deref().unwrap(), &choice("confirm")).unwrap_err(), "lead/gate_changed");
    draft(task, "two");
    assert_eq!(publish(task, None).unwrap().version.as_deref(), Some("publication-2"));
}

#[test]
fn concurrent_different_answers_have_one_winner() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    let gate = publish(temp.path(), None).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = ["confirm", "revise"].into_iter().map(|selected| {
        let barrier = barrier.clone();
        let task = temp.path().to_path_buf();
        let version = gate.version.clone().unwrap();
        std::thread::spawn(move || { barrier.wait(); answer(&task, "one", &version, &choice(selected)) })
    }).collect();
    let results: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.as_ref().err().is_some_and(|error| error == "lead/gate_closed")).count(), 1);
}

#[test]
fn identical_answers_are_idempotent_after_consumption() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    let gate = publish(temp.path(), None).unwrap();
    let version = gate.version.as_deref().unwrap();
    answer(temp.path(), "one", version, &choice("confirm")).unwrap();
    consume(temp.path(), "one", version).unwrap();
    let before = fs::read(temp.path().join(STATE_FILE)).unwrap();
    answer(temp.path(), "one", version, &choice("confirm")).unwrap();
    consume(temp.path(), "one", version).unwrap();
    assert_eq!(fs::read(temp.path().join(STATE_FILE)).unwrap(), before);
    assert_eq!(withdraw(temp.path(), "one", version).unwrap_err(), "lead/gate_closed");
}

#[test]
fn malformed_and_oversized_state_never_fall_back_to_drafts() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    fs::write(temp.path().join(STATE_FILE), "{").unwrap();
    assert_eq!(read(temp.path()).unwrap_err(), "lead/publication_invalid");
    assert_eq!(publish(temp.path(), None).unwrap_err(), "lead/publication_invalid");
    fs::write(temp.path().join(STATE_FILE), " ".repeat(MAX_BYTES + 1)).unwrap();
    assert_eq!(read(temp.path()).unwrap_err(), "lead/publication_too_large");
}

#[test]
fn failed_atomic_replace_preserves_draft_and_can_retry() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    let raw = fs::read(temp.path().join("gate.json")).unwrap();
    let mut state = State { generation: 1, current: Some(0), ..State::default() };
    let mut gate = parse_gate(std::str::from_utf8(&raw).unwrap()).unwrap();
    gate.version = Some(version(1));
    state.records.push(Record { generation: 1, gate, withdrawn: false, legacy: false, previous: None });
    fs::create_dir(temp.path().join(STATE_FILE)).unwrap();
    assert_eq!(save(temp.path(), &state).unwrap_err(), "lead/publication_write_failed");
    assert_eq!(fs::read(temp.path().join("gate.json")).unwrap(), raw);
    fs::remove_dir(temp.path().join(STATE_FILE)).unwrap();
    save(temp.path(), &state).unwrap();
    assert_eq!(read(temp.path()).unwrap().unwrap().id, "one");
    assert!(!fs::read_dir(temp.path()).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().contains("tmp.")));
}

#[test]
fn c_publication_requires_exact_review_and_does_not_write_acceptance() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    let items = json!([{"id":"business","check":"approve","by":"leader","passes":false}]).to_string();
    fs::write(task.join("acceptance.json"), &items).unwrap();
    initialize(task).unwrap();
    let checks = review_checks_raw(&items).unwrap();
    let mut gate = json!({"id":"c","type":"C","title":"review","summary":"proof","status":"pending","options":[],"review":{"revision":"build-1","checks":checks}});
    gate["review"]["checks"][0]["check"] = json!("outdated");
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    assert_eq!(publish(task, None).unwrap_err(), "lead/review_changed");
    gate["review"]["checks"][0]["check"] = json!("approve");
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    let gate = publish(task, None).unwrap();
    answer(task, "c", gate.version.as_deref().unwrap(), &choice("accept")).unwrap();
    assert_eq!(fs::read_to_string(task.join("acceptance.json")).unwrap(), items);
    assert_eq!(read(task).unwrap().unwrap().answer, Some(choice("accept")));
    let snapshot = view(task, review_checks_raw(&items).unwrap()).unwrap().unwrap();
    assert_eq!(snapshot.gate.unwrap().status, "answered");
    assert!(snapshot.checks[0].passes);
    let mut projected = review_checks_raw(&items).unwrap();
    project_checks(task, &mut projected).unwrap();
    assert!(projected[0].passes);
    projected[0].check = "new requirement".into();
    projected[0].passes = true;
    project_checks(task, &mut projected).unwrap();
    assert!(!projected[0].passes);
    projected.push(meowo_protocol::ipc::LeadCheckDto { id: "new".into(), check: "new".into(), passes: true, leader: true, waived: None, blocked: None, evidence: None });
    project_checks(task, &mut projected).unwrap();
    assert!(!projected[1].passes);
}

#[test]
fn answer_and_withdraw_are_serialized_without_losing_the_decision() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    let gate = publish(temp.path(), None).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [true, false].into_iter().map(|answering| {
        let barrier = barrier.clone();
        let task = temp.path().to_path_buf();
        let version = gate.version.clone().unwrap();
        std::thread::spawn(move || {
            barrier.wait();
            if answering { answer(&task, "one", &version, &choice("confirm")) }
            else { withdraw(&task, "one", &version) }
        })
    }).collect();
    let results: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let state = load(temp.path()).unwrap().unwrap();
    assert_eq!(state.records.len(), 1);
    let record = &state.records[0];
    assert_ne!(record.withdrawn, record.gate.answer.is_some());
    assert_eq!(state.current.is_some(), record.gate.answer.is_some());
}

#[test]
fn answer_and_reissue_are_serialized_against_the_published_version() {
    let temp = tempfile::tempdir().unwrap();
    draft(temp.path(), "one");
    let gate = publish(temp.path(), None).unwrap();
    draft(temp.path(), "two");
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [true, false].into_iter().map(|answering| {
        let barrier = barrier.clone();
        let task = temp.path().to_path_buf();
        let version = gate.version.clone().unwrap();
        std::thread::spawn(move || {
            barrier.wait();
            if answering { answer(&task, "one", &version, &choice("confirm")) }
            else { publish(&task, Some(&version)).map(|_| ()) }
        })
    }).collect();
    let results: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let current = read(temp.path()).unwrap().unwrap();
    assert!(current.id == "one" && current.status == "answered" || current.id == "two" && current.status == "pending");
}

#[test]
fn a_new_delivery_revision_pending_or_rejected_never_inherits_old_acceptance() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    let items = json!([{"id":"business","check":"approve","by":"leader","passes":false}]).to_string();
    fs::write(task.join("acceptance.json"), format!("\u{feff}{items}")).unwrap();
    initialize(task).unwrap();
    let checks = review_checks_raw(&items).unwrap();
    let make = |id: &str, revision: &str| json!({"id":id,"type":"C","title":"review","summary":"proof","status":"pending","options":[],"review":{"revision":revision,"checks":checks}}).to_string();
    fs::write(task.join("gate.json"), format!("\u{feff}{}", make("c1", "build-1"))).unwrap();
    let first = publish(task, None).unwrap();
    answer(task, "c1", first.version.as_deref().unwrap(), &choice("accept")).unwrap();
    consume(task, "c1", first.version.as_deref().unwrap()).unwrap();
    let mut projected = checks.clone();
    project_checks(task, &mut projected).unwrap();
    assert!(projected[0].passes);
    draft(task, "ordinary");
    let ordinary = publish(task, first.version.as_deref()).unwrap();
    project_checks(task, &mut projected).unwrap();
    assert!(projected[0].passes);
    fs::write(task.join("gate.json"), make("c2", "build-2")).unwrap();
    let second = publish(task, ordinary.version.as_deref()).unwrap();
    project_checks(task, &mut projected).unwrap();
    assert!(!projected[0].passes);
    answer(task, "c2", second.version.as_deref().unwrap(), &choice("reject")).unwrap();
    project_checks(task, &mut projected).unwrap();
    assert!(!projected[0].passes);
}

#[test]
fn migration_preserves_legacy_decisions_and_freezes_legacy_passes_once() {
    for status in ["answered", "consumed"] {
        let temp = tempfile::tempdir().unwrap();
        let task = temp.path();
        let items = json!([{"id":"old","check":"legacy signed","by":"leader","passes":true}]).to_string();
        fs::write(task.join("acceptance.json"), &items).unwrap();
        fs::write(task.join("gate.json"), json!({"id":"old-gate","type":"C","title":"old","summary":"","options":[],"status":status,"answer":{"choice":"accept","note":"original"}}).to_string()).unwrap();
        initialize(task).unwrap();
        let gate = read(task).unwrap().unwrap();
        assert_eq!(gate.status, status);
        assert!(gate.review.is_none());
        assert_eq!(gate.answer.as_ref().unwrap().note, "original");
        let mut checks = review_checks_raw(&items).unwrap();
        project_checks(task, &mut checks).unwrap();
        assert!(checks[0].passes);
        checks[0].check = "modified after migration".into();
        fs::write(task.join("acceptance.json"), json!([{"id":"old","check":"modified after migration","by":"leader","passes":true}]).to_string()).unwrap();
        initialize(task).unwrap();
        project_checks(task, &mut checks).unwrap();
        assert!(!checks[0].passes);
        if status == "answered" { consume(task, &gate.id, gate.version.as_deref().unwrap()).unwrap(); }
        draft(task, "next");
        publish(task, gate.version.as_deref()).unwrap();
        let mut original = review_checks_raw(&items).unwrap();
        project_checks(task, &mut original).unwrap();
        assert!(original[0].passes);
        assert_eq!(load(task).unwrap().unwrap().records[0].gate.status, "consumed");
    }
}

#[test]
fn new_c_requires_old_leader_items_to_be_visible_in_the_new_signing_scope() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    let mut items = json!([{"id":"old","check":"legacy signed","by":"leader","passes":true}, {"id":"new","check":"new delivery","by":"leader","passes":false}]);
    fs::write(task.join("acceptance.json"), items.to_string()).unwrap();
    initialize(task).unwrap();
    let write_c = |items: &serde_json::Value| {
        fs::write(task.join("acceptance.json"), items.to_string()).unwrap();
        fs::write(task.join("gate.json"), json!({"id":"new-c","type":"C","title":"review","summary":"both items","status":"pending","options":[],"review":{"revision":"build-2","checks":review_checks_raw(&items.to_string()).unwrap()}}).to_string()).unwrap();
    };
    write_c(&items);
    assert_eq!(publish(task, None).unwrap_err(), "lead/review_changed");
    let mut original = review_checks_raw(&items.to_string()).unwrap();
    project_checks(task, &mut original).unwrap();
    assert!(original[0].passes);
    items[0]["passes"] = json!(false);
    write_c(&items);
    let gate = publish(task, None).unwrap();
    assert!(gate.review.as_ref().unwrap().checks.iter().all(|check| !check.passes));
    answer(task, &gate.id, gate.version.as_deref().unwrap(), &choice("accept")).unwrap();
    let mut projected = review_checks_raw(&items.to_string()).unwrap();
    project_checks(task, &mut projected).unwrap();
    assert!(projected.iter().all(|check| check.passes));
}

#[test]
fn publish_and_withdraw_retries_do_not_reactivate_history() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let first = publish(task, None).unwrap();
    assert_eq!(publish(task, None).unwrap().version, first.version);
    withdraw(task, "one", first.version.as_deref().unwrap()).unwrap();
    withdraw(task, "one", first.version.as_deref().unwrap()).unwrap();
    assert_eq!(publish(task, None).unwrap().version, first.version);
    assert!(read(task).unwrap().is_none());
    draft(task, "two");
    let second = publish(task, None).unwrap();
    draft(task, "one");
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    assert_eq!(publish(task, None).unwrap().version, first.version);
    withdraw(task, "one", first.version.as_deref().unwrap()).unwrap();
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    assert_eq!(read(task).unwrap().unwrap().id, second.id);
    let mut changed: serde_json::Value = serde_json::from_str(&fs::read_to_string(task.join("gate.json")).unwrap()).unwrap();
    changed["summary"] = json!("changed");
    fs::write(task.join("gate.json"), changed.to_string()).unwrap();
    assert_eq!(publish(task, None).unwrap_err(), "lead/gate_id_reused");
}

#[test]
fn publication_reserves_space_for_a_maximum_sized_answer() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let mut gate: serde_json::Value = serde_json::from_str(&fs::read_to_string(task.join("gate.json")).unwrap()).unwrap();
    gate["summary"] = json!("x".repeat(MAX_BYTES - ANSWER_RESERVE - 2000));
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    let gate = publish(task, None).unwrap();
    let mut answer_value = choice("confirm");
    let overhead = serde_json::to_string_pretty(&answer_value).unwrap().len();
    answer_value.note = "x".repeat(MAX_ANSWER_BYTES - overhead);
    assert_eq!(serde_json::to_string_pretty(&answer_value).unwrap().len(), MAX_ANSWER_BYTES);
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    let mut oversized = answer_value.clone();
    oversized.note.push('x');
    assert_eq!(answer(task, "one", gate.version.as_deref().unwrap(), &oversized).unwrap_err(), "lead/answer_too_large");
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    answer(task, "one", gate.version.as_deref().unwrap(), &answer_value).unwrap();
    assert_eq!(read(task).unwrap().unwrap().answer, Some(answer_value));
}

#[test]
fn archived_history_keeps_retry_identity_without_growing_live_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    let mut previous = None;
    for index in 0..80 {
        draft(task, &format!("gate-{index}"));
        let gate = publish(task, previous.as_deref()).unwrap();
        answer(task, &gate.id, gate.version.as_deref().unwrap(), &choice("confirm")).unwrap();
        consume(task, &gate.id, gate.version.as_deref().unwrap()).unwrap();
        previous = gate.version;
    }
    let state = load(task).unwrap().unwrap();
    assert!(state.records.len() <= 32);
    assert_eq!(state.generation, 80);
    let original = find_record(task, &state, "gate-0").unwrap().unwrap();
    assert_eq!(original.gate.status, "consumed");
    assert_eq!(original.gate.answer, Some(choice("confirm")));
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    draft(task, "gate-0");
    assert_eq!(publish(task, None).unwrap().version.as_deref(), Some("publication-1"));
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    assert_eq!(read(task).unwrap().unwrap().id, "gate-79");
}

#[test]
fn archive_precommit_failure_leaves_live_state_authoritative_and_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let first = publish(task, None).unwrap();
    let mut candidate = load(task).unwrap().unwrap();
    candidate.records[0].withdrawn = true;
    candidate.generation = 2;
    let mut next = candidate.records[0].clone();
    next.generation = 2;
    next.gate.id = "two".into();
    next.gate.version = Some(version(2));
    next.withdrawn = false;
    candidate.records.push(next);
    candidate.current = Some(1);
    compact(task, &mut candidate).unwrap();
    assert_eq!(read(task).unwrap().unwrap().status, "pending");
    answer(task, "one", first.version.as_deref().unwrap(), &choice("confirm")).unwrap();
    consume(task, "one", first.version.as_deref().unwrap()).unwrap();
    let mut retry = load(task).unwrap().unwrap();
    retry.generation = 2;
    retry.records.push(candidate.records[0].clone());
    retry.current = Some(1);
    compact(task, &mut retry).unwrap();
    save(task, &retry).unwrap();
    assert_eq!(find_record(task, &retry, "one").unwrap().unwrap().gate.answer, Some(choice("confirm")));
    assert_eq!(read(task).unwrap().unwrap().id, "two");
}


fn archived_pair(task: &Path) {
    draft(task, "one");
    let first = publish(task, None).unwrap();
    answer(task, "one", first.version.as_deref().unwrap(), &choice("confirm")).unwrap();
    consume(task, "one", first.version.as_deref().unwrap()).unwrap();
    draft(task, "two");
    let second = publish(task, first.version.as_deref()).unwrap();
    withdraw(task, "two", second.version.as_deref().unwrap()).unwrap();
    let mut state = load(task).unwrap().unwrap();
    compact(task, &mut state).unwrap();
    save(task, &state).unwrap();
}

#[test]
fn missing_archive_cannot_reactivate_original_publish() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    archived_pair(task);
    fs::remove_file(history_path(task, "one")).unwrap();
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    draft(task, "one");
    assert_eq!(publish(task, None).unwrap_err(), "lead/publication_history_missing");
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    assert!(read(task).unwrap().is_none());
}

#[test]
fn archived_answer_corruption_and_hash_collision_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    archived_pair(task);
    let state = load(task).unwrap().unwrap();
    let record = find_record(task, &state, "one").unwrap().unwrap();
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    for mutation in ["missing_answer", "wrong_status", "wrong_id", "wrong_generation"] {
        let mut corrupt = record.clone();
        match mutation {
            "missing_answer" => corrupt.gate.answer = None,
            "wrong_status" => corrupt.gate.status = "pending".into(),
            "wrong_id" => corrupt.gate.id = "collision".into(),
            _ => { corrupt.generation = 2; corrupt.gate.version = Some(version(2)); }
        }
        fs::write(history_path(task, "one"), serde_json::to_string(&corrupt).unwrap()).unwrap();
        draft(task, "one");
        assert_eq!(publish(task, None).unwrap_err(), "lead/publication_invalid", "{mutation}");
        assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    }
}

#[test]
fn isolated_process_worker() {
    let Some(task) = std::env::var_os("MEOWO_TEST_PUBLICATION_TASK") else { return; };
    let task = std::path::PathBuf::from(task);
    let action = std::env::var("MEOWO_TEST_PUBLICATION_ACTION").unwrap();
    let result_file = std::env::var("MEOWO_TEST_PUBLICATION_RESULT").unwrap();
    if action == "hold" {
        let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(task.join(".gate-publications.lock")).unwrap();
        lock.lock().unwrap();
        fs::write(task.join(result_file), "locked").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    let start = std::time::Instant::now();
    while !task.join("release").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    fs::write(task.join(format!("{result_file}.entering")), "ready").unwrap();
    let result = answer(&task, "one", "publication-1", &choice(&action));
    fs::write(task.join(result_file), serde_json::to_string(&result).unwrap()).unwrap();
}

fn process_worker(task: &Path, action: &str, result: &str) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lead::publication::tests::isolated_process_worker", "--nocapture"])
        .env("MEOWO_TEST_PUBLICATION_TASK", task)
        .env("MEOWO_TEST_PUBLICATION_ACTION", action)
        .env("MEOWO_TEST_PUBLICATION_RESULT", result)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn().unwrap()
}

#[test]
fn independent_process_answers_have_one_winner_and_release_abandoned_lock() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "one");
    let gate = publish(task, None).unwrap();
    let mut holder = process_worker(task, "hold", "ready");
    let start = std::time::Instant::now();
    while !task.join("ready").exists() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let ready = task.join("ready").exists();
    let mut first = process_worker(task, "confirm", "first.json");
    let mut second = process_worker(task, "revise", "second.json");
    fs::write(task.join("release"), "go").unwrap();
    let start = std::time::Instant::now();
    while !(task.join("first.json.entering").exists() && task.join("second.json.entering").exists()) && start.elapsed() < std::time::Duration::from_secs(1) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let both_entering = task.join("first.json.entering").exists() && task.join("second.json.entering").exists();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let neither_completed = !task.join("first.json").exists() && !task.join("second.json").exists();
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    assert!(ready, "child must acquire the real publication lock before termination");
    assert!(both_entering && neither_completed, "both answer workers must reach the held lock without completing");
    let results: Vec<Result<(), String>> = ["first.json", "second.json"].iter().map(|name| serde_json::from_slice(&fs::read(task.join(name)).unwrap()).unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.as_ref().err().is_some_and(|error| error == "lead/gate_closed")).count(), 1);
    let saved = read(task).unwrap().unwrap();
    let selected = saved.answer.unwrap();
    assert_eq!(selected, choice(if results[0].is_ok() { "confirm" } else { "revise" }));
    consume(task, "one", gate.version.as_deref().unwrap()).unwrap();
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    answer(task, "one", gate.version.as_deref().unwrap(), &selected).unwrap();
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
}

#[test]
fn legacy_answer_and_initialization_never_hide_a_successful_decision() {
    for _ in 0..20 {
        let temp = tempfile::tempdir().unwrap();
        draft(temp.path(), "legacy");
        let old_version = super::super::read_gate(temp.path()).unwrap().version.unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let task = temp.path().to_path_buf();
        let start = barrier.clone();
        let answerer = std::thread::spawn(move || {
            start.wait();
            super::super::answer_gate_checked(&task, "legacy", &choice("confirm"), Some(&old_version))
        });
        let task = temp.path().to_path_buf();
        let initializer = std::thread::spawn(move || { barrier.wait(); initialize(&task) });
        let result = answerer.join().unwrap();
        initializer.join().unwrap().unwrap();
        let gate = read(temp.path()).unwrap();
        match result {
            Ok(()) => {
                let gate = gate.unwrap();
                assert_eq!(gate.status, "answered");
                assert_eq!(gate.answer, Some(choice("confirm")));
            }
            Err(reason) => {
                assert_eq!(reason, "lead/gate_changed");
                assert!(gate.is_none());
            }
        }
    }
}

#[test]
fn initialize_recovers_legacy_receipt_before_freezing_baseline() {
    for already_signed in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let task = temp.path();
        let items = json!([{"id":"leader","check":"approve","passes":false,"by":"leader"}]);
        fs::write(task.join("acceptance.json"), items.to_string()).unwrap();
        let gate = json!({"id":"legacy-c","type":"C","title":"review","status":"pending","review":{"revision":"old-build","checks":review_checks_raw(&items.to_string()).unwrap()}}).to_string();
        fs::write(task.join("gate.json"), &gate).unwrap();
        super::super::tests::seed_legacy_receipt(task, "legacy-c");
        super::super::recover_review(task).unwrap();
        fs::write(task.join("gate.json"), &gate).unwrap();
        if !already_signed { fs::write(task.join("acceptance.json"), items.to_string()).unwrap(); }
        initialize(task).unwrap();
        let imported = read(task).unwrap().unwrap();
        assert_eq!(imported.status, "answered");
        assert_eq!(imported.answer, Some(choice("accept")));
        assert!(load(task).unwrap().unwrap().legacy_checks[0].passes);
        let before = fs::read(task.join(STATE_FILE)).unwrap();
        initialize(task).unwrap();
        assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    }
}

#[test]
fn migration_lock_contention_is_bounded_and_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    draft(task, "legacy");
    let held = super::super::lock_gate(task).unwrap();
    assert_eq!(initialize(task).unwrap_err(), "lead/publication_busy");
    assert!(!exists(task));
    drop(held);
    initialize(task).unwrap();
    assert!(exists(task));
}

#[test]
fn initialize_retires_pending_c_id_even_if_review_is_added_later() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path();
    let items = json!([{"id":"leader","check":"approve","passes":false,"by":"leader"}]);
    fs::write(task.join("acceptance.json"), items.to_string()).unwrap();
    let mut gate = json!({"id":"old-c","type":"C","title":"review","status":"pending"});
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    assert_eq!(publish(task, None).unwrap_err(), "lead/publication_initialize_required");
    assert!(!exists(task));
    initialize(task).unwrap();
    assert!(read(task).unwrap().is_none());
    assert!(contains_gate_id(task, "old-c").unwrap());
    let before = fs::read(task.join(STATE_FILE)).unwrap();
    gate["review"] = json!({"revision":"new-build","checks":review_checks_raw(&items.to_string()).unwrap()});
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    assert_eq!(publish(task, None).unwrap_err(), "lead/gate_id_reused");
    assert_eq!(fs::read(task.join(STATE_FILE)).unwrap(), before);
    gate["id"] = json!("new-c");
    fs::write(task.join("gate.json"), gate.to_string()).unwrap();
    assert_eq!(publish(task, None).unwrap().id, "new-c");
}
