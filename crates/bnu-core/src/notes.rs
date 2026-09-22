//! vault 扫描与索引、笔记读写、全文检索。

use crate::{chunk, markdown};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 笔记元信息。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    pub id: String,
    pub title: String,
    pub rel_path: String,
    pub folder: String,
    pub tags: String,
    pub mtime: i64,
    pub size: i64,
    pub updated_at: i64,
}

/// 检索命中。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub note_id: String,
    pub title: String,
    pub start_line: i64,
    pub end_line: i64,
    pub snippet: String,
}

/// 扫描统计。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
}

/// 相对路径规范化：统一用 `/`。
pub fn note_id(rel: &str) -> String {
    rel.replace('\\', "/")
}

/// 校验笔记 id 安全：不允许绝对路径与 `..` 穿越，防止越权访问 vault 之外的文件。
pub fn ensure_safe_id(id: &str) -> Result<()> {
    anyhow::ensure!(!id.is_empty(), "笔记 id 不能为空");
    anyhow::ensure!(!id.contains('\0'), "笔记 id 非法");
    anyhow::ensure!(!id.starts_with('/') && !id.starts_with('\\'), "笔记 id 不能是绝对路径");
    anyhow::ensure!(!id.contains(':'), "笔记 id 非法");
    for part in id.split(['/', '\\']) {
        anyhow::ensure!(part != "..", "笔记 id 不能包含 ..");
    }
    Ok(())
}

/// 扫描 vault：索引新增/变更的 md，删除已不存在的记录。
pub fn scan_vault(conn: &Connection, root: &Path) -> Result<ScanStats> {
    let mut stats = ScanStats::default();
    let mut seen: Vec<String> = Vec::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        let path = entry.path();
        if !entry.file_type().is_file() || !is_markdown(path) {
            continue;
        }
        let rel = rel_path(root, path);
        seen.push(rel.clone());
        match index_file_if_changed(conn, root, path)? {
            true => stats.indexed += 1,
            false => stats.skipped += 1,
        }
    }

    // 清理已删除的文件
    let seen: HashSet<String> = seen.into_iter().collect();
    let mut stale: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id FROM notes")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for id in rows.flatten() {
            if !seen.contains(&id) {
                stale.push(id);
            }
        }
    }
    for id in stale {
        remove_from_index(conn, &id)?;
        stats.removed += 1;
    }
    Ok(stats)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase() == "md")
        .unwrap_or(false)
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn folder_of(rel: &str) -> String {
    match rel.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

/// mtime/size 未变化则跳过。
fn index_file_if_changed(conn: &Connection, root: &Path, path: &Path) -> Result<bool> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let size = meta.len() as i64;
    let id = note_id(&rel_path(root, path));

    let existing: Option<(i64, i64)> = {
        let mut stmt = conn.prepare("SELECT mtime, size FROM notes WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(r) => Some((r.get(0)?, r.get(1)?)),
            None => None,
        }
    };
    if existing == Some((mtime, size)) {
        return Ok(false);
    }
    index_file(conn, root, path)?;
    Ok(true)
}

/// 强制索引单个文件（写入/更新）。
pub fn index_file(conn: &Connection, root: &Path, path: &Path) -> Result<NoteMeta> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取笔记失败: {}", path.display()))?;
    let rel = note_id(&rel_path(root, path));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.clone());
    let doc = markdown::parse(&content, &stem);

    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let size = meta.len() as i64;
    let now = now_secs();
    let tags = doc.tags.join(",");
    let folder = folder_of(&rel);

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        r#"INSERT INTO notes (id, title, folder, tags, mtime, size, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
           ON CONFLICT(id) DO UPDATE SET
             title = excluded.title,
             folder = excluded.folder,
             tags = excluded.tags,
             mtime = excluded.mtime,
             size = excluded.size,
             updated_at = excluded.updated_at"#,
        params![rel, doc.title, folder, tags, mtime, size, now],
    )?;
    tx.execute("DELETE FROM chunks WHERE note_id = ?1", params![rel])?;
    tx.execute("DELETE FROM notes_fts WHERE note_id = ?1", params![rel])?;

    let chunks = chunk::chunk_markdown(&doc.body);
    for c in &chunks {
        tx.execute(
            "INSERT INTO chunks (note_id, seq, start_line, end_line, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![rel, c.seq as i64, c.start_line as i64, c.end_line as i64, c.text],
        )?;
        let chunk_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO notes_fts (chunk_id, note_id, title, body, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![chunk_id, rel, doc.title, c.text, tags],
        )?;
    }
    tx.commit()?;

    Ok(NoteMeta {
        id: rel.clone(),
        title: doc.title,
        rel_path: rel,
        folder,
        tags,
        mtime,
        size,
        updated_at: now,
    })
}

/// 从索引中移除（不删文件）。
pub fn remove_from_index(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM chunks WHERE note_id = ?1", params![id])?;
    tx.execute("DELETE FROM notes_fts WHERE note_id = ?1", params![id])?;
    tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(())
}

/// 笔记列表。
pub fn list_notes(conn: &Connection) -> Result<Vec<NoteMeta>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, folder, tags, mtime, size, updated_at
         FROM notes ORDER BY updated_at DESC, title ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: String = r.get(0)?;
        Ok(NoteMeta {
            rel_path: id.clone(),
            id,
            title: r.get(1)?,
            folder: r.get(2)?,
            tags: r.get(3)?,
            mtime: r.get(4)?,
            size: r.get(5)?,
            updated_at: r.get(6)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

/// 读取笔记原文（从磁盘）。
pub fn read_note(root: &Path, id: &str) -> Result<String> {
    ensure_safe_id(id)?;
    let path = root.join(id);
    Ok(std::fs::read_to_string(&path)
        .with_context(|| format!("读取笔记失败: {}", path.display()))?)
}

/// 写入笔记原文并重建索引。文件不存在则创建（含父目录）。
pub fn write_note(conn: &Connection, root: &Path, id: &str, content: &str) -> Result<NoteMeta> {
    ensure_safe_id(id)?;
    let path = root.join(id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("写入笔记失败: {}", path.display()))?;
    index_file(conn, root, &path)
}

/// 新建笔记：标题生成安全文件名，重名自动加序号。
pub fn create_note(conn: &Connection, root: &Path, folder: &str, title: &str) -> Result<NoteMeta> {
    let base = sanitize_filename(title);
    let folder = folder.trim_matches('/');
    let mut candidate = if folder.is_empty() {
        format!("{base}.md")
    } else {
        format!("{folder}/{base}.md")
    };
    let mut n = 2;
    while root.join(&candidate).exists() {
        candidate = if folder.is_empty() {
            format!("{base}-{n}.md")
        } else {
            format!("{folder}/{base}-{n}.md")
        };
        n += 1;
    }
    let content = format!("# {}\n\n", title.trim());
    write_note(conn, root, &candidate, &content)
}

/// 删除笔记文件（若存在）并从索引移除。
pub fn delete_note(conn: &Connection, root: &Path, id: &str) -> Result<()> {
    ensure_safe_id(id)?;
    let path = root.join(id);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("删除笔记失败: {}", path.display()))?;
    }
    remove_from_index(conn, id)
}

/// 全文检索。
///
/// - 查询 >= 3 字符：FTS5 trigram 子串匹配（中文友好）；
/// - 更短的查询：LIKE 兜底。
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 100) as i64;

    if q.chars().count() < 3 {
        let mut stmt = conn.prepare(
            r#"SELECT c.note_id, n.title, c.start_line, c.end_line, substr(c.text, 1, 160)
               FROM chunks c JOIN notes n ON n.id = c.note_id
               WHERE c.text LIKE '%' || ?1 || '%' OR n.title LIKE '%' || ?1 || '%'
               ORDER BY n.updated_at DESC LIMIT ?2"#,
        )?;
        let rows = stmt.query_map(params![q, limit], |r| {
            Ok(SearchHit {
                note_id: r.get(0)?,
                title: r.get(1)?,
                start_line: r.get(2)?,
                end_line: r.get(3)?,
                snippet: r.get(4)?,
            })
        })?;
        return Ok(rows.flatten().collect());
    }

    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    let match_expr = terms.join(" AND ");

    let mut stmt = conn.prepare(
        r#"SELECT f.note_id, f.title, c.start_line, c.end_line,
                  snippet(notes_fts, 3, '[', ']', '…', 12)
           FROM notes_fts f JOIN chunks c ON c.id = f.chunk_id
           WHERE notes_fts MATCH ?1
           ORDER BY rank LIMIT ?2"#,
    )?;
    let rows = stmt.query_map(params![match_expr, limit], |r| {
        Ok(SearchHit {
            note_id: r.get(0)?,
            title: r.get(1)?,
            start_line: r.get(2)?,
            end_line: r.get(3)?,
            snippet: r.get(4)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

fn sanitize_filename(title: &str) -> String {
    let mut s: String = title
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    s = s.trim_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s = "未命名".to_string();
    }
    if s.chars().count() > 60 {
        s = s.chars().take(60).collect();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    #[allow(unused_imports)]
    use crate::markdown as _markdown_reexport_check;

    fn setup() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        (dir, conn)
    }

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn scans_and_searches_chinese() {
        let (dir, conn) = setup();
        let root = dir.path();
        write(
            root,
            "工作/周会.md",
            "---\ntags: [会议]\n---\n# 周会纪要\n\n讨论了镜像拉取超时的问题，决定改用镜像站。\n",
        );
        write(root, "读书.md", "# 读书笔记\n\n今天读了 Rust 所有权相关章节。\n");

        let stats = scan_vault(&conn, root).unwrap();
        assert_eq!(stats.indexed, 2);

        let hits = search(&conn, "镜像拉取", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, "工作/周会.md");
        assert!(hits[0].start_line >= 1);

        let hits = search(&conn, "所有权", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, "读书.md");

        let notes = list_notes(&conn).unwrap();
        assert_eq!(notes.len(), 2);
        let meeting = notes.iter().find(|n| n.id == "工作/周会.md").unwrap();
        assert_eq!(meeting.title, "周会纪要");
        assert_eq!(meeting.folder, "工作");
        assert_eq!(meeting.tags, "会议");
    }

    #[test]
    fn rescan_skips_unchanged_and_removes_deleted() {
        let (dir, conn) = setup();
        let root = dir.path();
        write(root, "a.md", "# A\n内容一\n");
        write(root, "b.md", "# B\n内容二\n");
        let s1 = scan_vault(&conn, root).unwrap();
        assert_eq!((s1.indexed, s1.removed), (2, 0));

        let s2 = scan_vault(&conn, root).unwrap();
        assert_eq!((s2.indexed, s2.skipped), (0, 2));

        std::fs::remove_file(root.join("b.md")).unwrap();
        let s3 = scan_vault(&conn, root).unwrap();
        assert_eq!(s3.removed, 1);
        assert!(search(&conn, "内容二", 10).unwrap().is_empty());
    }

    #[test]
    fn create_write_and_delete_note() {
        let (dir, conn) = setup();
        let root = dir.path();
        let n = create_note(&conn, root, "随手记", "我的想法").unwrap();
        assert_eq!(n.id, "随手记/我的想法.md");
        let n2 = create_note(&conn, root, "随手记", "我的想法").unwrap();
        assert_eq!(n2.id, "随手记/我的想法-2.md");

        write_note(&conn, root, &n.id, "# 我的想法\n\n换成了新的正文，包含关键词：向量检索。\n").unwrap();
        let hits = search(&conn, "向量检索", 10).unwrap();
        assert_eq!(hits.len(), 1);

        delete_note(&conn, root, &n.id).unwrap();
        assert!(!root.join(&n.id).exists());
        assert!(search(&conn, "向量检索", 10).unwrap().is_empty());
    }

    #[test]
    fn short_query_falls_back_to_like() {
        let (dir, conn) = setup();
        let root = dir.path();
        write(root, "x.md", "# 标题\n\n关键词：图。\n");
        scan_vault(&conn, root).unwrap();
        let hits = search(&conn, "图", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
