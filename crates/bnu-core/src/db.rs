//! SQLite 连接与建表（幂等迁移）。

use rusqlite::{Connection, Result};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 4;

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
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
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

        -- 块向量（暴力检索：个人知识库规模足够，避免引入向量库依赖）
        -- 每个块只保留一份向量，切换嵌入模型后旧向量会被覆盖（按 model 过滤使用）。
        CREATE TABLE IF NOT EXISTS chunk_vectors (
            chunk_id   INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
            model      TEXT NOT NULL,
            dim        INTEGER NOT NULL,
            vec        BLOB NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_chunk_vectors_model ON chunk_vectors(model);

        -- 会议与录音分段
        CREATE TABLE IF NOT EXISTS meetings (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT 0,
            updated_at  INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            status      TEXT NOT NULL DEFAULT 'recording',
            asr_model   TEXT NOT NULL DEFAULT '',
            chat_model  TEXT NOT NULL DEFAULT '',
            transcript  TEXT NOT NULL DEFAULT '',
            minutes_json TEXT NOT NULL DEFAULT '',
            minutes_md  TEXT NOT NULL DEFAULT '',
            note_id     TEXT NOT NULL DEFAULT '',
            error       TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS meeting_segments (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
            seq         INTEGER NOT NULL,
            file        TEXT NOT NULL,
            src_rate    INTEGER NOT NULL DEFAULT 16000,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            bytes       INTEGER NOT NULL DEFAULT 0,
            status      TEXT NOT NULL DEFAULT 'recording',
            transcript  TEXT NOT NULL DEFAULT '',
            error       TEXT NOT NULL DEFAULT ''
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_meeting_segments_unique
            ON meeting_segments(meeting_id, seq);

        -- 知识图谱（P4）：实体 / 关系明细 / 块-实体 / 抽取账本
        -- 说明：`relations` 按「一次出现一行」存明细，聚合（强度=出现次数）在查询时做，
        -- 这样重建某篇笔记 = 先按 note_id 删除再插入，天然幂等，且保留出处用于回跳。
        CREATE TABLE IF NOT EXISTS entities (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,                  -- 规范名（trim / 全半角归一 / 英文小写）
            kind        TEXT NOT NULL DEFAULT 'other',  -- person/org/place/concept/event/other
            created_at  INTEGER NOT NULL DEFAULT 0,
            updated_at  INTEGER NOT NULL DEFAULT 0,
            UNIQUE(name, kind)
        );
        CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);

        CREATE TABLE IF NOT EXISTS relations (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id     TEXT NOT NULL,
            chunk_id    INTEGER REFERENCES chunks(id) ON DELETE SET NULL,
            src_id      INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            dst_id      INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            kind        TEXT NOT NULL,                  -- 关系短语（归一化小写）
            evidence    TEXT NOT NULL DEFAULT '',       -- 出处原句（截断）
            created_at  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_relations_note ON relations(note_id);
        CREATE INDEX IF NOT EXISTS idx_relations_src ON relations(src_id);
        CREATE INDEX IF NOT EXISTS idx_relations_dst ON relations(dst_id);

        CREATE TABLE IF NOT EXISTS chunk_entities (
            chunk_id    INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            note_id     TEXT NOT NULL,
            entity_id   INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            PRIMARY KEY (chunk_id, entity_id)
        );
        CREATE INDEX IF NOT EXISTS idx_chunk_entities_entity ON chunk_entities(entity_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_entities_note ON chunk_entities(note_id);

        CREATE TABLE IF NOT EXISTS graph_state (
            note_id      TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,                 -- 抽取时的笔记正文哈希
            model        TEXT NOT NULL DEFAULT '',      -- 抽取用的对话模型
            extracted_at INTEGER NOT NULL DEFAULT 0,
            entities     INTEGER NOT NULL DEFAULT 0,
            relations    INTEGER NOT NULL DEFAULT 0,
            error        TEXT NOT NULL DEFAULT ''
        );
        "#,
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
