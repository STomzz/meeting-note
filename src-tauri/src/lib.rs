//! Tauri 命令层：薄封装，真正的逻辑都在 `bnu-core`。

use bnu_core::audio_clip::{self, ClipInfo, ClipStat};
use bnu_core::db;
use bnu_core::graph::{self, ExtractOutcome, GraphProgress, GraphSnapshot, GraphStats, NodeDetail};
use bnu_core::meeting_note::{
    self, AudioRefView, MeetingNoteBrief, ProcessOutcome, ProcessProgress,
};
use bnu_core::meetings::{self, Meeting, MeetingDetail, Segment, SegmentStat};
use bnu_core::minutes::{self, MinutesOutcome};
use bnu_core::models::{self, ModelConfig, PublicModelConfig};
use bnu_core::notes::{self, NoteMeta, ScanStats, SearchHit};
use bnu_core::qa::{self, Answer};
use bnu_core::rate_limit::{RateLimiter, RATE_LIMIT_PER_MINUTE};
use bnu_core::retrieval::{self, RetrievalStatus};
use bnu_core::rusqlite::{Connection, Result as SqlResult};
use bnu_core::secret::SecretBox;
use bnu_core::transcribe::{self, TranscribeOutcome, TranscribeProgress};
use bnu_core::vectors::{self, EmbedProgress};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

mod webview_permissions;

struct AppState {
    conn: Mutex<Connection>,
    vault: Mutex<PathBuf>,
    secret: SecretBox,
    /// 会议音频目录：`<应用数据>/meetings`
    meetings_root: PathBuf,
    cancel_transcribe: Arc<AtomicBool>,
    /// 图谱抽取的取消标志
    cancel_graph: Arc<AtomicBool>,
    /// 会议笔记「一键处理」的取消标志
    cancel_meeting_note: Arc<AtomicBool>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn read_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn write_setting(conn: &Connection, key: &str, value: &str) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 笔记 / vault
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_vault(state: State<'_, AppState>) -> String {
    state.vault.lock().unwrap().to_string_lossy().to_string()
}

#[tauri::command]
fn set_vault(
    path: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let p = PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    std::fs::create_dir_all(&p).map_err(err)?;
    if !p.is_dir() {
        return Err("不是有效的文件夹".into());
    }
    let conn = state.conn.lock().unwrap();
    write_setting(&conn, "vault", &p.to_string_lossy()).map_err(err)?;
    *state.vault.lock().unwrap() = p.clone();
    // vault 里的音频（会议录音）要用 asset 协议播放：换 vault 后重新放行目录
    let _ = app.asset_protocol_scope().allow_directory(&p, true);
    Ok(p.to_string_lossy().to_string())
}

#[tauri::command]
fn scan_vault(state: State<'_, AppState>) -> Result<ScanStats, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::scan_vault(&conn, &root).map_err(err)
}

#[tauri::command]
fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteMeta>, String> {
    let conn = state.conn.lock().unwrap();
    notes::list_notes(&conn).map_err(err)
}

#[tauri::command]
fn read_note(id: String, state: State<'_, AppState>) -> Result<String, String> {
    let root = state.vault.lock().unwrap().clone();
    notes::read_note(&root, &id).map_err(err)
}

#[tauri::command]
fn write_note(id: String, content: String, state: State<'_, AppState>) -> Result<NoteMeta, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::write_note(&conn, &root, &id, &content).map_err(err)
}

#[tauri::command]
fn create_note(folder: String, title: String, state: State<'_, AppState>) -> Result<NoteMeta, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::create_note(&conn, &root, &folder, &title).map_err(err)
}

#[tauri::command]
fn delete_note(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::delete_note(&conn, &root, &id).map_err(err)
}

#[tauri::command]
fn search_notes(
    query: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchHit>, String> {
    let conn = state.conn.lock().unwrap();
    notes::search(&conn, &query, limit.unwrap_or(30)).map_err(err)
}

// ---------------------------------------------------------------------------
// 模型配置
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_model_config(state: State<'_, AppState>) -> Result<PublicModelConfig, String> {
    let conn = state.conn.lock().unwrap();
    models::load_config(&conn, &state.secret)
        .map(|c| c.public())
        .map_err(err)
}

#[tauri::command]
fn save_model_config(input: ModelConfig, state: State<'_, AppState>) -> Result<PublicModelConfig, String> {
    let conn = state.conn.lock().unwrap();
    let mut cfg = models::load_config(&conn, &state.secret).map_err(err)?;
    cfg.merge(input);
    models::save_config(&conn, &state.secret, &cfg).map_err(err)?;
    Ok(cfg.public())
}

#[tauri::command]
async fn test_model(capability: String, state: State<'_, AppState>) -> Result<String, String> {
    let cfg = {
        let conn = state.conn.lock().unwrap();
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    models::test_capability(&capability, &cfg).await.map_err(err)
}

#[tauri::command]
async fn list_available_models(
    base_url: String,
    api_key: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let key = match api_key.filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(k),
        None => {
            let conn = state.conn.lock().unwrap();
            let cfg = models::load_config(&conn, &state.secret).map_err(err)?;
            [cfg.chat, cfg.embedding, cfg.rerank, cfg.asr]
                .into_iter()
                .flatten()
                .find(|e| e.base_url.trim_end_matches('/') == base_url.trim_end_matches('/'))
                .and_then(|e| e.api_key)
        }
    };
    let endpoint = models::EndpointConfig {
        base_url,
        model: String::new(),
        params: serde_json::Value::Null,
        api_key: key,
    };
    models::list_models(&endpoint).await.map_err(err)
}

// ---------------------------------------------------------------------------
// 检索与问答
// ---------------------------------------------------------------------------

#[tauri::command]
fn retrieval_status(state: State<'_, AppState>) -> Result<RetrievalStatus, String> {
    let conn = state.conn.lock().map_err(err)?;
    let cfg = models::load_config(&conn, &state.secret).map_err(err)?;
    retrieval::status(&conn, &cfg).map_err(err)
}

#[tauri::command]
async fn build_vector_index(
    batch: Option<usize>,
    max_chunks: Option<usize>,
    state: State<'_, AppState>,
) -> Result<EmbedProgress, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    let embedding = cfg
        .embedding
        .as_ref()
        .ok_or_else(|| "未配置嵌入模型：请到「设置」配置嵌入端点后再构建向量索引".to_string())?;
    vectors::build(
        &state.conn,
        embedding,
        batch.unwrap_or(16),
        max_chunks.unwrap_or(256),
    )
    .await
    .map_err(err)
}

#[tauri::command]
async fn ask_question(
    question: String,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Answer, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    qa::answer(&state.conn, &cfg, &question, top_k.unwrap_or(6))
        .await
        .map_err(err)
}

// ---------------------------------------------------------------------------
// 会议（录音 / 转写 / 纪要）
// ---------------------------------------------------------------------------

#[tauri::command]
fn meeting_create(title: String, state: State<'_, AppState>) -> Result<Meeting, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::create(&conn, &title).map_err(err)
}

#[tauri::command]
fn meeting_list(state: State<'_, AppState>) -> Result<Vec<Meeting>, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::list(&conn).map_err(err)
}

#[tauri::command]
fn meeting_detail(id: String, state: State<'_, AppState>) -> Result<MeetingDetail, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::detail(&conn, &state.meetings_root, &id).map_err(err)
}

#[tauri::command]
fn meeting_rename(id: String, title: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::rename(&conn, &id, &title).map_err(err)
}

/// 删除会议；`deleteFiles` 为 true 时同时删除音频目录（界面上需二次确认）。
#[tauri::command]
fn meeting_delete(id: String, delete_files: bool, state: State<'_, AppState>) -> Result<(), String> {
    let dir = {
        let conn = state.conn.lock().map_err(err)?;
        meetings::delete(&conn, &id).map_err(err)?;
        meetings::meeting_dir(&state.meetings_root, &id)
    };
    if delete_files && dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除录音目录失败: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
fn meeting_dir(id: String, state: State<'_, AppState>) -> Result<String, String> {
    let dir = meetings::ensure_dir(&state.meetings_root, &id).map_err(err)?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
fn meeting_start_segment(
    id: String,
    seq: i64,
    sample_rate: u32,
    state: State<'_, AppState>,
) -> Result<Segment, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::start_segment(&conn, &state.meetings_root, &id, seq, sample_rate).map_err(err)
}

#[tauri::command]
fn meeting_append_pcm(
    id: String,
    seq: i64,
    sample_rate: u32,
    pcm_base64: String,
    state: State<'_, AppState>,
) -> Result<SegmentStat, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::append_pcm_base64(
        &conn,
        &state.meetings_root,
        &id,
        seq,
        sample_rate,
        &pcm_base64,
    )
    .map_err(err)
}

#[tauri::command]
fn meeting_close_segment(id: String, seq: i64, state: State<'_, AppState>) -> Result<Segment, String> {
    let conn = state.conn.lock().map_err(err)?;
    meetings::close_segment(&conn, &state.meetings_root, &id, seq).map_err(err)
}

#[tauri::command]
async fn meeting_transcribe(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<TranscribeOutcome, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    state.cancel_transcribe.store(false, Ordering::Relaxed);
    let cancel = state.cancel_transcribe.clone();
    let emitter = app.clone();
    let out = transcribe::transcribe_meeting(
        &state.conn,
        &state.meetings_root,
        &cfg,
        &id,
        &move |p: TranscribeProgress| {
            let _ = emitter.emit("meeting-progress", &p);
        },
        &cancel,
    )
    .await
    .map_err(err)?;
    let _ = app.emit("meeting-transcribed", &out);
    Ok(out)
}

#[tauri::command]
fn meeting_cancel_transcribe(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_transcribe.store(true, Ordering::Relaxed);
    Ok(())
}

/// 生成纪要并把 Markdown 存进 vault（`会议纪要/<标题>-<日期>.md`），同时回写会议记录。
#[tauri::command]
async fn meeting_generate_minutes(
    id: String,
    date: Option<String>,
    state: State<'_, AppState>,
) -> Result<MinutesOutcome, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    let outcome = minutes::generate(&state.conn, &cfg, &id).await.map_err(err)?;

    let (vault, title, created_at) = {
        let conn = state.conn.lock().map_err(err)?;
        let m = meetings::get(&conn, &id).map_err(err)?;
        (state.vault.lock().map_err(err)?.clone(), m.title, m.created_at)
    };
    let date_str = date
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| meetings::ymd_from_secs(created_at));
    let note_id = format!("会议纪要/{}-{}.md", notes::safe_filename(&title), date_str);
    let json = serde_json::to_string(&outcome.minutes).unwrap_or_default();
    {
        let conn = state.conn.lock().map_err(err)?;
        notes::write_note(&conn, &vault, &note_id, &outcome.markdown).map_err(err)?;
        meetings::set_minutes(&conn, &id, &outcome.markdown, &json, &note_id).map_err(err)?;
    }
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// 会议笔记（P7）：会议 = vault 里的一篇 md；`/v` 引用录音；一键处理
// ---------------------------------------------------------------------------

/// 会议页列表（`scanAll = true` 时额外列出其它目录里含 `/v` 的笔记）。
#[tauri::command]
fn meeting_notes_list(
    scan_all: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingNoteBrief>, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let conn = state.conn.lock().map_err(err)?;
    meeting_note::list_notes(&conn, &vault, scan_all.unwrap_or(false)).map_err(err)
}

/// 新建会议笔记：`会议/<日期>-<标题>.md`，返回笔记 id。
#[tauri::command]
fn meeting_note_new(
    title: String,
    date: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let conn = state.conn.lock().map_err(err)?;
    let (id, _) = meeting_note::create_note(&conn, &vault, &title, &date).map_err(err)?;
    Ok(id)
}

/// 解析一篇会议笔记里的音频引用（含时长与已有转写）。
#[tauri::command]
fn meeting_note_refs(
    note_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AudioRefView>, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let body = notes::read_note(&vault, &note_id).map_err(err)?;
    Ok(meeting_note::refs_of(&vault, &body))
}

/// 一键处理：转写引用的音频 → 写回转写块 → 生成纪要。进度走 `meeting-note-progress`。
#[tauri::command]
async fn meeting_note_process(
    note_id: String,
    force: Option<bool>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ProcessOutcome, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    let vault = state.vault.lock().map_err(err)?.clone();
    state.cancel_meeting_note.store(false, Ordering::Relaxed);
    let cancel = state.cancel_meeting_note.clone();
    let emitter = app.clone();
    let out = meeting_note::process(
        &state.conn,
        &vault,
        &cfg,
        &note_id,
        force.unwrap_or(false),
        &move |p: ProcessProgress| {
            let _ = emitter.emit("meeting-note-progress", &p);
        },
        &cancel,
    )
    .await
    .map_err(err)?;
    let _ = app.emit("meeting-note-processed", &out);
    Ok(out)
}

#[tauri::command]
fn meeting_note_cancel(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_meeting_note.store(true, Ordering::Relaxed);
    Ok(())
}

/// 一次性迁移：把旧会议（录音 + 转写 + 纪要）导出成 `会议/旧会议/` 下的会议笔记。
#[tauri::command]
fn meeting_note_migrate_legacy(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let conn = state.conn.lock().map_err(err)?;
    meeting_note::export_legacy(&conn, &vault, &state.meetings_root).map_err(err)
}

/// 开始一段会议录音（落在 vault 的 `会议音频/<笔记名>/` 下，序号自动递增）。
#[tauri::command]
fn audio_clip_start(
    note_id: String,
    sample_rate: u32,
    state: State<'_, AppState>,
) -> Result<ClipStat, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let dir = audio_clip::dir_for_note(&note_id);
    let seq = audio_clip::next_seq(&vault, &dir);
    audio_clip::start(&vault, &dir, seq, sample_rate).map_err(err)
}

#[tauri::command]
fn audio_clip_append(
    note_id: String,
    seq: i64,
    pcm_base64: String,
    state: State<'_, AppState>,
) -> Result<ClipStat, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let dir = audio_clip::dir_for_note(&note_id);
    audio_clip::append_base64(&vault, &dir, seq, &pcm_base64).map_err(err)
}

/// 结束当前分段，返回可插入笔记的 `/v` 引用行。
#[tauri::command]
fn audio_clip_close(
    note_id: String,
    seq: i64,
    state: State<'_, AppState>,
) -> Result<ClipStat, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let dir = audio_clip::dir_for_note(&note_id);
    audio_clip::close(&vault, &dir, seq).map_err(err)
}

/// 丢弃一个分段（界面上的「重录」）。
#[tauri::command]
fn audio_clip_discard(
    note_id: String,
    seq: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let dir = audio_clip::dir_for_note(&note_id);
    audio_clip::remove(&vault, &dir, seq).map_err(err)
}

/// 列出 vault 里的录音分段（`noteId` 为空则列出全部，供 `/v` 选择器用）。
#[tauri::command]
fn audio_clip_list(
    note_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ClipInfo>, String> {
    let vault = state.vault.lock().map_err(err)?.clone();
    let mut all = audio_clip::list(&vault).map_err(err)?;
    if let Some(note_id) = note_id.filter(|s| !s.trim().is_empty()) {
        let dir = audio_clip::dir_for_note(&note_id);
        all.retain(|c| c.dir == dir);
    }
    Ok(all)
}

// ---------------------------------------------------------------------------
// 知识图谱（P4）：抽取入口 + 图数据查询
// ---------------------------------------------------------------------------

#[tauri::command]
fn graph_stats(state: State<'_, AppState>) -> Result<GraphStats, String> {
    let conn = state.conn.lock().map_err(err)?;
    graph::stats(&conn).map_err(err)
}

#[tauri::command]
fn graph_snapshot(
    limit: Option<usize>,
    min_degree: Option<usize>,
    state: State<'_, AppState>,
) -> Result<GraphSnapshot, String> {
    let conn = state.conn.lock().map_err(err)?;
    graph::snapshot(&conn, limit.unwrap_or(1500), min_degree.unwrap_or(0)).map_err(err)
}

#[tauri::command]
fn graph_node_detail(entity_id: i64, state: State<'_, AppState>) -> Result<NodeDetail, String> {
    let conn = state.conn.lock().map_err(err)?;
    graph::node_detail(&conn, entity_id).map_err(err)
}

/// 抽取全部笔记（增量；`force = true` 时忽略哈希全量重抽）。
/// 进度走 `graph-progress` 事件，完成后另发 `graph-extracted`。
#[tauri::command]
async fn graph_extract_all(
    force: Option<bool>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ExtractOutcome, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    state.cancel_graph.store(false, Ordering::Relaxed);
    let cancel = state.cancel_graph.clone();
    let emitter = app.clone();
    let out = graph::extract_all(
        &state.conn,
        &cfg,
        force.unwrap_or(false),
        &move |p: GraphProgress| {
            let _ = emitter.emit("graph-progress", &p);
        },
        &cancel,
    )
    .await
    .map_err(err)?;
    let _ = app.emit("graph-extracted", &out);
    Ok(out)
}

/// 抽取单篇笔记（`force = false` 时未改动直接跳过）。
#[tauri::command]
async fn graph_extract_note(
    note_id: String,
    force: Option<bool>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ExtractOutcome, String> {
    let cfg = {
        let conn = state.conn.lock().map_err(err)?;
        models::load_config(&conn, &state.secret).map_err(err)?
    };
    state.cancel_graph.store(false, Ordering::Relaxed);
    let cancel = state.cancel_graph.clone();
    let emitter = app.clone();
    let mut limiter = RateLimiter::new(RATE_LIMIT_PER_MINUTE, std::time::Duration::from_secs(60));
    let mut json_mode = true;
    let out = graph::extract_note(
        &state.conn,
        &cfg,
        &note_id,
        force.unwrap_or(false),
        &mut limiter,
        &mut json_mode,
        &move |p: GraphProgress| {
            let _ = emitter.emit("graph-progress", &p);
        },
        &cancel,
    )
    .await
    .map_err(err)?;
    let _ = app.emit("graph-extracted", &out);
    Ok(out)
}

#[tauri::command]
fn graph_cancel_extract(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_graph.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 路径解析在不同平台可能失败（如 WSL 里没有 XDG 文档目录），逐级兜底
            let data_dir = app
                .path()
                .app_data_dir()
                .or_else(|_| app.path().home_dir().map(|h| h.join(".bnu-notes")))
                .map_err(|e| format!("无法确定应用数据目录: {e}"))?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("index.sqlite"))?;
            let secret = SecretBox::load_or_create(&data_dir.join("secret.key"))?;
            let meetings_root = data_dir.join("meetings");
            std::fs::create_dir_all(&meetings_root)?;

            let default_vault = app
                .path()
                .document_dir()
                .or_else(|_| app.path().home_dir())
                .map(|dir| dir.join("BNU-Notes"))
                .unwrap_or_else(|_| data_dir.join("vault"));
            std::fs::create_dir_all(&default_vault)?;
            let vault = read_setting(&conn, "vault")
                .map(PathBuf::from)
                .unwrap_or(default_vault);
            std::fs::create_dir_all(&vault).ok();

            // vault 里的录音要用 asset 协议播放，而静态 scope 只放行了应用数据目录
            if let Err(e) = app.asset_protocol_scope().allow_directory(&vault, true) {
                eprintln!("[bnu-notes] 放行 vault 资源目录失败: {e}");
            }

            app.manage(AppState {
                conn: Mutex::new(conn),
                vault: Mutex::new(vault),
                secret,
                meetings_root,
                cancel_transcribe: Arc::new(AtomicBool::new(false)),
                cancel_graph: Arc::new(AtomicBool::new(false)),
                cancel_meeting_note: Arc::new(AtomicBool::new(false)),
            });

            // Windows(WebView2)：显式放行本应用页面的麦克风/摄像头，否则窗口里的
            // getUserMedia 可能被引擎静默拒绝（Linux/Android 走各自平台默认路径）。
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = webview_permissions::grant_media_capture(&window) {
                    eprintln!("[bnu-notes] 媒体权限预置失败: {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_vault,
            set_vault,
            scan_vault,
            list_notes,
            read_note,
            write_note,
            create_note,
            delete_note,
            search_notes,
            get_model_config,
            save_model_config,
            test_model,
            list_available_models,
            retrieval_status,
            build_vector_index,
            ask_question,
            meeting_create,
            meeting_list,
            meeting_detail,
            meeting_rename,
            meeting_delete,
            meeting_dir,
            meeting_start_segment,
            meeting_append_pcm,
            meeting_close_segment,
            meeting_transcribe,
            meeting_cancel_transcribe,
            meeting_generate_minutes,
            meeting_notes_list,
            meeting_note_new,
            meeting_note_refs,
            meeting_note_process,
            meeting_note_cancel,
            meeting_note_migrate_legacy,
            audio_clip_start,
            audio_clip_append,
            audio_clip_close,
            audio_clip_discard,
            audio_clip_list,
            graph_stats,
            graph_snapshot,
            graph_node_detail,
            graph_extract_all,
            graph_extract_note,
            graph_cancel_extract
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
