//! 块向量存储与增量构建。
//!
//! 设计取舍：个人知识库规模（几千块）下，暴力余弦足够快（毫秒级），
//! 因此不引入向量库依赖，向量直接以 f32 小端 BLOB 存在 SQLite 里。
//! 每个块只保留一份向量；切换嵌入模型后旧向量按 `model` 过滤、逐步覆盖。

use crate::models::{self, EndpointConfig};
use crate::notes::now_secs;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;

/// f32 向量 → 小端字节。
pub fn vec_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for f in vector {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// 小端字节 → f32 向量。
pub fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// 归一化（归一化后点积即余弦）。
pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// 点积。
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn lock<'a>(m: &'a Mutex<Connection>) -> Result<std::sync::MutexGuard<'a, Connection>> {
    m.lock().map_err(|_| anyhow!("数据库锁定失败（锁已中毒）"))
}

fn upsert_inner(conn: &Connection, chunk_id: i64, model: &str, vector: &[f32]) -> Result<()> {
    let v = normalize(vector.to_vec());
    conn.execute(
        r#"INSERT INTO chunk_vectors (chunk_id, model, dim, vec, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT(chunk_id) DO UPDATE SET
             model = excluded.model,
             dim = excluded.dim,
             vec = excluded.vec,
             updated_at = excluded.updated_at"#,
        params![chunk_id, model, v.len() as i64, vec_to_blob(&v), now_secs()],
    )?;
    Ok(())
}

/// 写入单条向量。
pub fn upsert(conn: &Connection, chunk_id: i64, model: &str, vector: &[f32]) -> Result<()> {
    upsert_inner(conn, chunk_id, model, vector)
}

/// 读取某模型的全部向量。
pub fn load_all(conn: &Connection, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT chunk_id, vec FROM chunk_vectors WHERE model = ?1")?;
    let rows = stmt.query_map(params![model], |r| {
        let id: i64 = r.get(0)?;
        let blob: Vec<u8> = r.get(1)?;
        Ok((id, blob_to_vec(&blob)))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 索引状态。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorStats {
    /// 全部块数
    pub chunks: i64,
    /// 当前模型的向量数
    pub vectors: i64,
    /// 当前模型缺向量的块数
    pub pending: i64,
    /// 其他模型遗留的向量数（切换嵌入模型后会逐步覆盖）
    pub stale_vectors: i64,
}

/// 统计索引状态。
pub fn stats(conn: &Connection, model: &str) -> Result<VectorStats> {
    let chunks: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let vectors: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chunk_vectors WHERE model = ?1",
        params![model],
        |r| r.get(0),
    )?;
    let stale_vectors: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chunk_vectors WHERE model <> ?1",
        params![model],
        |r| r.get(0),
    )?;
    let pending: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM chunks c
           LEFT JOIN chunk_vectors v ON v.chunk_id = c.id AND v.model = ?1
           WHERE v.chunk_id IS NULL"#,
        params![model],
        |r| r.get(0),
    )?;
    Ok(VectorStats { chunks, vectors, pending, stale_vectors })
}

/// 取一批缺向量的块。
pub fn pending_chunks(conn: &Connection, model: &str, limit: usize) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        r#"SELECT c.id, c.text FROM chunks c
           LEFT JOIN chunk_vectors v ON v.chunk_id = c.id AND v.model = ?1
           WHERE v.chunk_id IS NULL
           ORDER BY c.id LIMIT ?2"#,
    )?;
    let rows = stmt.query_map(params![model, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 增量构建结果。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedProgress {
    /// 本次写入的块数
    pub embedded: usize,
    /// 仍未构建的块数
    pub remaining: i64,
    /// 向量维度
    pub dim: usize,
    pub elapsed_ms: u64,
}

/// 增量构建向量索引：每批 `batch` 条，本次最多处理 `max_chunks` 条。
///
/// 面向长任务：调用方可以循环调用直到 `remaining == 0`（便于 UI 显示进度）。
pub async fn build(
    conn: &Mutex<Connection>,
    cfg: &EndpointConfig,
    batch: usize,
    max_chunks: usize,
) -> Result<EmbedProgress> {
    if cfg.base_url.trim().is_empty() {
        bail!("嵌入端点未配置");
    }
    let model = cfg.model.clone();
    let batch = batch.clamp(1, 64);
    let max_chunks = max_chunks.clamp(1, 4096);
    let started = Instant::now();
    let mut progress = EmbedProgress::default();

    loop {
        if progress.embedded >= max_chunks {
            break;
        }
        let take = batch.min(max_chunks - progress.embedded);
        let pending = {
            let c = lock(conn)?;
            pending_chunks(&c, &model, take)?
        };
        if pending.is_empty() {
            break;
        }
        let texts: Vec<String> = pending.iter().map(|(_, t)| t.clone()).collect();
        let vectors = models::embed(cfg, &texts).await?;
        if vectors.len() != pending.len() {
            bail!("嵌入返回条数不一致：期望 {}，实际 {}", pending.len(), vectors.len());
        }
        {
            let mut c = lock(conn)?;
            let tx = c.transaction()?;
            for ((chunk_id, _), vector) in pending.iter().zip(vectors.iter()) {
                progress.dim = vector.len();
                let v = normalize(vector.clone());
                tx.execute(
                    r#"INSERT INTO chunk_vectors (chunk_id, model, dim, vec, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5)
                       ON CONFLICT(chunk_id) DO UPDATE SET
                         model = excluded.model,
                         dim = excluded.dim,
                         vec = excluded.vec,
                         updated_at = excluded.updated_at"#,
                    params![chunk_id, model, v.len() as i64, vec_to_blob(&v), now_secs()],
                )?;
            }
            tx.commit()?;
        }
        progress.embedded += pending.len();
    }

    progress.remaining = {
        let c = lock(conn)?;
        stats(&c, &model)?.pending
    };
    progress.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn blob_roundtrip_and_normalize() {
        let v = vec![0.5f32, -1.25, 3.0];
        let back = blob_to_vec(&vec_to_blob(&v));
        assert_eq!(back, v);
        let n = normalize(vec![3.0, 4.0]);
        assert!((dot(&n, &n) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stats_counts_pending_and_stale() {
        let conn = db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO notes (id, title, folder, tags) VALUES ('a.md', 'A', '', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES ('a.md', 0, 1, 2, '内容一')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES ('a.md', 1, 3, 4, '内容二')",
            [],
        )
        .unwrap();

        let s = stats(&conn, "bge-m3").unwrap();
        assert_eq!((s.chunks, s.vectors, s.pending), (2, 0, 2));

        upsert(&conn, 1, "bge-m3", &[1.0, 0.0]).unwrap();
        let s = stats(&conn, "bge-m3").unwrap();
        assert_eq!((s.vectors, s.pending), (1, 1));

        // 换模型：旧向量算 stale，新模型下全部待构建
        let s = stats(&conn, "other-model").unwrap();
        assert_eq!((s.vectors, s.pending, s.stale_vectors), (0, 2, 1));
    }
}
