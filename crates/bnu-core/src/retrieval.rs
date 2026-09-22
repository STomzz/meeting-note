//! 混合检索：FTS5 全文召回 + 向量召回 → RRF 融合 → 可选重排。
//!
//! 降级策略（不报错，只记录原因）：
//! - 未配置 embedding → 仅全文检索；
//! - 配置了但向量索引为空 → 仅全文检索，提示"尚未构建向量索引"；
//! - 嵌入/重排请求失败 → 跳过该环节并在 `trace.degraded` 里说明。

use crate::models::{self, ModelConfig};
use crate::notes;
use crate::vectors;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// RRF 平滑常数（经验值 60）。
pub const RRF_K: f32 = 60.0;

/// 检索到的片段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievedChunk {
    pub chunk_id: i64,
    pub note_id: String,
    pub title: String,
    pub start_line: i64,
    pub end_line: i64,
    pub text: String,
    pub score: f32,
    /// 命中来源：fts / vector / rerank
    pub sources: Vec<String>,
}

/// 检索过程说明（用于界面提示与排查）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalTrace {
    /// fts | hybrid | hybrid+rerank
    pub mode: String,
    pub fts_hits: usize,
    pub vector_hits: usize,
    pub reranked: bool,
    /// 降级原因（用户可见）
    pub degraded: Vec<String>,
    pub elapsed_ms: u64,
}

/// 检索参数。
#[derive(Debug, Clone)]
pub struct RetrieveOptions {
    /// 最终返回片段数
    pub top_k: usize,
    /// 每路召回的候选数
    pub candidate_k: usize,
    pub use_vectors: bool,
    pub use_rerank: bool,
    /// 重排得分下限：低于该值的片段会被丢弃（至少保留最高分那条），避免把不相关内容塞进提示词
    pub min_rerank_score: f32,
}

impl Default for RetrieveOptions {
    fn default() -> Self {
        Self {
            top_k: 6,
            candidate_k: 24,
            use_vectors: true,
            use_rerank: true,
            min_rerank_score: 0.05,
        }
    }
}

fn lock<'a>(m: &'a Mutex<Connection>) -> Result<std::sync::MutexGuard<'a, Connection>> {
    m.lock().map_err(|_| anyhow!("数据库锁定失败（锁已中毒）"))
}

/// 全文召回：返回按相关度排序的 chunk_id。
pub fn fts_candidates(conn: &Connection, query: &str, limit: usize) -> Result<Vec<i64>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 200) as i64;
    match notes::fts_match_expr(q) {
        Some(expr) => {
            let mut stmt = conn.prepare(
                "SELECT f.chunk_id FROM notes_fts f WHERE notes_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![expr, limit], |r| r.get::<_, i64>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        }
        None => {
            let mut stmt = conn.prepare(
                r#"SELECT c.id FROM chunks c JOIN notes n ON n.id = c.note_id
                   WHERE c.text LIKE '%' || ?1 || '%' OR n.title LIKE '%' || ?1 || '%'
                   ORDER BY n.updated_at DESC LIMIT ?2"#,
            )?;
            let rows = stmt.query_map(params![q, limit], |r| r.get::<_, i64>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        }
    }
}

/// 向量召回：暴力余弦（已按模型过滤）。
pub fn vector_candidates(
    conn: &Connection,
    model: &str,
    query_vec: &[f32],
    limit: usize,
) -> Result<Vec<i64>> {
    let qn = vectors::normalize(query_vec.to_vec());
    if qn.is_empty() {
        return Ok(Vec::new());
    }
    let mut scored: Vec<(i64, f32)> = vectors::load_all(conn, model)?
        .into_iter()
        .filter_map(|(id, v)| {
            if v.len() != qn.len() {
                return None;
            }
            Some((id, vectors::dot(&v, &qn)))
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit.clamp(1, 500));
    Ok(scored.into_iter().map(|(id, _)| id).collect())
}

/// RRF 融合得分：`score(id) = Σ weight / (K + rank)`。
pub fn rrf_scores(lists: &[(&[i64], f32)]) -> HashMap<i64, f32> {
    let mut scores: HashMap<i64, f32> = HashMap::new();
    for (list, weight) in lists {
        for (i, id) in list.iter().enumerate() {
            *scores.entry(*id).or_insert(0.0) += *weight / (RRF_K + (i + 1) as f32);
        }
    }
    scores
}

/// RRF 融合并按得分降序返回 chunk_id。
pub fn rrf(lists: &[(&[i64], f32)]) -> Vec<i64> {
    let scores = rrf_scores(lists);
    let mut out: Vec<(i64, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(id, _)| id).collect()
}

/// 按 id 顺序取回片段正文。
pub fn hydrate(conn: &Connection, ids: &[i64], scores: &HashMap<i64, f32>) -> Result<Vec<RetrievedChunk>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        r#"SELECT c.id, c.note_id, n.title, c.start_line, c.end_line, c.text
           FROM chunks c JOIN notes n ON n.id = c.note_id
           WHERE c.id = ?1"#,
    )?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            out.push(RetrievedChunk {
                chunk_id: r.get(0)?,
                note_id: r.get(1)?,
                title: r.get(2)?,
                start_line: r.get(3)?,
                end_line: r.get(4)?,
                text: r.get(5)?,
                score: scores.get(id).copied().unwrap_or(0.0),
                sources: Vec::new(),
            });
        }
    }
    Ok(out)
}

/// 检索能力状态（界面显示"当前检索模式/是否可构建向量索引"用）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalStatus {
    pub chunks: i64,
    pub vectors: i64,
    pub pending: i64,
    pub embed_model: String,
    pub rerank_model: String,
    pub chat_model: String,
    pub has_chat: bool,
    pub has_embedding: bool,
    pub has_rerank: bool,
    /// 向量索引是否可用于检索（已配置嵌入模型且已有向量）
    pub vector_ready: bool,
}

/// 汇总当前检索能力与索引状态。
pub fn status(conn: &Connection, cfg: &ModelConfig) -> Result<RetrievalStatus> {
    let embed_model = cfg.embedding.as_ref().map(|e| e.model.clone()).unwrap_or_default();
    let (vectors, pending, chunks) = if cfg.embedding.is_some() {
        let s = vectors::stats(conn, &embed_model)?;
        (s.vectors, s.pending, s.chunks)
    } else {
        let chunks: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        (0, chunks, chunks)
    };
    Ok(RetrievalStatus {
        chunks,
        vectors,
        pending,
        vector_ready: cfg.embedding.is_some() && vectors > 0,
        embed_model,
        rerank_model: cfg.rerank.as_ref().map(|e| e.model.clone()).unwrap_or_default(),
        chat_model: cfg.chat.as_ref().map(|e| e.model.clone()).unwrap_or_default(),
        has_chat: cfg.chat.is_some(),
        has_embedding: cfg.embedding.is_some(),
        has_rerank: cfg.rerank.is_some(),
    })
}

/// 混合检索主流程。
pub async fn retrieve(
    conn: &Mutex<Connection>,
    cfg: &ModelConfig,
    query: &str,
    opts: &RetrieveOptions,
) -> Result<(Vec<RetrievedChunk>, RetrievalTrace)> {
    let started = Instant::now();
    let mut trace = RetrievalTrace::default();
    let query = query.trim();
    if query.is_empty() {
        trace.mode = "fts".into();
        return Ok((Vec::new(), trace));
    }

    // 1) 全文召回（同步，不跨 await 持锁）
    let fts_hits: Vec<i64> = {
        let c = lock(conn)?;
        fts_candidates(&c, query, opts.candidate_k)?
    };
    trace.fts_hits = fts_hits.len();

    // 2) 向量召回（可选）
    let mut vector_hits: Vec<i64> = Vec::new();
    if opts.use_vectors {
        match cfg.embedding.as_ref() {
            None => {}
            Some(ec) => {
                let embed_model = ec.model.clone();
                let s = {
                    let c = lock(conn)?;
                    vectors::stats(&c, &embed_model)?
                };
                if s.vectors == 0 {
                    trace
                        .degraded
                        .push("尚未构建向量索引，本次仅用全文检索（可在设置页构建）".into());
                } else {
                    match models::embed(ec, &[query.to_string()]).await {
                        Ok(mut vs) => {
                            if let Some(qv) = vs.pop() {
                                let c = lock(conn)?;
                                vector_hits = vector_candidates(&c, &embed_model, &qv, opts.candidate_k)?;
                                trace.vector_hits = vector_hits.len();
                            }
                        }
                        Err(e) => trace.degraded.push(format!("嵌入调用失败，已跳过向量召回：{e}")),
                    }
                }
            }
        }
    }

    // 3) RRF 融合
    let lists: Vec<(&[i64], f32)> = vec![(fts_hits.as_slice(), 1.0), (vector_hits.as_slice(), 1.0)];
    let scores = rrf_scores(&lists);
    let mut ids = rrf(&lists);
    ids.truncate(opts.candidate_k);

    // 4) 重排（可选）
    let mut reranked = false;
    if opts.use_rerank && !ids.is_empty() {
        if let Some(rc) = cfg.rerank.as_ref() {
            let docs: Vec<String> = {
                let c = lock(conn)?;
                hydrate(&c, &ids, &scores)?.into_iter().map(|x| x.text).collect()
            };
            if docs.len() == ids.len() {
                match models::rerank(rc, query, &docs, Some(opts.top_k.min(docs.len()))).await {
                    Ok(hits) => {
                        let scored: Vec<(i64, f32)> = hits
                            .iter()
                            .filter_map(|h| ids.get(h.index).copied().map(|id| (id, h.score)))
                            .collect();
                        let best = scored.iter().map(|(_, s)| *s).fold(f32::MIN, f32::max);
                        // 全部低于阈值时至少保留最高分那条，避免返回空结果让人以为没命中
                        let threshold =
                            if best < opts.min_rerank_score { f32::MIN } else { opts.min_rerank_score };
                        let reordered: Vec<i64> = scored
                            .iter()
                            .filter(|(_, s)| *s >= threshold)
                            .map(|(id, _)| *id)
                            .collect();
                        if !reordered.is_empty() {
                            ids = reordered;
                            reranked = true;
                        }
                    }
                    Err(e) => trace.degraded.push(format!("重排调用失败，已跳过往后排序：{e}")),
                }
            }
        }
    }

    ids.truncate(opts.top_k);
    let mut chunks = {
        let c = lock(conn)?;
        hydrate(&c, &ids, &scores)?
    };
    for ch in chunks.iter_mut() {
        let mut sources = Vec::new();
        if fts_hits.contains(&ch.chunk_id) {
            sources.push("fts".to_string());
        }
        if vector_hits.contains(&ch.chunk_id) {
            sources.push("vector".to_string());
        }
        if reranked {
            sources.push("rerank".to_string());
        }
        ch.sources = sources;
    }

    trace.reranked = reranked;
    trace.mode = if reranked {
        "hybrid+rerank".into()
    } else if !vector_hits.is_empty() {
        "hybrid".into()
    } else {
        "fts".into()
    };
    trace.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok((chunks, trace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO notes (id, title, folder, tags) VALUES ('工作/周会.md', '周会纪要', '工作', '会议')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes (id, title, folder, tags) VALUES ('读书.md', '读书笔记', '', '')",
            [],
        )
        .unwrap();
        let rows = [
            ("工作/周会.md", "讨论了镜像拉取超时的问题，决定改用镜像站并重试。", "会议"),
            ("读书.md", "今天读了 Rust 所有权相关章节，理解了借用检查。", ""),
        ];
        for (note, text, tags) in rows {
            conn.execute(
                "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES (?1, 0, 1, 3, ?2)",
                params![note, text],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO notes_fts (chunk_id, note_id, title, body, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, note, if note == "工作/周会.md" { "周会纪要" } else { "读书笔记" }, text, tags],
            )
            .unwrap();
        }
    }

    #[test]
    fn rrf_prefers_documents_hit_by_both_lists() {
        let fts = vec![1i64, 2, 3];
        let vec = vec![3i64, 4];
        let merged = rrf(&[(&fts, 1.0), (&vec, 1.0)]);
        // 3 同时出现在两路，应排第一
        assert_eq!(merged.first(), Some(&3));
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn fts_and_short_query_paths() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let hits = fts_candidates(&conn, "镜像拉取", 10).unwrap();
        assert_eq!(hits.len(), 1);
        // 短查询走 LIKE 兜底
        let hits = fts_candidates(&conn, "读书", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn vector_candidates_orders_by_similarity() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        crate::vectors::upsert(&conn, 1, "m", &[1.0, 0.0]).unwrap();
        crate::vectors::upsert(&conn, 2, "m", &[0.0, 1.0]).unwrap();
        let hits = vector_candidates(&conn, "m", &[0.9, 0.1], 10).unwrap();
        assert_eq!(hits, vec![1, 2]);
        // 维度不一致的向量被跳过
        let hits = vector_candidates(&conn, "m", &[0.9, 0.1, 0.0], 10).unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn retrieve_degrades_without_embedding_config() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let m = Mutex::new(conn);
        let cfg = ModelConfig::default();
        let (chunks, trace) = retrieve(&m, &cfg, "镜像拉取", &RetrieveOptions::default())
            .await
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(trace.mode, "fts");
        assert!(trace.vector_hits == 0 && !trace.reranked);
        assert!(chunks[0].sources.contains(&"fts".to_string()));
    }

    #[tokio::test]
    async fn retrieve_warns_when_vectors_missing() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let m = Mutex::new(conn);
        let cfg = ModelConfig {
            embedding: Some(models::EndpointConfig {
                base_url: "http://127.0.0.1:1".into(),
                model: "bge-m3".into(),
                params: serde_json::Value::Null,
                api_key: None,
            }),
            ..Default::default()
        };
        let (chunks, trace) = retrieve(&m, &cfg, "镜像拉取", &RetrieveOptions::default())
            .await
            .unwrap();
        // 索引为空 → 不发请求，直接降级提示
        assert_eq!(chunks.len(), 1);
        assert_eq!(trace.mode, "fts");
        assert_eq!(trace.degraded.len(), 1);
    }
}
