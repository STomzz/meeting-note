//! 会议纪要生成：结构化 JSON（含解析失败自动修复）+ Markdown 渲染。
//!
//! 提示词与流程移植自既有会议后端（`backend/app/prompts/*`、`services/minutes.py`）：
//! 短会议一次生成；长会议（≥6 万字）先按片段抽取（map）再合并（reduce）；
//! JSON 解析失败时带错误与原输出做一次"修复重试"。

use crate::meetings;
use crate::models::{self, ChatMessage, ChatOptions, EndpointConfig, ModelConfig};
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Mutex;
use std::time::Instant;

/// 纪要生成温度（与既有后端一致）。
pub const MINUTES_TEMPERATURE: f32 = 0.2;
/// 纪要生成最大输出 token。
pub const MINUTES_MAX_TOKENS: u32 = 8192;
/// 超过该字数走 map-reduce。
pub const MAP_REDUCE_THRESHOLD_CHARS: usize = 60_000;
/// map 阶段每片字数。
pub const CHUNK_CHARS: usize = 20_000;
/// 转写文本上限。
pub const MAX_TRANSCRIPT_CHARS: usize = 1_500_000;
/// 修复重试时携带的上一次输出长度上限。
pub const REPAIR_PREVIEW_CHARS: usize = 6_000;

pub const MINUTES_SYSTEM_PROMPT: &str = "你是一名专业的会议纪要整理助手。你的任务是把会议的语音转写文本整理成一份结构化、可直接分发的会议纪要。\n\
\n\
【铁律】\n\
1. 只能依据【会议信息】和【转写原文】中的事实，严禁编造任何内容。\n\
2. 原文未提到的人名、数字、期限、结论、决议一律不要写；无法确定的信息留空或不写，绝不猜测。\n\
3. 转写文本可能存在同音字、错别字、断句错误、口语重复，请结合上下文修正明显的识别错误并去掉口头语，但不得改变原意。\n\
4. 待办事项必须来自原文中明确的承诺、要求或分工；owner（责任人）和 due（期限）只有在原文明确提及时才填写，否则为 null。\n\
5. 原文没有对应内容时，数组字段返回 []，字符串字段返回 null 或简短空说明，不要凑内容。\n\
6. 语言与原文一致（默认中文）；保持客观、书面化、条目清晰。\n\
\n\
【输出格式】\n\
只输出一个合法的 JSON 对象，不要输出 Markdown 代码块、解释或任何多余文字。JSON 结构如下：\n\
\n\
{\n\
  \"title\": \"会议标题（可用用户提供的标题，否则从内容概括一个简短标题）\",\n\
  \"meeting_date\": \"会议日期（原文/用户提供时才填，否则 null）\",\n\
  \"participants\": [\"参会人姓名（原文/用户明确提供时才填）\"],\n\
  \"overview\": \"整体摘要，3~6 句，说清楚会议主题、讨论重点和整体结论\",\n\
  \"topics\": [\n\
    {\"title\": \"议题名称\", \"points\": [\"该议题下的讨论要点，按发言逻辑归纳\"]}\n\
  ],\n\
  \"decisions\": [\"已达成的决议/结论，逐条列出\"],\n\
  \"action_items\": [\n\
    {\"task\": \"待办事项\", \"owner\": \"责任人或 null\", \"due\": \"期限或 null\", \"source\": \"对应的原文关键句（用于核对）\"}\n\
  ],\n\
  \"risks\": [\"风险、阻塞、遗留问题\"],\n\
  \"next_steps\": [\"下一步计划/时间安排\"]\n\
}";

pub const CHUNK_SYSTEM_PROMPT: &str = "你是一名会议内容抽取助手。这次你只处理一场长会议中的【一个片段】，请从这个片段里抽取信息，供后续合并使用。\n\
\n\
【铁律】\n\
1. 只依据片段原文，严禁编造；片段未提及的内容不要写。\n\
2. 修正明显的语音识别错误，去掉口头语，但不得改变原意。\n\
3. owner（责任人）和 due（期限）只有在原文明确提及时才填写，否则为 null。\n\
4. 不要输出任何解释，只输出一个合法 JSON 对象。\n\
\n\
【输出格式】\n\
{\n\
  \"overview\": \"本片段的要点，1~3 句\",\n\
  \"topics\": [{\"title\": \"议题\", \"points\": [\"要点\"]}],\n\
  \"decisions\": [\"本片段中达成的决议/结论\"],\n\
  \"action_items\": [{\"task\": \"待办\", \"owner\": \"责任人或 null\", \"due\": \"期限或 null\", \"source\": \"原文关键句\"}],\n\
  \"risks\": [\"风险/遗留问题\"],\n\
  \"next_steps\": [\"下一步计划\"]\n\
}";

pub const MERGE_SYSTEM_PROMPT: &str = "你是一名会议纪要整理助手。输入是一场长会议按时间顺序分段抽取出的多份片段信息，请把它们合并成一份完整的结构化会议纪要。\n\
\n\
【铁律】\n\
1. 只使用片段信息里出现的事实，严禁编造。\n\
2. 合并同一议题的重复内容，保留时间顺序与因果关系；不同片段中的同一事项不要重复列出。\n\
3. owner（责任人）和 due（期限）只有在片段中明确提及时才填写，否则为 null。\n\
4. 只输出一个合法 JSON 对象，不要输出 Markdown 代码块、解释或多余文字。\n\
\n\
【输出格式】\n\
{\n\
  \"title\": \"会议标题（从内容概括或沿用用户提供）\",\n\
  \"meeting_date\": \"会议日期或 null\",\n\
  \"participants\": [\"参会人（片段中明确出现时才填）\"],\n\
  \"overview\": \"整场会议的整体摘要，3~6 句\",\n\
  \"topics\": [{\"title\": \"议题名称\", \"points\": [\"讨论要点\"]}],\n\
  \"decisions\": [\"达成的决议/结论\"],\n\
  \"action_items\": [{\"task\": \"待办\", \"owner\": \"责任人或 null\", \"due\": \"期限或 null\", \"source\": \"原文关键句\"}],\n\
  \"risks\": [\"风险、阻塞、遗留问题\"],\n\
  \"next_steps\": [\"下一步计划\"]\n\
}";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Topic {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub points: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionItem {
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

/// 结构化纪要（字段名与提示词里的 JSON 结构一致，即 snake_case）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Minutes {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub meeting_date: Option<String>,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub topics: Vec<Topic>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<ActionItem>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesOutcome {
    pub minutes: Minutes,
    pub markdown: String,
    pub model: String,
    pub elapsed_ms: u64,
    pub completion_tokens: Option<u64>,
    pub repaired: bool,
    pub used_map_reduce: bool,
}

/// 从模型输出里提取第一个合法 JSON 对象（容忍 ``` 围栏与前后废话）。
pub fn extract_json_object(text: &str) -> Option<Value> {
    if text.trim().is_empty() {
        return None;
    }
    let cleaned = strip_code_fence(text.trim());
    for (i, ch) in cleaned.char_indices() {
        if ch != '{' {
            continue;
        }
        let mut de = serde_json::Deserializer::from_str(&cleaned[i..]);
        if let Ok(value) = Value::deserialize(&mut de) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    None
}

fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let after = match trimmed.strip_prefix("```") {
        // 去掉语言标记那一行
        Some(rest) => match rest.find('\n') {
            Some(idx) => &rest[idx + 1..],
            None => rest,
        },
        None => trimmed,
    };
    after.trim().trim_end_matches("```").trim().to_string()
}

/// 按段落切分转写文本，尽量保持段落完整（移植自 `split_transcript`）。
pub fn split_transcript(transcript: &str, chunk_chars: usize) -> Vec<String> {
    if transcript.chars().count() <= chunk_chars {
        return vec![transcript.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_len = 0usize;
    for line in transcript.lines() {
        let line_len = line.chars().count() + 1;
        if !current.is_empty() && current_len + line_len > chunk_chars {
            chunks.push(current.join("\n").trim().to_string());
            current.clear();
            current_len = 0;
        }
        current.push(line);
        current_len += line_len;
    }
    if !current.is_empty() {
        chunks.push(current.join("\n").trim().to_string());
    }
    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

/// 纪要生成用的用户提示词。
pub fn minutes_user_prompt(
    title: &str,
    meeting_date: Option<&str>,
    participants: &[String],
    transcript: &str,
) -> String {
    let participants_text =
        if participants.is_empty() { "未提供".to_string() } else { participants.join("、") };
    format!(
        "【会议信息】\n标题：{}\n日期：{}\n参会人：{}\n\n【转写原文】\n{}\n",
        if title.trim().is_empty() { "未提供" } else { title.trim() },
        meeting_date.unwrap_or("未提供"),
        participants_text,
        transcript
    )
}

/// 把结构化纪要渲染成 Markdown（移植自 `rendering.py`）。
pub fn render_markdown(m: &Minutes) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("# {}", m.title.trim()));
    lines.push(String::new());

    let mut meta: Vec<String> = Vec::new();
    if let Some(d) = m.meeting_date.as_deref().filter(|d| !d.trim().is_empty()) {
        meta.push(format!("**日期**：{}", d.trim()));
    }
    if !m.participants.is_empty() {
        meta.push(format!("**参会人**：{}", m.participants.join("、")));
    }
    if !meta.is_empty() {
        lines.extend(meta);
        lines.push(String::new());
    }

    lines.push("## 一、会议摘要".into());
    lines.push(String::new());
    lines.push(if m.overview.trim().is_empty() { "（无）".into() } else { m.overview.trim().into() });
    lines.push(String::new());

    lines.push("## 二、议题与讨论".into());
    lines.push(String::new());
    if m.topics.is_empty() {
        lines.push("（无）".into());
        lines.push(String::new());
    } else {
        for (i, topic) in m.topics.iter().enumerate() {
            lines.push(format!("### {}. {}", i + 1, topic.title.trim()));
            lines.push(String::new());
            for point in &topic.points {
                lines.push(format!("- {}", point.trim()));
            }
            lines.push(String::new());
        }
    }

    lines.push("## 三、决议事项".into());
    lines.push(String::new());
    if m.decisions.is_empty() {
        lines.push("（无）".into());
    } else {
        for (i, d) in m.decisions.iter().enumerate() {
            lines.push(format!("{}. {}", i + 1, d.trim()));
        }
    }
    lines.push(String::new());

    lines.push("## 四、待办事项".into());
    lines.push(String::new());
    if m.action_items.is_empty() {
        lines.push("（无）".into());
    } else {
        lines.push("| 序号 | 待办事项 | 责任人 | 期限 |".into());
        lines.push("| --- | --- | --- | --- |".into());
        for (i, item) in m.action_items.iter().enumerate() {
            let owner = item.owner.clone().unwrap_or_else(|| "—".into());
            let due = item.due.clone().unwrap_or_else(|| "—".into());
            let task = item.task.replace('|', "\\|");
            lines.push(format!("| {} | {} | {} | {} |", i + 1, task.trim(), owner.trim(), due.trim()));
        }
    }
    lines.push(String::new());

    if !m.risks.is_empty() {
        lines.push("## 五、风险与遗留问题".into());
        lines.push(String::new());
        for r in &m.risks {
            lines.push(format!("- {}", r.trim()));
        }
        lines.push(String::new());
    }

    if !m.next_steps.is_empty() {
        lines.push("## 六、下一步计划".into());
        lines.push(String::new());
        for s in &m.next_steps {
            lines.push(format!("- {}", s.trim()));
        }
        lines.push(String::new());
    }

    let mut out = lines.join("\n");
    while out.ends_with("\n\n") {
        out.pop();
    }
    out.trim_end().to_string() + "\n"
}

/// 调用对话模型，优先使用 `response_format=json_object`，上游不支持时降级（记住能力）。
async fn chat_json(
    cfg: &EndpointConfig,
    messages: &[ChatMessage],
    json_mode: &mut bool,
) -> Result<models::ChatReply> {
    let mut opts = ChatOptions {
        max_tokens: Some(MINUTES_MAX_TOKENS),
        temperature: Some(MINUTES_TEMPERATURE),
        extra: None,
    };
    if *json_mode {
        let mut extra = Map::new();
        extra.insert("response_format".into(), json!({ "type": "json_object" }));
        opts.extra = Some(extra);
    }
    match models::chat(cfg, messages, &opts).await {
        Ok(reply) => Ok(reply),
        Err(e) => {
            let msg = e.to_string();
            if *json_mode && msg.contains("400") {
                // 上游不支持 response_format：降级重试一次
                *json_mode = false;
                opts.extra = None;
                models::chat(cfg, messages, &opts).await
            } else {
                Err(e)
            }
        }
    }
}

fn parse_minutes(content: &str) -> (Option<Minutes>, Option<String>) {
    let Some(value) = extract_json_object(content) else {
        return (None, Some("输出中找不到合法 JSON 对象".into()));
    };
    match serde_json::from_value::<Minutes>(value) {
        Ok(m) => {
            if m.title.trim().is_empty() && m.overview.trim().is_empty() && m.topics.is_empty() {
                (None, Some("JSON 缺少 title/overview/topics 等有效字段".into()))
            } else {
                (Some(m), None)
            }
        }
        Err(e) => (None, Some(format!("JSON 结构不匹配: {e}"))),
    }
}

/// 生成会议纪要。
pub async fn generate(
    conn: &Mutex<Connection>,
    cfg: &ModelConfig,
    meeting_id: &str,
) -> Result<MinutesOutcome> {
    let started = Instant::now();
    let chat_cfg = cfg
        .chat
        .as_ref()
        .ok_or_else(|| anyhow!("未配置对话模型：请到「设置」里填写对话端点"))?
        .clone();
    let (meeting, transcript) = {
        let c = conn.lock().map_err(|_| anyhow!("数据库锁定失败"))?;
        let m = meetings::get(&c, meeting_id)?;
        let t: String = c.query_row(
            "SELECT transcript FROM meetings WHERE id = ?1",
            rusqlite::params![meeting_id],
            |r| r.get(0),
        )?;
        (m, t)
    };
    let transcript = transcript.trim().to_string();
    if transcript.is_empty() {
        return Err(anyhow!("这场会议还没有转写文本，请先转写"));
    }
    if transcript.chars().count() > MAX_TRANSCRIPT_CHARS {
        return Err(anyhow!(
            "转写文本过长（{} 字），上限 {} 字",
            transcript.chars().count(),
            MAX_TRANSCRIPT_CHARS
        ));
    }

    let mut json_mode = true;
    let mut repaired = false;
    let use_map_reduce = transcript.chars().count() >= MAP_REDUCE_THRESHOLD_CHARS;

    let (content, tokens) = if use_map_reduce {
        let chunks = split_transcript(&transcript, CHUNK_CHARS);
        let mut partials: Vec<Value> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let messages = vec![
                ChatMessage::system(CHUNK_SYSTEM_PROMPT),
                ChatMessage::user(format!(
                    "【片段序号】第 {} / {} 段\n【片段原文】\n{}",
                    i + 1,
                    chunks.len(),
                    chunk
                )),
            ];
            let reply = chat_json(&chat_cfg, &messages, &mut json_mode).await?;
            if let Some(v) = extract_json_object(&reply.content) {
                partials.push(v);
            }
        }
        if partials.is_empty() {
            return Err(anyhow!("长会议分段抽取全部失败，无法生成纪要"));
        }
        let merge_input: String = serde_json::to_string_pretty(&partials)?
            .chars()
            .take(MAX_TRANSCRIPT_CHARS)
            .collect();
        let prompt = minutes_user_prompt(
            &meeting.title,
            None,
            &[],
            &format!("以下是按时间顺序的分段抽取结果：\n{merge_input}"),
        );
        let messages = vec![ChatMessage::system(MERGE_SYSTEM_PROMPT), ChatMessage::user(prompt)];
        let reply = chat_json(&chat_cfg, &messages, &mut json_mode).await?;
        (reply.content, reply.completion_tokens)
    } else {
        let prompt = minutes_user_prompt(&meeting.title, None, &[], &transcript);
        let messages = vec![ChatMessage::system(MINUTES_SYSTEM_PROMPT), ChatMessage::user(prompt)];
        let reply = chat_json(&chat_cfg, &messages, &mut json_mode).await?;
        (reply.content, reply.completion_tokens)
    };

    let (mut minutes, mut err) = parse_minutes(&content);
    let mut tokens = tokens;
    if minutes.is_none() {
        // 修复重试一次
        repaired = true;
        let repair_prompt = format!(
            "你上一步的输出不是合法的目标 JSON，解析失败。\n\n【解析错误】\n{}\n\n【你上一步的输出】\n{}\n\n请严格按之前约定的 JSON 结构重新输出，只输出 JSON 对象本身，不要任何解释或 Markdown 代码块。",
            err.clone().unwrap_or_default(),
            content.chars().take(REPAIR_PREVIEW_CHARS).collect::<String>()
        );
        let messages =
            vec![ChatMessage::system(MINUTES_SYSTEM_PROMPT), ChatMessage::user(repair_prompt)];
        let reply = chat_json(&chat_cfg, &messages, &mut json_mode).await?;
        tokens = reply.completion_tokens.or(tokens);
        let (m, e2) = parse_minutes(&reply.content);
        minutes = m;
        err = e2;
    }
    let minutes = minutes.ok_or_else(|| {
        anyhow!("纪要 JSON 两次解析失败：{}", err.unwrap_or_else(|| "未知错误".into()))
    })?;

    let markdown = render_markdown(&minutes);
    Ok(MinutesOutcome {
        minutes,
        markdown,
        model: chat_cfg.model.clone(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        completion_tokens: tokens,
        repaired,
        used_map_reduce: use_map_reduce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_noisy_output() {
        let raw = "好的，这是结果：\n```json\n{\"title\": \"周会\", \"overview\": \"讨论了镜像站\", \"topics\": []}\n```\n以上。";
        let v = extract_json_object(raw).unwrap();
        assert_eq!(v["title"], "周会");

        // 纯 JSON 也支持
        let v = extract_json_object("{\"title\":\"a\"}").unwrap();
        assert_eq!(v["title"], "a");
        assert!(extract_json_object("没有 JSON").is_none());
    }

    #[test]
    fn split_transcript_keeps_paragraphs() {
        let text = (1..=10).map(|i| format!("第{i}行内容")).collect::<Vec<_>>().join("\n");
        let chunks = split_transcript(&text, 30);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| !c.is_empty()));
        assert_eq!(chunks.join("\n"), text);
    }

    #[test]
    fn parses_minutes_with_missing_fields() {
        let (m, err) = parse_minutes(r#"{"title":"周会","action_items":[{"task":"部署"}]}"#);
        assert!(err.is_none());
        let m = m.unwrap();
        assert_eq!(m.title, "周会");
        assert_eq!(m.action_items.len(), 1);
        assert_eq!(m.action_items[0].task, "部署");
        assert!(m.action_items[0].owner.is_none());
        assert!(m.participants.is_empty());
    }

    #[test]
    fn rejects_empty_json_object() {
        let (m, err) = parse_minutes("{}");
        assert!(m.is_none());
        assert!(err.unwrap().contains("有效字段"));
    }

    #[test]
    fn renders_markdown_with_action_table() {
        let m: Minutes = serde_json::from_value(json!({
            "title": "周会纪要",
            "meeting_date": "2026-09-20",
            "participants": ["张三", "李四"],
            "overview": "讨论了镜像站方案。",
            "topics": [{"title": "镜像站", "points": ["改用镜像站", "下周部署"]}],
            "decisions": ["改用镜像站"],
            "action_items": [{"task": "部署 | 验证", "owner": "张三", "due": "下周三"}],
            "risks": ["网络不稳"],
            "next_steps": ["周五复盘"]
        }))
        .unwrap();
        let md = render_markdown(&m);
        assert!(md.starts_with("# 周会纪要"));
        assert!(md.contains("**参会人**：张三、李四"));
        assert!(md.contains("### 1. 镜像站"));
        assert!(md.contains("| 1 | 部署 \\| 验证 | 张三 | 下周三 |"));
        assert!(md.contains("## 五、风险与遗留问题"));
        assert!(md.contains("## 六、下一步计划"));
    }

    #[test]
    fn empty_minutes_render_placeholders() {
        let m = Minutes { title: "空".into(), ..Default::default() };
        let md = render_markdown(&m);
        assert!(md.contains("（无）"));
        assert!(md.contains("## 三、决议事项"));
    }
}
