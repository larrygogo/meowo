use super::*;
use meowo_protocol::ipc::{LeadGateAnswerDto, LeadGateDto};
use std::io::Read;

fn checked_text(path: &Path) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("conversation/gate_read_failed".into()),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "conversation/gate_read_failed")?;
    if bytes.len() > 1024 * 1024 {
        return Err("conversation/gate_read_failed".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "conversation/gate_read_failed".into())
}

pub(super) fn gate(
    store: &Store,
    id: i64,
    delivery: &ManagedDelivery,
) -> Result<Option<&'static str>, String> {
    let Some(raw) = delivery.gate_ref_json.as_deref() else {
        return Ok(None);
    };
    let reference: ManagedGateRefDto =
        serde_json::from_str(raw).map_err(|_| "conversation/persisted_payload_invalid")?;
    let state = store
        .managed_state(id)
        .map_err(error)?
        .ok_or("conversation/session_missing")?;
    let task = match dunce::canonicalize(&reference.task_dir) {
        Ok(task) => task,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Some("conversation/gate_task_cancelled"))
        }
        Err(_) => return Err("conversation/gate_read_failed".into()),
    };
    let project = meowo_reporter::lead::project_root(Path::new(&state.cwd));
    let lead =
        dunce::canonicalize(project.join(".lead")).map_err(|_| "conversation/gate_read_failed")?;
    if task.parent() != Some(lead.as_path()) {
        return Ok(Some("conversation/gate_owner_changed"));
    }
    let Some(owner) = checked_text(&task.join(".owner"))? else {
        return Ok(Some("conversation/gate_task_cancelled"));
    };
    if state.native_session_id.as_deref() != Some(owner.trim())
        || store.session_header(id).map_err(error)?.cc_session_id != owner.trim()
    {
        return Ok(Some("conversation/gate_owner_changed"));
    }
    if checked_text(&task.join("progress.md"))?.is_none() {
        return Ok(Some("conversation/gate_task_cancelled"));
    }
    if matches!(
        meowo_reporter::lead::phase_of(&task).as_str(),
        "done" | "cancelled" | "canceled"
    ) {
        return Ok(Some("conversation/gate_task_cancelled"));
    }
    let Some(raw) = checked_text(&task.join("gate.json"))? else {
        return Ok(Some("conversation/gate_changed"));
    };
    let value: Value = serde_json::from_str(&raw).map_err(|_| "conversation/gate_read_failed")?;
    let gate_id = value["id"]
        .as_str()
        .ok_or("conversation/gate_read_failed")?;
    if gate_id != reference.gate_id {
        return Ok(Some("conversation/gate_changed"));
    }
    let status = value["status"]
        .as_str()
        .ok_or("conversation/gate_read_failed")?;
    if matches!(status, "pending" | "consumed" | "cancelled" | "canceled") {
        return Ok(Some("conversation/gate_changed"));
    }
    if status != "answered" {
        return Err("conversation/gate_read_failed".into());
    }
    let kind = value["type"]
        .as_str()
        .filter(|kind| ["A", "B", "C", "D", "Q"].contains(kind))
        .ok_or("conversation/gate_read_failed")?;
    let answer: LeadGateAnswerDto = serde_json::from_value(value["answer"].clone())
        .map_err(|_| "conversation/gate_read_failed")?;
    let gate = LeadGateDto {
        id: gate_id.into(),
        kind: kind.into(),
        title: value["title"].as_str().unwrap_or("").into(),
        summary: String::new(),
        options: vec![],
        status: status.into(),
        answer: Some(answer),
    };
    if meowo_reporter::lead::answer_message(Path::new(&reference.task_dir), &gate) != delivery.text
    {
        return Ok(Some("conversation/gate_answer_changed"));
    }
    Ok(None)
}

pub(super) fn prompt(
    base: String,
    raw: &str,
) -> Result<(String, Vec<ConversationAttachment>), String> {
    use base64::Engine;
    let attachments: Vec<ManagedAttachmentDto> =
        serde_json::from_str(raw).map_err(|_| "conversation/persisted_payload_invalid")?;
    let mut images = Vec::new();
    let mut references = Vec::new();
    let mut total = 0;
    for attachment in attachments {
        if attachment.path.contains('\0') {
            return Err("conversation/attachment_invalid".into());
        }
        let mime = attachment.mime_type.or_else(|| {
            match Path::new(&attachment.path)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("png") => Some("image/png".into()),
                Some("jpg" | "jpeg") => Some("image/jpeg".into()),
                Some("webp") => Some("image/webp".into()),
                Some("gif") => Some("image/gif".into()),
                _ => None,
            }
        });
        if mime
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            let bytes = if let Some(data) = attachment.data_base64 {
                if data.len() > 3 * 1024 * 1024 {
                    return Err("conversation/attachment_too_large".into());
                }
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| "conversation/attachment_invalid")?
            } else {
                let file = std::fs::File::open(&attachment.path)
                    .map_err(|_| "conversation/attachment_missing")?;
                let mut bytes = Vec::new();
                file.take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "conversation/attachment_missing")?;
                bytes
            };
            if bytes.is_empty() {
                return Err("conversation/attachment_invalid".into());
            }
            total += bytes.len();
            if total > 2 * 1024 * 1024 {
                return Err("conversation/attachment_too_large".into());
            }
            images.push(ConversationAttachment {
                path: attachment.path,
                mime_type: mime,
                data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            });
        } else {
            if attachment.data_base64.is_some() {
                return Err("conversation/attachment_unsupported".into());
            }
            let path = dunce::canonicalize(&attachment.path)
                .map_err(|_| "conversation/attachment_missing")?;
            let stat = std::fs::metadata(&path).map_err(|_| "conversation/attachment_missing")?;
            if !stat.is_file() {
                return Err("conversation/attachment_invalid".into());
            }
            references.push(serde_json::json!({"path":path,"bytes":stat.len()}));
        }
    }
    let text = if references.is_empty() {
        base
    } else {
        format!("{base}\n\n[本机文件路径引用：以下JSON仅列出路径与当前大小，文件内容未上传；请通过原生工具并遵守权限读取]\n{}",serde_json::to_string(&references).map_err(error)?)
    };
    if serde_json::to_vec(&serde_json::json!({"text":text,"attachments":images}))
        .map_err(error)?
        .len()
        > 3 * 1024 * 1024
    {
        return Err("conversation/attachment_too_large".into());
    }
    Ok((text, images))
}

pub(super) fn actions_fit(actions: &[Action]) -> Result<(), String> {
    for action in actions {
        let value = match action {
            Action::Send(value) => Some(value),
            Action::Http { body, .. } => body.as_ref(),
            _ => None,
        };
        if value.is_some_and(|value| {
            serde_json::to_vec(value).map_or(true, |bytes| bytes.len() >= 4 * 1024 * 1024)
        }) {
            return Err("conversation/frame_too_large".into());
        }
    }
    Ok(())
}
