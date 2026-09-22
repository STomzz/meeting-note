//! vault 内的录音片段：`<vault>/会议音频/<目录>/seg_0001.wav`
//!
//! 与旧会议模块（应用数据目录 + `meetings` 表）不同，这里**文件即数据**：
//! - 目录名由会议笔记的文件名推导（前端只传 `noteId`，避免两边各写一套规则）；
//! - 分段按序号命名，序号 = 目录里已有的最大值 + 1；
//! - 时长直接读 WAV 头算，不落库；
//! - 所有路径都限制在 vault 内（拒绝绝对路径与 `..`）。

use crate::audio;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// vault 内的音频根目录。
pub const AUDIO_DIR: &str = "会议音频";
/// 分段文件前缀。
pub const SEG_PREFIX: &str = "seg_";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipInfo {
    /// 相对 `会议音频/` 的目录（一般对应一篇会议笔记）
    pub dir: String,
    pub file: String,
    /// vault 相对路径（可直接写进 `/v` 引用）
    pub path: String,
    pub seq: i64,
    pub bytes: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub modified_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipStat {
    pub dir: String,
    pub file: String,
    pub path: String,
    pub seq: i64,
    pub bytes: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    /// 可直接插入笔记的引用行，如 `/v 会议音频/周会/seg_0001.wav`
    pub ref_line: String,
}

/// 会议笔记 id → 音频目录名：去掉 `会议/` 前缀与 `.md`，路径分隔换成 `_`。
pub fn dir_for_note(note_id: &str) -> String {
    let id = note_id.trim().trim_start_matches('/');
    let stem = id.strip_suffix(".md").unwrap_or(id);
    let stem = stem.strip_prefix("会议/").unwrap_or(stem);
    let mut s: String =
        stem.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    s = s.trim().trim_matches('.').trim().to_string();
    if s.is_empty() {
        return "未命名会议".to_string();
    }
    s.chars().take(60).collect()
}

/// 相对目录的安全校验：只允许 vault 内的相对路径。
fn safe_rel(rel: &str) -> Result<String> {
    let trimmed = rel.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("音频目录不能为空"));
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with('~')
        || trimmed.contains(':')
    {
        return Err(anyhow!("音频目录必须是 vault 内的相对路径：{trimmed}"));
    }
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(anyhow!("音频目录非法：{trimmed}"));
        }
        if part.contains('\\') {
            return Err(anyhow!("音频目录不能包含反斜杠：{trimmed}"));
        }
    }
    Ok(trimmed.to_string())
}

/// 某目录的绝对路径（按需创建）。
pub fn clip_dir(vault: &Path, dir: &str, create: bool) -> Result<PathBuf> {
    let path = vault.join(AUDIO_DIR).join(safe_rel(dir)?);
    if create {
        std::fs::create_dir_all(&path)
            .with_context(|| format!("创建音频目录失败: {}", path.display()))?;
    }
    Ok(path)
}

pub fn seg_file(seq: i64) -> String {
    format!("{SEG_PREFIX}{seq:04}.wav")
}

pub fn seg_path(vault: &Path, dir: &str, seq: i64) -> Result<PathBuf> {
    Ok(clip_dir(vault, dir, false)?.join(seg_file(seq)))
}

/// vault 相对路径（统一 `/` 分隔）→ 用于写入 `/v` 引用与转写缓存键。
pub fn rel_path(dir: &str, seq: i64) -> String {
    format!("{AUDIO_DIR}/{}/{}", safe_rel(dir).unwrap_or_default(), seg_file(seq))
}

/// 下一个可用序号（目录不存在 → 1）。
pub fn next_seq(vault: &Path, dir: &str) -> i64 {
    let Ok(path) = clip_dir(vault, dir, false) else { return 1 };
    let Ok(rd) = std::fs::read_dir(&path) else { return 1 };
    let mut max = 0i64;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(rest) = name.strip_prefix(SEG_PREFIX) {
            if let Some(num) = rest.strip_suffix(".wav") {
                if let Ok(n) = num.parse::<i64>() {
                    max = max.max(n);
                }
            }
        }
    }
    max + 1
}

fn stat_of(vault: &Path, dir: &str, seq: i64) -> Result<ClipStat> {
    let path = seg_path(vault, dir, seq)?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let (sample_rate, duration_ms) = wav_meta(&path);
    Ok(ClipStat {
        dir: safe_rel(dir)?,
        file: seg_file(seq),
        path: rel_path(dir, seq),
        seq,
        bytes,
        duration_ms,
        sample_rate,
        ref_line: format!("/v {}", rel_path(dir, seq)),
    })
}

/// 读 WAV 头拿采样率与时长（文件缺失/损坏 → (0, 0)）。
///
/// **只读文件头**（4 KB），不把整个 WAV 读进内存——会议页列表/引用解析会对每个分段调用它，
/// 一场 1 小时会议的音频上百 MB，整读代价太大。
/// 头里的 data 长度是 0（录音尚未收尾）或与实际文件大小不符时，按文件大小估算。
pub fn wav_meta(path: &Path) -> (u32, u64) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (0, 0);
    };
    let mut head = [0u8; 4096];
    let n = match std::io::Read::read(&mut file, &mut head) {
        Ok(n) => n,
        Err(_) => return (0, 0),
    };
    let Some(info) = crate::audio::parse_wav_head(&head[..n]) else {
        return (0, 0);
    };
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let actual = file_size.saturating_sub(info.data_offset as u64);
    let data_len = if info.data_length > 0 {
        (info.data_length as u64).min(actual)
    } else {
        actual
    };
    (
        info.sample_rate,
        data_len * 1000 / (info.byte_rate.max(1) as u64),
    )
}

/// 开始一个分段：写占位 WAV 头（长度在 close 时回写）。
pub fn start(vault: &Path, dir: &str, seq: i64, sample_rate: u32) -> Result<ClipStat> {
    clip_dir(vault, dir, true)?;
    let path = seg_path(vault, dir, seq)?;
    if !path.exists() {
        let header = audio::build_wav(&[], sample_rate, 1, 16);
        std::fs::write(&path, &header)?;
    }
    stat_of(vault, dir, seq)
}

/// 追加 PCM（16bit 小端、单声道、采集采样率）。
pub fn append(vault: &Path, dir: &str, seq: i64, pcm: &[u8]) -> Result<ClipStat> {
    if pcm.is_empty() {
        return stat_of(vault, dir, seq);
    }
    let path = seg_path(vault, dir, seq)?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .with_context(|| format!("打开录音分段失败: {}", path.display()))?;
    file.write_all(pcm)?;
    file.flush()?;
    stat_of(vault, dir, seq)
}

/// 关闭分段：回写 RIFF/data 长度。
pub fn close(vault: &Path, dir: &str, seq: i64) -> Result<ClipStat> {
    let path = seg_path(vault, dir, seq)?;
    if path.exists() {
        let total = std::fs::metadata(&path)?.len();
        let data_len = total.saturating_sub(44) as u32;
        let mut file = OpenOptions::new().write(true).open(&path)?;
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&(36u32 + data_len).to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data_len.to_le_bytes())?;
        file.flush()?;
    }
    stat_of(vault, dir, seq)
}

/// 追加 base64 编码的 PCM（Tauri IPC 传输用）。
pub fn append_base64(vault: &Path, dir: &str, seq: i64, pcm_base64: &str) -> Result<ClipStat> {
    use base64::Engine;
    let pcm = base64::engine::general_purpose::STANDARD
        .decode(pcm_base64.trim())
        .map_err(|e| anyhow!("PCM base64 解码失败: {e}"))?;
    append(vault, dir, seq, &pcm)
}

/// 丢弃一个分段（用户在界面上主动「重录 / 删除这一段」时调用）。
pub fn remove(vault: &Path, dir: &str, seq: i64) -> Result<()> {
    let path = seg_path(vault, dir, seq)?;
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("删除录音分段失败: {}", path.display()))?;
    }
    Ok(())
}

/// 列出 `会议音频/` 下的所有分段（深度 ≤ 3，按目录+序号排序）。
pub fn list(vault: &Path) -> Result<Vec<ClipInfo>> {
    let root = vault.join(AUDIO_DIR);
    let mut out: Vec<ClipInfo> = Vec::new();
    if root.is_dir() {
        walk(&root, &root, 0, &mut out)?;
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir).then(a.seq.cmp(&b.seq)));
    Ok(out)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<ClipInfo>) -> Result<()> {
    if depth > 3 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, depth + 1, out)?;
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(SEG_PREFIX) || !name.ends_with(".wav") {
            continue;
        }
        let Some(seq) = name
            .strip_prefix(SEG_PREFIX)
            .and_then(|s| s.strip_suffix(".wav"))
            .and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        let dir_rel = path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let modified_at = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (sample_rate, duration_ms) = wav_meta(&path);
        out.push(ClipInfo {
            dir: dir_rel.clone(),
            file: name.clone(),
            path: format!("{AUDIO_DIR}/{dir_rel}/{name}"),
            seq,
            bytes,
            duration_ms,
            sample_rate,
            modified_at,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_id_maps_to_audio_dir() {
        assert_eq!(dir_for_note("会议/2026-09-22 周会.md"), "2026-09-22 周会");
        assert_eq!(dir_for_note("会议/子目录/周会.md"), "子目录_周会");
        assert_eq!(dir_for_note("随手记.md"), "随手记");
        assert_eq!(dir_for_note("会议/.md"), "未命名会议");
    }

    #[test]
    fn rejects_path_escape() {
        assert!(safe_rel("../evil").is_err());
        assert!(safe_rel("/etc").is_err());
        assert!(safe_rel("C:/x").is_err());
        assert!(safe_rel("a/../../b").is_err());
        assert!(safe_rel("正常 目录/子").is_ok());
    }

    #[test]
    fn record_lifecycle_produces_valid_wav() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path();
        let name = "2026-09-22 周会";
        assert_eq!(next_seq(vault, name), 1);

        let rate = 16_000u32;
        let s = start(vault, name, 1, rate).unwrap();
        assert_eq!(s.bytes, 44);
        assert_eq!(s.ref_line, "/v 会议音频/2026-09-22 周会/seg_0001.wav");

        // 1 秒 PCM16
        let pcm = vec![0x22u8; rate as usize * 2];
        let s = append(vault, name, 1, &pcm).unwrap();
        assert_eq!(s.bytes, 44 + pcm.len() as u64);
        assert_eq!(s.duration_ms, 1000);

        let s = close(vault, name, 1).unwrap();
        assert_eq!(s.duration_ms, 1000);
        let bytes = std::fs::read(vault.join("会议音频").join(name).join("seg_0001.wav")).unwrap();
        let info = audio::parse_wav(&bytes).unwrap();
        assert_eq!(info.sample_rate, 16_000);
        assert_eq!(info.channels, 1);
        assert_eq!(info.data_length as u64, pcm.len() as u64);

        assert_eq!(next_seq(vault, name), 2);
        let clips = list(vault).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].dir, name);
        assert_eq!(clips[0].duration_ms, 1000);
        assert_eq!(clips[0].path, "会议音频/2026-09-22 周会/seg_0001.wav");

        // 再录一段：序号递增、互不影响
        start(vault, name, 2, rate).unwrap();
        append(vault, name, 2, &pcm[..3200]).unwrap();
        close(vault, name, 2).unwrap();
        let clips = list(vault).unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].duration_ms, 100);
    }

    #[test]
    fn wav_meta_only_needs_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path();
        let name = "头解析";
        start(vault, name, 1, 16_000).unwrap();
        // 2 秒数据：文件 64 KB 远大于 4 KB 头缓冲
        append(vault, name, 1, &vec![0x33u8; 16_000 * 2 * 2]).unwrap();
        let path = seg_path(vault, name, 1).unwrap();
        // 收尾前（头里长度还是 0）：按文件大小估算
        assert_eq!(wav_meta(&path).1, 2000);
        close(vault, name, 1).unwrap();
        assert_eq!(wav_meta(&path).1, 2000);

        // 截断数据区（头声明 2s、实际只剩 1s）→ 报可播放长度 1s，不虚报
        let bytes = std::fs::read(&path).unwrap();
        let keep = 44 + 16_000 * 2;
        std::fs::write(&path, &bytes[..keep]).unwrap();
        let (rate, dur) = wav_meta(&path);
        assert_eq!(rate, 16_000);
        assert_eq!(dur, 1000);
    }
}
