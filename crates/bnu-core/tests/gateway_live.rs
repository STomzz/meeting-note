//! 真实网关联调测试（默认 `#[ignore]`，需要密钥，勿在 CI 里跑）。
//!
//! ```bash
//! BNU_TEST_API_KEY=sk-xxx \
//!   cargo test -p bnu-core --test gateway_live -- --ignored --nocapture
//! ```
//!
//! 可用环境变量覆盖：BNU_TEST_BASE_URL / BNU_TEST_API_KEY /
//! BNU_TEST_CHAT_MODEL / BNU_TEST_EMBED_MODEL / BNU_TEST_RERANK_MODEL / BNU_TEST_ASR_MODEL

use bnu_core::models::{self, ChatMessage, ChatOptions, EndpointConfig, ModelConfig};
use serde_json::json;

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

fn endpoint(model_env: &str, model_default: &str, params: serde_json::Value) -> EndpointConfig {
    EndpointConfig {
        base_url: env_or("BNU_TEST_BASE_URL", "https://chatapi.bnu.edu.cn"),
        model: env_or(model_env, model_default),
        params,
        api_key: Some(env_or("BNU_TEST_API_KEY", "")),
    }
}

fn live_config() -> ModelConfig {
    ModelConfig {
        chat: Some(endpoint(
            "BNU_TEST_CHAT_MODEL",
            "Qwen-Inno-35B-v1",
            json!({ "chat_template_kwargs": { "enable_thinking": false } }),
        )),
        embedding: Some(endpoint("BNU_TEST_EMBED_MODEL", "bge-m3", json!({}))),
        rerank: Some(endpoint("BNU_TEST_RERANK_MODEL", "bge-reranker-v2-m3", json!({}))),
        asr: Some(endpoint("BNU_TEST_ASR_MODEL", "qwen3-asr-1.7b", json!({ "language": "zh" }))),
    }
}

fn assert_key(cfg: &ModelConfig) {
    let key = cfg
        .chat
        .as_ref()
        .and_then(|c| c.api_key.as_deref())
        .unwrap_or("");
    assert!(!key.is_empty(), "需要设置 BNU_TEST_API_KEY");
}

#[tokio::test]
#[ignore]
async fn live_list_models() {
    let cfg = live_config();
    assert_key(&cfg);
    let ids = models::list_models(cfg.chat.as_ref().unwrap()).await.unwrap();
    println!("模型列表（{} 个）: {}", ids.len(), ids.join(", "));
    assert!(!ids.is_empty());
}

#[tokio::test]
#[ignore]
async fn live_chat_returns_content_without_thinking() {
    let cfg = live_config();
    assert_key(&cfg);
    let started = std::time::Instant::now();
    let reply = models::chat(
        cfg.chat.as_ref().unwrap(),
        &[ChatMessage::user("只回复两个字：正常")],
        &ChatOptions { max_tokens: Some(64), ..Default::default() },
    )
    .await
    .unwrap();
    println!(
        "chat {} ms | finish={} | tokens={:?} | content={}",
        started.elapsed().as_millis(),
        reply.finish_reason,
        reply.completion_tokens,
        reply.content
    );
    assert!(!reply.content.trim().is_empty());
}

#[tokio::test]
#[ignore]
async fn live_embed_rerank_and_asr() {
    let cfg = live_config();
    assert_key(&cfg);

    let vectors = models::embed(
        cfg.embedding.as_ref().unwrap(),
        &["向量检索".to_string(), "会议纪要".to_string()],
    )
    .await
    .unwrap();
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    println!("embed: {} 条，维度 {}", vectors.len(), dim);
    assert!(dim > 0);

    let docs = vec![
        "向量检索通过嵌入相似度召回语义相近的片段".to_string(),
        "今天的午饭是牛肉面".to_string(),
    ];
    let hits = models::rerank(cfg.rerank.as_ref().unwrap(), "什么是向量检索", &docs, Some(2))
        .await
        .unwrap();
    for h in &hits {
        println!("rerank: #{} score={:.6}", h.index, h.score);
    }
    assert_eq!(hits.first().map(|h| h.index), Some(0));

    let wav = models::silent_wav(0.5, 16000);
    let text = models::transcribe(cfg.asr.as_ref().unwrap(), wav, "probe.wav")
        .await
        .unwrap();
    println!("asr: 静音 0.5s -> {:?}", text);
}

#[tokio::test]
#[ignore]
async fn live_test_capability_all() {
    let cfg = live_config();
    assert_key(&cfg);
    for cap in ["chat", "embedding", "rerank", "asr"] {
        let started = std::time::Instant::now();
        match models::test_capability(cap, &cfg).await {
            Ok(msg) => println!("[{cap}] {} ms -> {msg}", started.elapsed().as_millis()),
            Err(e) => panic!("[{cap}] 失败: {e}"),
        }
    }
}

/// 端到端：索引 → 构建向量 → 混合检索 + 重排 → 问答（带引用）。
#[tokio::test]
#[ignore]
async fn live_qa_end_to_end() {
    use bnu_core::{db, notes, qa, retrieval, vectors};
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("周会.md"),
        "# 周会纪要\n\n2026-09-20 周会：讨论了镜像拉取超时的问题，决定改用镜像站并重试；下周三前完成部署。\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("读书.md"),
        "# 读书笔记\n\n今天读了 Rust 所有权的章节，理解了借用检查。\n",
    )
    .unwrap();

    let conn = db::open_memory().unwrap();
    let stats = notes::scan_vault(&conn, dir.path()).unwrap();
    println!("索引：{} 篇", stats.indexed);
    let m = Mutex::new(conn);

    let progress = vectors::build(&m, cfg.embedding.as_ref().unwrap(), 16, 64).await.unwrap();
    println!(
        "向量索引：embedded={} remaining={} dim={} {}ms",
        progress.embedded, progress.remaining, progress.dim, progress.elapsed_ms
    );
    assert_eq!(progress.remaining, 0);

    let status = {
        let c = m.lock().unwrap();
        retrieval::status(&c, &cfg).unwrap()
    };
    println!(
        "状态：chunks={} vectors={} ready={}",
        status.chunks, status.vectors, status.vector_ready
    );
    assert!(status.vector_ready);

    let answer = qa::answer(&m, &cfg, "镜像拉取超时最后是怎么解决的？", 6).await.unwrap();
    println!(
        "模式={} 耗时={}ms tokens={:?}\n回答：{}",
        answer.trace.mode, answer.elapsed_ms, answer.completion_tokens, answer.answer
    );
    for (i, s) in answer.sources.iter().enumerate() {
        println!(
            "  [{}] {} 第 {}-{} 行 来源={:?}",
            i + 1,
            s.note_id,
            s.start_line,
            s.end_line,
            s.sources
        );
    }
    assert!(!answer.answer.is_empty());
    assert!(answer.sources.iter().any(|s| s.note_id == "周会.md"));
    assert_eq!(answer.trace.mode, "hybrid+rerank");
}
