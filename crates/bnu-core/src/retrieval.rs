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

/// 图谱扩展这一路的 RRF 权重（低于全文/向量，避免喧宾夺主）。
pub const GRAPH_WEIGHT: f32 = 0.5;

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
    /// fts | hybrid | hybrid+rerank（+graph 表示图谱扩展参与）
    pub mode: String,
    pub fts_hits: usize,
    pub vector_hits: usize,
    pub reranked: bool,
    /// 图谱扩展召回的候选片段数
    pub graph_hits: usize,
    /// 最终结果里来自图谱扩展的片段数（>0 表示增强生效）
    pub graph_added: usize,
    /// 本次用到的种子实体数（命中片段的实体）
    pub graph_entities: usize,
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
    /// 是否用知识图谱做邻居扩展（图谱为空时自然无效，不会报错）
    pub expand_entities: bool,
    /// 图谱扩展最多补多少条候选
    pub expand_k: usize,
}

impl Default for RetrieveOptions {
    fn default() -> Self {
        Self {
            top_k: 6,
            candidate_k: 24,
            use_vectors: true,
            use_rerank: true,
            min_rerank_score: 0.05,
            expand_entities: true,
            expand_k: 4,
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

/// 这批片段涉及的实体 id（去重）。
pub fn entities_for_chunks(conn: &Connection, chunk_ids: &[i64]) -> Result<Vec<i64>> {
    if chunk_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT DISTINCT entity_id FROM chunk_entities WHERE chunk_id IN ({}) ORDER BY entity_id",
        placeholders(chunk_ids.len(), 1)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |r| r.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 生成 `?1, ?2, ...` 形式的占位符（SQLite 不支持数组参数）。
fn placeholders(n: usize, start: usize) -> String {
    (0..n).map(|i| format!("?{}", start + i)).collect::<Vec<_>>().join(", ")
}

/// 图谱邻居扩展：从命中片段出发，经「共享实体」找回其它相关片段。
///
/// 打分 = Σ 共享实体数 / 该实体出现的块数 —— 出现在越少块里的实体越"专指"，
/// 权重越高，避免「会议」「项目」这类高频实体把整个库都拉进来。
/// 图谱为空（没抽取过）时返回空，调用方自然降级，不报错。
pub fn graph_expand(conn: &Connection, seed_ids: &[i64], limit: usize) -> Result<Vec<i64>> {
    if seed_ids.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let n = seed_ids.len();
    let sql = format!(
        r#"WITH sel AS (
               SELECT entity_id, COUNT(*) AS shared
               FROM chunk_entities WHERE chunk_id IN ({seed_ph}) GROUP BY entity_id
           ), links AS (
               SELECT entity_id, COUNT(*) AS n FROM chunk_entities GROUP BY entity_id
           )
           SELECT ce.chunk_id, SUM(sel.shared * 1.0 / links.n) AS score
           FROM chunk_entities ce
           JOIN sel ON sel.entity_id = ce.entity_id
           JOIN links ON links.entity_id = ce.entity_id
           WHERE ce.chunk_id NOT IN ({seed_ph})
           GROUP BY ce.chunk_id
           ORDER BY score DESC, ce.chunk_id
           LIMIT ?{limit_ph}"#,
        seed_ph = placeholders(n, 1),
        limit_ph = 2 * n + 1,
    );
    let params_iter = seed_ids
        .iter()
        .copied()
        .chain(seed_ids.iter().copied())
        .chain(std::iter::once(limit.clamp(1, 200) as i64));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params_iter), |r| r.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
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
    /// 图谱实体/关系数（0 表示还没抽取过）
    pub graph_entities: i64,
    pub graph_relations: i64,
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
        graph_entities: conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?,
        graph_relations: conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))?,
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
    let mut scores = rrf_scores(&lists);
    let mut ids = rrf(&lists);
    ids.truncate(opts.candidate_k);

    // 3.5) 图谱邻居扩展：把命中片段经共享实体牵连出的片段并入融合（权重更低）
    let mut graph_hits: Vec<i64> = Vec::new();
    if opts.expand_entities && opts.expand_k > 0 && !ids.is_empty() {
        let (entities, expanded) = {
            let c = lock(conn)?;
            (entities_for_chunks(&c, &ids)?, graph_expand(&c, &ids, opts.expand_k)?)
        };
        trace.graph_entities = entities.len();
        if !expanded.is_empty() {
            graph_hits = expanded;
            trace.graph_hits = graph_hits.len();
            let lists: Vec<(&[i64], f32)> = vec![
                (fts_hits.as_slice(), 1.0),
                (vector_hits.as_slice(), 1.0),
                (graph_hits.as_slice(), GRAPH_WEIGHT),
            ];
            scores = rrf_scores(&lists);
            ids = rrf(&lists);
            ids.truncate(opts.candidate_k + opts.expand_k);
        }
    }

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
        if graph_hits.contains(&ch.chunk_id) {
            sources.push("graph".to_string());
        }
        if reranked {
            sources.push("rerank".to_string());
        }
        ch.sources = sources;
    }
    trace.graph_added = chunks.iter().filter(|c| c.sources.iter().any(|s| s == "graph")).count();

    trace.reranked = reranked;
    let mut mode = if reranked {
        "hybrid+rerank".to_string()
    } else if !vector_hits.is_empty() {
        "hybrid".to_string()
    } else {
        "fts".to_string()
    };
    if trace.graph_added > 0 {
        mode.push_str("+graph");
    }
    trace.mode = mode;
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
        // 没有图谱数据时扩展静默失效
        assert_eq!((trace.graph_hits, trace.graph_added, trace.graph_entities), (0, 0, 0));
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

    fn add_entity(conn: &Connection, name: &str, kind: &str) -> i64 {
        conn.execute("INSERT INTO entities (name, kind) VALUES (?1, ?2)", params![name, kind])
            .unwrap();
        conn.last_insert_rowid()
    }

    fn link_entity(conn: &Connection, chunk_id: i64, note_id: &str, entity_id: i64) {
        conn.execute(
            "INSERT OR IGNORE INTO chunk_entities (chunk_id, note_id, entity_id) VALUES (?1, ?2, ?3)",
            params![chunk_id, note_id, entity_id],
        )
        .unwrap();
    }

    #[test]
    fn graph_expand_prefers_rare_entities() {
        let conn = db::open_memory().unwrap();
        // c1 同时含「张三」「北京大学」「会议」；c2 共享北京大学；c4 共享张三；c3 无关
        conn.execute(
            "INSERT INTO notes (id, title) VALUES ('n.md', '笔记')",
            [],
        )
        .unwrap();
        let mut ids = Vec::new();
        for (seq, text) in [
            "张三在北京大学参加周会。",
            "北京大学的实验室在装修。",
            "读了 Rust 所有权相关章节。",
            "张三和李四合作发表论文。",
        ]
        .iter()
        .enumerate()
        {
            conn.execute(
                "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES ('n.md', ?1, 1, 2, ?2)",
                params![seq as i64, text],
            )
            .unwrap();
            ids.push(conn.last_insert_rowid());
        }
        let (c1, c2, c3, c4) = (ids[0], ids[1], ids[2], ids[3]);
        // 高频实体「会议」：挂到 c1 + 10 个填充块
        let e_meeting = add_entity(&conn, "会议", "event");
        let mut filler = Vec::new();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES ('n.md', ?1, 1, 2, '填充')",
                params![100 + i],
            )
            .unwrap();
            filler.push(conn.last_insert_rowid());
        }
        let e_zhang = add_entity(&conn, "张三", "person");
        let e_pku = add_entity(&conn, "北京大学", "org");
        link_entity(&conn, c1, "n.md", e_zhang);
        link_entity(&conn, c1, "n.md", e_pku);
        link_entity(&conn, c1, "n.md", e_meeting);
        link_entity(&conn, c2, "n.md", e_pku);
        link_entity(&conn, c4, "n.md", e_zhang);
        for f in &filler {
            link_entity(&conn, *f, "n.md", e_meeting);
        }

        let expanded = graph_expand(&conn, &[c1], 2).unwrap();
        assert_eq!(expanded, vec![c2, c4], "应优先共享低频实体（北京大学/张三），而不是高频的「会议」");
        assert!(!expanded.contains(&c3));
        // 种子为空 → 不扩展
        assert!(graph_expand(&conn, &[], 5).unwrap().is_empty());
        // 实体 id 去重
        assert_eq!(entities_for_chunks(&conn, &[c1, c2]).unwrap().len(), 3);
    }

    #[tokio::test]
    async fn retrieve_expands_via_graph_and_can_be_disabled() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let c1: i64 = conn
            .query_row("SELECT id FROM chunks WHERE note_id = '工作/周会.md'", [], |r| r.get(0))
            .unwrap();
        let c2: i64 = conn
            .query_row("SELECT id FROM chunks WHERE note_id = '读书.md'", [], |r| r.get(0))
            .unwrap();
        let e = add_entity(&conn, "镜像站", "concept");
        link_entity(&conn, c1, "工作/周会.md", e);
        link_entity(&conn, c2, "读书.md", e);
        let m = Mutex::new(conn);
        let cfg = ModelConfig::default();

        let (chunks, trace) = retrieve(&m, &cfg, "镜像拉取", &RetrieveOptions::default()).await.unwrap();
        assert_eq!(chunks.len(), 2, "图谱把读书笔记也带出来了");
        assert_eq!(trace.graph_entities, 1);
        assert_eq!(trace.graph_hits, 1);
        assert_eq!(trace.graph_added, 1);
        assert_eq!(trace.mode, "fts+graph");
        let extra = chunks.iter().find(|c| c.chunk_id == c2).unwrap();
        assert_eq!(extra.sources, vec!["graph".to_string()]);

        let opts = RetrieveOptions { expand_entities: false, ..Default::default() };
        let (chunks, trace) = retrieve(&m, &cfg, "镜像拉取", &opts).await.unwrap();
        assert_eq!(chunks.len(), 1, "关掉扩展后只返回全文命中");
        assert_eq!((trace.graph_hits, trace.graph_added), (0, 0));
    }
}
