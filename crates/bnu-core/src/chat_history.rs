//! 问答会话历史：文件落在应用数据目录，结构由前端定义（`{ version, conversations }`），
//! 这里只负责读、原子写与兜底，避免前后端两份 schema 各自演化。

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// 历史文件名（与备份 zip 打包时同名）。
pub const FILE_NAME: &str = "chat-history.json";

pub fn path_in(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// 当前版本号（前端写入时也会带上；缺失或更旧时读取方自行兜底）。
pub fn default_payload() -> Value {
    json!({ "version": 1, "conversations": [] })
}

/// 读取历史；文件不存在时返回空骨架。
pub fn load(data_dir: &Path) -> Result<Value> {
    let path = path_in(data_dir);
    if !path.exists() {
        return Ok(default_payload());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("读取会话历史失败: {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(default_payload());
    }
    let value: Value =
        serde_json::from_str(&text).with_context(|| "会话历史不是合法 JSON（可删除该文件重建）")?;
    if !value.is_object() {
        return Ok(default_payload());
    }
    Ok(value)
}

/// 原子写入：先写 `.tmp` 再 rename，避免中断时留下半截文件。
pub fn save(data_dir: &Path, payload: &Value) -> Result<()> {
    fs::create_dir_all(data_dir)
        .with_context(|| format!("创建数据目录失败: {}", data_dir.display()))?;
    let path = path_in(data_dir);
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(payload).context("序列化会话历史失败")?;
    fs::write(&tmp, text).with_context(|| format!("写入失败: {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("落盘失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_empty_skeleton() {
        let dir = tempfile::tempdir().unwrap();
        let v = load(dir.path()).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["conversations"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let payload = json!({
            "version": 1,
            "conversations": [{ "id": "c1", "title": "镜像问题", "entries": [] }]
        });
        save(dir.path(), &payload).unwrap();
        let back = load(dir.path()).unwrap();
        assert_eq!(back, payload);
        // 不残留临时文件
        assert!(!dir.path().join("chat-history.json.tmp").exists());
    }

    #[test]
    fn broken_json_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE_NAME), "{oops").unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("合法 JSON"), "{err}");
    }

    #[test]
    fn empty_file_falls_back_to_skeleton() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE_NAME), "   ").unwrap();
        assert_eq!(load(dir.path()).unwrap(), default_payload());
    }
}
