//! 会议存储：会议元信息、录音分段（边录边写 WAV）、转写与纪要结果。
//!
//! 文件布局：`<root>/meetings/<meeting_id>/seg_0001.wav`
//! - 分段文件在开始录音时就写好 44 字节 WAV 头（占位长度），PCM 追加写入；
//! - 关闭分段时回写 RIFF/data 长度，因此录音过程中断电也只会丢最后一段的头部信息；
//! - 音频按采集时的采样率保存（保真播放），送 ASR 前再由 [`crate::audio`] 重采样到 16k 单声道。

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const STATUS_RECORDING: &str = "recording";
pub const STATUS_RECORDED: &str = "recorded";
pub const STATUS_TRANSCRIBING: &str = "transcribing";
pub const STATUS_TRANSCRIBED: &str = "transcribed";
pub const STATUS_MINUTES: &str = "minutes";
pub const STATUS_FAILED: &str = "failed";

pub const SEG_RECORDING: &str = "recording";
pub const SEG_RECORDED: &str = "recorded";
pub const SEG_DONE: &str = "done";
pub const SEG_FAILED: &str = "failed";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_secs() -> i64 {
    now_ms() / 1000
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub duration_ms: i64,
    pub status: String,
    pub segments: i64,
    pub transcribed_segments: i64,
    pub has_transcript: bool,
    pub has_minutes: bool,
    pub note_id: String,
    pub asr_model: String,
    pub chat_model: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: i64,
    pub meeting_id: String,
    pub seq: i64,
    pub file: String,
    pub src_rate: i64,
    pub duration_ms: i64,
    pub bytes: i64,
    pub status: String,
    pub transcript: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub segments: Vec<Segment>,
    pub transcript: String,
    pub minutes_md: String,
    pub dir: String,
}

/// 会议音频目录。
pub fn meeting_dir(root: &Path, id: &str) -> PathBuf {
    root.join("meetings").join(id)
}

fn seg_file_name(seq: i64) -> String {
    format!("seg_{seq:04}.wav")
}

fn seg_path(root: &Path, meeting_id: &str, seq: i64) -> PathBuf {
    meeting_dir(root, meeting_id).join(seg_file_name(seq))
}

/// 新建会议。
pub fn create(conn: &Connection, title: &str) -> Result<Meeting> {
    let id = format!("m{}", now_ms());
    let title = title.trim();
    let title = if title.is_empty() { "未命名会议" } else { title };
    let now = now_secs();
    conn.execute(
        r#"INSERT INTO meetings (id, title, created_at, updated_at, status)
           VALUES (?1, ?2, ?3, ?3, ?4)"#,
        params![id, title, now, STATUS_RECORDING],
    )?;
    get(conn, &id)
}

fn row_to_meeting(r: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: r.get("id")?,
        title: r.get("title")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        duration_ms: r.get("duration_ms")?,
        status: r.get("status")?,
        segments: r.get("segments")?,
        transcribed_segments: r.get("transcribed_segments")?,
        has_transcript: !r.get::<_, String>("transcript")?.trim().is_empty(),
        has_minutes: !r.get::<_, String>("minutes_md")?.trim().is_empty(),
        note_id: r.get("note_id")?,
        asr_model: r.get("asr_model")?,
        chat_model: r.get("chat_model")?,
        error: r.get("error")?,
    })
}

const MEETING_SELECT: &str = r#"SELECT m.*,
        (SELECT COUNT(*) FROM meeting_segments s WHERE s.meeting_id = m.id) AS segments,
        (SELECT COUNT(*) FROM meeting_segments s WHERE s.meeting_id = m.id AND s.status = 'done')
            AS transcribed_segments
    FROM meetings m"#;

/// 单个会议。
pub fn get(conn: &Connection, id: &str) -> Result<Meeting> {
    let sql = format!("{MEETING_SELECT} WHERE m.id = ?1");
    conn.query_row(&sql, params![id], row_to_meeting)
        .map_err(|e| anyhow!("会议不存在或读取失败: {e}"))
}

/// 会议列表（最近优先）。
pub fn list(conn: &Connection) -> Result<Vec<Meeting>> {
    let sql = format!("{MEETING_SELECT} ORDER BY m.created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_meeting)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 会议详情（含分段、转写、纪要）。
pub fn detail(conn: &Connection, root: &Path, id: &str) -> Result<MeetingDetail> {
    let meeting = get(conn, id)?;
    let (transcript, minutes_md): (String, String) = conn.query_row(
        "SELECT transcript, minutes_md FROM meetings WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, meeting_id, seq, file, src_rate, duration_ms, bytes, status, transcript, error
         FROM meeting_segments WHERE meeting_id = ?1 ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(params![id], |r| {
        Ok(Segment {
            id: r.get(0)?,
            meeting_id: r.get(1)?,
            seq: r.get(2)?,
            file: r.get(3)?,
            src_rate: r.get(4)?,
            duration_ms: r.get(5)?,
            bytes: r.get(6)?,
            status: r.get(7)?,
            transcript: r.get(8)?,
            error: r.get(9)?,
        })
    })?;
    let mut segments = Vec::new();
    for s in rows {
        segments.push(s?);
    }
    Ok(MeetingDetail {
        meeting,
        segments,
        transcript,
        minutes_md,
        dir: meeting_dir(root, id).to_string_lossy().to_string(),
    })
}

pub fn rename(conn: &Connection, id: &str, title: &str) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err(anyhow!("标题不能为空"));
    }
    conn.execute(
        "UPDATE meetings SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now_secs()],
    )?;
    Ok(())
}

/// 删除会议记录，返回音频目录（由调用方决定是否删除文件）。
pub fn delete(conn: &Connection, id: &str) -> Result<PathBuf> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM meeting_segments WHERE meeting_id = ?1", params![id])?;
    tx.execute("DELETE FROM meetings WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(PathBuf::from(id))
}

pub fn set_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE meetings SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now_secs()],
    )?;
    Ok(())
}

pub fn set_error(conn: &Connection, id: &str, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE meetings SET error = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, error, now_secs()],
    )?;
    Ok(())
}

pub fn set_models(conn: &Connection, id: &str, asr_model: &str, chat_model: &str) -> Result<()> {
    conn.execute(
        "UPDATE meetings SET asr_model = ?2, chat_model = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, asr_model, chat_model, now_secs()],
    )?;
    Ok(())
}

pub fn set_transcript(conn: &Connection, id: &str, transcript: &str) -> Result<()> {
    conn.execute(
        "UPDATE meetings SET transcript = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, transcript, now_secs()],
    )?;
    Ok(())
}

pub fn set_minutes(conn: &Connection, id: &str, minutes_md: &str, minutes_json: &str, note_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE meetings SET minutes_md = ?2, minutes_json = ?3, note_id = ?4, status = ?5, updated_at = ?6
         WHERE id = ?1",
        params![id, minutes_md, minutes_json, note_id, STATUS_MINUTES, now_secs()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 分段（边录边写）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentStat {
    pub seq: i64,
    pub duration_ms: i64,
    pub bytes: i64,
    pub status: String,
}

/// 开始一个新分段：写占位 WAV 头并登记。
pub fn start_segment(
    conn: &Connection,
    root: &Path,
    meeting_id: &str,
    seq: i64,
    sample_rate: u32,
) -> Result<Segment> {
    let dir = meeting_dir(root, meeting_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建会议目录失败: {}", dir.display()))?;
    let path = seg_path(root, meeting_id, seq);
    // 占位头：长度随后回写
    if !path.exists() {
        let header = crate::audio::build_wav(&[], sample_rate, 1, 16);
        std::fs::write(&path, &header)?;
    }
    conn.execute(
        r#"INSERT INTO meeting_segments (meeting_id, seq, file, src_rate, status)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT(meeting_id, seq) DO UPDATE SET status = excluded.status"#,
        params![meeting_id, seq, seg_file_name(seq), sample_rate as i64, SEG_RECORDING],
    )?;
    segment_by_seq(conn, meeting_id, seq)
}

/// 追加 PCM（16bit 小端、单声道、采集采样率）。
pub fn append_pcm(
    conn: &Connection,
    root: &Path,
    meeting_id: &str,
    seq: i64,
    sample_rate: u32,
    pcm: &[u8],
) -> Result<SegmentStat> {
    if pcm.is_empty() {
        return Err(anyhow!("PCM 数据为空"));
    }
    let path = seg_path(root, meeting_id, seq);
    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .with_context(|| format!("分段文件不存在: {}", path.display()))?;
    file.seek(SeekFrom::End(0))?;
    file.write_all(pcm)?;
    file.flush()?;
    let bytes = file.metadata()?.len() as i64 - 44;

    let duration_ms = if sample_rate > 0 {
        bytes / 2 * 1000 / sample_rate as i64
    } else {
        0
    };
    conn.execute(
        "UPDATE meeting_segments SET bytes = ?3, duration_ms = ?4, src_rate = ?5 WHERE meeting_id = ?1 AND seq = ?2",
        params![meeting_id, seq, bytes, duration_ms, sample_rate as i64],
    )?;
    conn.execute(
        "UPDATE meetings SET duration_ms = (SELECT COALESCE(SUM(duration_ms), 0) FROM meeting_segments WHERE meeting_id = ?1), updated_at = ?2 WHERE id = ?1",
        params![meeting_id, now_secs()],
    )?;
    Ok(SegmentStat {
        seq,
        duration_ms,
        bytes,
        status: SEG_RECORDING.to_string(),
    })
}

/// 关闭分段：回写 WAV 头长度，状态置为 recorded。
pub fn close_segment(conn: &Connection, root: &Path, meeting_id: &str, seq: i64) -> Result<Segment> {
    let path = seg_path(root, meeting_id, seq);
    if path.exists() {
        let total = std::fs::metadata(&path)?.len();
        let data_len = total.saturating_sub(44) as u32;
        let mut file = OpenOptions::new().write(true).open(&path)?;
        // RIFF size = 36 + data
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&(36u32 + data_len).to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data_len.to_le_bytes())?;
        file.flush()?;
    }
    conn.execute(
        "UPDATE meeting_segments SET status = ?3 WHERE meeting_id = ?1 AND seq = ?2",
        params![meeting_id, seq, SEG_RECORDED],
    )?;
    conn.execute(
        "UPDATE meetings SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![meeting_id, STATUS_RECORDED, now_secs()],
    )?;
    segment_by_seq(conn, meeting_id, seq)
}

pub fn segment_by_seq(conn: &Connection, meeting_id: &str, seq: i64) -> Result<Segment> {
    conn.query_row(
        "SELECT id, meeting_id, seq, file, src_rate, duration_ms, bytes, status, transcript, error
         FROM meeting_segments WHERE meeting_id = ?1 AND seq = ?2",
        params![meeting_id, seq],
        |r| {
            Ok(Segment {
                id: r.get(0)?,
                meeting_id: r.get(1)?,
                seq: r.get(2)?,
                file: r.get(3)?,
                src_rate: r.get(4)?,
                duration_ms: r.get(5)?,
                bytes: r.get(6)?,
                status: r.get(7)?,
                transcript: r.get(8)?,
                error: r.get(9)?,
            })
        },
    )
    .map_err(|e| anyhow!("分段不存在: {e}"))
}

/// 读取分段 WAV 内容。
pub fn read_segment(root: &Path, meeting_id: &str, seq: i64) -> Result<Vec<u8>> {
    let path = seg_path(root, meeting_id, seq);
    std::fs::read(&path).with_context(|| format!("读取录音失败: {}", path.display()))
}

/// 待转写分段（无转写文本且未标记完成）。
pub fn pending_segments(conn: &Connection, meeting_id: &str) -> Result<Vec<Segment>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, meeting_id, seq, file, src_rate, duration_ms, bytes, status, transcript, error
           FROM meeting_segments
           WHERE meeting_id = ?1 AND status <> 'done'
           ORDER BY seq ASC"#,
    )?;
    let rows = stmt.query_map(params![meeting_id], |r| {
        Ok(Segment {
            id: r.get(0)?,
            meeting_id: r.get(1)?,
            seq: r.get(2)?,
            file: r.get(3)?,
            src_rate: r.get(4)?,
            duration_ms: r.get(5)?,
            bytes: r.get(6)?,
            status: r.get(7)?,
            transcript: r.get(8)?,
            error: r.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for s in rows {
        out.push(s?);
    }
    Ok(out)
}

/// 更新分段转写结果。
pub fn set_segment_result(
    conn: &Connection,
    segment_id: i64,
    status: &str,
    transcript: &str,
    error: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE meeting_segments SET status = ?2, transcript = ?3, error = ?4 WHERE id = ?1",
        params![segment_id, status, transcript, error],
    )?;
    Ok(())
}

/// 存储目录是否存在（用于"打开文件夹"）。
pub fn ensure_dir(root: &Path, id: &str) -> Result<PathBuf> {
    let dir = meeting_dir(root, id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 追加 base64 编码的 PCM（Tauri IPC 传输用；避免把大数组当 JSON 数字传）。
pub fn append_pcm_base64(
    conn: &Connection,
    root: &Path,
    meeting_id: &str,
    seq: i64,
    sample_rate: u32,
    pcm_base64: &str,
) -> Result<SegmentStat> {
    use base64::Engine;
    let pcm = base64::engine::general_purpose::STANDARD
        .decode(pcm_base64.trim())
        .map_err(|e| anyhow!("PCM base64 解码失败: {e}"))?;
    append_pcm(conn, root, meeting_id, seq, sample_rate, &pcm)
}

/// epoch 秒 → `YYYY-MM-DD`（UTC；前端一般会传本地日期，这里只是兜底）。
pub fn ymd_from_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn create_segment_append_and_close() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let m = create(&conn, "周会").unwrap();
        assert_eq!(m.status, STATUS_RECORDING);

        let seg = start_segment(&conn, dir.path(), &m.id, 1, 48_000).unwrap();
        assert_eq!(seg.status, SEG_RECORDING);
        assert_eq!(std::fs::metadata(seg_path(dir.path(), &m.id, 1)).unwrap().len(), 44);

        // 1 秒 48k 单声道 PCM16 = 96000 字节
        let pcm = vec![0u8; 96_000];
        let stat = append_pcm(&conn, dir.path(), &m.id, 1, 48_000, &pcm).unwrap();
        assert_eq!(stat.bytes, 96_000);
        assert_eq!(stat.duration_ms, 1000);

        let seg = close_segment(&conn, dir.path(), &m.id, 1).unwrap();
        assert_eq!(seg.status, SEG_RECORDED);
        let wav = read_segment(dir.path(), &m.id, 1).unwrap();
        let info = crate::audio::parse_wav(&wav).unwrap();
        assert_eq!(info.data_length, 96_000);
        assert_eq!(crate::audio::duration_ms(&wav), Some(1000));

        let detail = detail(&conn, dir.path(), &m.id).unwrap();
        assert_eq!(detail.segments.len(), 1);
        assert_eq!(detail.meeting.duration_ms, 1000);
        assert_eq!(detail.meeting.status, STATUS_RECORDED);
    }

    #[test]
    fn pending_segments_shrink_after_done() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let m = create(&conn, "评审").unwrap();
        for seq in 1..=3 {
            start_segment(&conn, dir.path(), &m.id, seq, 16_000).unwrap();
            close_segment(&conn, dir.path(), &m.id, seq).unwrap();
        }
        assert_eq!(pending_segments(&conn, &m.id).unwrap().len(), 3);

        let s1 = segment_by_seq(&conn, &m.id, 1).unwrap();
        set_segment_result(&conn, s1.id, SEG_DONE, "第一段内容", "").unwrap();
        assert_eq!(pending_segments(&conn, &m.id).unwrap().len(), 2);

        let s2 = segment_by_seq(&conn, &m.id, 2).unwrap();
        set_segment_result(&conn, s2.id, SEG_FAILED, "", "上游 400").unwrap();
        // 失败段仍在待处理列表里，便于重试
        assert_eq!(pending_segments(&conn, &m.id).unwrap().len(), 2);
    }

    #[test]
    fn transcript_and_minutes_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let m = create(&conn, "复盘").unwrap();
        set_transcript(&conn, &m.id, "[00:00] 讨论了镜像站方案").unwrap();
        set_minutes(&conn, &m.id, "# 复盘\n\n## 一、会议摘要\n\n改了镜像站。\n", "{}", "会议纪要/复盘.md").unwrap();
        let detail = detail(&conn, dir.path(), &m.id).unwrap();
        assert!(detail.meeting.has_transcript);
        assert!(detail.meeting.has_minutes);
        assert_eq!(detail.meeting.status, STATUS_MINUTES);
        assert_eq!(detail.meeting.note_id, "会议纪要/复盘.md");
        assert!(detail.minutes_md.contains("会议摘要"));
    }

    #[test]
    fn ymd_and_base64_helpers() {
        assert_eq!(ymd_from_secs(0), "1970-01-01");
        assert_eq!(ymd_from_secs(1_700_000_000), "2023-11-14");

        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let m = create(&conn, "base64").unwrap();
        start_segment(&conn, dir.path(), &m.id, 1, 16_000).unwrap();
        // "AAAAAA==" = 4 个零字节
        let stat = append_pcm_base64(&conn, dir.path(), &m.id, 1, 16_000, "AAAAAA==").unwrap();
        assert_eq!(stat.bytes, 4);
        assert_eq!(stat.duration_ms, 0); // 4 字节 = 2 采样 ≈ 0ms
    }

    #[test]
    fn delete_removes_rows() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let m = create(&conn, "临时").unwrap();
        start_segment(&conn, dir.path(), &m.id, 1, 16_000).unwrap();
        delete(&conn, &m.id).unwrap();
        assert!(get(&conn, &m.id).is_err());
        assert_eq!(list(&conn).unwrap().len(), 0);
        assert!(pending_segments(&conn, &m.id).unwrap().is_empty());
    }
}
