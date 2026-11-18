use super::{fixed_choices, parse_gate, review_checks_raw};
use meowo_protocol::ipc::{LeadGateAnswerDto, LeadGateDto};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

const STATE_FILE: &str = ".gate-publications.json";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_ANSWER_BYTES: usize = 32 * 1024;
const ANSWER_RESERVE: usize = MAX_ANSWER_BYTES + 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    generation: u64,
    current: Option<usize>,
    records: Vec<Record>,
    archived: std::collections::BTreeMap<String, u64>,
    legacy_checks: Vec<meowo_protocol::ipc::LeadCheckDto>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    generation: u64,
    gate: LeadGateDto,
    withdrawn: bool,
    legacy: bool,
    previous: Option<String>,
}

fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("lead/publication_unreadable".into()),
    };
    let mut raw = String::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_string(&mut raw)
        .map_err(|_| "lead/publication_invalid")?;
    if raw.len() > MAX_BYTES {
        return Err("lead/publication_too_large".into());
    }
    Ok(Some(raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_owned()))
}

fn valid_record(record: &Record) -> bool {
    record.generation > 0
        && record.gate.version.as_deref() == Some(version(record.generation).as_str())
        && matches!(record.gate.status.as_str(), "pending" | "answered" | "consumed")
        && (record.gate.status == "pending") == record.gate.answer.is_none()
        && (!record.withdrawn || record.gate.status == "pending")
}

fn load(task: &Path) -> Result<Option<State>, String> {
    let Some(raw) = read_bounded(&task.join(STATE_FILE))? else { return Ok(None); };
    let state: State = serde_json::from_str(&raw).map_err(|_| "lead/publication_invalid")?;
    if state.current.is_some_and(|index| index >= state.records.len())
        || state.records.last().map_or(0, |record| record.generation) != state.generation
        || state.records.iter().enumerate().any(|(index, record)| {
            !valid_record(record)
                || (index > 0 && record.generation <= state.records[index - 1].generation)
                || state.archived.contains_key(&record.gate.id)
                || state.records[..index].iter().any(|previous| previous.gate.id == record.gate.id)
        })
        || state.archived.values().any(|generation| *generation == 0 || *generation > state.generation)
        || state.current.is_some_and(|index| state.records[index].withdrawn)
    {
        return Err("lead/publication_invalid".into());
    }
    Ok(Some(state))
}

fn history_path(task: &Path, id: &str) -> std::path::PathBuf {
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    task.join(".gate-history").join(format!("{hash:016x}.json"))
}

fn find_record(task: &Path, state: &State, id: &str) -> Result<Option<Record>, String> {
    if let Some(record) = state.records.iter().find(|record| record.gate.id == id) {
        return Ok(Some(record.clone()));
    }
    let Some(generation) = state.archived.get(id) else { return Ok(None); };
    let raw = read_bounded(&history_path(task, id))?.ok_or("lead/publication_history_missing")?;
    let record: Record = serde_json::from_str(&raw).map_err(|_| "lead/publication_invalid")?;
    if record.gate.id != id || record.generation != *generation || !valid_record(&record) {
        return Err("lead/publication_invalid".into());
    }
    Ok(Some(record))
}

fn compact(task: &Path, state: &mut State) -> Result<(), String> {
    let current_generation = state.current.map(|index| state.records[index].generation);
    let review_generation = state.records.iter().rev()
        .find(|record| record.gate.kind == "C" && !record.legacy).map(|record| record.generation);
    let mut kept = Vec::new();
    for record in &state.records {
        if Some(record.generation) == current_generation || Some(record.generation) == review_generation
            || record.generation == state.generation {
            kept.push(record.clone());
            continue;
        }
        let raw = serde_json::to_string_pretty(record).map_err(|_| "lead/publication_invalid")?;
        let path = history_path(task, &record.gate.id);
        if let Some(existing) = read_bounded(&path)? {
            let existing: Record = serde_json::from_str(&existing).map_err(|_| "lead/publication_invalid")?;
            if existing.gate.id != record.gate.id || existing.generation != record.generation {
                return Err("lead/publication_invalid".into());
            }
        }
        meowo_agent::fsutil::write_atomic(&path, &raw).map_err(|_| "lead/publication_write_failed")?;
        state.archived.insert(record.gate.id.clone(), record.generation);
    }
    state.records = kept;
    state.current = current_generation.and_then(|generation| state.records.iter().position(|record| record.generation == generation));
    Ok(())
}

fn save(task: &Path, state: &State) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(state).map_err(|_| "lead/publication_invalid")?;
    if raw.len() > MAX_BYTES {
        return Err("lead/publication_too_large".into());
    }
    meowo_agent::fsutil::write_atomic(&task.join(STATE_FILE), &raw)
        .map_err(|_| "lead/publication_write_failed".into())
}

fn lock(task: &Path) -> Result<std::fs::File, String> {
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(task.join(".gate-publications.lock")).map_err(|_| "lead/publication_write_failed")?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err("lead/publication_busy".into()),
            Err(_) => return Err("lead/publication_write_failed".into()),
        }
    }
}

fn version(generation: u64) -> String {
    format!("publication-{generation}")
}

fn current(state: &mut State, id: &str, expected: &str) -> Result<usize, String> {
    let index = state.current.ok_or("lead/gate_changed")?;
    let gate = &state.records[index].gate;
    if gate.id != id || gate.version.as_deref() != Some(expected) {
        return Err("lead/gate_changed".into());
    }
    Ok(index)
}

pub fn read(task: &Path) -> Result<Option<LeadGateDto>, String> {
    let Some(state) = load(task)? else { return Ok(None); };
    Ok(state.current.map(|index| state.records[index].gate.clone()))
}

pub fn contains_gate_id(task: &Path, id: &str) -> Result<bool, String> {
    Ok(load(task)?.is_some_and(|state| state.archived.contains_key(id) || state.records.iter().any(|record| record.gate.id == id)))
}

pub fn exists(task: &Path) -> bool {
    task.join(STATE_FILE).exists()
}

pub struct View {
    pub gate: Option<LeadGateDto>,
    pub checks: Vec<meowo_protocol::ipc::LeadCheckDto>,
}

pub fn view(task: &Path, mut checks: Vec<meowo_protocol::ipc::LeadCheckDto>) -> Result<Option<View>, String> {
    let Some(state) = load(task)? else { return Ok(None); };
    project(&state, &mut checks);
    Ok(Some(View { gate: state.current.map(|index| state.records[index].gate.clone()), checks }))
}

pub fn project_checks(task: &Path, checks: &mut [meowo_protocol::ipc::LeadCheckDto]) -> Result<(), String> {
    if let Some(state) = load(task)? { project(&state, checks); }
    Ok(())
}

fn project(state: &State, checks: &mut [meowo_protocol::ipc::LeadCheckDto]) {
    let latest_review = state.records.iter().rev().find(|record| record.gate.kind == "C" && !record.legacy);
    for check in checks.iter_mut().filter(|check| check.leader) {
        check.passes = if latest_review.is_none() {
            state.legacy_checks.iter().any(|bound| bound.passes && bound.leader
                && bound.id == check.id && bound.check == check.check
                && bound.waived == check.waived && bound.blocked == check.blocked && bound.evidence == check.evidence)
        } else { latest_review.is_some_and(|record| {
            !record.withdrawn
                && record.gate.answer.as_ref().is_some_and(|answer| answer.choice == "accept")
                && record.gate.review.as_ref().is_some_and(|review| review.checks.iter().any(|bound| {
                    !bound.passes && bound.waived.is_none() && bound.leader
                        && bound.id == check.id && bound.check == check.check
                        && bound.waived == check.waived && bound.blocked == check.blocked
                        && bound.evidence == check.evidence
                }))
        }) };
    }
}

fn initial_state(task: &Path) -> Result<State, String> {
    if task.join("gate.json").exists() { super::recover_legacy_review_locked(task)?; }
    let legacy_checks = read_bounded(&task.join("acceptance.json"))?
        .map(|raw| review_checks_raw(&raw)).transpose()?.unwrap_or_default();
    let mut state = State { legacy_checks, ..State::default() };
    if let Some(raw) = read_bounded(&task.join("gate.json"))? {
        let mut gate = parse_gate(&raw).ok_or("lead/gate_invalid")?;
        if gate.status != "pending" {
            if gate.answer.is_none() { return Err("lead/gate_invalid".into()); }
            state.generation = 1;
            state.current = Some(0);
            gate.version = Some(version(1));
            state.records.push(Record { generation: 1, gate, withdrawn: false, legacy: true, previous: None });
        }
    }
    Ok(state)
}

pub fn initialize(task: &Path) -> Result<(), String> {
    let _migration = super::lock_gate(task)?;
    let _lock = lock(task)?;
    if load(task)?.is_some() { return Ok(()); }
    let mut state = initial_state(task)?;
    if state.records.is_empty() {
        if let Some(raw) = read_bounded(&task.join("gate.json"))? {
            let mut gate = parse_gate(&raw).ok_or("lead/gate_invalid")?;
            if gate.kind == "C" && gate.status == "pending" {
                state.generation = 1;
                gate.version = Some(version(1));
                state.records.push(Record { generation: 1, gate, withdrawn: true, legacy: true, previous: None });
            }
        }
    }
    save(task, &state)
}

pub fn publish(task: &Path, expected: Option<&str>) -> Result<LeadGateDto, String> {
    publish_checked(task, expected, |_| Ok(()))
}

pub(super) fn publish_checked(
    task: &Path,
    expected: Option<&str>,
    before_publish: impl FnOnce(&LeadGateDto) -> Result<(), String>,
) -> Result<LeadGateDto, String> {
    let _migration = super::lock_gate(task)?;
    let _lock = lock(task)?;
    let mut state = match load(task)? {
        Some(state) => state,
        None => {
            if read_bounded(&task.join("gate.json"))?.and_then(|raw| parse_gate(&raw)).is_some_and(|gate| gate.kind == "C") {
                return Err("lead/publication_initialize_required".into());
            }
            initial_state(task)?
        }
    };
    let raw = read_bounded(&task.join("gate.json"))?.ok_or("lead/gate_missing")?;
    let mut gate = parse_gate(&raw).ok_or("lead/gate_invalid")?;
    if gate.status != "pending" || gate.answer.is_some() {
        return Err("lead/gate_invalid".into());
    }
    if let Some(record) = find_record(task, &state, &gate.id)? {
        let mut frozen = record.gate.clone();
        frozen.status = "pending".into();
        frozen.answer = None;
        frozen.version = None;
        let mut proposed = gate.clone();
        proposed.version = None;
        if !record.legacy && record.previous.as_deref() == expected
            && serde_json::to_value(&frozen).ok() == serde_json::to_value(&proposed).ok() {
            return Ok(record.gate.clone());
        }
        return Err("lead/gate_id_reused".into());
    }
    let active = state.current.map(|index| &state.records[index].gate);
    if active.and_then(|gate| gate.version.as_deref()) != expected {
        return Err("lead/gate_changed".into());
    }
    if active.is_some_and(|gate| gate.status == "answered") {
        return Err("lead/decision_unconsumed".into());
    }
    if gate.kind == "C" {
        let review = gate.review.as_ref().ok_or("lead/review_reissue")?;
        let acceptance = read_bounded(&task.join("acceptance.json"))?.ok_or("lead/acceptance_invalid")?;
        if review.revision.trim().is_empty()
            || review.checks.iter().any(|check| check.leader && check.waived.is_none() && check.passes)
            || review.checks != review_checks_raw(&acceptance)? {
            return Err("lead/review_changed".into());
        }
    }
    before_publish(&gate)?;
    state.generation = state.generation.checked_add(1).ok_or("lead/publication_invalid")?;
    gate.version = Some(version(state.generation));
    if let Some(index) = state.current {
        if state.records[index].gate.status == "pending" {
            state.records[index].withdrawn = true;
        }
    }
    state.current = Some(state.records.len());
    state.records.push(Record { generation: state.generation, gate: gate.clone(), withdrawn: false, legacy: false, previous: expected.map(str::to_owned) });
    let mut size = serde_json::to_string_pretty(&state).map_err(|_| "lead/publication_invalid")?.len();
    if size > MAX_BYTES / 2 || state.records.len() > 32 {
        compact(task, &mut state)?;
        size = serde_json::to_string_pretty(&state).map_err(|_| "lead/publication_invalid")?.len();
    }
    if size > MAX_BYTES - ANSWER_RESERVE { return Err("lead/publication_too_large".into()); }
    save(task, &state)?;
    Ok(gate)
}

pub fn answer(task: &Path, id: &str, expected: &str, answer: &LeadGateAnswerDto) -> Result<(), String> {
    if serde_json::to_string_pretty(answer).map_err(|_| "lead/publication_invalid")?.len() > MAX_ANSWER_BYTES {
        return Err("lead/answer_too_large".into());
    }
    let _lock = lock(task)?;
    let mut state = load(task)?.ok_or("lead/gate_missing")?;
    let index = current(&mut state, id, expected)?;
    let gate = &mut state.records[index].gate;
    if gate.status != "pending" {
        return if gate.answer.as_ref() == Some(answer) { Ok(()) } else { Err("lead/gate_closed".into()) };
    }
    let valid = match fixed_choices(&gate.kind) {
        Some(choices) => choices.contains(&answer.choice.as_str()),
        None if gate.kind == "Q" && answer.choice == "other" && !gate.options.iter().any(|option| option.id == "other") => !answer.note.trim().is_empty(),
        None => gate.options.iter().any(|option| option.id == answer.choice),
    };
    if !valid { return Err("lead/gate_bad_choice".into()); }
    gate.answer = Some(answer.clone());
    gate.status = "answered".into();
    save(task, &state)
}

pub fn consume(task: &Path, id: &str, expected: &str) -> Result<(), String> {
    let _lock = lock(task)?;
    let mut state = load(task)?.ok_or("lead/gate_missing")?;
    let index = current(&mut state, id, expected)?;
    let gate = &mut state.records[index].gate;
    if gate.status == "consumed" { return Ok(()); }
    if gate.status != "answered" { return Err("lead/gate_closed".into()); }
    gate.status = "consumed".into();
    save(task, &state)
}

pub fn withdraw(task: &Path, id: &str, expected: &str) -> Result<(), String> {
    let _lock = lock(task)?;
    let mut state = load(task)?.ok_or("lead/gate_missing")?;
    if find_record(task, &state, id)?.is_some_and(|record| record.withdrawn
        && record.gate.version.as_deref() == Some(expected) && record.gate.answer.is_none()) {
        return Ok(());
    }
    let index = current(&mut state, id, expected)?;
    if state.records[index].gate.status != "pending" { return Err("lead/gate_closed".into()); }
    state.records[index].withdrawn = true;
    state.current = None;
    save(task, &state)
}

#[cfg(test)]
mod tests;
