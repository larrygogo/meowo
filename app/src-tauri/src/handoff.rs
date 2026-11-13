//! 把会话复制到另一个 provider：导出当前会话的结构化历史成交接文件，在同一工作目录
//! 用目标 agent 起一个**独立的**新会话，由前端把「请读交接文件」发进去。
//!
//! 原会话不结束、不标记接替，也不串接续链（用户要求：转移不如直接复制）——两边各自
//! 继续，想回原引擎直接回原会话即可。早先「切换引擎」留下的接续链数据照常回看。

use std::collections::HashMap;

use meowo_protocol::ipc::{LineageEntryDto, SwitchStartedDto};

/// 交接文件的注入语里引用的文件名。固定名 + 时间戳目录：路径可预测性无所谓
/// （内容只是用户自己的会话记录），同秒多次切换靠目录里的 session id 区分。
const HANDOFF_FILE: &str = "handoff.md";

#[tauri::command]
pub(crate) async fn copy_session_to_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    target_provider: String,
    options: Option<HashMap<String, String>>,
) -> Result<SwitchStartedDto, String> {
    let broker = state.ptys.clone();
    let db = state.db_path.clone();
    // 文件 IO + DB + PTY spawn，全程不碰主线程消息泵（命令线程纪律）。
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db)?;
        let header = store
            .session_header(session_id)
            .map_err(|e| e.to_string())?;
        let source = meowo_agent::by_id(&header.provider).ok_or("未知来源 agent")?;
        // 来源门槛 = 现成的结构化 transcript 能力谓词，不新增能力槽也不按身份分支：
        // 没有结构化历史就没有可交接的东西（gemini/opencode 只能当复制目标）。
        let spec = source
            .telemetry()
            .and_then(|telemetry| telemetry.transcript())
            .filter(|spec| spec.supports_chat())
            .ok_or("该 Agent 不提供结构化会话记录，无法携带上下文切换")?;
        let target = meowo_agent::resolve(Some(&target_provider)).ok_or("未知目标 agent")?;
        if !target.is_installed() {
            return Err(format!("{} 未安装", target.display_name()));
        }
        let cwd = header
            .cwd
            .clone()
            .ok_or("会话缺少工作目录，无法在原目录复制")?;
        let dir = crate::terminal::validate_new_session_cwd(&cwd)?;

        // == 导出 ==
        let transcript = spec
            .resolve_transcript_path(None, header.cwd.as_deref(), &header.cc_session_id)
            .ok_or("未找到会话记录文件")?;
        let delta = meowo_agent::read_chat_delta(spec, &transcript, 0, None);
        if delta.items.is_empty() {
            return Err("会话记录为空，没有可交接的上下文".into());
        }
        let context = store
            .session_context(&header.cc_session_id)
            .unwrap_or_default();
        let meta = meowo_agent::HandoffMeta {
            from_provider_name: source.display_name(),
            to_provider_name: target.display_name(),
            cwd: Some(dir.as_str()),
            model: context.model.as_deref(),
            title: header.title.as_deref(),
        };
        let body = meowo_agent::render_history(&delta.items, &meta);
        let handoff_dir =
            meowo_agent::fsutil::handoff_root().join(format!("{}-{}", crate::now_ms(), session_id));
        std::fs::create_dir_all(&handoff_dir).map_err(|e| format!("创建交接目录失败: {e}"))?;
        let handoff_path = handoff_dir.join(HANDOFF_FILE);
        std::fs::write(&handoff_path, &body).map_err(|e| format!("写入交接文件失败: {e}"))?;

        // == 起新会话（与 new_session 同构）。刻意**不继承**旧 launch_args：不同 agent
        // 的 option id 会撞名，旧选择对新 provider 无意义甚至有害。==
        let selections = options.unwrap_or_default();
        let mut argv = target.launch_argv();
        argv.extend(meowo_agent::resolve_launch_args(
            target.launch_options(),
            &selections,
        ));
        let argv = crate::relay::augment_argv(target.id(), argv);
        let active_profile = crate::profile::active_id(target.id().as_str());
        let env = crate::terminal::launch_env_for_profile(
            Some(&target_provider),
            active_profile.as_deref(),
        );
        // 本次痛点：目标 kimi 停在「是否信任此目录」屏，对话页答不了、hook 不来、新会话不落库，
        // 看板上表现为「复制后新会话没出现」。起进程前替用户预写信任记录
        // （用户亲手选定目录与目标 agent，语义上已表达这份信任；codex 的 trusted_hash 是同一先例）。
        crate::terminal::pretrust_workspace(&target_provider, active_profile.as_deref(), &dir);
        let temp_id = broker
            .start_pending(
                app.clone(),
                &argv,
                Some(&dir),
                &env,
                100,
                30,
                target.id().as_str(),
                &selections,
                // 不串接续链：复制出来的是独立会话，原会话照常可用。
                None,
            )
            // 原会话没动过，失败只需说清楚；交接文件留在盘上，路径随报错给出，可手动喂给新 agent。
            .map_err(|e| {
                format!(
                    "启动新会话失败: {e}。交接文件已保留: {}",
                    handoff_path.display()
                )
            })?;
        crate::watch::emit_board_changed(&app, "copy_to_provider");
        Ok(SwitchStartedDto {
            temp_id,
            handoff_path: handoff_path.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 接续链回看（冷路径：侧栏链徽标的弹层用）。按时间升序返回链上各段。
#[tauri::command]
pub(crate) async fn get_session_lineage(
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
) -> Result<Vec<LineageEntryDto>, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db)?;
        let chain = store
            .session_lineage_chain(session_id)
            .map_err(|e| e.to_string())?;
        Ok(chain
            .into_iter()
            .map(|entry| LineageEntryDto {
                id: entry.id,
                provider: entry.provider,
                started_at: entry.started_at,
                ended_at: entry.ended_at,
                model: entry.model,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}
