//! 检索增强问答：检索本地笔记片段 → 拼提示词 → 对话模型作答（带引用编号）。
//!
//! 设计要点：
//! - 提示词强制"只依据片段回答"，片段为空时直接给出兜底文案，不调用模型（省 token、防幻觉）；
//! - 回答里的 `[1] [2]` 与返回的 `sources` 顺序一一对应，前端可点击回跳笔记行号。

use crate::models::{self, ChatMessage, ChatOptions, ModelConfig};
use crate::retrieval::{self, RetrievalTrace, RetrieveOptions, RetrievedChunk};
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;

/// 单个片段注入提示词的最大字符数。
pub const MAX_CHUNK_CHARS: usize = 1200;

/// 系统提示词。
pub const SYSTEM_PROMPT: &str = "你是「BNU Notes」的本地知识库助手，只能依据用户提供的笔记片段回答。\n\
规则：\n\
1. 只使用片段中的信息，不要编造，也不要引入片段之外的知识；\n\
2. 片段不足以回答时，直接说明笔记里没有相关内容，并提示可以换个说法或补充笔记；\n\
3. 用简体中文回答，条理清晰，必要时分点；\n\
4. 每个结论后面用 [编号] 标注来源（例如 [1][2]），编号对应片段顺序。";

/// 问答结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub question: String,
    pub answer: String,
    /// 引用来源（与回答里的 [编号] 对应）
    pub sources: Vec<RetrievedChunk>,
    pub trace: RetrievalTrace,
    pub model: String,
    pub elapsed_ms: u64,
    pub completion_tokens: Option<u64>,
}

fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// 组装提示词（片段带标题、路径、行号，便于模型标注来源）。
pub fn build_messages(question: &str, chunks: &[RetrievedChunk]) -> Vec<ChatMessage> {
    let mut ctx = String::new();
    for (i, c) in chunks.iter().enumerate() {
        ctx.push_str(&format!(
            "[{}] 《{}》（{} 第 {}-{} 行）\n{}\n\n",
            i + 1,
            c.title,
            c.note_id,
            c.start_line,
            c.end_line,
            truncate_chars(c.text.trim(), MAX_CHUNK_CHARS),
        ));
    }
    let user = format!(
        "以下是从我的本地笔记中检索到的片段：\n\n{}\n请依据这些片段回答我的问题：{}",
        ctx,
        question.trim()
    );
    vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(user)]
}

/// 无检索结果时的兜底回答（不调用模型）。
pub fn no_result_answer(question: &str) -> String {
    format!(
        "在本地笔记里没有找到与「{}」相关的内容。\n\n可以试试：\n- 换个说法或关键词再问一次；\n- 如果笔记是刚加进 vault 的，先到「笔记」页点一次「扫描 vault」；\n- 在「设置」里配置嵌入模型并构建向量索引，语义检索能显著提高召回。",
        question.trim()
    )
}

/// 问答主流程。
pub async fn answer(
    conn: &Mutex<Connection>,
    cfg: &ModelConfig,
    question: &str,
    top_k: usize,
) -> Result<Answer> {
    let started = Instant::now();
    let question = question.trim();
    if question.is_empty() {
        return Err(anyhow!("问题不能为空"));
    }
    let chat_cfg = cfg
        .chat
        .as_ref()
        .ok_or_else(|| anyhow!("未配置对话模型：请到「设置」里填写对话端点（问答需要它）"))?;

    let opts = RetrieveOptions { top_k: top_k.clamp(1, 12), ..Default::default() };
    let (chunks, trace) = retrieval::retrieve(conn, cfg, question, &opts).await?;

    if chunks.is_empty() {
        return Ok(Answer {
            question: question.to_string(),
            answer: no_result_answer(question),
            sources: Vec::new(),
            trace,
            model: chat_cfg.model.clone(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            completion_tokens: None,
        });
    }

    let messages = build_messages(question, &chunks);
    let reply = models::chat(
        chat_cfg,
        &messages,
        &ChatOptions { max_tokens: Some(1024), temperature: Some(0.2), ..Default::default() },
    )
    .await?;

    Ok(Answer {
        question: question.to_string(),
        answer: reply.content,
        sources: chunks,
        trace,
        model: chat_cfg.model.clone(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        completion_tokens: reply.completion_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::notes;
    use serde_json::json;

    fn chunk(note: &str, title: &str, text: &str) -> RetrievedChunk {
        RetrievedChunk {
            chunk_id: 1,
            note_id: note.to_string(),
            title: title.to_string(),
            start_line: 3,
            end_line: 9,
            text: text.to_string(),
            score: 0.02,
            sources: vec!["fts".to_string()],
        }
    }

    #[test]
    fn prompt_contains_citation_context() {
        let msgs = build_messages(
            "镜像拉取超时怎么办",
            &[chunk("工作/周会.md", "周会纪要", "决定改用镜像站并重试")],
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("不要编造"));
        let user = &msgs[1].content;
        assert!(user.contains("[1] 《周会纪要》"));
        assert!(user.contains("工作/周会.md 第 3-9 行"));
        assert!(user.contains("镜像拉取超时怎么办"));
    }

    #[tokio::test]
    async fn empty_index_returns_fallback_without_model_call() {
        let conn = Mutex::new(db::open_memory().unwrap());
        // 故意把端点指向不可达地址：空索引时不应该发起请求
        let cfg = ModelConfig {
            chat: Some(models::EndpointConfig {
                base_url: "http://127.0.0.1:1".into(),
                model: "m".into(),
                params: json!({}),
                api_key: None,
            }),
            ..Default::default()
        };
        let a = answer(&conn, &cfg, "镜像拉取超时怎么办", 6).await.unwrap();
        assert!(a.answer.contains("没有找到"));
        assert!(a.sources.is_empty());
        assert_eq!(a.completion_tokens, None);
    }

    #[tokio::test]
    async fn missing_chat_config_is_an_error() {
        let conn = Mutex::new(db::open_memory().unwrap());
        let err = answer(&conn, &ModelConfig::default(), "随便问问", 6)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("未配置对话模型"), "{err}");
    }

    #[tokio::test]
    async fn hit_is_injected_into_prompt() {
        // 建一个索引：命中时把片段拼进提示词（用一个假的对话端点验证请求体不方便，
        // 这里只验证检索命中后不会走兜底文案）
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        std::fs::write(
            dir.path().join("周会.md"),
            "# 周会纪要\n\n讨论了镜像拉取超时的问题，决定改用镜像站。\n",
        )
        .unwrap();
        notes::scan_vault(&conn, dir.path()).unwrap();
        let conn = Mutex::new(conn);
        let cfg = ModelConfig {
            chat: Some(models::EndpointConfig {
                base_url: "http://127.0.0.1:1".into(),
                model: "m".into(),
                params: json!({}),
                api_key: None,
            }),
            ..Default::default()
        };
        // 命中 → 会尝试调用模型 → 因为端点不可达而报错（而不是返回兜底文案）
        let err = answer(&conn, &cfg, "镜像拉取超时", 6).await.unwrap_err().to_string();
        assert!(err.contains("请求失败") || err.contains("对话模型返回"), "{err}");
    }
}
