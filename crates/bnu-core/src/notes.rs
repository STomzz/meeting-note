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
    anyhow::ensure!(
        !id.starts_with('/') && !id.starts_with('\\'),
        "笔记 id 不能是绝对路径"
    );
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
    // 图谱数据随索引失效：块 id 会变，旧关系/溯源必须一起清掉（下次抽取重建）
    crate::graph::invalidate_note(&tx, &rel)?;

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
    crate::graph::invalidate_note(&tx, id)?;
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
    std::fs::write(&path, content).with_context(|| format!("写入笔记失败: {}", path.display()))?;
    index_file(conn, root, &path)
}

/// 新建笔记：标题生成安全文件名，重名自动加序号。
pub fn create_note(conn: &Connection, root: &Path, folder: &str, title: &str) -> Result<NoteMeta> {
    let base = safe_filename(title);
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
        std::fs::remove_file(&path).with_context(|| format!("删除笔记失败: {}", path.display()))?;
    }
    remove_from_index(conn, id)
}

// ------------------------------------------------------------ 文件夹与移动

/// 文件夹信息（左侧目录树用；空文件夹也会返回）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub path: String,
    /// 直接放在这个文件夹里的笔记数（不含子文件夹）。
    pub note_count: i64,
}

/// 校验 vault 内相对目录：统一 `/`，拒绝绝对路径、`..`、隐藏名与非法字符。
pub fn safe_dir_rel(rel: &str) -> Result<String> {
    let rel = rel.trim().trim_matches('/').replace('\\', "/");
    anyhow::ensure!(!rel.is_empty(), "文件夹名不能为空");
    anyhow::ensure!(
        !rel.contains('\0') && !rel.contains(':'),
        "文件夹名含非法字符：{rel}"
    );
    for part in rel.split('/') {
        anyhow::ensure!(!part.is_empty(), "文件夹路径非法：{rel}");
        anyhow::ensure!(
            part != "." && part != "..",
            "文件夹路径不能包含 . / ..：{rel}"
        );
        anyhow::ensure!(!part.starts_with('.'), "文件夹名不能以 . 开头：{rel}");
        anyhow::ensure!(
            !part
                .chars()
                .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*' | '\\') || c.is_control()),
            "文件夹名含非法字符：{rel}"
        );
    }
    Ok(rel)
}

/// `会议音频/` 是录音数据目录，不允许当笔记文件夹用。
fn ensure_not_audio_dir(rel: &str) -> Result<()> {
    let audio = crate::audio_clip::AUDIO_DIR;
    anyhow::ensure!(
        rel != audio && !rel.starts_with(&format!("{audio}/")),
        "「{audio}」是录音数据目录，不能作为笔记文件夹"
    );
    Ok(())
}

/// 新建文件夹（支持多级），返回规范化相对路径。
pub fn create_folder(root: &Path, rel: &str) -> Result<String> {
    let rel = safe_dir_rel(rel)?;
    ensure_not_audio_dir(&rel)?;
    let path = root.join(&rel);
    std::fs::create_dir_all(&path)
        .with_context(|| format!("创建文件夹失败: {}", path.display()))?;
    Ok(rel)
}

/// 列出 vault 里的文件夹（隐藏目录与 `会议音频/` 除外；空文件夹也返回）。
pub fn list_folders(conn: &Connection, root: &Path) -> Result<Vec<FolderInfo>> {
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT folder, COUNT(*) FROM notes WHERE folder <> '' GROUP BY folder")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for (folder, count) in rows.flatten() {
            counts.insert(folder, count);
        }
    }
    let audio = crate::audio_clip::AUDIO_DIR;
    let mut out: Vec<FolderInfo> = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return false;
            }
            if e.file_type().is_dir() && rel_path(root, e.path()) == audio {
                return false;
            }
            true
        })
        .flatten()
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let rel = rel_path(root, entry.path());
        out.push(FolderInfo {
            path: rel.clone(),
            note_count: counts.get(&rel).copied().unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 重命名文件夹（只改最后一段名字），返回新路径。
pub fn rename_folder(conn: &Connection, root: &Path, rel: &str, new_name: &str) -> Result<String> {
    let rel = safe_dir_rel(rel)?;
    ensure_not_audio_dir(&rel)?;
    let name = safe_dir_rel(new_name)?;
    anyhow::ensure!(!name.contains('/'), "新名字只能是一层文件夹名：{new_name}");
    let old = root.join(&rel);
    anyhow::ensure!(old.is_dir(), "文件夹不存在：{rel}");
    let parent = folder_of(&rel);
    let new_rel = if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    };
    ensure_not_audio_dir(&new_rel)?;
    let new_path = root.join(&new_rel);
    anyhow::ensure!(!new_path.exists(), "同名文件夹已存在：{new_rel}");
    std::fs::rename(&old, &new_path)
        .with_context(|| format!("重命名文件夹失败: {}", old.display()))?;
    scan_vault(conn, root)?;
    Ok(new_rel)
}

/// 把笔记移动到另一个文件夹（保留文件名，重名自动加序号），返回新 id。
pub fn move_note(conn: &Connection, root: &Path, id: &str, folder: &str) -> Result<String> {
    ensure_safe_id(id)?;
    let folder = folder.trim();
    let folder = if folder.is_empty() {
        String::new()
    } else {
        safe_dir_rel(folder)?
    };
    if !folder.is_empty() {
        ensure_not_audio_dir(&folder)?;
    }
    let old = root.join(id);
    anyhow::ensure!(old.is_file(), "笔记不存在：{id}");
    if folder_of(id) == folder {
        return Ok(id.to_string());
    }
    let file = old
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let stem = Path::new(&file)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    if !folder.is_empty() {
        std::fs::create_dir_all(root.join(&folder))?;
    }
    let mut candidate = if folder.is_empty() {
        file.clone()
    } else {
        format!("{folder}/{file}")
    };
    let mut n = 2;
    while root.join(&candidate).exists() {
        candidate = if folder.is_empty() {
            format!("{stem}-{n}.md")
        } else {
            format!("{folder}/{stem}-{n}.md")
        };
        n += 1;
    }
    std::fs::rename(&old, root.join(&candidate))
        .with_context(|| format!("移动笔记失败: {}", old.display()))?;
    scan_vault(conn, root)?;
    Ok(candidate)
}

/// 重命名笔记（同目录，仅换标题；重名自动加序号），返回新 id。
pub fn rename_note(conn: &Connection, root: &Path, id: &str, title: &str) -> Result<String> {
    ensure_safe_id(id)?;
    let old = root.join(id);
    anyhow::ensure!(old.is_file(), "笔记不存在：{id}");
    let folder = folder_of(id);
    let base = safe_filename(title);
    let mut candidate = if folder.is_empty() {
        format!("{base}.md")
    } else {
        format!("{folder}/{base}.md")
    };
    if candidate == id {
        return Ok(id.to_string());
    }
    let mut n = 2;
    while root.join(&candidate).exists() {
        candidate = if folder.is_empty() {
            format!("{base}-{n}.md")
        } else {
            format!("{folder}/{base}-{n}.md")
        };
        n += 1;
    }
    std::fs::rename(&old, root.join(&candidate))
        .with_context(|| format!("重命名笔记失败: {}", old.display()))?;
    scan_vault(conn, root)?;
    Ok(candidate)
}

/// 查询词 >= 3 字符时返回 FTS5 MATCH 表达式；更短返回 `None`（调用方走 LIKE 兜底）。
pub fn fts_match_expr(query: &str) -> Option<String> {
    let q = query.trim();
    if q.chars().count() < 3 {
        return None;
    }
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

/// 宽松匹配表达式：把长串（整句中文问题）拆成 3-gram，用 OR 连接。
///
/// trigram 分词器下，整句中文问题会被当成**一个 phrase** 去 AND 匹配 → 必然空手；
/// 严格匹配无结果时用这个表达式兜底（BM25 仍会按命中词数与稀有度排序）。
pub fn fts_relaxed_expr(query: &str) -> Option<String> {
    /// 单个查询词超过这个长度就拆 3-gram。
    const LONG_TERM: usize = 8;
    /// 3-gram 数量上限（避免超长问题生成巨型表达式）。
    const MAX_GRAMS: usize = 24;

    let mut grams: Vec<String> = Vec::new();
    for term in query.split_whitespace() {
        let chars: Vec<char> = term.chars().collect();
        if chars.len() < 3 {
            continue; // trigram 索引匹配不了 <3 字符，交给 LIKE 兜底
        }
        let pieces: Vec<String> = if chars.len() <= LONG_TERM {
            vec![chars.iter().collect()]
        } else {
            chars.windows(3).map(|w| w.iter().collect()).collect()
        };
        for p in pieces {
            if !grams.contains(&p) {
                grams.push(p);
            }
            if grams.len() >= MAX_GRAMS {
                break;
            }
        }
        if grams.len() >= MAX_GRAMS {
            break;
        }
    }
    if grams.is_empty() {
        return None;
    }
    Some(
        grams
            .iter()
            .map(|g| format!("\"{}\"", g.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
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

    let Some(match_expr) = fts_match_expr(q) else {
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        r#"SELECT f.note_id, f.title, c.start_line, c.end_line,
                  snippet(notes_fts, 3, '[', ']', '…', 12)
           FROM notes_fts f JOIN chunks c ON c.id = f.chunk_id
           WHERE notes_fts MATCH ?1
           ORDER BY rank LIMIT ?2"#,
    )?;
    let hits: Vec<SearchHit> = stmt
        .query_map(params![match_expr, limit], |r| {
            Ok(SearchHit {
                note_id: r.get(0)?,
                title: r.get(1)?,
                start_line: r.get(2)?,
                end_line: r.get(3)?,
                snippet: r.get(4)?,
            })
        })?
        .flatten()
        .collect();
    if !hits.is_empty() {
        return Ok(hits);
    }

    // 严格匹配空手（典型：整句中文问题被当成一个 phrase）→ 3-gram 宽松兜底
    let Some(relaxed) = fts_relaxed_expr(q) else {
        return Ok(Vec::new());
    };
    if relaxed == match_expr {
        return Ok(hits);
    }
    let rows = stmt.query_map(params![relaxed, limit], |r| {
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

/// 把标题转成安全的文件名（去掉路径分隔符等非法字符，限长 60）。
pub fn safe_filename(title: &str) -> String {
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
        write(
            root,
            "读书.md",
            "# 读书笔记\n\n今天读了 Rust 所有权相关章节。\n",
        );

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

        write_note(
            &conn,
            root,
            &n.id,
            "# 我的想法\n\n换成了新的正文，包含关键词：向量检索。\n",
        )
        .unwrap();
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

    #[test]
    fn long_chinese_question_falls_back_to_grams() {
        let (dir, conn) = setup();
        let root = dir.path();
        write(
            root,
            "a.md",
            "# 周会\n\n镜像拉取超时的问题由张伟跟进，下周给结论。\n",
        );
        write(root, "b.md", "# 读书\n\n今天读了 Rust 所有权相关章节。\n");
        scan_vault(&conn, root).unwrap();

        // 整句问题：严格 phrase 匹配必然空手 → 3-gram 兜底应命中 a.md
        let hits = search(&conn, "镜像拉取超时的问题是谁跟进、怎么解决的？", 10).unwrap();
        assert_eq!(hits.len(), 1, "长中文问题应有兜底命中：{hits:?}");
        assert_eq!(hits[0].note_id, "a.md");

        // 纯关键词仍走严格匹配（不因兜底改变排序）
        let hits = search(&conn, "Rust 所有权", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, "b.md");

        // 库里没有的词 → 依旧空手，不硬凑
        assert!(search(&conn, "量子纠缠退相干实验", 10).unwrap().is_empty());

        let expr = fts_relaxed_expr("镜像拉取超时的问题是谁跟进").unwrap();
        assert!(expr.contains("\"镜像拉\"") && expr.contains("OR"), "{expr}");
        assert!(fts_relaxed_expr("图").is_none(), "太短的词交给 LIKE 兜底");
    }

    #[test]
    fn folders_create_list_rename_and_move() {
        let (dir, conn) = setup();
        let root = dir.path();
        write(root, "工作/周会.md", "# 周会\n\n讨论了镜像站。\n");
        write(root, "根笔记.md", "# 根笔记\n\n正文。\n");
        write(root, "备份/根笔记.md", "# 占位\n\n占位。\n");

        // 新建（多级）空文件夹
        assert_eq!(create_folder(root, "项目/子目录").unwrap(), "项目/子目录");
        assert!(root.join("项目/子目录").is_dir());
        assert!(
            create_folder(root, "会议音频/恶意").is_err(),
            "录音目录不能当笔记文件夹"
        );
        assert!(create_folder(root, "../逃逸").is_err());
        assert!(create_folder(root, ".隐藏").is_err());

        // 列表：空文件夹也在；隐藏目录与会议音频排除
        std::fs::create_dir_all(root.join("会议音频/x")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        scan_vault(&conn, root).unwrap();
        let folders = list_folders(&conn, root).unwrap();
        let paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
        assert!(
            paths.contains(&"工作") && paths.contains(&"项目") && paths.contains(&"项目/子目录"),
            "{paths:?}"
        );
        assert!(
            !paths
                .iter()
                .any(|p| p.starts_with("会议音频") || p.starts_with(".git")),
            "{paths:?}"
        );
        assert_eq!(
            folders
                .iter()
                .find(|f| f.path == "工作")
                .unwrap()
                .note_count,
            1
        );

        // 移动：目标已有同名文件 → 自动加序号；同目录移动保持原 id
        assert_eq!(
            move_note(&conn, root, "根笔记.md", "备份").unwrap(),
            "备份/根笔记-2.md"
        );
        assert_eq!(
            move_note(&conn, root, "工作/周会.md", "工作").unwrap(),
            "工作/周会.md"
        );
        assert!(!root.join("根笔记.md").exists());
        assert!(root.join("备份/根笔记-2.md").is_file());

        // 重命名文件夹：名字只允许一层；重名要报错
        assert_eq!(
            rename_folder(&conn, root, "项目", "新项目").unwrap(),
            "新项目"
        );
        assert!(root.join("新项目/子目录").is_dir());
        assert!(
            rename_folder(&conn, root, "新项目", "工作").is_err(),
            "重名文件夹应报错"
        );
        assert!(
            rename_folder(&conn, root, "新项目", "a/b").is_err(),
            "不允许带路径"
        );

        // 重命名笔记：索引里旧 id 消失、新 id 出现
        let rid = rename_note(&conn, root, "工作/周会.md", "周会（重命名）").unwrap();
        assert_eq!(rid, "工作/周会（重命名）.md");
        let ids: Vec<String> = list_notes(&conn)
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert!(ids.contains(&rid), "{ids:?}");
        assert!(!ids.contains(&"工作/周会.md".to_string()), "{ids:?}");
        assert_eq!(
            search(&conn, "镜像站", 10).unwrap().len(),
            1,
            "重命名后全文索引仍可用"
        );
    }
}
