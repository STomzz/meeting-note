//! BNU Notes 核心库。
//!
//! 与平台无关：vault（md 文件夹）扫描与索引、Markdown 解析与分块、
//! SQLite(+FTS5) 全文检索。所有逻辑都在这里，Tauri 命令层只做薄封装。
//!
//! 行号约定：所有 chunk 的 `start_line` / `end_line` 都是**文件内 1-based 行号**，
//! 用于问答引用回跳。

pub mod audio;
pub mod chunk;
pub mod db;
pub mod markdown;
pub mod meetings;
pub mod minutes;
pub mod models;
pub mod notes;
pub mod qa;
pub mod retrieval;
pub mod secret;
pub mod transcribe;
pub mod vectors;

/// 重导出，方便上层（Tauri 壳）使用同一版本的 rusqlite 类型。
pub use rusqlite;
