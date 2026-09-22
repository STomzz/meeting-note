//! 会议转写编排：静音切分 → 令牌桶限流 → ASR → 汇总带时间戳的转写文本。
//!
//! 移植自既有会议 App 的 `upload_queue.dart` / `rate_limiter.dart`：
//! - 上游限流按「每用户 10 请求/分钟」，本地限到 9，把 429 变成"排队"；
//! - 重试 3 次：429 → 15s×次数，其它网络错误 → 2s×次数；确定性 4xx（非 429）不重试。

use crate::audio;
use crate::meetings;
use crate::models::{self, ModelConfig};
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 每分钟最大请求数（上游 10，留 1 个余量给纪要）。
pub const RATE_LIMIT_PER_MINUTE: usize = 9;
/// 最大重试次数。
pub const MAX_RETRIES: usize = 3;
/// 429 退避基数（秒）。
pub const BACKOFF_RATE_LIMIT_SECS: u64 = 15;
/// 其它错误退避基数（秒）。
pub const BACKOFF_OTHER_SECS: u64 = 2;

/// 令牌桶（滑动窗口）限流。
pub struct RateLimiter {
    max_requests: usize,
    window: Duration,
    stamps: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self { max_requests: max_requests.max(1), window, stamps: VecDeque::new() }
    }

    /// 取得一个令牌；必要时等到最早的请求滑出窗口。返回累计等待时长。
    pub async fn acquire(&mut self) -> Duration {
        let mut waited = Duration::ZERO;
        loop {
            let now = Instant::now();
            while let Some(front) = self.stamps.front() {
                if now.duration_since(*front) >= self.window {
                    self.stamps.pop_front();
                } else {
                    break;
                }
            }
            if self.stamps.len() < self.max_requests {
                self.stamps.push_back(now);
                return waited;
            }
            let first = *self.stamps.front().expect("非空");
            let wait = self.window.saturating_sub(now.duration_since(first));
            let wait = if wait > Duration::ZERO { wait } else { Duration::from_millis(200) };
            tokio::time::sleep(wait).await;
            waited += wait;
        }
    }
}

/// 转写进度（推送给前端）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeProgress {
    pub meeting_id: String,
    pub segment_seq: i64,
    pub segments_total: usize,
    pub segments_done: usize,
    pub chunks_total: usize,
    pub chunks_done: usize,
    pub current_text: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeOutcome {
    pub transcript: String,
    pub segments_total: usize,
    pub segments_done: usize,
    pub failed_segments: usize,
    pub chunks: usize,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

fn mmss(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// 判断错误是否为"确定性失败"（非 429 的 4xx，不该重试）。
fn is_fatal(err: &str) -> bool {
    if let Some(idx) = err.find("返回 ") {
        let rest = &err[idx + "返回 ".len()..];
        let code: String = rest.chars().take(3).collect();
        if let Ok(code) = code.parse::<u16>() {
            return code != 429 && (400..500).contains(&code);
        }
    }
    false
}

/// 转写一场会议（串行 + 限流 + 重试）。
pub async fn transcribe_meeting(
    conn: &Mutex<Connection>,
    root: &Path,
    cfg: &ModelConfig,
    meeting_id: &str,
    progress: &(dyn Fn(TranscribeProgress) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<TranscribeOutcome> {
    let started = Instant::now();
    let asr_cfg = cfg
        .asr
        .as_ref()
        .ok_or_else(|| anyhow!("未配置语音转写模型：请到「设置」里填写语音转写端点"))?
        .clone();

    let (meeting, pending, all_segments) = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        (
            meetings::get(&c, meeting_id)?,
            meetings::pending_segments(&c, meeting_id)?,
            meetings::detail(&c, root, meeting_id)?.segments,
        )
    };
    if all_segments.is_empty() {
        return Err(anyhow!("这场会议还没有录音分段"));
    }

    // 起始偏移：已完成分段的时长之和
    let done_before: i64 = all_segments
        .iter()
        .filter(|s| s.status == meetings::SEG_DONE)
        .map(|s| s.duration_ms)
        .sum();

    {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        meetings::set_models(&c, meeting_id, &asr_cfg.model, &meeting.chat_model)?;
        meetings::set_status(&c, meeting_id, meetings::STATUS_TRANSCRIBING)?;
        meetings::set_error(&c, meeting_id, "")?;
    }

    let mut limiter = RateLimiter::new(RATE_LIMIT_PER_MINUTE, Duration::from_secs(60));
    let mut out = TranscribeOutcome {
        segments_total: all_segments.len(),
        ..Default::default()
    };
    let mut lines: Vec<String> = Vec::new();
    let mut offset_ms = done_before.max(0) as u64;

    for seg in &pending {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let wav = match meetings::read_segment(root, meeting_id, seg.seq) {
            Ok(w) => w,
            Err(e) => {
                out.errors.push(format!("第 {} 段读取失败：{e}", seg.seq));
                let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
                meetings::set_segment_result(&c, seg.id, meetings::SEG_FAILED, "", &e.to_string())?;
                out.failed_segments += 1;
                continue;
            }
        };
        let normalized = match audio::normalize_to_16k_mono(&wav) {
            Ok(n) => n,
            Err(e) => {
                out.errors.push(format!("第 {} 段音频转换失败：{e}", seg.seq));
                let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
                meetings::set_segment_result(&c, seg.id, meetings::SEG_FAILED, "", &e.to_string())?;
                out.failed_segments += 1;
                continue;
            }
        };
        let chunks = audio::split_speech_pcm(&normalized);
        let chunk_total = chunks.len();
        let mut texts: Vec<(u64, String)> = Vec::new();
        let mut seg_failed: Option<String> = None;

        for (i, chunk) in chunks.into_iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                out.cancelled = true;
                break;
            }
            progress(TranscribeProgress {
                meeting_id: meeting_id.to_string(),
                segment_seq: seg.seq,
                segments_total: out.segments_total,
                segments_done: out.segments_done,
                chunks_total: chunk_total,
                chunks_done: i,
                current_text: String::new(),
                message: format!("第 {} 段 · 第 {}/{} 块", seg.seq, i + 1, chunk_total),
            });

            // 限流 + 重试
            let mut attempt = 1usize;
            let mut result: Option<String> = None;
            let mut last_err = String::new();
            while attempt <= MAX_RETRIES {
                let waited = limiter.acquire().await;
                if waited > Duration::from_secs(1) {
                    progress(TranscribeProgress {
                        meeting_id: meeting_id.to_string(),
                        segment_seq: seg.seq,
                        segments_total: out.segments_total,
                        segments_done: out.segments_done,
                        chunks_total: chunk_total,
                        chunks_done: i,
                        current_text: String::new(),
                        message: format!("限流等待 {}s（上游 9 请求/分钟）", waited.as_secs()),
                    });
                }
                match models::transcribe(&asr_cfg, chunk.wav.clone(), "chunk.wav").await {
                    Ok(text) => {
                        result = Some(text);
                        break;
                    }
                    Err(e) => {
                        last_err = e.to_string();
                        if is_fatal(&last_err) || attempt == MAX_RETRIES {
                            break;
                        }
                        let rate_limited = last_err.contains("429");
                        let base = if rate_limited { BACKOFF_RATE_LIMIT_SECS } else { BACKOFF_OTHER_SECS };
                        let backoff = Duration::from_secs(base * attempt as u64);
                        progress(TranscribeProgress {
                            meeting_id: meeting_id.to_string(),
                            segment_seq: seg.seq,
                            segments_total: out.segments_total,
                            segments_done: out.segments_done,
                            chunks_total: chunk_total,
                            chunks_done: i,
                            current_text: String::new(),
                            message: format!(
                                "第 {} 次失败，{}s 后重试：{}",
                                attempt,
                                backoff.as_secs(),
                                last_err.chars().take(80).collect::<String>()
                            ),
                        });
                        tokio::time::sleep(backoff).await;
                        attempt += 1;
                    }
                }
            }

            match result {
                Some(text) => {
                    out.chunks += 1;
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        texts.push((offset_ms + chunk.start_ms, text));
                    }
                }
                None => {
                    seg_failed = Some(last_err);
                    break;
                }
            }
        }

        let seg_text = texts
            .iter()
            .map(|(ms, t)| format!("[{}] {}", mmss(*ms), t))
            .collect::<Vec<_>>()
            .join("\n");
        {
            let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
            match &seg_failed {
                Some(err) => {
                    meetings::set_segment_result(&c, seg.id, meetings::SEG_FAILED, &seg_text, err)?;
                    out.failed_segments += 1;
                    out.errors.push(format!("第 {} 段转写失败：{}", seg.seq, err));
                }
                None => {
                    meetings::set_segment_result(&c, seg.id, meetings::SEG_DONE, &seg_text, "")?;
                    out.segments_done += 1;
                }
            }
        }
        if !seg_text.is_empty() {
            lines.push(seg_text.clone());
        }
        offset_ms += seg.duration_ms.max(0) as u64;

        // 每段结束就更新整场转写文本，UI 可以实时看到
        {
            let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
            meetings::set_transcript(&c, meeting_id, &lines.join("\n\n"))?;
        }
        progress(TranscribeProgress {
            meeting_id: meeting_id.to_string(),
            segment_seq: seg.seq,
            segments_total: out.segments_total,
            segments_done: out.segments_done,
            chunks_total: out.segments_total,
            chunks_done: out.segments_done,
            current_text: seg_text.clone(),
            message: format!("第 {} 段完成", seg.seq),
        });
    }

    out.transcript = lines.join("\n\n");
    out.elapsed_ms = started.elapsed().as_millis() as u64;
    {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        meetings::set_transcript(&c, meeting_id, &out.transcript)?;
        let status = if out.failed_segments > 0 {
            meetings::STATUS_FAILED
        } else if out.cancelled {
            meetings::STATUS_RECORDED
        } else {
            meetings::STATUS_TRANSCRIBED
        };
        meetings::set_status(&c, meeting_id, status)?;
        if !out.errors.is_empty() {
            meetings::set_error(&c, meeting_id, &out.errors.join("；"))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_classification() {
        assert!(is_fatal("语音转写返回 400: bad audio"));
        assert!(!is_fatal("语音转写返回 429: too many requests"));
        assert!(!is_fatal("语音转写返回 500: boom"));
        assert!(!is_fatal("请求失败: connection reset"));
    }

    #[test]
    fn mmss_formats_hours() {
        assert_eq!(mmss(0), "00:00:00");
        assert_eq!(mmss(65_000), "00:01:05");
        assert_eq!(mmss(3_725_000), "01:02:05");
    }

    #[tokio::test]
    async fn rate_limiter_allows_within_window_then_waits() {
        // 2 次 / 300ms：前两次立即通过，第三次需要等待
        let mut limiter = RateLimiter::new(2, Duration::from_millis(300));
        assert!(limiter.acquire().await < Duration::from_millis(50));
        assert!(limiter.acquire().await < Duration::from_millis(50));
        let waited = limiter.acquire().await;
        assert!(waited >= Duration::from_millis(250), "第三次应等待窗口滑出，实际 {waited:?}");
    }
}
