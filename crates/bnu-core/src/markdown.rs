//! Markdown 解析：标题与 frontmatter 标签提取。
//!
//! 注意：`body` 返回**完整原文**（含 frontmatter），保证 chunk 行号与文件一一对应，
//! 便于检索结果按行号回跳。

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDoc {
    /// 第一个一级标题；没有则用调用方给的兜底标题。
    pub title: String,
    /// frontmatter 中的 tags。
    pub tags: Vec<String>,
    /// 完整原文。
    pub body: String,
}

/// 解析 Markdown 文本。
pub fn parse(content: &str, fallback_title: &str) -> ParsedDoc {
    let (front, rest) = split_frontmatter(content);
    let tags = front.map(parse_tags).unwrap_or_default();
    let title = find_h1(rest).unwrap_or_else(|| fallback_title.trim().to_string());
    ParsedDoc {
        title,
        tags,
        body: content.to_string(),
    }
}

/// 拆出 frontmatter 与正文。frontmatter 必须是文件开头的 `---` 块。
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let mut lines = content.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, content);
    };
    if first.trim_end() != "---" {
        return (None, content);
    }
    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        if line.trim_end() == "---" {
            let front = &content[first.len()..consumed - line.len()];
            return (Some(front), &content[consumed..]);
        }
    }
    (None, content)
}

/// 从 frontmatter 文本中提取 tags，兼容：
/// `tags: [a, b]` / `tags: a, b` / `tags:` 后跟 `- a` 列表。
fn parse_tags(front: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_tags_list = false;
    for raw in front.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("tags:") {
            in_tags_list = true;
            let rest = rest.trim();
            if !rest.is_empty() {
                let inner = rest.trim_start_matches('[').trim_end_matches(']');
                for t in inner.split(',') {
                    push_tag(&mut out, t);
                }
                in_tags_list = false;
            }
            continue;
        }
        if in_tags_list {
            if let Some(item) = line.strip_prefix('-') {
                push_tag(&mut out, item);
            } else if !line.is_empty() {
                in_tags_list = false;
            }
        }
    }
    out
}

fn push_tag(out: &mut Vec<String>, raw: &str) {
    let t = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if !t.is_empty() && !out.iter().any(|x| x == t) {
        out.push(t.to_string());
    }
}

/// 找第一个 ATX 一级标题（`# xxx`）。
fn find_h1(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_and_tags() {
        let md = "---\ntags: [工作, 会议]\n---\n\n# 周会纪要\n\n内容";
        let doc = parse(md, "untitled");
        assert_eq!(doc.title, "周会纪要");
        assert_eq!(doc.tags, vec!["工作", "会议"]);
        assert!(doc.body.contains("周会纪要"));
    }

    #[test]
    fn parses_tag_list_style() {
        let md = "---\ntags:\n  - 项目\n  - 想法\nfoo: bar\n---\n正文";
        let doc = parse(md, "t");
        assert_eq!(doc.tags, vec!["项目", "想法"]);
    }

    #[test]
    fn falls_back_to_filename_title() {
        let doc = parse("没有标题的正文", "我的笔记");
        assert_eq!(doc.title, "我的笔记");
        assert!(doc.tags.is_empty());
    }

    #[test]
    fn no_frontmatter_body_is_full_text() {
        let md = "# hello\nworld\n";
        let doc = parse(md, "x");
        assert_eq!(doc.body, md);
    }
}
