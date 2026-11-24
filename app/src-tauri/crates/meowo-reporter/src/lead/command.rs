use super::{publication, review_checks, slug_ok};
use serde_json::{json, Value};
use std::path::Path;

/// `lead root [--cwd <目录>]`：打印本项目的 Lead 目录（不存在就建好）。skill 让 agent 开任务前先跑它：
/// 任务改存 Meowo 数据目录后（任务 20261124-lead-store-in-meowo），位置由项目键算出，agent 自己拼不出来。
/// 输出就是一行绝对路径，agent 直接拿去用；不加 JSON 包装。
pub fn root(args: &[String]) -> Result<String, String> {
    let cwd = match args {
        [] => std::env::current_dir().map_err(|_| "lead/cwd_unavailable")?,
        [flag, dir] if flag == "--cwd" => std::path::PathBuf::from(dir),
        _ => return Err("lead/command_invalid".into()),
    };
    let home = meowo_agent::lead_paths::ensure_lead_home(&cwd).ok_or("lead/home_unavailable")?;
    Ok(home.display().to_string())
}

pub fn run(args: &[String]) -> Result<Value, String> {
    run_with_owner(args, crate::proc::owner_pid("kimi"))
}

fn run_with_owner(args: &[String], kimi_pid: Option<u32>) -> Result<Value, String> {
    let operation = args.first().map(String::as_str).ok_or("lead/command_invalid")?;
    let mut task = None;
    let mut expected = None;
    let mut id = None;
    let mut pairs = args[1..].chunks_exact(2);
    for pair in &mut pairs {
        let slot = match pair[0].as_str() {
            "--task" => &mut task,
            "--version" => &mut expected,
            "--id" => &mut id,
            _ => return Err("lead/command_invalid".into()),
        };
        if slot.replace(pair[1].as_str()).is_some() { return Err("lead/command_invalid".into()); }
    }
    if !pairs.remainder().is_empty() { return Err("lead/command_invalid".into()); }
    let task = Path::new(task.ok_or("lead/command_invalid")?).canonicalize().map_err(|_| "lead/task_missing")?;
    if !task.is_dir()
        || task.parent().and_then(Path::file_name).is_none_or(|name| name != ".lead")
        || task.file_name().is_none_or(|name| !slug_ok(&name.to_string_lossy())) {
        return Err("lead/command_invalid".into());
    }
    match operation {
        "initialize" | "status" if expected.is_none() && id.is_none() => {
            if operation == "initialize" { publication::initialize(&task)?; }
        }
        "publish" if id.is_none() => {
            if !publication::exists(&task) { return Err("lead/publication_initialize_required".into()); }
            let expected = expected.ok_or("lead/command_invalid")?;
            publication::publish_checked(&task, (expected != "none").then_some(expected), |gate| {
                let Some(pid) = kimi_pid else { return Ok(()); };
                let owner = super::read_checked(&task.join(".owner")).map_err(|_| "lead/waiter_required")?;
                let ready = crate::lead_waiter::target_waiter_ready(&task, &gate.id, owner.trim(), Some(pid));
                if !ready { return Err("lead/waiter_required".into()); }
                Ok(())
            })?;
        }
        "withdraw" | "consume" => {
            let id = id.ok_or("lead/command_invalid")?;
            let expected = expected.ok_or("lead/command_invalid")?;
            if operation == "withdraw" { publication::withdraw(&task, id, expected)?; }
            else { publication::consume(&task, id, expected)?; }
        }
        _ => return Err("lead/command_invalid".into()),
    }
    let mut issues = Vec::new();
    let checks = review_checks(&task).unwrap_or_else(|reason| {
        issues.push(reason);
        Vec::new()
    });
    match publication::view(&task, checks) {
        Ok(Some(view)) => Ok(json!({"initialized":true, "snapshotAvailable":true, "gate":view.gate, "checks":view.checks, "issues":issues})),
        Ok(None) => Ok(json!({"initialized":false, "snapshotAvailable":true, "issues":issues})),
        Err(reason) if operation != "status" => {
            issues.push(reason);
            Ok(json!({"snapshotAvailable":false, "issues":issues}))
        }
        Err(reason) => Err(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn call(task: &Path, operation: &str, extra: &[&str]) -> Result<Value, String> {
        let mut args = vec![operation.to_owned(), "--task".into(), task.to_string_lossy().into_owned()];
        args.extend(extra.iter().map(|value| (*value).to_owned()));
        run_with_owner(&args, None)
    }

    #[test]
    fn explicit_commands_preserve_drafts_and_return_current_not_historical_card() {
        let temp = tempfile::tempdir().unwrap();
        let task = temp.path().join(".lead/20261116-command");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join("acceptance.json"), "[]").unwrap();
        let draft = json!({"id":"one","type":"A","status":"pending"}).to_string();
        fs::write(task.join("gate.json"), &draft).unwrap();
        assert_eq!(call(&task, "publish", &["--version", "none"]).unwrap_err(), "lead/publication_initialize_required");
        call(&task, "initialize", &[]).unwrap();
        assert!(call(&task, "publish", &[]).is_err());
        let first = call(&task, "publish", &["--version", "none"]).unwrap();
        assert_eq!(first["gate"]["version"], "publication-1");
        call(&task, "withdraw", &["--id", "one", "--version", "publication-1"]).unwrap();
        assert!(call(&task, "publish", &["--version", "none"]).unwrap()["gate"].is_null());
        fs::write(task.join("gate.json"), json!({"id":"two","type":"A","status":"pending"}).to_string()).unwrap();
        call(&task, "publish", &["--version", "none"]).unwrap();
        publication::answer(&task, "two", "publication-2", &meowo_protocol::ipc::LeadGateAnswerDto {choice:"confirm".into(),note:String::new()}).unwrap();
        let before = fs::read(task.join("gate.json")).unwrap();
        let consumed = call(&task, "consume", &["--id", "two", "--version", "publication-2"]).unwrap();
        assert_eq!(consumed["gate"]["status"], "consumed");
        assert_eq!(fs::read(task.join("gate.json")).unwrap(), before);
        fs::write(task.join("gate.json"), draft).unwrap();
        assert_eq!(call(&task, "publish", &["--version", "none"]).unwrap()["gate"]["id"], "two");
    }

    #[test]
    fn missing_or_broken_acceptance_does_not_hide_committed_operations() {
        let temp = tempfile::tempdir().unwrap();
        let task = temp.path().join(".lead/20261116-command");
        fs::create_dir_all(&task).unwrap();
        let initialized = call(&task, "initialize", &[]).unwrap();
        assert_eq!(initialized["initialized"], true);
        assert_eq!(initialized["issues"], json!(["lead/acceptance_invalid"]));
        fs::write(task.join("acceptance.json"), "broken").unwrap();
        fs::write(task.join("gate.json"), json!({"id":"one","type":"A","status":"pending"}).to_string()).unwrap();
        let published = call(&task, "publish", &["--version", "none"]).unwrap();
        assert_eq!(published["gate"]["id"], "one");
        assert_eq!(published["issues"], json!(["lead/acceptance_invalid"]));
        assert_eq!(call(&task, "status", &[]).unwrap()["gate"]["id"], "one");
        publication::answer(&task, "one", "publication-1", &meowo_protocol::ipc::LeadGateAnswerDto {choice:"confirm".into(),note:String::new()}).unwrap();
        assert_eq!(call(&task, "consume", &["--id", "one", "--version", "publication-1"]).unwrap()["gate"]["status"], "consumed");
        fs::write(task.join("gate.json"), json!({"id":"two","type":"A","status":"pending"}).to_string()).unwrap();
        call(&task, "publish", &["--version", "publication-1"]).unwrap();
        assert!(call(&task, "withdraw", &["--id", "two", "--version", "publication-2"]).unwrap()["gate"].is_null());
    }

    #[test]
    fn kimi_publish_requires_target_waiter_but_historical_retry_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let task = temp.path().join(".lead/20261116-kimi");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join("acceptance.json"), "[]").unwrap();
        fs::write(task.join(".owner"), "session_target").unwrap();
        call(&task, "initialize", &[]).unwrap();
        let pid = std::process::id();
        let started = crate::lead_waiter::process_started(pid).unwrap();
        let publish = || run_with_owner(&["publish".into(), "--task".into(), task.to_string_lossy().into_owned(), "--version".into(), "none".into()], Some(pid));
        fs::write(task.join("gate.json"), json!({"id":"new","type":"A","status":"pending"}).to_string()).unwrap();
        let before = fs::read(task.join(".gate-publications.json")).unwrap();
        assert_eq!(publish().unwrap_err(), "lead/waiter_required");
        let mut record = crate::lead_waiter::WaiterRecord {pid, started, gate:"old".into(),session:"session_target".into(),provider:"kimi".into(),cli_pid:Some(pid),cli_started:Some(started)};
        fs::write(crate::lead_waiter::waiter_path(&task), serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(publish().unwrap_err(), "lead/waiter_required");
        assert_eq!(fs::read(task.join(".gate-publications.json")).unwrap(), before);
        record.gate = "new".into();
        fs::write(crate::lead_waiter::waiter_path(&task), serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(publish().unwrap()["gate"]["id"], "new");
        call(&task, "withdraw", &["--id", "new", "--version", "publication-1"]).unwrap();
        fs::remove_file(crate::lead_waiter::waiter_path(&task)).unwrap();
        let before = fs::read(task.join(".gate-publications.json")).unwrap();
        assert!(publish().unwrap()["gate"].is_null());
        assert_eq!(fs::read(task.join(".gate-publications.json")).unwrap(), before);
    }

    #[test]
    fn command_rejects_bad_paths_unknown_flags_and_duplicate_arguments() {
        let temp = tempfile::tempdir().unwrap();
        assert!(call(temp.path(), "initialize", &[]).is_err());
        assert!(call(temp.path(), "status", &["--task", "x"]).is_err());
        assert!(call(temp.path(), "status", &["--session", "x"]).is_err());
        assert!(run(&["status".into(), "--task".into()]).is_err());
    }
}
