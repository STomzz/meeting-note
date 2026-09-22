//! SQLite 连接与建表（幂等迁移）。

use rusqlite::{Connection, Result};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;

/// 打开（或创建）数据库文件，启用 WAL 并执行迁移。
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// 内存库（测试用）。
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

/// 幂等建表。
///
/// `notes_fts` 使用 **trigram** 分词器：对中文按 3 字符滑窗做子串匹配，
/// 无需分词服务即可支持中文搜索（查询词需 >= 3 个字符，短词走 LIKE 兜底）。
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS notes (
            id          TEXT PRIMARY KEY,          -- 相对 vault 的路径（统一 / 分隔）
            title       TEXT NOT NULL,
            folder      TEXT NOT NULL DEFAULT '',
            tags        TEXT NOT NULL DEFAULT '',  -- 逗号分隔
            mtime       INTEGER NOT NULL DEFAULT 0,
            size        INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL DEFAULT 0,
            updated_at  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS chunks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id     TEXT NOT NULL,
            seq         INTEGER NOT NULL,
            start_line  INTEGER NOT NULL,
            end_line    INTEGER NOT NULL,
            text        TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_chunks_note ON chunks(note_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
            chunk_id UNINDEXED,
            note_id  UNINDEXED,
            title,
            body,
            tags,
            tokenize = "trigram"
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
