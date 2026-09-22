//! Markdown 分块：标题优先切分 + 段落打包 + 长段落硬切。
//!
//! 行号全部为文件内 1-based 行号。

/// 目标块大小（字符数）。
pub const TARGET_CHUNK_CHARS: usize = 600;
/// 单块上限，超过则硬切。
pub const MAX_CHUNK_CHARS: usize = 900;
/// 相邻块保留的段落重叠上限（字符数）。
pub const OVERLAP_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub seq: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

/// 把 Markdown 全文切成带行号的块。
pub fn chunk_markdown(body: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = body.split('\n').collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Vec::new();
    }

    // 1) 按标题行划分 section（heading 行属于新 section）
    let mut sections: Vec<(usize, usize)> = Vec::new();
    let mut start = 1usize;
    for (i, line) in lines.iter().enumerate() {
        let line_no = i + 1;
        if line_no > 1 && is_heading(line) {
            sections.push((start, line_no - 1));
            start = line_no;
        }
    }
    sections.push((start, lines.len()));

    // 2) 每个 section 视长度直接成块或按段落打包
    let mut out: Vec<Chunk> = Vec::new();
    for (s, e) in sections {
        let text = section_text(&lines, s, e);
        if text.trim().is_empty() {
            continue;
        }
        if char_len(&text) <= TARGET_CHUNK_CHARS {
            push_chunk(&mut out, s, e, &text);
        } else {
            let paras = paragraphs(&lines, s, e);
            pack_paragraphs(&mut out, paras);
        }
    }
    for (i, c) in out.iter_mut().enumerate() {
        c.seq = i;
    }
    out
}

fn is_heading(line: &str) -> bool {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ')
}

fn section_text(lines: &[&str], start: usize, end: usize) -> String {
    lines[start - 1..end].join("\n")
}

/// 段落（空行分隔）及其行号范围，1-based、闭区间。
fn paragraphs(lines: &[&str], start: usize, end: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut cur_start = start;
    let mut buf: Vec<&str> = Vec::new();
    for i in start..=end {
        let line = lines[i - 1];
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((cur_start, i - 1, buf.join("\n")));
                buf.clear();
            }
            cur_start = i + 1;
        } else {
            if buf.is_empty() {
                cur_start = i;
            }
            buf.push(line);
        }
    }
    if !buf.is_empty() {
        out.push((cur_start, end, buf.join("\n")));
    }
    out
}

fn pack_paragraphs(out: &mut Vec<Chunk>, paras: Vec<(usize, usize, String)>) {
    let mut cur: Vec<(usize, usize, String)> = Vec::new();
    let mut cur_len = 0usize;

    let flush = |out: &mut Vec<Chunk>, cur: &mut Vec<(usize, usize, String)>| {
        if cur.is_empty() {
            return;
        }
        let s = cur.first().unwrap().0;
        let e = cur.last().unwrap().1;
        let text = cur
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        push_chunk(out, s, e, &text);
        // 重叠：保留最后一段（若不大），避免跨块语义断裂
        let keep = cur
            .last()
            .cloned()
            .filter(|(_, _, t)| char_len(t) <= OVERLAP_CHARS.max(200));
        cur.clear();
        if let Some(k) = keep {
            cur.push(k);
        }
    };

    for p in paras {
        let plen = char_len(&p.2);
        if plen > MAX_CHUNK_CHARS {
            flush(out, &mut cur);
            cur_len = 0;
            for piece in hard_split(&p.2, MAX_CHUNK_CHARS) {
                push_chunk(out, p.0, p.1, &piece);
            }
            continue;
        }
        if cur_len + plen > TARGET_CHUNK_CHARS && !cur.is_empty() {
            flush(out, &mut cur);
            cur_len = cur.iter().map(|(_, _, t)| char_len(t) + 2).sum();
        }
        cur_len += plen + 2;
        cur.push(p);
    }
    flush(out, &mut cur);
}

fn push_chunk(out: &mut Vec<Chunk>, start: usize, end: usize, text: &str) {
    out.push(Chunk {
        seq: out.len(),
        start_line: start,
        end_line: end,
        text: text.trim_end().to_string(),
    });
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// 按字符硬切长段（尽量在换行处断开）。
fn hard_split(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        if char_len(&cur) + char_len(line) > max && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if char_len(line) > max {
            for ch in line.chars() {
                cur.push(ch);
                if char_len(&cur) >= max {
                    out.push(std::mem::take(&mut cur));
                }
            }
        } else {
            cur.push_str(line);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_by_headings_with_line_numbers() {
        let md = "# A\ntext a\n\n## B\ntext b1\ntext b2\n";
        let cs = chunk_markdown(md);
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].start_line, cs[0].end_line), (1, 3));
        assert_eq!((cs[1].start_line, cs[1].end_line), (4, 7));
    }

    #[test]
    fn long_section_is_packed_into_multiple_chunks() {
        let mut md = String::from("# 标题\n");
        for i in 0..60 {
            md.push_str(&format!(
                "这是第 {} 段内容，用来把章节撑长一些，顺便验证段落打包与重叠逻辑是否正确。\n\n",
                i
            ));
        }
        let cs = chunk_markdown(&md);
        assert!(cs.len() >= 3, "got {} chunks", cs.len());
        for c in &cs {
            assert!(char_len(&c.text) <= MAX_CHUNK_CHARS);
        }
    }

    #[test]
    fn empty_body_yields_no_chunks() {
        assert!(chunk_markdown("\n\n   \n").is_empty());
    }
}
