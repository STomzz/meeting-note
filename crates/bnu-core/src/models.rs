//! 模型端点配置（加密存储）与 OpenAI 兼容客户端。
//!
//! 四类能力：chat / embedding / rerank / asr。
//! - 配置存在 SQLite settings 表，API Key 用本机密钥加密（见 `secret`）；
//! - 对外（前端）只暴露脱敏配置；
//! - HTTP 调用集中在 reqwest，便于整体替换或加代理。

use crate::secret::SecretBox;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const SETTING_KEY: &str = "model_config";

/// 单个端点配置（内存态，api_key 为明文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// 附加请求参数（如 `{"chat_template_kwargs": {"enable_thinking": false}}`）。
    #[serde(default)]
    pub params: Value,
    /// 明文 key，仅内存；保存时 `None` 表示"保持原值"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// 四类能力配置。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    #[serde(default)]
    pub chat: Option<EndpointConfig>,
    #[serde(default)]
    pub embedding: Option<EndpointConfig>,
    #[serde(default)]
    pub rerank: Option<EndpointConfig>,
    #[serde(default)]
    pub asr: Option<EndpointConfig>,
}

/// 前端可见的脱敏端点。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicEndpoint {
    pub base_url: String,
    pub model: String,
    pub params: Value,
    pub has_key: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PublicModelConfig {
    pub chat: Option<PublicEndpoint>,
    pub embedding: Option<PublicEndpoint>,
    pub rerank: Option<PublicEndpoint>,
    pub asr: Option<PublicEndpoint>,
}

impl ModelConfig {
    pub fn public(&self) -> PublicModelConfig {
        let f = |e: &Option<EndpointConfig>| {
            e.as_ref().map(|e| PublicEndpoint {
                base_url: e.base_url.clone(),
                model: e.model.clone(),
                params: e.params.clone(),
                has_key: e.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
            })
        };
        PublicModelConfig {
            chat: f(&self.chat),
            embedding: f(&self.embedding),
            rerank: f(&self.rerank),
            asr: f(&self.asr),
        }
    }

    /// 合并保存请求：未提供 api_key 时保留旧值。
    pub fn merge(&mut self, incoming: ModelConfig) {
        fn merge_one(old: &Option<EndpointConfig>, new: Option<EndpointConfig>) -> Option<EndpointConfig> {
            match new {
                None => old.clone(),
                Some(mut n) => {
                    if n.api_key.is_none() {
                        n.api_key = old.as_ref().and_then(|o| o.api_key.clone());
                    }
                    if n.base_url.trim().is_empty() {
                        return None; // 空 base_url 视为删除该能力配置
                    }
                    Some(n)
                }
            }
        }
        self.chat = merge_one(&self.chat, incoming.chat);
        self.embedding = merge_one(&self.embedding, incoming.embedding);
        self.rerank = merge_one(&self.rerank, incoming.rerank);
        self.asr = merge_one(&self.asr, incoming.asr);
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredEndpoint {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    api_key_enc: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredConfig {
    chat: Option<StoredEndpoint>,
    embedding: Option<StoredEndpoint>,
    rerank: Option<StoredEndpoint>,
    asr: Option<StoredEndpoint>,
}

impl StoredEndpoint {
    fn from_config(e: &EndpointConfig, secrets: &SecretBox) -> Result<Self> {
        let api_key_enc = match e.api_key.as_deref() {
            Some("") => None,
            Some(k) => Some(secrets.encrypt(k)?),
            None => None,
        };
        Ok(Self {
            base_url: e.base_url.clone(),
            model: e.model.clone(),
            params: e.params.clone(),
            api_key_enc,
        })
    }

    fn to_config(&self, secrets: &SecretBox) -> Result<EndpointConfig> {
        let api_key = match &self.api_key_enc {
            Some(enc) if !enc.is_empty() => Some(secrets.decrypt(enc)?),
            _ => None,
        };
        Ok(EndpointConfig {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            params: self.params.clone(),
            api_key,
        })
    }
}

/// 读取配置（解密 key）。
pub fn load_config(conn: &Connection, secrets: &SecretBox) -> Result<ModelConfig> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![SETTING_KEY],
            |r| r.get(0),
        )
        .ok();
    let Some(raw) = raw else {
        return Ok(ModelConfig::default());
    };
    let stored: StoredConfig = serde_json::from_str(&raw).context("解析模型配置失败")?;
    let conv = |e: &Option<StoredEndpoint>| -> Result<Option<EndpointConfig>> {
        match e {
            Some(e) => Ok(Some(e.to_config(secrets)?)),
            None => Ok(None),
        }
    };
    Ok(ModelConfig {
        chat: conv(&stored.chat)?,
        embedding: conv(&stored.embedding)?,
        rerank: conv(&stored.rerank)?,
        asr: conv(&stored.asr)?,
    })
}

/// 保存配置（加密 key）。
pub fn save_config(conn: &Connection, secrets: &SecretBox, cfg: &ModelConfig) -> Result<()> {
    let conv = |e: &Option<EndpointConfig>| -> Result<Option<StoredEndpoint>> {
        match e {
            Some(e) => Ok(Some(StoredEndpoint::from_config(e, secrets)?)),
            None => Ok(None),
        }
    };
    let stored = StoredConfig {
        chat: conv(&cfg.chat)?,
        embedding: conv(&cfg.embedding)?,
        rerank: conv(&cfg.rerank)?,
        asr: conv(&cfg.asr)?,
    };
    let json = serde_json::to_string(&stored)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SETTING_KEY, json],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP 客户端
// ---------------------------------------------------------------------------

/// 把 base_url 归一化成 OpenAI 兼容的 `/v1` 根地址。
pub fn api_root(base_url: &str) -> String {
    let b = base_url.trim().trim_end_matches('/');
    if b.ends_with("/v1") {
        b.to_string()
    } else {
        format!("{b}/v1")
    }
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("创建 HTTP 客户端失败")
}

fn auth(req: reqwest::RequestBuilder, cfg: &EndpointConfig) -> reqwest::RequestBuilder {
    match cfg.api_key.as_deref() {
        Some(k) if !k.is_empty() => req.bearer_auth(k),
        _ => req,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub content: String,
    pub finish_reason: String,
    pub completion_tokens: Option<u64>,
    /// 模型有思考过程时（部分模型返回 reasoning 字段）
    pub reasoning: Option<String>,
}

/// 调用对话补全（非流式）。若 `content` 为空但有 `reasoning`，会回退返回思考文本。
pub async fn chat(cfg: &EndpointConfig, messages: &[ChatMessage], opts: &ChatOptions) -> Result<ChatReply> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置对话模型端点");
    }
    let mut body: Map<String, Value> = match &cfg.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    body.insert("model".into(), json!(cfg.model));
    body.insert("messages".into(), serde_json::to_value(messages)?);
    body.insert("stream".into(), json!(false));
    if let Some(mt) = opts.max_tokens {
        body.insert("max_tokens".into(), json!(mt));
    }
    if let Some(t) = opts.temperature {
        body.insert("temperature".into(), json!(t));
    }

    let url = format!("{}/chat/completions", api_root(&cfg.base_url));
    let resp = auth(http()?.post(&url), cfg)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("对话模型返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }
    let v: Value = serde_json::from_str(&text).context("解析对话响应失败")?;
    let choice = v
        .get("choices")
        .and_then(|c| c.get(0))
        .ok_or_else(|| anyhow!("响应缺少 choices: {}", truncate(&text, 300)))?;
    let finish = choice.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("").to_string();
    let msg = choice.get("message").cloned().unwrap_or(Value::Null);
    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").trim().to_string();
    let reasoning = msg
        .get("reasoning")
        .or_else(|| msg.get("reasoning_content"))
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let completion_tokens = v.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|t| t.as_u64());

    if content.is_empty() {
        if let Some(r) = &reasoning {
            if finish == "length" {
                bail!("模型思考太长被截断（finish_reason=length）。建议：提高 max_tokens，或设置 params.chat_template_kwargs.enable_thinking=false");
            }
            return Ok(ChatReply { content: r.clone(), finish_reason: finish, completion_tokens, reasoning });
        }
        bail!("模型返回内容为空（finish_reason={finish}）");
    }
    Ok(ChatReply { content, finish_reason: finish, completion_tokens, reasoning })
}

/// 文本嵌入。
pub async fn embed(cfg: &EndpointConfig, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置嵌入模型端点");
    }
    let url = format!("{}/embeddings", api_root(&cfg.base_url));
    let resp = auth(http()?.post(&url), cfg)
        .json(&json!({ "model": cfg.model, "input": inputs }))
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("嵌入模型返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }
    let v: Value = serde_json::from_str(&text).context("解析嵌入响应失败")?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("响应缺少 data: {}", truncate(&text, 200)))?;
    let mut out = Vec::with_capacity(data.len());
    for item in data {
        let arr = item
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow!("嵌入项缺少 embedding"))?;
        out.push(arr.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect());
    }
    Ok(out)
}

/// 重排。
#[derive(Debug, Clone, Serialize)]
pub struct RerankHit {
    pub index: usize,
    pub score: f32,
}

pub async fn rerank(
    cfg: &EndpointConfig,
    query: &str,
    documents: &[String],
    top_n: Option<usize>,
) -> Result<Vec<RerankHit>> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置重排模型端点");
    }
    let url = format!("{}/rerank", api_root(&cfg.base_url));
    let mut body = json!({ "model": cfg.model, "query": query, "documents": documents });
    if let Some(n) = top_n {
        body["top_n"] = json!(n);
    }
    let resp = auth(http()?.post(&url), cfg)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("重排模型返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }
    let v: Value = serde_json::from_str(&text).context("解析重排响应失败")?;
    let results = v
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| anyhow!("响应缺少 results: {}", truncate(&text, 200)))?;
    let mut out = Vec::with_capacity(results.len());
    for item in results {
        let index = item.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        let score = item
            .get("relevance_score")
            .or_else(|| item.get("score"))
            .and_then(|s| s.as_f64())
            .unwrap_or(0.0) as f32;
        out.push(RerankHit { index, score });
    }
    Ok(out)
}

/// 语音转写（OpenAI 兼容 /audio/transcriptions）。
pub async fn transcribe(cfg: &EndpointConfig, wav: Vec<u8>, filename: &str) -> Result<String> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置语音转写端点");
    }
    let url = format!("{}/audio/transcriptions", api_root(&cfg.base_url));
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name(filename.to_string())
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", cfg.model.clone());
    if let Some(lang) = cfg.params.get("language").and_then(|l| l.as_str()) {
        form = form.text("language", lang.to_string());
    }
    let resp = auth(http()?.post(&url), cfg)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("语音转写返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }
    let v: Value = serde_json::from_str(&text).context("解析转写响应失败")?;
    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
        return Ok(t.to_string());
    }
    // 兼容部分实现返回 chat 结构
    if let Some(t) = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
    {
        return Ok(t.to_string());
    }
    bail!("转写响应缺少 text 字段: {}", truncate(&text, 200));
}

/// 拉取模型列表（同时校验 base_url + key）。
pub async fn list_models(cfg: &EndpointConfig) -> Result<Vec<String>> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置 base_url");
    }
    let url = format!("{}/models", api_root(&cfg.base_url));
    let resp = auth(http()?.get(&url), cfg)
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("模型列表返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }
    let v: Value = serde_json::from_str(&text).context("解析模型列表失败")?;
    let ids = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(ids)
}

/// 生成一段静音 WAV（用于 ASR 连通性测试）。
pub fn silent_wav(seconds: f32, sample_rate: u32) -> Vec<u8> {
    let samples = (seconds * sample_rate as f32) as u32;
    let data_len = samples * 2; // PCM16 单声道
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend(std::iter::repeat(0u8).take(data_len as usize));
    out
}

/// 连接测试：按能力执行最小请求。
pub async fn test_capability(capability: &str, cfg: &ModelConfig) -> Result<String> {
    match capability {
        "chat" => {
            let e = cfg.chat.as_ref().ok_or_else(|| anyhow!("未配置对话模型"))?;
            let reply = chat(e, &[ChatMessage::user("连接测试，请只回复：ok")], &ChatOptions { max_tokens: Some(64), ..Default::default() }).await?;
            Ok(format!("对话正常（模型 {}）：{}", e.model, truncate(&reply.content, 80)))
        }
        "embedding" => {
            let e = cfg.embedding.as_ref().ok_or_else(|| anyhow!("未配置嵌入模型"))?;
            let v = embed(e, &["连接测试".to_string()]).await?;
            let dim = v.first().map(|x| x.len()).unwrap_or(0);
            Ok(format!("嵌入正常（模型 {}），向量维度 {}", e.model, dim))
        }
        "rerank" => {
            let e = cfg.rerank.as_ref().ok_or_else(|| anyhow!("未配置重排模型"))?;
            let docs = vec!["向量检索是一种语义检索方法".to_string(), "今天天气不错".to_string()];
            let hits = rerank(e, "什么是向量检索", &docs, Some(2)).await?;
            let desc = hits
                .iter()
                .map(|h| format!("#{}={:.4}", h.index, h.score))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!("重排正常（模型 {}）：{}", e.model, desc))
        }
        "asr" => {
            let e = cfg.asr.as_ref().ok_or_else(|| anyhow!("未配置语音转写模型"))?;
            let wav = silent_wav(0.5, 16000);
            let text = transcribe(e, wav, "probe.wav").await?;
            Ok(format!("语音转写接口正常（模型 {}），静音 0.5s 返回 {} 字", e.model, text.chars().count()))
        }
        other => bail!("未知能力: {other}"),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, secret::SecretBox};

    fn setup() -> (tempfile::TempDir, Connection, SecretBox) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        let sb = SecretBox::load_or_create(&dir.path().join("secret.key")).unwrap();
        (dir, conn, sb)
    }

    #[test]
    fn api_root_normalization() {
        assert_eq!(api_root("https://chatapi.bnu.edu.cn/"), "https://chatapi.bnu.edu.cn/v1");
        assert_eq!(api_root("https://chatapi.bnu.edu.cn/v1"), "https://chatapi.bnu.edu.cn/v1");
        assert_eq!(api_root("http://127.0.0.1:8080/v1/"), "http://127.0.0.1:8080/v1");
    }

    #[test]
    fn config_roundtrip_encrypts_key() {
        let (_d, conn, sb) = setup();
        let cfg = ModelConfig {
            chat: Some(EndpointConfig {
                base_url: "https://chatapi.bnu.edu.cn".into(),
                model: "Qwen-Inno-35B-v1".into(),
                params: json!({"chat_template_kwargs": {"enable_thinking": false}}),
                api_key: Some("sk-secret-123".into()),
            }),
            embedding: Some(EndpointConfig {
                base_url: "https://chatapi.bnu.edu.cn".into(),
                model: "bge-m3".into(),
                params: Value::Null,
                api_key: Some("sk-secret-123".into()),
            }),
            ..Default::default()
        };
        save_config(&conn, &sb, &cfg).unwrap();

        // 落库的内容里不应出现明文 key
        let raw: String = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![SETTING_KEY], |r| r.get(0))
            .unwrap();
        assert!(!raw.contains("sk-secret-123"), "明文 key 泄漏: {raw}");

        let loaded = load_config(&conn, &sb).unwrap();
        assert_eq!(loaded.chat.as_ref().unwrap().model, "Qwen-Inno-35B-v1");
        assert_eq!(loaded.chat.as_ref().unwrap().api_key.as_deref(), Some("sk-secret-123"));
        assert_eq!(loaded.embedding.as_ref().unwrap().model, "bge-m3");
    }

    #[test]
    fn merge_keeps_existing_key_and_supports_delete() {
        let (_d, conn, sb) = setup();
        let mut cfg = ModelConfig {
            chat: Some(EndpointConfig {
                base_url: "https://a".into(),
                model: "m1".into(),
                params: Value::Null,
                api_key: Some("k1".into()),
            }),
            ..Default::default()
        };
        save_config(&conn, &sb, &cfg).unwrap();

        // 更新模型名但不传 key → 保留旧 key
        let incoming = ModelConfig {
            chat: Some(EndpointConfig {
                base_url: "https://a".into(),
                model: "m2".into(),
                params: Value::Null,
                api_key: None,
            }),
            ..Default::default()
        };
        cfg.merge(incoming);
        assert_eq!(cfg.chat.as_ref().unwrap().model, "m2");
        assert_eq!(cfg.chat.as_ref().unwrap().api_key.as_deref(), Some("k1"));

        // base_url 置空 → 删除该能力
        cfg.merge(ModelConfig {
            chat: Some(EndpointConfig { base_url: "".into(), model: "".into(), params: Value::Null, api_key: None }),
            ..Default::default()
        });
        assert!(cfg.chat.is_none());
    }

    #[test]
    fn public_view_masks_key() {
        let cfg = ModelConfig {
            asr: Some(EndpointConfig {
                base_url: "https://x".into(),
                model: "qwen3-asr-1.7b".into(),
                params: Value::Null,
                api_key: Some("k".into()),
            }),
            ..Default::default()
        };
        let p = cfg.public();
        let asr = p.asr.unwrap();
        assert!(asr.has_key);
        assert_eq!(asr.model, "qwen3-asr-1.7b");
        let json = serde_json::to_string(&asr).unwrap();
        assert!(!json.contains("\"k\""));
    }

    #[test]
    fn silent_wav_header() {
        let wav = silent_wav(0.5, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 16000);
    }
}
