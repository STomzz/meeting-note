//! 「会议 = 一篇 Markdown 笔记」：`/v` 音频引用、转写块、纪要段落的解析与幂等重写，
//! 以及「一键处理」（转写所有引用音频 + 生成纪要）的编排。
//!
//! # 约定（人可读、可手写、幂等）
//!
//! ```text
//! # 2026-09-22 周会
//!
//! 张三说镜像拉取超时，先换镜像站。          ← 手写内容：程序永远不动
//! /v 会议音频/2026-09-22 周会/seg_0001.wav  ← 音频引用（/v 或 /video）
//! > 🎙 转写 00:00:00–00:03:41 · seg_0001.wav ← 紧跟引用后面的 `>` 块由程序生成
//! > 张三：镜像超时的问题……
//!
//! ## 会议纪要（AI 整理）                   ← 分界线：从这里到文件末尾由 LLM 生成
//! ```
//!
//! - 引用行必须是行首（允许前导空白）+ `/v` 或 `/video` + 空白 + 路径；
//! - 路径相对 vault，也可以只写文件名（会去 `会议音频/` 下按名字找）；
//! - 紧跟在引用后面的连续 `>` 块视为**程序生成**的转写，重新处理时整块替换
//!   （自己的备注请写在引用行上方）；
//! - `## 会议纪要（AI 整理）` 取**最后一次**出现的位置作为分界，之后的内容整段重写。

use crate::audio;
use crate::audio_clip;
use crate::minutes;
use crate::models::{self, EndpointConfig, ModelConfig};
use crate::notes;
use crate::rate_limit::{
    backoff_for, is_fatal_http, RateLimiter, MAX_RETRIES, RATE_LIMIT_PER_MINUTE,
};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 会议笔记目录（新建会议的默认位置）。
pub const MEETING_DIR: &str = "会议";
/// 纪要段落的分界标题。
pub const MINUTES_HEADING: &str = "## 会议纪要（AI 整理）";
/// 新建会议时写在纪要段落里的提示（有它 = 还没生成过纪要）。
pub const MINUTES_HINT: &str = "_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_";
/// 程序生成的转写块首行标记。
pub const TRANSCRIPT_TAG: &str = "🎙 转写";
/// 音频引用关键词。
pub const REF_TOKENS: [&str; 2] = ["/v", "/video"];

/// 一条音频引用的原始信息（尚未解析路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRef {
    /// 1-based 行号
    pub line: usize,
    pub token: String,
    /// 用户在笔记里写的路径原文
    pub raw: String,
}

/// 供前端展示的音频引用（含解析结果、时长、已有转写）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRefView {
    pub line: usize,
    pub token: String,
    pub raw: String,
    /// 解析出的 vault 相对路径（找不到为空）
    pub path: String,
    pub file_name: String,
    pub exists: bool,
    pub bytes: u64,
    pub duration_ms: u64,
    pub has_transcript: bool,
    pub transcript: String,
}

/// 会议页列表项。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteBrief {
    pub note_id: String,
    pub title: String,
    pub folder: String,
    pub updated_at: i64,
    pub audio_total: usize,
    pub audio_missing: usize,
    pub transcribed: usize,
    pub has_minutes: bool,
    pub duration_ms: u64,
    pub audio_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessProgress {
    pub note_id: String,
    /// parse / transcribe / write / minutes / done
    pub phase: String,
    pub audio_total: usize,
    pub audio_done: usize,
    pub audio_skipped: usize,
    pub audio_failed: usize,
    pub current: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutcome {
    pub note_id: String,
    pub audio_total: usize,
    pub audio_done: usize,
    pub audio_skipped: usize,
    pub audio_failed: usize,
    pub transcript_chars: usize,
    pub minutes_chars: usize,
    pub model: String,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

/// 某个音频引用对应的转写文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    /// 转写块标题里的元信息，如 `00:00:00–00:03:41 · seg_0001.wav`
    pub meta: String,
    pub text: String,
}

// ---------------------------------------------------------------- 解析

/// 解析一行是否为音频引用：`/v 路径` 或 `/video 路径`。
pub fn parse_ref_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    for token in REF_TOKENS {
        let Some(rest) = trimmed.strip_prefix(token) else { continue };
        // 关键词后面必须是空白或行尾，避免把 `/video` 认成 `/v` + `ideo`
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let raw = rest.trim().trim_matches(|c| c == '`' || c == '"' || c == '\'' || c == '<' || c == '>');
        if !raw.is_empty() {
            return Some((token.to_string(), raw.to_string()));
        }
    }
    None
}

/// 扫出笔记里所有音频引用（按行号）。
pub fn parse_refs(body: &str) -> Vec<RawRef> {
    body.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            parse_ref_line(line).map(|(token, raw)| RawRef { line: i + 1, token, raw })
        })
        .collect()
}

/// vault 相对路径（统一 `/` 分隔）。
fn rel_of(vault: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(vault).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// 安全地拼接 vault 内相对路径（拒绝绝对路径、`..`、盘符）。
fn checked_join(vault: &Path, rel: &str) -> Option<(PathBuf, String)> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains(':') || rel.starts_with('~') {
        return None;
    }
    let mut out = vault.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return None;
        }
        out.push(part);
    }
    Some((out, rel.to_string()))
}

fn find_by_name(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth > 3 || !dir.is_dir() {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if entry.file_name().to_string_lossy() == name {
            return Some(path);
        }
    }
    for sub in subdirs {
        if let Some(hit) = find_by_name(&sub, name, depth + 1) {
            return Some(hit);
        }
    }
    None
}

/// 解析引用路径 → (绝对路径, vault 相对路径)。
///
/// 支持：① 相对 vault 的路径（`会议音频/xx/seg_0001.wav`）；② 裸文件名（先去 `会议音频/` 里找，再试 vault 根）。
pub fn resolve_path(vault: &Path, raw: &str) -> Option<(PathBuf, String)> {
    let cleaned = raw.trim().trim_start_matches("./");
    if cleaned.is_empty() {
        return None;
    }
    let normalized = cleaned.replace('\\', "/");
    if normalized.contains('/') {
        let (abs, rel) = checked_join(vault, &normalized)?;
        if abs.is_file() {
            return Some((abs, rel));
        }
        return None;
    }
    if let Some(abs) = find_by_name(&vault.join(audio_clip::AUDIO_DIR), &normalized, 0) {
        if let Some(rel) = rel_of(vault, &abs) {
            return Some((abs, rel));
        }
    }
    let (abs, rel) = checked_join(vault, &normalized)?;
    if abs.is_file() {
        return Some((abs, rel));
    }
    None
}

/// 从行数组里取出 `idx` 之后紧跟的 `>` 块（若存在）→ (结束行下标（不含）, 文本)。
fn block_after(lines: &[&str], idx: usize) -> Option<(usize, String)> {
    let start = idx + 1;
    if start >= lines.len() || !lines[start].trim_start().starts_with('>') {
        return None;
    }
    let mut end = start;
    while end < lines.len() && lines[end].trim_start().starts_with('>') {
        end += 1;
    }
    let text = lines[start..end]
        .iter()
        .map(|l| l.trim_start().trim_start_matches('>').trim_start_matches(' '))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    Some((end, text))
}

/// 扫出所有引用及其转写块（供前端展示与处理复用）。
pub fn refs_of(vault: &Path, body: &str) -> Vec<AudioRefView> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some((token, raw)) = parse_ref_line(line) else { continue };
        let resolved = resolve_path(vault, &raw);
        let (path, exists, bytes, duration_ms) = match &resolved {
            Some((abs, rel)) => {
                let bytes = std::fs::metadata(abs).map(|m| m.len()).unwrap_or(0);
                let (_, dur) = audio_clip::wav_meta(abs);
                (rel.clone(), true, bytes, dur)
            }
            None => (String::new(), false, 0, 0),
        };
        let transcript = block_after(&lines, i)
            .map(|(_, t)| {
                t.lines()
                    .filter(|l| !l.contains(TRANSCRIPT_TAG))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        let has_transcript = !transcript.is_empty();
        out.push(AudioRefView {
            line: i + 1,
            token,
            raw,
            file_name: resolved
                .as_ref()
                .and_then(|(abs, _)| abs.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_default(),
            path,
            exists,
            bytes,
            duration_ms,
            has_transcript,
            transcript,
        });
    }
    out
}

// ------------------------------------------------------- 转写块 / 纪要段

fn transcript_lines(entry: &TranscriptEntry) -> Vec<String> {
    let mut out = vec![format!("> {TRANSCRIPT_TAG} {}", entry.meta.trim()), ">".to_string()];
    for line in entry.text.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            out.push(">".to_string());
        } else {
            out.push(format!("> {t}"));
        }
    }
    out
}

/// 幂等写入转写：为 `entries` 里出现的每个引用行插入/替换其后的 `>` 块。
///
/// 不在 `entries` 里的引用保持原样（比如这次处理失败的音频，之前的转写不会被清掉）。
pub fn replace_transcripts(body: &str, entries: &BTreeMap<usize, TranscriptEntry>) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + entries.len() * 4);
    let mut i = 0usize;
    while i < lines.len() {
        out.push(lines[i].to_string());
        let Some((_, _raw)) = parse_ref_line(lines[i]) else {
            i += 1;
            continue;
        };
        let line_no = i + 1;
        // 暂存旧的转写块
        let mut j = i + 1;
        let mut existing: Vec<String> = Vec::new();
        while j < lines.len() && lines[j].trim_start().starts_with('>') {
            existing.push(lines[j].to_string());
            j += 1;
        }
        match entries.get(&line_no) {
            Some(entry) if !entry.text.trim().is_empty() => out.extend(transcript_lines(entry)),
            // 处理了但没识别出内容 → 清掉旧块（避免留下过期文本）
            Some(_) => {}
            // 本次没处理这条引用 → 旧块原样保留
            None => out.extend(existing),
        }
        i = j;
    }
    let mut s = out.join("\n");
    if body.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 切出「正文」与「已有纪要」：以最后一次出现 `## 会议纪要（AI 整理）` 为界。
pub fn split_minutes(body: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = body.lines().collect();
    let idx = lines.iter().rposition(|l| l.trim() == MINUTES_HEADING);
    match idx {
        None => (body.trim_end().to_string(), None),
        Some(i) => {
            let head = lines[..i].join("\n").trim_end().to_string();
            let tail = lines[i..].join("\n").trim_end().to_string();
            (head, Some(tail))
        }
    }
}

/// 纪要段是否有真实内容（不是新建时的占位提示）。
pub fn minutes_ready(minutes: Option<&str>) -> bool {
    match minutes {
        None => false,
        Some(t) => {
            let body: String = t
                .lines()
                .filter(|l| l.trim() != MINUTES_HEADING)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            !body.is_empty() && !body.contains(MINUTES_HINT.trim())
        }
    }
}

/// 写回纪要段（保留正文，替换旧纪要）。
pub fn upsert_minutes(body: &str, minutes_md: &str) -> String {
    let (head, _) = split_minutes(body);
    let mut out = head.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(MINUTES_HEADING);
    out.push_str("\n\n");
    out.push_str(minutes_md.trim());
    out.push('\n');
    out
}

/// 处理时喂给对话模型的「手写正文」：去掉音频引用行、转写块与纪要段。
pub fn handwritten_text(body: &str) -> String {
    let (head, _) = split_minutes(body);
    let lines: Vec<&str> = head.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0usize;
    while i < lines.len() {
        if parse_ref_line(lines[i]).is_some() {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_start().starts_with('>') {
                j += 1;
            }
            i = j;
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n").trim().to_string()
}

/// 汇总所有转写文本（作为纪要输入的第二部分）。
pub fn transcripts_text(entries: &BTreeMap<usize, TranscriptEntry>) -> String {
    let mut out: Vec<String> = Vec::new();
    for (n, (_, entry)) in entries.iter().enumerate() {
        if entry.text.trim().is_empty() {
            continue;
        }
        out.push(format!("【录音 {}】{}\n{}", n + 1, entry.meta.trim(), entry.text.trim()));
    }
    out.join("\n\n")
}

/// 新建会议笔记的骨架。
pub fn skeleton(title: &str, date: &str) -> String {
    format!(
        "# {}\n\n> {} 会议记录：随手写下要点；用 `/v 音频文件名` 引用录音（录音面板会自动插入）。\n\n\n{}\n\n{}\n",
        title.trim(),
        date.trim(),
        MINUTES_HEADING,
        MINUTES_HINT
    )
}

// ------------------------------------------------------------ 转写缓存

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_stamp(abs: &Path) -> (i64, u64) {
    match std::fs::metadata(abs) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (mtime, m.len())
        }
        Err(_) => (0, 0),
    }
}

/// 读缓存：路径 + mtime + size + ASR 模型全一致才算命中。
pub fn load_cache(
    conn: &Connection,
    rel: &str,
    stamp: (i64, u64),
    asr_model: &str,
    force: bool,
) -> Result<Option<String>> {
    if force {
        return Ok(None);
    }
    let row: Option<(String, i64, u64, String)> = conn
        .query_row(
            "SELECT text, mtime, size, asr_model FROM audio_transcripts WHERE path = ?1",
            rusqlite::params![rel],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(match row {
        Some((text, mtime, size, model))
            if mtime == stamp.0
                && size == stamp.1
                && model == asr_model
                && !text.trim().is_empty() =>
        {
            Some(text)
        }
        _ => None,
    })
}

pub fn save_cache(
    conn: &Connection,
    rel: &str,
    stamp: (i64, u64),
    asr_model: &str,
    text: &str,
    error: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO audio_transcripts(path, mtime, size, asr_model, text, error, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(path) DO UPDATE SET
             mtime = excluded.mtime,
             size = excluded.size,
             asr_model = excluded.asr_model,
             text = excluded.text,
             error = excluded.error,
             updated_at = excluded.updated_at",
        rusqlite::params![rel, stamp.0, stamp.1, asr_model, text, error, now_secs()],
    )?;
    Ok(())
}

// ------------------------------------------------------------ 转写编排

fn mmss(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// 转写一个音频文件（静音切段 + 串行限流 + 重试），返回带时间戳的文本。
async fn transcribe_clip(
    asr_cfg: &EndpointConfig,
    bytes: &[u8],
    limiter: &mut RateLimiter,
    cancel: &AtomicBool,
    on_msg: &mut (dyn FnMut(String) + Send),
) -> Result<String> {
    let pcm = audio::normalize_to_16k_mono(bytes)?;
    let chunks = audio::split_speech_pcm(&pcm);
    if chunks.is_empty() {
        return Ok(String::new());
    }
    let total = chunks.len();
    let mut lines: Vec<String> = Vec::new();
    for (i, chunk) in chunks.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("已取消");
        }
        let mut attempt = 1usize;
        let mut last_err = String::new();
        let mut text: Option<String> = None;
        while attempt <= MAX_RETRIES {
            let waited = limiter.acquire().await;
            if waited > Duration::from_secs(1) {
                on_msg(format!(
                    "限流等待 {}s（上游 {} 请求/分钟）",
                    waited.as_secs(),
                    RATE_LIMIT_PER_MINUTE
                ));
            }
            match models::transcribe(asr_cfg, chunk.wav.clone(), "chunk.wav").await {
                Ok(t) => {
                    text = Some(t);
                    break;
                }
                Err(e) => {
                    last_err = e.to_string();
                    if is_fatal_http(&last_err) || attempt == MAX_RETRIES {
                        break;
                    }
                    let backoff = backoff_for(&last_err, attempt);
                    on_msg(format!(
                        "第 {} 次失败，{}s 后重试：{}",
                        attempt,
                        backoff.as_secs(),
                        last_err.chars().take(80).collect::<String>()
                    ));
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }
        let Some(t) = text else {
            anyhow::bail!("转写失败：{last_err}");
        };
        let t = t.trim().to_string();
        if !t.is_empty() {
            lines.push(format!("[{}] {}", mmss(chunk.start_ms), t));
        }
        on_msg(format!("转写中 {}/{} 段", i + 1, total));
    }
    Ok(lines.join("\n"))
}

/// 一键处理：转写笔记里引用的所有音频 → 写回转写块 → 生成纪要段。
///
/// 手写内容与纪要段的旧内容不会被当作文本输入；转写缓存按文件戳命中，重跑不重复烧 ASR。
pub async fn process(
    conn: &Mutex<Connection>,
    vault: &Path,
    cfg: &ModelConfig,
    note_id: &str,
    force: bool,
    progress: &(dyn Fn(ProcessProgress) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<ProcessOutcome> {
    let started = Instant::now();
    let mut out = ProcessOutcome { note_id: note_id.to_string(), ..Default::default() };
    let asr_cfg = cfg
        .asr
        .clone()
        .ok_or_else(|| anyhow!("未配置语音转写模型：请到「设置」里填写转写端点"))?;

    let body = notes::read_note(vault, note_id)?;
    let title = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        c.query_row(
            "SELECT title FROM notes WHERE id = ?1",
            rusqlite::params![note_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| {
            note_id.trim_end_matches(".md").rsplit('/').next().unwrap_or("会议").to_string()
        })
    };

    let refs = parse_refs(&body);
    out.audio_total = refs.len();

    let note = note_id.to_string();
    let emit = |phase: &str,
                message: String,
                current: &str,
                done: usize,
                skipped: usize,
                failed: usize| {
        progress(ProcessProgress {
            note_id: note.clone(),
            phase: phase.to_string(),
            audio_total: refs.len(),
            audio_done: done,
            audio_skipped: skipped,
            audio_failed: failed,
            current: current.to_string(),
            message,
        });
    };
    emit(
        "parse",
        format!("找到 {} 处音频引用", refs.len()),
        "",
        out.audio_done,
        out.audio_skipped,
        out.audio_failed,
    );

    let mut entries: BTreeMap<usize, TranscriptEntry> = BTreeMap::new();
    let mut errors: Vec<String> = Vec::new();
    let mut limiter = RateLimiter::new(RATE_LIMIT_PER_MINUTE, Duration::from_secs(60));
    let mut cancelled = false;

    for (idx, r) in refs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let Some((abs, rel)) = resolve_path(vault, &r.raw) else {
            out.audio_failed += 1;
            errors.push(format!("第 {} 行：找不到音频「{}」", r.line, r.raw));
            emit(
                "transcribe",
                format!("第 {} 行找不到音频：{}", r.line, r.raw),
                &r.raw,
                out.audio_done,
                out.audio_skipped,
                out.audio_failed,
            );
            continue;
        };
        let file_name = abs.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stamp = file_stamp(&abs);
        let cached = {
            let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
            load_cache(&c, &rel, stamp, &asr_cfg.model, force)?
        };
        let (text, skipped) = match cached {
            Some(t) => (t, true),
            None => {
                emit(
                    "transcribe",
                    format!("转写 {}（{} / {}）", file_name, idx + 1, refs.len()),
                    &file_name,
                    out.audio_done,
                    out.audio_skipped,
                    out.audio_failed,
                );
                let bytes = std::fs::read(&abs)
                    .with_context(|| format!("读取音频失败: {}", abs.display()))?;
                let done = out.audio_done;
                let skipped_now = out.audio_skipped;
                let failed = out.audio_failed;
                let total = refs.len();
                let note_c = note.clone();
                let mut on_msg = |m: String| {
                    progress(ProcessProgress {
                        note_id: note_c.clone(),
                        phase: "transcribe".to_string(),
                        audio_total: total,
                        audio_done: done,
                        audio_skipped: skipped_now,
                        audio_failed: failed,
                        current: file_name.clone(),
                        message: m,
                    });
                };
                match transcribe_clip(&asr_cfg, &bytes, &mut limiter, cancel, &mut on_msg).await {
                    Ok(t) => {
                        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
                        save_cache(&c, &rel, file_stamp(&abs), &asr_cfg.model, &t, "")?;
                        (t, false)
                    }
                    Err(e) => {
                        if cancel.load(Ordering::Relaxed) {
                            cancelled = true;
                            break;
                        }
                        out.audio_failed += 1;
                        errors.push(format!("第 {} 行（{}）：{}", r.line, file_name, e));
                        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
                        let _ = save_cache(
                            &c,
                            &rel,
                            stamp,
                            &asr_cfg.model,
                            "",
                            &e.to_string(),
                        );
                        emit(
                            "transcribe",
                            format!("转写失败：{file_name}"),
                            &file_name,
                            out.audio_done,
                            out.audio_skipped,
                            out.audio_failed,
                        );
                        continue;
                    }
                }
            }
        };
        if skipped {
            out.audio_skipped += 1;
        } else {
            out.audio_done += 1;
        }
        let dur = audio_clip::wav_meta(&abs).1;
        entries.insert(
            r.line,
            TranscriptEntry {
                meta: format!("{}–{} · {}", mmss(0), mmss(dur), file_name),
                text,
            },
        );
        emit(
            "transcribe",
            if skipped {
                format!("复用缓存：{}（不重复调用转写）", file_name)
            } else {
                format!("完成 {} / {}", idx + 1, refs.len())
            },
            &file_name,
            out.audio_done,
            out.audio_skipped,
            out.audio_failed,
        );
    }

    out.transcript_chars = entries.values().map(|e| e.text.chars().count()).sum();

    // ① 先把转写写回笔记（即使后面纪要失败，转写也已落盘）
    let with_transcripts = replace_transcripts(&body, &entries);
    if with_transcripts != body {
        emit(
            "write",
            "写回转写文本…".to_string(),
            "",
            out.audio_done,
            out.audio_skipped,
            out.audio_failed,
        );
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        notes::write_note(&c, vault, note_id, &with_transcripts)?;
    }

    // ② 生成纪要
    if !cancelled {
        let chat_cfg = cfg.chat.clone();
        match chat_cfg {
            None => errors.push("未配置对话模型，已跳过纪要生成（转写已保存）".to_string()),
            Some(chat_cfg) => {
                let hand = handwritten_text(&with_transcripts);
                let transcript = transcripts_text(&entries);
                if hand.trim().is_empty() && transcript.trim().is_empty() {
                    errors.push(
                        "笔记里没有可整理的内容：既没有手写记录，也没有成功的转写".to_string(),
                    );
                } else {
                    emit(
                        "minutes",
                        "生成会议纪要…".to_string(),
                        "",
                        out.audio_done,
                        out.audio_skipped,
                        out.audio_failed,
                    );
                    match minutes::generate_from_text(&chat_cfg, &title, &hand, &transcript).await
                    {
                        Ok(m) => {
                            let final_body = upsert_minutes(&with_transcripts, &m.markdown);
                            let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
                            notes::write_note(&c, vault, note_id, &final_body)?;
                            out.minutes_chars = m.markdown.chars().count();
                            out.model = m.model.clone();
                            emit(
                                "done",
                                format!("纪要生成完成（{}ms，模型 {}）", m.elapsed_ms, m.model),
                                "",
                                out.audio_done,
                                out.audio_skipped,
                                out.audio_failed,
                            );
                        }
                        Err(e) => errors.push(format!("纪要生成失败：{e}")),
                    }
                }
            }
        }
    }

    out.cancelled = cancelled;
    out.errors = errors;
    out.elapsed_ms = started.elapsed().as_millis() as u64;
    emit(
        "done",
        if out.cancelled {
            "已取消".to_string()
        } else {
            format!(
                "处理结束：转写 {} 段（复用 {}、失败 {}），用时 {:.1}s",
                out.audio_done,
                out.audio_skipped,
                out.audio_failed,
                out.elapsed_ms as f64 / 1000.0
            )
        },
        "",
        out.audio_done,
        out.audio_skipped,
        out.audio_failed,
    );
    Ok(out)
}

// ------------------------------------------------------------ 会议列表

fn brief_of(vault: &Path, note_id: &str, title: &str, folder: &str, updated_at: i64) -> MeetingNoteBrief {
    let body = notes::read_note(vault, note_id).unwrap_or_default();
    let refs = refs_of(vault, &body);
    let (_, minutes) = split_minutes(&body);
    MeetingNoteBrief {
        note_id: note_id.to_string(),
        title: title.to_string(),
        folder: folder.to_string(),
        updated_at,
        audio_total: refs.len(),
        audio_missing: refs.iter().filter(|r| !r.exists).count(),
        transcribed: refs.iter().filter(|r| r.has_transcript).count(),
        has_minutes: minutes_ready(minutes.as_deref()),
        duration_ms: refs.iter().map(|r| r.duration_ms).sum(),
        audio_bytes: refs.iter().map(|r| r.bytes).sum(),
    }
}

/// 会议页列表：`会议/` 下的笔记；`scan_all = true` 时额外把其它目录里含 `/v` 的笔记也列出来。
pub fn list_notes(conn: &Connection, vault: &Path, scan_all: bool) -> Result<Vec<MeetingNoteBrief>> {
    let mut stmt = conn.prepare("SELECT id, title, folder, updated_at FROM notes ORDER BY updated_at DESC")?;
    let rows: Vec<(String, String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let prefix = format!("{MEETING_DIR}/");
    let mut out: Vec<MeetingNoteBrief> = Vec::new();
    for (id, title, folder, updated_at) in rows {
        let in_meeting_dir = id.starts_with(&prefix) || id == format!("{MEETING_DIR}.md");
        if !in_meeting_dir {
            if !scan_all {
                continue;
            }
            let body = notes::read_note(vault, &id).unwrap_or_default();
            if parse_refs(&body).is_empty() {
                continue;
            }
        }
        out.push(brief_of(vault, &id, &title, &folder, updated_at));
    }
    Ok(out)
}

/// 新建会议笔记：`会议/<日期>-<标题>.md`，返回笔记 id。
pub fn create_note(
    conn: &Connection,
    vault: &Path,
    title: &str,
    date: &str,
) -> Result<(String, String)> {
    let title = title.trim();
    if title.is_empty() {
        return Err(anyhow!("请填写会议标题"));
    }
    let dir = vault.join(MEETING_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    let mut stem = format!("{}-{}", date.trim(), title);
    stem = stem
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    stem = stem.trim().trim_matches('.').trim().to_string();
    let mut id = format!("{MEETING_DIR}/{stem}.md");
    let mut n = 2;
    while vault.join(&id).exists() {
        id = format!("{MEETING_DIR}/{stem}-{n}.md");
        n += 1;
    }
    let body = skeleton(title, date);
    notes::write_note(conn, vault, &id, &body)?;
    Ok((id, body))
}

// ------------------------------------------------------------ 旧会议迁移

/// 把 `[hh:mm:ss]` 时间戳的转写行按分段时长分摊到各段。
fn distribute_transcript(
    lines: &[(Option<u64>, String)],
    segs: &[(u64, u64)],
) -> Vec<Vec<String>> {
    let mut buckets: Vec<Vec<String>> = vec![Vec::new(); segs.len().max(1)];
    if segs.is_empty() {
        buckets[0] = lines.iter().map(|(_, t)| t.clone()).collect();
        return buckets;
    }
    let mut last = 0usize;
    for (ms, text) in lines {
        let idx = match ms {
            None => last,
            Some(ms) => {
                let mut found = segs.len() - 1;
                for (i, (start, end)) in segs.iter().enumerate() {
                    if *ms >= *start && *ms < *end {
                        found = i;
                        break;
                    }
                    if *ms < *start {
                        found = i;
                        break;
                    }
                }
                found
            }
        };
        last = idx;
        buckets[idx].push(text.clone());
    }
    buckets
}

/// 解析带 `[hh:mm:ss]` 前缀的转写行。
fn parse_timestamped_lines(text: &str) -> Vec<(Option<u64>, String)> {
    text.lines()
        .map(|line| {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix('[') {
                if let Some((ts, body)) = rest.split_once(']') {
                    let parts: Vec<u64> = ts.split(':').filter_map(|p| p.trim().parse().ok()).collect();
                    if parts.len() == 3 {
                        let ms =
                            (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000;
                        return (Some(ms), body.trim().to_string());
                    }
                }
            }
            (None, t.to_string())
        })
        .filter(|(_, t)| !t.is_empty())
        .collect()
}

/// 一次性迁移：把旧会议（`meetings` 表）导出成 `会议/旧会议/` 下的会议笔记。
///
/// - 旧录音会**复制**（不移动、不删除）到 `会议音频/旧会议-<标题>/seg_XXXX.wav`；
/// - 旧转写按时间戳尽量分摊到各分段（识别不出时间戳就整体放在第一段下面）；
/// - 旧纪要写进 `## 会议纪要（AI 整理）` 段落；
/// - 已经存在的目标笔记会跳过，不覆盖。
pub fn export_legacy(
    conn: &Connection,
    vault: &Path,
    meetings_root: &Path,
) -> Result<Vec<String>> {
    let rows: Vec<(String, String, String, String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, transcript, minutes_md, created_at FROM meetings ORDER BY created_at",
        )?;
        let collected = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        collected
    };
    let mut out: Vec<String> = Vec::new();
    for (id, title, transcript, minutes_md, created_at) in rows {
        if transcript.trim().is_empty() && minutes_md.trim().is_empty() {
            continue;
        }
        let segs = crate::meetings::detail(conn, meetings_root, &id)
            .map(|d| d.segments)
            .unwrap_or_default();
        if segs.is_empty() && transcript.trim().is_empty() {
            continue;
        }
        let dir_name = format!("旧会议-{}", title.trim());
        let date = {
            let d = created_at.max(0);
            let days = d / 86_400;
            // 简单把 unix 秒折算成 UTC 日期（迁移用途，够用）
            let (y, m, day) = civil_from_days(days);
            format!("{y:04}-{m:02}-{day:02}")
        };
        let safe_title: String = title
            .trim()
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
            .collect();
        let note_id =
            format!("{MEETING_DIR}/旧会议/{date}-{}.md", safe_title.trim().trim_matches('.').trim());
        let exists: bool = conn
            .query_row("SELECT 1 FROM notes WHERE id = ?1", rusqlite::params![note_id], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if exists {
            continue;
        }

        // 复制录音并准备引用行
        let mut ref_lines: Vec<String> = Vec::new();
        let mut seg_ranges: Vec<(u64, u64)> = Vec::new();
        let mut cursor = 0u64;
        for seg in &segs {
            let src = crate::meetings::meeting_dir(meetings_root, &id).join(&seg.file);
            if !src.is_file() {
                continue;
            }
            let seq = audio_clip::next_seq(vault, &dir_name);
            let _ = audio_clip::start(vault, &dir_name, seq, seg.src_rate.max(1) as u32);
            let dst = audio_clip::seg_path(vault, &dir_name, seq)?;
            std::fs::copy(&src, &dst)
                .with_context(|| format!("复制旧录音失败: {}", src.display()))?;
            let _ = audio_clip::close(vault, &dir_name, seq);
            let dur = audio_clip::wav_meta(&dst).1.max(seg.duration_ms.max(0) as u64);
            seg_ranges.push((cursor, cursor + dur.max(1)));
            cursor += dur;
            ref_lines.push(format!("/v {}", audio_clip::rel_path(&dir_name, seq)));
        }

        let buckets = distribute_transcript(&parse_timestamped_lines(&transcript), &seg_ranges);
        let mut body = format!(
            "# {}\n\n> 由旧版会议迁移（{}）；原始录音已复制到 `会议音频/{}`。\n\n",
            title.trim(),
            date,
            dir_name
        );
        for (i, r) in ref_lines.iter().enumerate() {
            body.push_str(r);
            body.push('\n');
            let text = buckets.get(i).map(|b| b.join("\n")).unwrap_or_default();
            if !text.trim().is_empty() {
                let meta = if segs.len() == 1 {
                    "旧版整体转写".to_string()
                } else {
                    format!("旧版转写（第 {} 段）", i + 1)
                };
                let entry = TranscriptEntry { meta, text };
                for line in transcript_lines(&entry) {
                    body.push_str(&line);
                    body.push('\n');
                }
            }
            body.push('\n');
        }
        if ref_lines.is_empty() && !transcript.trim().is_empty() {
            body.push_str("## 旧版转写（未找到录音文件）\n\n");
            body.push_str(transcript.trim());
            body.push_str("\n\n");
        }
        body.push_str(MINUTES_HEADING);
        body.push_str("\n\n");
        if minutes_md.trim().is_empty() {
            body.push_str(MINUTES_HINT);
        } else {
            body.push_str(minutes_md.trim());
        }
        body.push('\n');

        notes::write_note(conn, vault, &note_id, &body)?;
        out.push(note_id);
    }
    Ok(out)
}

/// 由 Unix 天数算 (年, 月, 日)（Howard Hinnant 的 civil_from_days）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_vault() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_clip(vault: &Path, dir: &str, seq: i64, seconds: u64) -> String {
        let rate = 16_000u32;
        audio_clip::start(vault, dir, seq, rate).unwrap();
        let pcm = vec![0x11u8; rate as usize * 2 * seconds as usize];
        audio_clip::append(vault, dir, seq, &pcm).unwrap();
        audio_clip::close(vault, dir, seq).unwrap();
        audio_clip::rel_path(dir, seq)
    }

    #[test]
    fn parses_ref_lines() {
        assert_eq!(parse_ref_line("/v a.wav"), Some(("/v".into(), "a.wav".into())));
        assert_eq!(parse_ref_line("  /video 会议音频/x/seg_0001.wav "), Some(("/video".into(), "会议音频/x/seg_0001.wav".into())));
        assert_eq!(parse_ref_line("/v"), None);
        assert_eq!(parse_ref_line("/video"), None);
        assert_eq!(parse_ref_line("看 /v a.wav 这里"), None, "必须行首");
        assert_eq!(parse_ref_line("/very good"), None, "不能把 /very 认成 /v");
        assert_eq!(parse_ref_line("/v `a.wav`"), Some(("/v".into(), "a.wav".into())));
    }

    #[test]
    fn parses_refs_with_line_numbers() {
        let body = "# 会\n\n开头\n/v a.wav\n中间\n/video 会议音频/b/seg_0002.wav\n";
        let refs = parse_refs(body);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].line, 4);
        assert_eq!(refs[1].line, 6);
        assert_eq!(refs[1].token, "/video");
    }

    #[test]
    fn resolves_relative_and_bare_names() {
        let tmp = tmp_vault();
        let vault = tmp.path();
        write_clip(vault, "周会", 1, 1);
        let (abs, rel) = resolve_path(vault, "会议音频/周会/seg_0001.wav").unwrap();
        assert!(abs.is_file());
        assert_eq!(rel, "会议音频/周会/seg_0001.wav");

        // 裸文件名
        let (_, rel) = resolve_path(vault, "seg_0001.wav").unwrap();
        assert_eq!(rel, "会议音频/周会/seg_0001.wav");

        // 逃逸与不存在
        assert!(resolve_path(vault, "../../etc/passwd").is_none());
        assert!(resolve_path(vault, "/etc/passwd").is_none());
        assert!(resolve_path(vault, "nope.wav").is_none());
    }

    #[test]
    fn transcript_blocks_are_idempotent() {
        let body = "# 会\n\n手写一句。\n/v a.wav\n\n后面还有手写。\n";
        let mut entries = BTreeMap::new();
        entries.insert(
            4usize,
            TranscriptEntry { meta: "00:00:00–00:00:10 · a.wav".into(), text: "甲：你好\n乙：再见".into() },
        );
        let once = replace_transcripts(body, &entries);
        assert!(once.contains("> 🎙 转写 00:00:00–00:00:10 · a.wav"));
        assert!(once.contains("> 甲：你好"));
        assert!(once.contains("> 乙：再见"));
        assert!(once.contains("后面还有手写。"));
        assert!(once.contains("\n\n后面还有手写。"), "块后要保留空行");

        let twice = replace_transcripts(&once, &entries);
        assert_eq!(once, twice, "重复处理不应堆叠");
    }

    #[test]
    fn replace_only_touches_entries_and_keeps_user_text() {
        let body = "# 会\n\n手写 A\n/v a.wav\n> 🎙 转写 旧内容\n> 旧行\n\n手写 B\n/v b.wav\n> 手工写的备注（会被当转写，文档已说明）\n";
        let mut entries = BTreeMap::new();
        entries.insert(
            4usize,
            TranscriptEntry { meta: "m1".into(), text: "新内容".into() },
        );
        let out = replace_transcripts(body, &entries);
        assert!(out.contains("> 新内容"));
        assert!(!out.contains("旧内容"));
        assert!(out.contains("手写 A") && out.contains("手写 B"));
        assert!(out.contains("> 手工写的备注（会被当转写，文档已说明）"), "b.wav 未处理，其块保持原样");

        // 处理了但没识别出内容 → 清掉旧块
        let mut empty = BTreeMap::new();
        empty.insert(4usize, TranscriptEntry { meta: "m1".into(), text: "  ".into() });
        let cleared = replace_transcripts(body, &empty);
        assert!(!cleared.contains("旧内容"));
        assert!(cleared.contains("> 手工写的备注（会被当转写，文档已说明）"), "未处理的引用不受影响");
    }

    #[test]
    fn minutes_split_and_upsert() {
        let body = "# 会\n\n手写\n\n## 会议纪要（AI 整理）\n\n旧纪要\n";
        let (head, minutes) = split_minutes(body);
        assert!(head.contains("手写") && !head.contains("会议纪要"));
        assert!(minutes.as_deref().unwrap().contains("旧纪要"));
        assert!(minutes_ready(minutes.as_deref()));

        let updated = upsert_minutes(body, "# 新标题\n\n- 决议");
        assert!(updated.contains("手写"));
        assert!(updated.contains("# 新标题"));
        assert!(!updated.contains("旧纪要"));
        assert_eq!(updated.matches(MINUTES_HEADING).count(), 1);

        // 没有纪要段时直接追加
        let appended = upsert_minutes("# 会\n\n手写\n", "内容");
        assert!(appended.contains(MINUTES_HEADING) && appended.ends_with("内容\n"));

        // 占位提示不算「已生成」
        let fresh = skeleton("周会", "2026-09-22");
        let (_, m) = split_minutes(&fresh);
        assert!(!minutes_ready(m.as_deref()));
    }

    #[test]
    fn handwritten_excludes_generated_parts() {
        let body = "# 会\n\n手写 A\n/v a.wav\n> 🎙 转写 m\n> 转写内容\n\n手写 B\n\n## 会议纪要（AI 整理）\n\n纪要内容\n";
        let hand = handwritten_text(body);
        assert!(hand.contains("手写 A") && hand.contains("手写 B"));
        assert!(!hand.contains("转写内容"));
        assert!(!hand.contains("纪要内容"));
        assert!(!hand.contains("/v a.wav"));

        let mut entries = BTreeMap::new();
        entries.insert(4usize, TranscriptEntry { meta: "m".into(), text: "T1".into() });
        assert!(transcripts_text(&entries).contains("T1"));
    }

    #[test]
    fn refs_view_reports_status() {
        let tmp = tmp_vault();
        let vault = tmp.path();
        write_clip(vault, "周会", 1, 2);
        let body = "/v 会议音频/周会/seg_0001.wav\n> 🎙 转写 m\n> 甲：内容\n\n/v 缺失.wav\n";
        let refs = refs_of(vault, body);
        assert_eq!(refs.len(), 2);
        assert!(refs[0].exists);
        assert_eq!(refs[0].duration_ms, 2000);
        assert!(refs[0].has_transcript);
        assert_eq!(refs[0].transcript, "甲：内容");
        assert!(!refs[1].exists);
        assert!(!refs[1].has_transcript);
    }

    #[test]
    fn cache_round_trip_respects_stamp_and_model() {
        let conn = crate::db::open_memory().unwrap();
        let tmp = tmp_vault();
        let rel = write_clip(tmp.path(), "周会", 1, 1);
        let abs = tmp.path().join(&rel);
        let stamp = file_stamp(&abs);
        assert!(load_cache(&conn, &rel, stamp, "asr-1", false).unwrap().is_none());
        save_cache(&conn, &rel, stamp, "asr-1", "文本", "").unwrap();
        assert_eq!(load_cache(&conn, &rel, stamp, "asr-1", false).unwrap().as_deref(), Some("文本"));
        assert!(load_cache(&conn, &rel, stamp, "asr-2", false).unwrap().is_none(), "换模型要重转");
        assert!(load_cache(&conn, &rel, stamp, "asr-1", true).unwrap().is_none(), "force 要重转");
        assert!(load_cache(&conn, &rel, (stamp.0 + 5, stamp.1), "asr-1", false).unwrap().is_none(), "文件变了要重转");
    }

    #[test]
    fn create_note_makes_skeleton_and_dedupes_ids() {
        let conn = crate::db::open_memory().unwrap();
        let tmp = tmp_vault();
        let vault = tmp.path();
        let (id, body) = create_note(&conn, vault, "周会", "2026-09-22").unwrap();
        assert_eq!(id, "会议/2026-09-22-周会.md");
        assert!(body.contains("# 周会"));
        assert!(body.contains(MINUTES_HEADING));
        let (id2, _) = create_note(&conn, vault, "周会", "2026-09-22").unwrap();
        assert_eq!(id2, "会议/2026-09-22-周会-2.md");
        let (id3, _) = create_note(&conn, vault, "坏/名字:测试", "2026-09-22").unwrap();
        assert!(id3.contains("坏-名字-测试"));
    }

    #[test]
    fn list_notes_covers_meeting_dir_and_scans_others() {
        let conn = crate::db::open_memory().unwrap();
        let tmp = tmp_vault();
        let vault = tmp.path();
        create_note(&conn, vault, "周会", "2026-09-22").unwrap();
        notes::write_note(&conn, vault, "其它/随手记.md", "简介\n/v 会议音频/周会/seg_0001.wav\n").unwrap();
        notes::write_note(&conn, vault, "其它/普通笔记.md", "没有引用\n").unwrap();
        write_clip(vault, "周会", 1, 1);

        let only_meeting = list_notes(&conn, vault, false).unwrap();
        assert_eq!(only_meeting.len(), 1);
        assert_eq!(only_meeting[0].note_id, "会议/2026-09-22-周会.md");

        let all = list_notes(&conn, vault, true).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|b| b.note_id == "其它/随手记.md"));
    }

    #[test]
    fn distributes_transcript_by_timestamp() {
        let lines = parse_timestamped_lines("[00:00:05] 第一句\n[00:05:00] 第二句\n没有时间戳的收尾\n");
        assert_eq!(lines[0].0, Some(5_000));
        assert_eq!(lines[1].0, Some(300_000));
        assert_eq!(lines[2].0, None);
        let buckets = distribute_transcript(&lines, &[(0, 120_000), (120_000, 240_000), (240_000, 360_000)]);
        assert_eq!(buckets.len(), 3);
        assert!(buckets[0][0].contains("第一句"));
        assert!(buckets[2][0].contains("第二句"));
        assert!(buckets[2][1].contains("没有时间戳的收尾"), "无时间戳的行跟上一行同段");
    }

    #[test]
    fn exports_legacy_meeting_with_audio() {
        let conn = crate::db::open_memory().unwrap();
        let tmp = tmp_vault();
        let vault = tmp.path();
        let meetings_root = tmp.path().join("appdata").join("meetings");
        let m = crate::meetings::create(&conn, "老周会").unwrap();
        crate::meetings::start_segment(&conn, &meetings_root, &m.id, 1, 16_000).unwrap();
        crate::meetings::append_pcm(&conn, &meetings_root, &m.id, 1, 16_000, &vec![0u8; 32_000])
            .unwrap();
        crate::meetings::close_segment(&conn, &meetings_root, &m.id, 1).unwrap();
        crate::meetings::set_transcript(&conn, &m.id, "[00:00:01] 甲：老会议的转写").unwrap();
        crate::meetings::set_minutes(&conn, &m.id, "### 结论\n- 老纪要", "", "").unwrap();

        let exported = export_legacy(&conn, vault, &meetings_root).unwrap();
        assert_eq!(exported.len(), 1);
        let body = notes::read_note(vault, &exported[0]).unwrap();
        assert!(body.contains("# 老周会"));
        assert!(body.contains("/v 会议音频/旧会议-老周会/seg_0001.wav"), "应有音频引用：{body}");
        assert!(body.contains("甲：老会议的转写"));
        assert!(body.contains("- 老纪要"));
        assert!(vault.join("会议音频/旧会议-老周会/seg_0001.wav").is_file(), "录音应被复制进 vault");
        // 二次迁移：目标已存在 → 跳过，不重复
        assert!(export_legacy(&conn, vault, &meetings_root).unwrap().is_empty());
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
    }
}
