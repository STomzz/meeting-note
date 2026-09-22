//! 上游模型调用的限流与重试退避（转写 / 图谱抽取共用）。
//!
//! 移植自既有会议 App 的 `rate_limiter.dart` 与转写重试策略：
//! - 上游限流按「每用户 10 请求/分钟」，本地限到 9，把 429 变成"排队"；
//! - 重试 3 次：429 → 15s×次数，其它网络错误 → 2s×次数；确定性 4xx（非 429）不重试。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 每分钟最大请求数（上游 10，留 1 个余量给纪要 / 抽取）。
pub const RATE_LIMIT_PER_MINUTE: usize = 9;
/// 最大尝试次数。
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

/// 判断错误是否为"确定性失败"（非 429 的 4xx，不该重试）。
pub fn is_fatal_http(err: &str) -> bool {
    if let Some(idx) = err.find("返回 ") {
        let rest = &err[idx + "返回 ".len()..];
        let code: String = rest.chars().take(3).collect();
        if let Ok(code) = code.parse::<u16>() {
            return code != 429 && (400..500).contains(&code);
        }
    }
    false
}

/// 第 `attempt` 次失败后的退避时长：429 → 15s×n，其它 → 2s×n。
pub fn backoff_for(err: &str, attempt: usize) -> Duration {
    let base = if err.contains("429") { BACKOFF_RATE_LIMIT_SECS } else { BACKOFF_OTHER_SECS };
    Duration::from_secs(base * attempt.max(1) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_classification() {
        assert!(is_fatal_http("语音转写返回 400: bad audio"));
        assert!(is_fatal_http("对话模型返回 403: forbidden"));
        assert!(!is_fatal_http("语音转写返回 429: too many requests"));
        assert!(!is_fatal_http("语音转写返回 500: boom"));
        assert!(!is_fatal_http("请求失败: connection reset"));
    }

    #[test]
    fn backoff_scales_with_attempt() {
        assert_eq!(backoff_for("语音转写返回 429: too many requests", 1), Duration::from_secs(15));
        assert_eq!(backoff_for("语音转写返回 429: too many requests", 3), Duration::from_secs(45));
        assert_eq!(backoff_for("请求失败: connection reset", 2), Duration::from_secs(4));
        // attempt=0 也至少退避一个基数
        assert_eq!(backoff_for("boom", 0), Duration::from_secs(BACKOFF_OTHER_SECS));
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
