//! 模型端点配置（加密存储）与 OpenAI 兼容客户端。
//!
//! 四类能力：chat / embedding / rerank / asr。
//! - 配置存在 SQLite settings 表，API Key 用本机密钥加密（见 `secret`）；
//! - 对外（前端）只暴露脱敏配置；
//! - HTTP 调用集中在 reqwest，便于整体替换或加代理。

use crate::secret::SecretBox;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
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

/// TLS 根证书库：**系统证书库 + 内置 Mozilla 根证书**。
///
/// 为什么要合并：`rustls-native-certs` 在 Android 上依赖 `openssl-probe`，而后者
/// 只认 Termux 的路径（`/data/data/com.termux/files/usr/etc/tls/cert.pem`）——
/// 普通 APK 里一个系统根证书都读不到，于是所有 `https://` 请求都会
/// `invalid peer certificate: UnknownIssuer`（Windows 读系统证书库，所以桌面端正常）。
/// 这里再并一份内置根证书兜底：公网 CA 签发的证书（如 `*.bnu.edu.cn` 的 DigiCert）都能验过。
pub fn root_store() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        // 个别证书解析失败不影响其它（Android 上本来就一个都读不到）
        let _ = roots.add(cert);
    }
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    roots
}

/// TLS 客户端配置（模型调用统一用它，别各建各的）。
pub fn tls_config() -> Result<rustls::ClientConfig> {
    let roots = root_store();
    if roots.is_empty() {
        bail!("没有可用的 TLS 根证书（系统证书库与内置根证书都为空）");
    }
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// 统一的 HTTP 客户端（模型调用都走这里）。
fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .use_preconfigured_tls(tls_config()?)
        .build()
        .context("创建 HTTP 客户端失败")
}

/// 把 reqwest 的错误连**根因**一起写出来。
///
/// `reqwest::Error` 的 `Display` 只有最外层（`error sending request for url (...)`），
/// 真正有用的（`invalid peer certificate: UnknownIssuer`、`connection refused`…）藏在 source 链里。
/// Android 上「系统根证书读不到」这个坑就是被这层吞掉的，排障时只看到一句「请求失败」。
fn req_err(url: &str, e: reqwest::Error) -> anyhow::Error {
    let mut chain = vec![e.to_string()];
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        let text = s.to_string();
        if !chain.iter().any(|c| c.contains(&text)) {
            chain.push(text);
        }
        src = s.source();
    }
    anyhow!("请求失败: {url} → {}", chain.join(" → "))
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
    /// 额外请求体字段（例如 `response_format`），直接并入请求 JSON。
    pub extra: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub content: String,
    pub finish_reason: String,
    pub completion_tokens: Option<u64>,
    /// 模型有思考过程时（部分模型返回 reasoning 字段）
    pub reasoning: Option<String>,
}

/// 拼对话补全的请求体（流式 / 非流式共用）。
fn build_chat_body(
    cfg: &EndpointConfig,
    messages: &[ChatMessage],
    opts: &ChatOptions,
    stream: bool,
) -> Result<Map<String, Value>> {
    let mut body: Map<String, Value> = match &cfg.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    body.insert("model".into(), json!(cfg.model));
    body.insert("messages".into(), serde_json::to_value(messages)?);
    body.insert("stream".into(), json!(stream));
    if let Some(mt) = opts.max_tokens {
        body.insert("max_tokens".into(), json!(mt));
    }
    if let Some(t) = opts.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(extra) = &opts.extra {
        for (k, v) in extra {
            body.insert(k.clone(), v.clone());
        }
    }
    Ok(body)
}

/// 调用对话补全（非流式）。若 `content` 为空但有 `reasoning`，会回退返回思考文本。
pub async fn chat(cfg: &EndpointConfig, messages: &[ChatMessage], opts: &ChatOptions) -> Result<ChatReply> {
    if cfg.base_url.trim().is_empty() {
        bail!("未配置对话模型端点");
    }
    let body = build_chat_body(cfg, messages, opts, false)?;

    let url = format!("{}/chat/completions", api_root(&cfg.base_url));
    let resp = auth(http()?.post(&url), cfg)
        .json(&body)
        .send()
        .await
        .map_err(|e| req_err(&url, e))?;
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

/// 流式增量事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatStreamEvent {
    /// 正文增量
    Delta(String),
    /// 思考过程增量（部分模型返回 reasoning_content）
    Reasoning(String),
}

/// SSE 累计器：把每一行 `data:` 折进结果，便于单测（不需要网络）。
#[derive(Debug, Default, Clone)]
pub struct ChatStreamAccum {
    pub content: String,
    pub reasoning: String,
    pub finish_reason: String,
    pub completion_tokens: Option<u64>,
    pub done: bool,
}

impl ChatStreamAccum {
    /// 吃掉一行 `data:` 的内容，返回要向上抛的事件。
    pub fn feed(&mut self, data: &str) -> Option<ChatStreamEvent> {
        let data = data.trim();
        if data.is_empty() {
            return None;
        }
        if data == "[DONE]" {
            self.done = true;
            return None;
        }
        let v: Value = serde_json::from_str(data).ok()?;

        if let Some(t) = v.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|t| t.as_u64()) {
            self.completion_tokens = Some(t);
        }
        let choice = v.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first())?;
        if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
            if !fr.is_empty() {
                self.finish_reason = fr.to_string();
            }
        }
        let delta = choice.get("delta")?;
        if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
            if !c.is_empty() {
                self.content.push_str(c);
                return Some(ChatStreamEvent::Delta(c.to_string()));
            }
        }
        if let Some(r) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
            if !r.is_empty() {
                self.reasoning.push_str(r);
                return Some(ChatStreamEvent::Reasoning(r.to_string()));
            }
        }
        None
    }

    pub fn into_reply(self) -> ChatReply {
        let reasoning = if self.reasoning.trim().is_empty() {
            None
        } else {
            Some(self.reasoning.trim().to_string())
        };
        let finish_reason = if self.finish_reason.is_empty() {
            "stop".to_string()
        } else {
            self.finish_reason
        };
        ChatReply { content: self.content.trim().to_string(), finish_reason, completion_tokens: self.completion_tokens, reasoning }
    }
}

/// 解析一行 SSE：只认 `data:` 前缀，返回其后的载荷（按规范去掉一个前导空格）。
pub fn sse_payload(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("data:")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    Some(rest.trim_end_matches(['\r', '\n']))
}

/// 从字节缓冲里取出一整行（含换行）。
///
/// 必须按字节缓冲、**成行后再解码**：网络分块可能把多字节汉字截成两半，
/// 若对每个分块单独 `from_utf8_lossy` 会产生替换字符（乱码）。
fn take_line(buf: &mut Vec<u8>) -> Option<String> {
    let pos = buf.iter().position(|b| *b == b'\n')?;
    let line: Vec<u8> = buf.drain(..=pos).collect();
    Some(String::from_utf8_lossy(&line).into_owned())
}

/// 流式对话补全。
///
/// `on_event` 返回 `false` 表示调用方要求中止：此时停止读取，`finish_reason` 记为 `cancelled`，
/// 已生成的部分照常返回（前端可保留半截回答）。
pub async fn chat_stream<F>(
    cfg: &EndpointConfig,
    messages: &[ChatMessage],
    opts: &ChatOptions,
    mut on_event: F,
) -> Result<ChatReply>
where
    F: FnMut(ChatStreamEvent) -> bool,
{
    if cfg.base_url.trim().is_empty() {
        bail!("未配置对话模型端点");
    }
    let body = build_chat_body(cfg, messages, opts, true)?;
    let url = format!("{}/chat/completions", api_root(&cfg.base_url));
    let resp = auth(http()?.post(&url), cfg)
        .json(&body)
        .send()
        .await
        .map_err(|e| req_err(&url, e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("对话模型返回 {}: {}", status.as_u16(), truncate(&text, 300));
    }

    let mut stream = resp.bytes_stream();
    let mut acc = ChatStreamAccum::default();
    let mut buf: Vec<u8> = Vec::new();
    let mut cancelled = false;

    'outer: while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("读取流式响应失败")?;
        buf.extend_from_slice(&bytes);
        while let Some(line) = take_line(&mut buf) {
            let Some(data) = sse_payload(&line) else { continue };
            if let Some(ev) = acc.feed(data) {
                if !on_event(ev) {
                    cancelled = true;
                    break 'outer;
                }
            }
            if acc.done {
                break 'outer;
            }
        }
    }
    // 收尾：最后一行可能没有换行符
    if !cancelled && !acc.done {
        let tail = String::from_utf8_lossy(&buf);
        if let Some(data) = sse_payload(&tail) {
            let _ = acc.feed(data);
        }
    }

    let reply = acc.into_reply();
    if cancelled {
        return Ok(ChatReply { finish_reason: "cancelled".into(), ..reply });
    }
    if reply.content.is_empty() {
        if reply.finish_reason == "length" {
            bail!("模型思考太长被截断（finish_reason=length）。建议：提高 max_tokens，或设置 params.chat_template_kwargs.enable_thinking=false");
        }
        if let Some(r) = reply.reasoning.clone() {
            return Ok(ChatReply { content: r, ..reply });
        }
        bail!("模型返回内容为空（finish_reason={}）", reply.finish_reason);
    }
    Ok(reply)
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
        .map_err(|e| req_err(&url, e))?;
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
        .map_err(|e| req_err(&url, e))?;
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
        .map_err(|e| req_err(&url, e))?;
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
        .map_err(|e| req_err(&url, e))?;
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
    fn root_store_merges_native_and_bundled() {
        let bundled = webpki_roots::TLS_SERVER_ROOTS.len();
        assert!(bundled > 100, "内置根证书数量异常：{bundled}");
        let merged = root_store();
        assert!(
            merged.len() >= bundled,
            "合并后的根证书库不该比内置的还少：{} < {bundled}",
            merged.len()
        );
        assert!(tls_config().is_ok(), "TLS 配置应能建出来");
    }

    /// 模拟 Android：系统证书库读不到（`openssl-probe` 只认 Termux 路径）时，
    /// **只靠内置根证书**要能验过网关注书（`*.bnu.edu.cn` 是 DigiCert 签的）。
    ///
    /// 需要网络：`cargo test -p bnu-core --lib -- --ignored --nocapture bundled_roots`
    #[tokio::test]
    #[ignore]
    async fn bundled_roots_cover_gateway_tls() {
        let host = std::env::var("BNU_TEST_BASE_URL")
            .unwrap_or_else(|_| "https://chatapi.bnu.edu.cn/bnuapi".to_string());
        let url = format!("{}/v1/models", host.trim_end_matches('/'));
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let resp = client
            .get(&url)
            .send()
            .await
            .expect("只装内置根证书时也应验过网关证书（Android 走的正是这条路）");
        println!("{url} → {}", resp.status());
        assert!(
            resp.status().is_success() || resp.status().as_u16() == 401,
            "意外状态码：{}",
            resp.status()
        );

        // 反证：根证书库为空 = 修复前 Android 的状态，必须失败（用户看到的就是这个）
        let bare = reqwest::Client::builder()
            .use_preconfigured_tls(
                rustls::ClientConfig::builder()
                    .with_root_certificates(rustls::RootCertStore::empty())
                    .with_no_client_auth(),
            )
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let err = bare.get(&url).send().await.unwrap_err();
        let is_connect = err.is_connect();
        let text = req_err(&url, err).to_string();
        println!("空根证书库 → {text}");
        assert!(
            text.contains("UnknownIssuer") || is_connect,
            "空根证书库应当验不过证书，实际：{text}"
        );
    }

    #[test]
    fn api_root_normalization() {
        assert_eq!(api_root("https://chatapi.bnu.edu.cn/"), "https://chatapi.bnu.edu.cn/v1");
        assert_eq!(api_root("https://chatapi.bnu.edu.cn/v1"), "https://chatapi.bnu.edu.cn/v1");
        // 2026-09-23 起的新默认：机器通道在 /bnuapi 下
        assert_eq!(
            api_root("https://chatapi.bnu.edu.cn/bnuapi"),
            "https://chatapi.bnu.edu.cn/bnuapi/v1"
        );
        assert_eq!(
            api_root("https://chatapi.bnu.edu.cn/bnuapi/v1/"),
            "https://chatapi.bnu.edu.cn/bnuapi/v1"
        );
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

    #[test]
    fn sse_payload_only_matches_data_lines() {
        assert_eq!(sse_payload("data: {\"a\":1}\r"), Some("{\"a\":1}"));
        assert_eq!(sse_payload("event: ping"), None);
        assert_eq!(sse_payload(": comment"), None);
    }

    #[test]
    fn take_line_keeps_multibyte_chars_intact_across_chunks() {
        let full = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n";
        // 在「你」(3 字节) 的中间切开：模拟网络分块
        let cut = full.find('你').unwrap() + 1;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&full.as_bytes()[..cut]);
        assert!(take_line(&mut buf).is_none(), "行未收全时不应返回");
        buf.extend_from_slice(&full.as_bytes()[cut..]);
        let line = take_line(&mut buf).unwrap();
        assert!(line.contains("你好"), "不应出现替换字符: {line:?}");
        assert!(!line.contains('\u{FFFD}'), "不应出现替换字符: {line:?}");
        let mut acc = ChatStreamAccum::default();
        assert_eq!(
            acc.feed(sse_payload(&line).unwrap()),
            Some(ChatStreamEvent::Delta("你好".into()))
        );
    }

    #[test]
    fn stream_accum_collects_deltas_reasoning_and_usage() {
        let mut acc = ChatStreamAccum::default();
        assert_eq!(acc.feed(" {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]} "), Some(ChatStreamEvent::Delta("你好".into())));
        assert_eq!(
            acc.feed("{\"choices\":[{\"delta\":{\"reasoning_content\":\"想想\"}}]}"),
            Some(ChatStreamEvent::Reasoning("想想".into()))
        );
        // 只有 finish_reason，没有 delta：不抛事件
        assert_eq!(acc.feed("{\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}"), None);
        assert_eq!(acc.feed("{\"choices\":[],\"usage\":{\"completion_tokens\":42}}"), None);
        assert_eq!(acc.feed("[DONE]"), None);
        // 空行 / 非法 JSON 静默跳过
        assert_eq!(acc.feed("  "), None);
        assert_eq!(acc.feed("{oops"), None);

        assert!(acc.done);
        let reply = acc.into_reply();
        assert_eq!(reply.content, "你好");
        assert_eq!(reply.reasoning.as_deref(), Some("想想"));
        assert_eq!(reply.finish_reason, "stop");
        assert_eq!(reply.completion_tokens, Some(42));
    }

    #[test]
    fn stream_accum_falls_back_to_reasoning_when_content_empty() {
        let mut acc = ChatStreamAccum::default();
        acc.feed("{\"choices\":[{\"delta\":{\"reasoning_content\":\"只有思考\"}}]}");
        let reply = acc.into_reply();
        assert!(reply.content.is_empty());
        assert_eq!(reply.reasoning.as_deref(), Some("只有思考"));
        assert_eq!(reply.finish_reason, "stop", "缺省 finish_reason 视为 stop");
    }
}
