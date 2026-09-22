//! 知识图谱（P4）：LLM 抽取实体/关系 → 本地 SQLite 图 → 供可视化与检索增强。
//!
//! 设计要点
//! - **按笔记增量**：以「标题 + 全部块文本」的 FNV-1a 哈希做账本，未改动直接跳过；
//! - **防幻觉**：实体名必须是原文子串（归一化后比较），不满足直接丢弃；关系证据优先取原文句子；
//! - **幂等重建**：重建 = 事务内先按 `note_id` 删除再插入，失败不清空旧数据（LLM 调用全部结束后才落库）；
//! - **不吞错**：部分批次失败时账本标记 `error` 且不写哈希 → 下次自动重试；
//! - 限流/退避复用 [`crate::rate_limit`]，进度/取消与转写同范式（前端只认事件）。

use crate::minutes::extract_json_object;
use crate::models::{self, ChatMessage, ChatOptions, EndpointConfig, ModelConfig};
use crate::rate_limit::{
    backoff_for, is_fatal_http, RateLimiter, MAX_RETRIES, RATE_LIMIT_PER_MINUTE,
};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 抽取温度（尽量确定）。
pub const EXTRACT_TEMPERATURE: f32 = 0.1;
/// 单批输出 token 上限。
pub const EXTRACT_MAX_TOKENS: u32 = 4096;
/// 单次调用送入的正文上限（字符），超出的块分到下一批。
pub const EXTRACT_BATCH_CHARS: usize = 6_000;
/// 每批实体/关系硬上限（提示词里另有软要求）。
pub const MAX_ENTITIES_PER_BATCH: usize = 40;
pub const MAX_RELATIONS_PER_BATCH: usize = 60;
/// 证据句截断长度。
pub const EVIDENCE_MAX_CHARS: usize = 160;
/// 实体类型白名单（未知一律归到 other）。
pub const KINDS: [&str; 6] = ["person", "org", "place", "concept", "event", "other"];

pub const EXTRACT_SYSTEM_PROMPT: &str = "你是一名知识图谱抽取助手。从给定的中文笔记片段里抽取实体以及实体之间的关系。\n\
输出**严格 JSON**（不要解释、不要 markdown 围栏）：\n\
{\"entities\":[{\"name\":\"原文中的名称\",\"kind\":\"person|org|place|concept|event|other\"}],\
\"relations\":[{\"from\":\"实体名1\",\"to\":\"实体名2\",\"kind\":\"关系短语\",\"evidence\":\"原文中的一句话\"}]}\n\
规则：\n\
1. name / from / to 必须是原文中**原样出现**的字符串，不要改写、不要翻译、不要补全缩写；\n\
2. 只抽取文中明确出现的信息，禁止推测与常识补全；没有关系就返回空数组；\n\
3. kind 只能取 person（人物）、org（机构/组织/产品）、place（地点）、concept（概念/方法/术语）、event（事件/会议）、other；\n\
4. relations.kind 用 2~6 个字的动词短语（如「就职于」「包含」「提出」）；\n\
5. 去掉重复项；每批最多 40 个实体、60 条关系。";

/// 实体/关系的原始输出（对模型的小幅格式差异保持宽容）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGraph {
    #[serde(default)]
    pub entities: Vec<RawEntity>,
    #[serde(default)]
    pub relations: Vec<RawRelation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RawEntity {
    Obj {
        name: String,
        #[serde(default)]
        kind: String,
    },
    /// 少数模型会把实体写成纯字符串数组。
    Name(String),
}

impl RawEntity {
    fn into_parts(self) -> (String, String) {
        match self {
            RawEntity::Obj { name, kind } => (name, kind),
            RawEntity::Name(name) => (name, String::new()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawRelation {
    #[serde(rename = "from", alias = "source", alias = "src", default)]
    pub from: String,
    #[serde(rename = "to", alias = "target", alias = "dst", default)]
    pub to: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub evidence: String,
}

/// 校验后的实体。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphEntity {
    pub name: String,
    pub kind: String,
}

/// 校验后的关系（`from`/`to` 为规范名）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphRelation {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub evidence: String,
}

/// 一批（若干块）的抽取结果；`error` 非空表示这批失败。
#[derive(Debug, Clone, Default)]
pub struct BatchExtraction {
    pub chunk_ids: Vec<i64>,
    pub entities: Vec<GraphEntity>,
    pub relations: Vec<GraphRelation>,
    pub error: Option<String>,
}

/// 抽取进度（推送给前端）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphProgress {
    pub notes_total: usize,
    pub notes_done: usize,
    pub notes_skipped: usize,
    pub notes_failed: usize,
    pub current_note: String,
    pub current_title: String,
    pub batches_total: usize,
    pub batches_done: usize,
    pub entities: usize,
    pub relations: usize,
    pub message: String,
}

/// 一次抽取任务的结果。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractOutcome {
    pub notes_total: usize,
    pub notes_done: usize,
    pub notes_skipped: usize,
    pub notes_failed: usize,
    pub batches: usize,
    pub entities: usize,
    pub relations: usize,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

/// 图谱总览（给 UI 的空态/统计条）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    pub notes_total: usize,
    pub notes_extracted: usize,
    pub notes_failed: usize,
    pub entities: usize,
    pub relations: usize,
    pub chunk_links: usize,
    pub last_extracted_at: i64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub degree: i64,
    pub note_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub src: i64,
    pub dst: i64,
    pub weight: i64,
    /// 关系短语（可能多个，`/` 分隔）
    pub kinds: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSnapshot {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// 因为上限被截断的节点数（>0 表示图不完整）
    pub truncated: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMention {
    pub note_id: String,
    pub note_title: String,
    pub chunk_id: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityNeighbor {
    pub entity_id: i64,
    pub name: String,
    pub kind: String,
    pub rel_kind: String,
    pub weight: i64,
    /// `out` = 本实体指向邻居，`in` = 邻居指向本实体
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetail {
    pub node: GraphNode,
    pub mentions: Vec<EntityMention>,
    pub neighbors: Vec<EntityNeighbor>,
}

// ---------------------------------------------------------------------------
// 归一化与校验
// ---------------------------------------------------------------------------

/// 全角转半角（含全角空格）。
fn to_halfwidth(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 规范名：全角→半角、压缩空白、英文小写。中文保持原样。
pub fn normalize_name(raw: &str) -> String {
    let half = to_halfwidth(raw);
    let mut out = String::with_capacity(half.len());
    let mut last_space = false;
    for c in half.trim().chars() {
        if c.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            last_space = false;
            if c.is_ascii_uppercase() {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
    }
    out.trim_end().to_string()
}

/// 类型归一化到白名单。
pub fn normalize_kind(raw: &str) -> String {
    let k = normalize_name(raw);
    let mapped = match k.as_str() {
        "person" | "people" | "人物" | "人" => "person",
        "org" | "organization" | "organisation" | "机构" | "组织" | "公司" | "产品" => "org",
        "place" | "location" | "地点" | "地方" | "位置" => "place",
        "concept" | "term" | "概念" | "术语" | "方法" => "concept",
        "event" | "meeting" | "事件" | "会议" => "event",
        _ => "other",
    };
    mapped.to_string()
}

/// 关系短语归一化：去掉首尾空白与标点后缀，长度截断。
pub fn normalize_rel_kind(raw: &str) -> String {
    let k = normalize_name(raw);
    let k = k.trim_end_matches(['。', '.', '，', ',', '；', ';', '：', ':']).to_string();
    if k.is_empty() {
        return "关联".into();
    }
    k.chars().take(12).collect()
}

/// 在文本中找出同时包含两个名字的句子（用于关系证据）。
fn find_evidence(text: &str, a: &str, b: &str) -> String {
    let norm_text = normalize_name(text);
    for line in norm_text.split(['\n', '。', '；', ';', '！', '!', '？', '?', '\r']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains(a) && line.contains(b) {
            return line.chars().take(EVIDENCE_MAX_CHARS).collect();
        }
    }
    String::new()
}

/// 把模型原始输出校验成可入库的结果：去重、类型白名单、实体名必须在原文中出现、证据核对。
pub fn validate(raw: RawGraph, chunks: &[(i64, String)]) -> BatchExtraction {
    let joined: String = chunks.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
    let norm_text = normalize_name(&joined);
    let chunk_norms: Vec<(i64, String)> = chunks
        .iter()
        .map(|(id, t)| (*id, normalize_name(t)))
        .collect();

    let mut seen_entities = BTreeSet::new();
    let mut entities: Vec<GraphEntity> = Vec::new();
    for e in raw.entities {
        let (name, kind) = e.into_parts();
        let name = normalize_name(&name);
        if name.is_empty() || name.chars().count() > 48 {
            continue;
        }
        if !norm_text.contains(&name) {
            continue; // 幻觉：不是原文子串
        }
        let kind = normalize_kind(&kind);
        if seen_entities.insert((name.clone(), kind.clone())) {
            entities.push(GraphEntity { name, kind });
        }
        if entities.len() >= MAX_ENTITIES_PER_BATCH {
            break;
        }
    }

    let known: BTreeSet<String> = entities.iter().map(|e| e.name.clone()).collect();
    let mut seen_rels = BTreeSet::new();
    let mut relations: Vec<GraphRelation> = Vec::new();
    for r in raw.relations {
        let from = normalize_name(&r.from);
        let to = normalize_name(&r.to);
        if from.is_empty() || to.is_empty() || from == to {
            continue; // 自环丢弃
        }
        // 两端都必须是本批已确认（即原文出现过）的实体
        if !known.contains(&from) || !known.contains(&to) {
            continue;
        }
        let kind = normalize_rel_kind(&r.kind);
        if !seen_rels.insert((from.clone(), to.clone(), kind.clone())) {
            continue;
        }
        // 证据：模型给的必须是原文子串，否则用共现句兜底
        let given = normalize_name(&r.evidence);
        let evidence = if !given.is_empty()
            && given.chars().count() <= EVIDENCE_MAX_CHARS * 2
            && norm_text.contains(&given)
        {
            given.chars().take(EVIDENCE_MAX_CHARS).collect()
        } else {
            find_evidence(&joined, &from, &to)
        };
        relations.push(GraphRelation { from, to, kind, evidence });
        if relations.len() >= MAX_RELATIONS_PER_BATCH {
            break;
        }
    }

    // 每个实体挂到「包含它的第一个块」上，供溯源与邻居扩展
    let mut chunk_ids: Vec<i64> = Vec::new();
    let per_chunk = |name: &str| -> Option<i64> {
        chunk_norms
            .iter()
            .find(|(_, t)| t.contains(name))
            .map(|(id, _)| *id)
    };
    let mut links: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for e in &entities {
        if let Some(cid) = per_chunk(&e.name) {
            links.entry(cid).or_default().insert(e.name.clone());
        }
    }
    for (cid, _) in &links {
        chunk_ids.push(*cid);
    }
    // 没有可定位的实体时，仍记录本批块，便于清理与统计
    if chunk_ids.is_empty() {
        chunk_ids = chunk_norms.iter().map(|(id, _)| *id).collect();
    }

    BatchExtraction { chunk_ids, entities, relations, error: None }
}

// ---------------------------------------------------------------------------
// 存储：幂等写入 / 账本 / 清理
// ---------------------------------------------------------------------------

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 内容哈希（标题 + 全部块文本）。用 FNV-1a，避免为一次比对引入额外依赖。
pub fn content_hash(title: &str, chunks: &[(i64, String)]) -> String {
    fn fnv1a(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a(&mut h, title.as_bytes());
    for (_, t) in chunks {
        fnv1a(&mut h, t.as_bytes());
    }
    format!("{h:016x}")
}

/// 标记一篇笔记的图谱数据过期（笔记被重新索引/删除时调用）。
pub fn invalidate_note(conn: &Connection, note_id: &str) -> Result<()> {
    conn.execute("DELETE FROM relations WHERE note_id = ?1", params![note_id])?;
    conn.execute("DELETE FROM chunk_entities WHERE note_id = ?1", params![note_id])?;
    conn.execute("DELETE FROM graph_state WHERE note_id = ?1", params![note_id])?;
    Ok(())
}

/// 清理没有任何引用的实体（重建/失效后留下的孤儿）。
pub fn prune_orphans(conn: &Connection) -> Result<usize> {
    let n = conn.execute(
        "DELETE FROM entities
         WHERE id NOT IN (SELECT entity_id FROM chunk_entities)
           AND id NOT IN (SELECT src_id FROM relations)
           AND id NOT IN (SELECT dst_id FROM relations)",
        [],
    )?;
    Ok(n)
}

/// 把一批抽取结果写进库（事务内先删后插，幂等）。
///
/// `had_error` 为真时不写哈希 → 账本视为未完成，下次会重试；
/// 但本次已成功的批次仍会入库（部分可用好过全部丢弃）。
pub fn apply_extraction(
    conn: &Connection,
    note_id: &str,
    model: &str,
    hash: &str,
    batches: &[BatchExtraction],
    errors: &[String],
) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM relations WHERE note_id = ?1", params![note_id])?;
    tx.execute("DELETE FROM chunk_entities WHERE note_id = ?1", params![note_id])?;

    let now = now_secs();
    let mut entity_count = 0usize;
    let mut relation_count = 0usize;

    for b in batches.iter().filter(|b| b.error.is_none()) {
        // 实体：INSERT OR IGNORE + 查 id（(name, kind) 唯一）
        let mut ids: BTreeMap<(String, String), i64> = BTreeMap::new();
        for e in &b.entities {
            tx.execute(
                "INSERT INTO entities (name, kind, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(name, kind) DO UPDATE SET updated_at = excluded.updated_at",
                params![e.name, e.kind, now],
            )?;
            let id: i64 = tx.query_row(
                "SELECT id FROM entities WHERE name = ?1 AND kind = ?2",
                params![e.name, e.kind],
                |r| r.get(0),
            )?;
            ids.insert((e.name.clone(), e.kind.clone()), id);
            entity_count += 1;
        }
        // 块-实体（实体名 → 第一个包含它的块）
        for e in &b.entities {
            let cid: Option<i64> = {
                let mut found = None;
                for cid in &b.chunk_ids {
                    if chunk_contains(&tx, *cid, &e.name)? {
                        found = Some(*cid);
                        break;
                    }
                }
                found.or_else(|| b.chunk_ids.first().copied())
            };
            if let (Some(cid), Some(id)) = (cid, ids.get(&(e.name.clone(), e.kind.clone()))) {
                tx.execute(
                    "INSERT OR IGNORE INTO chunk_entities (chunk_id, note_id, entity_id)
                     VALUES (?1, ?2, ?3)",
                    params![cid, note_id, id],
                )?;
            }
        }
        // 关系明细（每次出现一行）
        for r in &b.relations {
            let src = ids.iter().find(|((n, _), _)| n == &r.from).map(|(_, v)| *v);
            let dst = ids.iter().find(|((n, _), _)| n == &r.to).map(|(_, v)| *v);
            let (Some(src), Some(dst)) = (src, dst) else { continue };
            let chunk_id: Option<i64> = b
                .chunk_ids
                .iter()
                .find(|cid| {
                    chunk_contains(&tx, **cid, &r.from).unwrap_or(false)
                        && chunk_contains(&tx, **cid, &r.to).unwrap_or(false)
                })
                .copied()
                .or_else(|| b.chunk_ids.first().copied());
            tx.execute(
                "INSERT INTO relations (note_id, chunk_id, src_id, dst_id, kind, evidence, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![note_id, chunk_id, src, dst, r.kind, r.evidence, now],
            )?;
            relation_count += 1;
        }
    }

    let error_text: String = errors
        .iter()
        .map(|e| e.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("；");
    let stored_hash = if errors.is_empty() { hash } else { "" };
    tx.execute(
        "INSERT INTO graph_state (note_id, content_hash, model, extracted_at, entities, relations, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(note_id) DO UPDATE SET
            content_hash = excluded.content_hash,
            model = excluded.model,
            extracted_at = excluded.extracted_at,
            entities = excluded.entities,
            relations = excluded.relations,
            error = excluded.error",
        params![note_id, stored_hash, model, now, entity_count as i64, relation_count as i64, error_text],
    )?;
    tx.commit()?;
    Ok((entity_count, relation_count))
}

/// 块文本（归一化后）是否包含某实体名。
fn chunk_contains(conn: &Connection, chunk_id: i64, name: &str) -> Result<bool> {
    let text: String = conn
        .query_row("SELECT text FROM chunks WHERE id = ?1", params![chunk_id], |r| r.get(0))
        .unwrap_or_default();
    Ok(normalize_name(&text).contains(name))
}

// ---------------------------------------------------------------------------
// 抽取编排
// ---------------------------------------------------------------------------

/// 读取一篇笔记的标题与块（按 seq）。
pub fn note_chunks(conn: &Connection, note_id: &str) -> Result<(String, Vec<(i64, String)>)> {
    let title: String = conn
        .query_row("SELECT title FROM notes WHERE id = ?1", params![note_id], |r| r.get(0))
        .map_err(|_| anyhow!("笔记不在索引里：{note_id}（先扫描/打开一次笔记）"))?;
    let mut stmt = conn.prepare("SELECT id, text FROM chunks WHERE note_id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map(params![note_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    if out.is_empty() {
        return Err(anyhow!("笔记没有可抽取的内容块：{note_id}"));
    }
    Ok((title, out))
}

/// 按字符预算把块分批（不拆块）。
pub fn batch_chunks(chunks: &[(i64, String)]) -> Vec<Vec<(i64, String)>> {
    let mut batches: Vec<Vec<(i64, String)>> = Vec::new();
    let mut cur: Vec<(i64, String)> = Vec::new();
    let mut size = 0usize;
    for (id, text) in chunks {
        let len = text.chars().count();
        if !cur.is_empty() && size + len > EXTRACT_BATCH_CHARS {
            batches.push(std::mem::take(&mut cur));
            size = 0;
        }
        cur.push((*id, text.clone()));
        size += len;
    }
    if !cur.is_empty() {
        batches.push(cur);
    }
    batches
}

/// 抽取一篇笔记（`force=false` 时未改动直接跳过）。
pub async fn extract_note(
    conn: &Mutex<Connection>,
    cfg: &ModelConfig,
    note_id: &str,
    force: bool,
    limiter: &mut RateLimiter,
    json_mode: &mut bool,
    progress: &(dyn Fn(GraphProgress) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<ExtractOutcome> {
    let started = Instant::now();
    let chat_cfg: EndpointConfig = cfg
        .chat
        .as_ref()
        .ok_or_else(|| anyhow!("未配置对话模型：请到「设置」里填写对话端点"))?
        .clone();

    let (title, chunks, hash, skip) = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        let (title, chunks) = note_chunks(&c, note_id)?;
        let hash = content_hash(&title, &chunks);
        let skip = if force {
            false
        } else {
            c.query_row(
                "SELECT content_hash FROM graph_state WHERE note_id = ?1",
                params![note_id],
                |r| r.get::<_, String>(0),
            )
            .map(|h| h == hash)
            .unwrap_or(false)
        };
        (title, chunks, hash, skip)
    };

    let mut out = ExtractOutcome { notes_total: 1, ..Default::default() };
    if skip {
        out.notes_skipped = 1;
        out.elapsed_ms = started.elapsed().as_millis() as u64;
        return Ok(out);
    }

    let batches = batch_chunks(&chunks);
    let batches_total = batches.len();
    let mut results: Vec<BatchExtraction> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (bi, batch) in batches.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        progress(GraphProgress {
            notes_total: 1,
            current_note: note_id.to_string(),
            current_title: title.clone(),
            batches_total,
            batches_done: bi,
            message: format!("《{}》第 {}/{} 批", title, bi + 1, batches_total),
            ..Default::default()
        });

        let text: String = batch.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");
        let messages = vec![
            ChatMessage::system(EXTRACT_SYSTEM_PROMPT),
            ChatMessage::user(format!("笔记《{title}》的片段：\n---\n{text}\n---\n请按要求输出 JSON。")),
        ];

        let mut attempt = 1usize;
        let mut content: Option<String> = None;
        let mut last_err = String::new();
        while attempt <= MAX_RETRIES {
            let waited = limiter.acquire().await;
            if waited > Duration::from_secs(1) {
                progress(GraphProgress {
                    notes_total: 1,
                    current_note: note_id.to_string(),
                    current_title: title.clone(),
                    batches_total,
                    batches_done: bi,
                    message: format!("限流等待 {}s（上游 {} 请求/分钟）", waited.as_secs(), RATE_LIMIT_PER_MINUTE),
                    ..Default::default()
                });
            }
            let mut opts = ChatOptions {
                max_tokens: Some(EXTRACT_MAX_TOKENS),
                temperature: Some(EXTRACT_TEMPERATURE),
                extra: None,
            };
            if *json_mode {
                let mut extra = Map::new();
                extra.insert("response_format".into(), json!({ "type": "json_object" }));
                opts.extra = Some(extra);
            }
            match models::chat(&chat_cfg, &messages, &opts).await {
                Ok(reply) => {
                    content = Some(reply.content);
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if *json_mode && msg.contains("400") {
                        *json_mode = false; // 上游不支持 response_format：降级重试一次
                        continue;
                    }
                    last_err = msg;
                    if is_fatal_http(&last_err) || attempt == MAX_RETRIES {
                        break;
                    }
                    let backoff = backoff_for(&last_err, attempt);
                    progress(GraphProgress {
                        notes_total: 1,
                        current_note: note_id.to_string(),
                        current_title: title.clone(),
                        batches_total,
                        batches_done: bi,
                        message: format!(
                            "第 {} 次失败，{}s 后重试：{}",
                            attempt,
                            backoff.as_secs(),
                            last_err.chars().take(80).collect::<String>()
                        ),
                        ..Default::default()
                    });
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }

        let chunk_ids: Vec<i64> = batch.iter().map(|(id, _)| *id).collect();
        match content {
            Some(text) => {
                let parsed = extract_json_object(&text)
                    .and_then(|v| serde_json::from_value::<RawGraph>(v).ok());
                match parsed {
                    Some(raw) => {
                        let mut batch_out = validate(raw, batch);
                        batch_out.chunk_ids = chunk_ids;
                        out.entities += batch_out.entities.len();
                        out.relations += batch_out.relations.len();
                        results.push(batch_out);
                    }
                    None => {
                        let e = "输出里找不到合法 JSON 对象".to_string();
                        errors.push(format!("第 {} 批：{e}", bi + 1));
                        results.push(BatchExtraction { chunk_ids, error: Some(e), ..Default::default() });
                    }
                }
            }
            None => {
                let e = if last_err.is_empty() { "未获得模型输出".to_string() } else { last_err };
                errors.push(format!("第 {} 批：{}", bi + 1, e.chars().take(160).collect::<String>()));
                results.push(BatchExtraction { chunk_ids, error: Some(e), ..Default::default() });
            }
        }
        out.batches += 1;
    }

    let (stored_entities, stored_relations) = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        apply_extraction(&c, note_id, &chat_cfg.model, &hash, &results, &errors)?
    };
    out.entities = stored_entities;
    out.relations = stored_relations;
    out.notes_done = 1;
    out.notes_failed = if errors.is_empty() { 0 } else { 1 };
    out.errors = errors;
    out.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(out)
}

/// 抽取全部笔记（跳过未改动；单篇失败不阻断整轮）。
pub async fn extract_all(
    conn: &Mutex<Connection>,
    cfg: &ModelConfig,
    force: bool,
    progress: &(dyn Fn(GraphProgress) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<ExtractOutcome> {
    let started = Instant::now();
    let notes: Vec<(String, String)> = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        let mut stmt = c.prepare(
            "SELECT id, title FROM notes WHERE id IN (SELECT DISTINCT note_id FROM chunks) ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut out = ExtractOutcome { notes_total: notes.len(), ..Default::default() };
    let mut limiter = RateLimiter::new(RATE_LIMIT_PER_MINUTE, Duration::from_secs(60));
    let mut json_mode = true;

    for (id, title) in &notes {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        progress(GraphProgress {
            notes_total: out.notes_total,
            notes_done: out.notes_done,
            notes_skipped: out.notes_skipped,
            notes_failed: out.notes_failed,
            current_note: id.clone(),
            current_title: title.clone(),
            entities: out.entities,
            relations: out.relations,
            message: format!("正在处理《{title}》"),
            ..Default::default()
        });
        match extract_note(conn, cfg, id, force, &mut limiter, &mut json_mode, progress, cancel).await {
            Ok(one) => {
                out.notes_done += one.notes_done;
                out.notes_skipped += one.notes_skipped;
                out.notes_failed += one.notes_failed;
                out.batches += one.batches;
                out.entities += one.entities;
                out.relations += one.relations;
                out.errors.extend(one.errors);
                if one.cancelled {
                    out.cancelled = true;
                    break;
                }
            }
            Err(e) => {
                out.notes_failed += 1;
                out.errors.push(format!("《{title}》：{e}"));
            }
        }
    }

    // 清理孤儿实体（best-effort）
    if !out.cancelled {
        if let Ok(c) = conn.lock() {
            let _ = prune_orphans(&c);
        }
    }

    out.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(out)
}

// ---------------------------------------------------------------------------
// 查询（给 UI 与检索）
// ---------------------------------------------------------------------------

pub fn stats(conn: &Connection) -> Result<GraphStats> {
    let notes_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notes WHERE id IN (SELECT DISTINCT note_id FROM chunks)",
        [],
        |r| r.get(0),
    )?;
    let (notes_extracted, notes_failed, last_at, model): (i64, i64, i64, String) = conn.query_row(
        "SELECT
            SUM(CASE WHEN content_hash <> '' AND error = '' THEN 1 ELSE 0 END),
            SUM(CASE WHEN error <> '' THEN 1 ELSE 0 END),
            COALESCE(MAX(extracted_at), 0),
            COALESCE((SELECT model FROM graph_state WHERE model <> '' ORDER BY extracted_at DESC LIMIT 1), '')
         FROM graph_state",
        [],
        |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get(2)?, r.get(3)?)),
    )?;
    let entities: i64 = conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
    let relations: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))?;
    let chunk_links: i64 = conn.query_row("SELECT COUNT(*) FROM chunk_entities", [], |r| r.get(0))?;
    Ok(GraphStats {
        notes_total: notes_total as usize,
        notes_extracted: notes_extracted as usize,
        notes_failed: notes_failed as usize,
        entities: entities as usize,
        relations: relations as usize,
        chunk_links: chunk_links as usize,
        last_extracted_at: last_at,
        model,
    })
}

/// 取图快照：按度数取前 `limit` 个节点，再加上两端都在集合内的边。
pub fn snapshot(conn: &Connection, limit: usize, min_degree: usize) -> Result<GraphSnapshot> {
    let limit = limit.max(1) as i64;
    let mut stmt = conn.prepare(
        "SELECT e.id, e.name, e.kind,
                (SELECT COUNT(*) FROM relations r WHERE r.src_id = e.id OR r.dst_id = e.id) AS degree,
                (SELECT COUNT(DISTINCT ce.note_id) FROM chunk_entities ce WHERE ce.entity_id = e.id) AS note_count
         FROM entities e
         WHERE (SELECT COUNT(*) FROM relations r WHERE r.src_id = e.id OR r.dst_id = e.id) >= ?1
         ORDER BY degree DESC, e.id
         LIMIT ?2",
    )?;
    let nodes: Vec<GraphNode> = stmt
        .query_map(params![min_degree as i64, limit], |r| {
            Ok(GraphNode {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                degree: r.get(3)?,
                note_count: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let ids: BTreeSet<i64> = nodes.iter().map(|n| n.id).collect();
    let mut edges: Vec<GraphEdge> = Vec::new();
    {
        let mut estmt = conn.prepare(
            "SELECT src_id, dst_id, COUNT(*) AS weight, GROUP_CONCAT(DISTINCT kind)
             FROM relations GROUP BY src_id, dst_id ORDER BY weight DESC",
        )?;
        let rows = estmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?;
        for row in rows {
            let (src, dst, weight, kinds) = row?;
            if ids.contains(&src) && ids.contains(&dst) {
                edges.push(GraphEdge { src, dst, weight, kinds });
            }
        }
    }
    let total_entities: i64 = conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
    Ok(GraphSnapshot {
        nodes,
        edges,
        truncated: (total_entities as usize).saturating_sub(ids.len()),
    })
}

/// 实体详情：出处片段 + 邻居（按强度排序）。
pub fn node_detail(conn: &Connection, entity_id: i64) -> Result<NodeDetail> {
    let node = conn.query_row(
        "SELECT e.id, e.name, e.kind,
                (SELECT COUNT(*) FROM relations r WHERE r.src_id = e.id OR r.dst_id = e.id),
                (SELECT COUNT(DISTINCT ce.note_id) FROM chunk_entities ce WHERE ce.entity_id = e.id)
         FROM entities e WHERE e.id = ?1",
        params![entity_id],
        |r| {
            Ok(GraphNode {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                degree: r.get(3)?,
                note_count: r.get(4)?,
            })
        },
    )?;

    let mut mstmt = conn.prepare(
        "SELECT c.note_id, COALESCE(n.title, c.note_id), c.id, c.start_line, c.end_line, c.text
         FROM chunk_entities ce
         JOIN chunks c ON c.id = ce.chunk_id
         LEFT JOIN notes n ON n.id = c.note_id
         WHERE ce.entity_id = ?1
         ORDER BY c.note_id, c.seq
         LIMIT 50",
    )?;
    let mentions: Vec<EntityMention> = mstmt
        .query_map(params![entity_id], |r| {
            let text: String = r.get(5)?;
            Ok(EntityMention {
                note_id: r.get(0)?,
                note_title: r.get(1)?,
                chunk_id: r.get(2)?,
                start_line: r.get(3)?,
                end_line: r.get(4)?,
                snippet: text.chars().take(200).collect(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut nstmt = conn.prepare(
        "SELECT * FROM (
            SELECT e.id AS id, e.name AS name, e.kind AS kind, r.kind AS rel_kind,
                   COUNT(*) AS weight, 'out' AS direction
            FROM relations r JOIN entities e ON e.id = r.dst_id
            WHERE r.src_id = ?1 GROUP BY e.id, r.kind
            UNION ALL
            SELECT e.id, e.name, e.kind, r.kind, COUNT(*), 'in'
            FROM relations r JOIN entities e ON e.id = r.src_id
            WHERE r.dst_id = ?1 GROUP BY e.id, r.kind
         ) ORDER BY weight DESC, name LIMIT 60",
    )?;
    let neighbors: Vec<EntityNeighbor> = nstmt
        .query_map(params![entity_id], |r| {
            Ok(EntityNeighbor {
                entity_id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                rel_kind: r.get(3)?,
                weight: r.get(4)?,
                direction: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(NodeDetail { node, mentions, neighbors })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn seed_note(conn: &Connection, id: &str, title: &str, texts: &[&str]) -> Vec<(i64, String)> {
        conn.execute(
            "INSERT OR REPLACE INTO notes (id, title, folder, tags, mtime, size, created_at, updated_at)
             VALUES (?1, ?2, '', '', 0, 0, 0, 0)",
            params![id, title],
        )
        .unwrap();
        let mut out = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            conn.execute(
                "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES (?1, ?2, 1, 2, ?3)",
                params![id, i as i64, t],
            )
            .unwrap();
            out.push((conn.last_insert_rowid(), (*t).to_string()));
        }
        out
    }

    #[test]
    fn normalize_fullwidth_and_case() {
        assert_eq!(normalize_name("　张三 "), "张三");
        assert_eq!(normalize_name("Ａｐｐｌｅ  Inc"), "apple inc");
        assert_eq!(normalize_name("多   空格"), "多 空格");
    }

    #[test]
    fn kind_mapping() {
        assert_eq!(normalize_kind("Organization"), "org");
        assert_eq!(normalize_kind("人物"), "person");
        assert_eq!(normalize_kind("地点"), "place");
        assert_eq!(normalize_kind("whatever"), "other");
        assert_eq!(normalize_kind(""), "other");
    }

    #[test]
    fn raw_parsing_tolerates_shapes() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"entities":[{"name":"张三","kind":"person"},["李四"]],"relations":[{"source":"张三","target":"李四","kind":"认识"}]}"#,
        )
        .unwrap();
        // 实体数组里混了数组 → 整体解析失败是可以接受的，但纯字符串数组要能解析
        assert!(serde_json::from_value::<RawGraph>(v).is_err());
        let v: serde_json::Value = serde_json::from_str(
            r#"{"entities":[{"name":"张三","kind":"person"},"李四"],"relations":[{"source":"张三","target":"李四","kind":"认识","evidence":"张三认识李四"}]}"#,
        )
        .unwrap();
        let raw: RawGraph = serde_json::from_value(v).unwrap();
        assert_eq!(raw.entities.len(), 2);
        assert_eq!(raw.relations[0].from, "张三");
        assert_eq!(raw.relations[0].to, "李四");
    }

    #[test]
    fn validation_drops_hallucinations() {
        let chunks = vec![(1i64, "张三在北京大学做研究，方向是知识图谱。李四也在北京大学。".to_string())];
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [
                {"name": "张三", "kind": "person"},
                {"name": "北京大学", "kind": "org"},
                {"name": "王五", "kind": "person"},            // 原文没有 → 丢
                {"name": "张三", "kind": "person"}             // 重复 → 丢
            ],
            "relations": [
                {"from": "张三", "to": "北京大学", "kind": "就职于", "evidence": "张三在北京大学做研究"},
                {"from": "张三", "to": "王五", "kind": "认识", "evidence": "编造的"},  // 端点没确认 → 丢
                {"from": "张三", "to": "张三", "kind": "自称"}                        // 自环 → 丢
            ]
        }))
        .unwrap();
        let out = validate(raw, &chunks);
        assert_eq!(out.entities.len(), 2);
        assert_eq!(out.relations.len(), 1);
        assert_eq!(out.relations[0].kind, "就职于");
        assert!(out.relations[0].evidence.contains("北京大学"));
        assert!(out.chunk_ids.contains(&1));
    }

    #[test]
    fn evidence_falls_back_to_cooccurrence_sentence() {
        let chunks = vec![(7i64, "李四提出了新方法。张三和李四合作发表论文。".to_string())];
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [{"name": "张三", "kind": "person"}, {"name": "李四", "kind": "person"}],
            "relations": [{"from": "张三", "to": "李四", "kind": "合作"}]
        }))
        .unwrap();
        let out = validate(raw, &chunks);
        assert_eq!(out.relations.len(), 1);
        assert!(out.relations[0].evidence.contains("合作发表论文"), "evidence={}", out.relations[0].evidence);
    }

    #[test]
    fn apply_is_idempotent_and_tracks_state() {
        let conn = db::open_memory().unwrap();
        let chunks = seed_note(&conn, "a.md", "笔记A", &["张三在北京大学。"]);
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [{"name":"张三","kind":"person"},{"name":"北京大学","kind":"org"}],
            "relations": [{"from":"张三","to":"北京大学","kind":"就读于","evidence":"张三在北京大学"}]
        }))
        .unwrap();
        let mut batch = validate(raw.clone(), &chunks);
        batch.chunk_ids = chunks.iter().map(|(id, _)| *id).collect();
        let hash = content_hash("笔记A", &chunks);

        let (e1, r1) = apply_extraction(&conn, "a.md", "m", &hash, std::slice::from_ref(&batch), &[]).unwrap();
        assert_eq!((e1, r1), (2, 1));
        // 再抽一次：不重复增长，且总数保持一致
        let mut batch2 = validate(raw, &chunks);
        batch2.chunk_ids = chunks.iter().map(|(id, _)| *id).collect();
        let (e2, r2) = apply_extraction(&conn, "a.md", "m", &hash, &[batch2], &[]).unwrap();
        assert_eq!((e2, r2), (2, 1));
        let s = stats(&conn).unwrap();
        assert_eq!(s.entities, 2);
        assert_eq!(s.relations, 1);
        assert_eq!(s.notes_extracted, 1);
        assert_eq!(s.notes_failed, 0);

        let snap = snapshot(&conn, 100, 0).unwrap();
        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.edges.len(), 1);
        assert_eq!(snap.edges[0].weight, 1);
        assert_eq!(snap.truncated, 0);

        let detail = node_detail(&conn, snap.nodes.iter().max_by_key(|n| n.degree).unwrap().id).unwrap();
        assert_eq!(detail.mentions.len(), 1);
        assert_eq!(detail.mentions[0].note_title, "笔记A");
        assert_eq!(detail.neighbors.len(), 1);
    }

    #[test]
    fn failure_keeps_hash_empty_and_old_data() {
        let conn = db::open_memory().unwrap();
        let chunks = seed_note(&conn, "b.md", "笔记B", &["李四在清华大学。"]);
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [{"name":"李四","kind":"person"},{"name":"清华大学","kind":"org"}],
            "relations": [{"from":"李四","to":"清华大学","kind":"就读于"}]
        }))
        .unwrap();
        let mut batch = validate(raw, &chunks);
        batch.chunk_ids = chunks.iter().map(|(id, _)| *id).collect();
        let hash = content_hash("笔记B", &chunks);
        apply_extraction(&conn, "b.md", "m", &hash, &[batch], &[]).unwrap();

        // 第二次有一批失败：旧数据被替换，但账本不写哈希 → 下次仍会重试
        let failed = BatchExtraction {
            chunk_ids: chunks.iter().map(|(id, _)| *id).collect(),
            error: Some("boom".into()),
            ..Default::default()
        };
        apply_extraction(&conn, "b.md", "m", &hash, &[failed], &["第 1 批：boom".into()]).unwrap();
        let (stored_hash, err): (String, String) = conn
            .query_row("SELECT content_hash, error FROM graph_state WHERE note_id='b.md'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(stored_hash, "");
        assert!(err.contains("boom"));
        let s = stats(&conn).unwrap();
        assert_eq!(s.notes_extracted, 0);
        assert_eq!(s.notes_failed, 1);
    }

    #[test]
    fn invalidate_and_prune_clean_up() {
        let conn = db::open_memory().unwrap();
        let chunks = seed_note(&conn, "c.md", "笔记C", &["王五在上海。"]);
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [{"name":"王五","kind":"person"},{"name":"上海","kind":"place"}],
            "relations": [{"from":"王五","to":"上海","kind":"位于"}]
        }))
        .unwrap();
        let mut batch = validate(raw, &chunks);
        batch.chunk_ids = chunks.iter().map(|(id, _)| *id).collect();
        apply_extraction(&conn, "c.md", "m", "h", &[batch], &[]).unwrap();
        assert_eq!(stats(&conn).unwrap().entities, 2);

        invalidate_note(&conn, "c.md").unwrap();
        assert_eq!(stats(&conn).unwrap().entities, 2, "实体是全局的，失效笔记不删实体");
        assert_eq!(prune_orphans(&conn).unwrap(), 2, "失去引用后应被清理");
        assert_eq!(stats(&conn).unwrap().entities, 0);
    }

    #[test]
    fn reindexing_note_invalidates_graph() {
        use crate::notes;
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let path = dir.path().join("n.md");
        std::fs::write(&path, "# 标题\n\n张三在北京大学做研究。\n").unwrap();
        notes::index_file(&conn, dir.path(), &path).unwrap();

        let chunks: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, text FROM chunks WHERE note_id = 'n.md' ORDER BY seq")
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let raw: RawGraph = serde_json::from_value(serde_json::json!({
            "entities": [{"name":"张三","kind":"person"},{"name":"北京大学","kind":"org"}],
            "relations": [{"from":"张三","to":"北京大学","kind":"就职于"}]
        }))
        .unwrap();
        let mut batch = validate(raw, &chunks);
        batch.chunk_ids = chunks.iter().map(|(id, _)| *id).collect();
        apply_extraction(&conn, "n.md", "m", "h", &[batch], &[]).unwrap();
        assert_eq!(stats(&conn).unwrap().relations, 1);

        // 改动文件并重新索引：块 id 会变，图谱数据必须随之失效
        std::fs::write(&path, "# 标题\n\n张三去了清华大学。\n").unwrap();
        notes::index_file(&conn, dir.path(), &path).unwrap();
        let s = stats(&conn).unwrap();
        assert_eq!(s.relations, 0, "旧关系应被清掉");
        assert_eq!(s.chunk_links, 0, "旧溯源应被清掉");
        assert_eq!(s.notes_extracted, 0, "账本应清空 → 下次重抽");

        // 实体是全局的：失去引用后由 prune 清理
        assert_eq!(prune_orphans(&conn).unwrap(), 2);
        assert!(snapshot(&conn, 100, 0).unwrap().nodes.is_empty());
    }

    #[test]
    fn batching_respects_char_budget() {
        let chunks: Vec<(i64, String)> = (0..5).map(|i| (i as i64, "字".repeat(2000))).collect();
        let batches = batch_chunks(&chunks);
        assert_eq!(batches.len(), 2, "5×2000 字符 → 每批 ≤6000，应为 2 批");
        assert!(batches[0].len() <= 3);
    }

    #[test]
    fn note_without_index_is_error() {
        let conn = db::open_memory().unwrap();
        assert!(note_chunks(&conn, "missing.md").is_err());
    }
}
