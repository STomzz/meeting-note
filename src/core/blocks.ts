/**
 * 笔记的「块」模型：所见即所得编辑的基础。
 *
 * 把一篇 md 切成块（一段连续非空行 = 一块；空行、代码围栏、`/v` 引用行各自成块），
 * 编辑时**只替换被改的那一块**，其余块保持逐字节原样——这样会议笔记里的
 * `/v 会议音频/...` 引用行、`> 🎙 转写` 块、`## 会议纪要（AI 整理）` 都不会
 * 被编辑器顺手重排（第三方 md 编辑器常见的坑）。
 *
 * 不变式：`joinBlocks(splitBlocks(md)) === md`（任何输入都逐字节相等）。
 */

import { parseRefLine } from './meetingNote'

export interface Block {
  /** 在 blocks 数组里的序号（渲染 key / 编辑目标） */
  id: number
  /** 原文起始行（1-based） */
  startLine: number
  /** 原文结束行（1-based，含） */
  endLine: number
  /** 原始 markdown 文本（多行用 \n 连接） */
  text: string
  /** 空行块：不可编辑，只用于还原原文 */
  gap: boolean
  /** 代码围栏块：整体编辑，内部不切分 */
  fence: boolean
}

const BLANK_RE = /^\s*$/
const FENCE_RE = /^\s*(```|~~~)/
const HEAD_RE = /^(#{1,6})\s+/
const UL_RE = /^\s*[-*+]\s+/
const OL_RE = /^\s*\d+[.)]\s+/
const QUOTE_RE = /^\s*>\s?/

/** 把 md 切成块。 */
export function splitBlocks(md: string): Block[] {
  const lines = md.split('\n')
  const out: Block[] = []
  let i = 0
  let line = 1
  while (i < lines.length) {
    const startLine = line
    if (BLANK_RE.test(lines[i])) {
      const buf: string[] = []
      while (i < lines.length && BLANK_RE.test(lines[i])) {
        buf.push(lines[i])
        i += 1
        line += 1
      }
      out.push({ id: out.length, startLine, endLine: line - 1, text: buf.join('\n'), gap: true, fence: false })
      continue
    }
    if (FENCE_RE.test(lines[i])) {
      const mark = lines[i].trimStart().slice(0, 3)
      const buf: string[] = [lines[i]]
      i += 1
      line += 1
      while (i < lines.length) {
        buf.push(lines[i])
        const closed = lines[i].trimStart().startsWith(mark)
        i += 1
        line += 1
        if (closed) break
      }
      out.push({ id: out.length, startLine, endLine: line - 1, text: buf.join('\n'), gap: false, fence: true })
      continue
    }
    if (parseRefLine(lines[i])) {
      // 音频引用独占一块：预览里一行一个播放器，编辑正文时也不会顺手把它带走
      out.push({ id: out.length, startLine, endLine: line, text: lines[i], gap: false, fence: false })
      i += 1
      line += 1
      continue
    }
    const buf: string[] = []
    while (
      i < lines.length &&
      !BLANK_RE.test(lines[i]) &&
      !FENCE_RE.test(lines[i]) &&
      !parseRefLine(lines[i])
    ) {
      buf.push(lines[i])
      i += 1
      line += 1
    }
    out.push({ id: out.length, startLine, endLine: line - 1, text: buf.join('\n'), gap: false, fence: false })
  }
  return out
}

/** 还原成 md（与 splitBlocks 互逆，逐字节相等）。 */
export function joinBlocks(blocks: Block[]): string {
  return blocks.map((b) => b.text).join('\n')
}

/**
 * 重算行号。
 *
 * 注意**不动 id**：id 是编辑句柄，必须对未改动的块保持稳定——
 * 否则「提交当前段 → 立刻点另一段编辑」会因为重新编号而编辑到别的块上。
 */
function renumberLines(blocks: Block[]): Block[] {
  let line = 1
  return blocks.map((b) => {
    const startLine = line
    const endLine = line + b.text.split('\n').length - 1
    line = endLine + 1
    return { ...b, startLine, endLine }
  })
}

/** 给新切出来的块分配不与现有 id 冲突的新 id。 */
function freshIds(blocks: Block[], start: number): Block[] {
  return blocks.map((b, i) => ({ ...b, id: start + i }))
}

function nextId(blocks: Block[]): number {
  return blocks.reduce((max, b) => Math.max(max, b.id), 0) + 1
}

/** 新建一个空行块（id 由调用方分配）。 */
function emptyGap(): Block {
  return { id: 0, startLine: 0, endLine: 0, text: '', gap: true, fence: false }
}

/** 删掉某一块（「空列表项回车退出列表」等），不留空行。 */
export function dropBlock(blocks: Block[], id: number): Block[] {
  return renumberLines(blocks.filter((b) => b.id !== id))
}

/** 把某一块的文本换成新文本（新文本内部若有空行会再切块），其余块原样不动。 */
export function replaceBlock(blocks: Block[], id: number, text: string): Block[] {
  const idx = blocks.findIndex((b) => b.id === id)
  if (idx < 0) return blocks
  const sub = freshIds(splitBlocks(text), nextId(blocks))
  return renumberLines([...blocks.slice(0, idx), ...sub, ...blocks.slice(idx + 1)])
}

/** 在文末追加内容（必要时先补一个空行块），用于插入 `/v` 引用等。 */
export function appendBlocks(blocks: Block[], text: string): Block[] {
  const last = blocks[blocks.length - 1]
  const needGap = Boolean(last) && !last.gap
  let at = nextId(blocks)
  const out = [...blocks]
  if (needGap) out.push({ ...emptyGap(), id: at++ })
  out.push(...freshIds(splitBlocks(text), at))
  return renumberLines(out)
}

/**
 * 在下标 index 的块后面插入一个段落（空段落 ≠ 空行，可以立刻编辑）。
 * `text` 非空时直接把内容放进去（回车拆分段落用）。
 */
export function insertParagraphAtIndex(
  blocks: Block[],
  index: number,
  text = '',
): { blocks: Block[]; id: number } {
  const maxId = nextId(blocks)
  // 跨过紧跟在后面的空行，让新段与上一段之间仍只有一个空行
  let at = Math.min(Math.max(index, -1) + 1, blocks.length)
  if (blocks[at]?.gap) at += 1
  const out = [...blocks.slice(0, at)]
  let cursor = maxId
  if (out.length && !out[out.length - 1].gap) out.push({ ...emptyGap(), id: cursor++ })
  const id = cursor
  out.push({ id, startLine: 0, endLine: 0, text, gap: false, fence: false })
  const after = blocks.slice(at)
  if (after.length && !after[0].gap) out.push({ ...emptyGap(), id: cursor + 1 })
  return { blocks: renumberLines([...out, ...after]), id }
}

/** 在某块后面插入一个段落（找不到该块时追加到末尾）。 */
export function insertParagraphAfter(
  blocks: Block[],
  id: number,
  text = '',
): { blocks: Block[]; id: number } {
  const idx = blocks.findIndex((b) => b.id === id)
  if (idx < 0) return appendParagraph(blocks, text)
  return insertParagraphAtIndex(blocks, idx, text)
}

/** 在文末追加一个段落。 */
export function appendParagraph(blocks: Block[], text = ''): { blocks: Block[]; id: number } {
  const last = blocks[blocks.length - 1]
  const maxId = nextId(blocks)
  const out = [...blocks]
  let cursor = maxId
  if (last && !last.gap) out.push({ ...emptyGap(), id: cursor++ })
  const id = cursor
  out.push({ id, startLine: 0, endLine: 0, text, gap: false, fence: false })
  return { blocks: renumberLines(out), id }
}

/**
 * 在下标 index 的块之前插入一段 markdown（会按行切块）。
 *
 * 用于把 `/v` 引用插到「会议纪要」标题之前——落到纪要段里的引用会被下次一键处理整段覆盖。
 */
export function insertBlocksBefore(blocks: Block[], index: number, text: string): Block[] {
  const at = Math.min(Math.max(index, 0), blocks.length)
  let cursor = nextId(blocks)
  const sub = freshIds(splitBlocks(text), cursor)
  cursor += sub.length
  const out = [...blocks.slice(0, at)]
  if (out.length && !out[out.length - 1].gap) out.push({ ...emptyGap(), id: cursor++ })
  out.push(...sub)
  const after = blocks.slice(at)
  if (after.length && !after[0].gap) out.push({ ...emptyGap(), id: cursor++ })
  return renumberLines([...out, ...after])
}

/** 在指定块后面插入一块（用于「在这段后面新起一段」）。 */
export function insertBlockAfter(blocks: Block[], id: number, text = ''): Block[] {
  const idx = blocks.findIndex((b) => b.id === id)
  if (idx < 0) return appendBlocks(blocks, text)
  // 原本紧跟其后的空行直接跨过去，让新段与上一段之间仍有一个空行
  const skipGap = idx + 1 < blocks.length && blocks[idx + 1].gap
  const at = skipGap ? idx + 2 : idx + 1
  const before = blocks.slice(0, at)
  const after = blocks.slice(at)
  const sub = splitBlocks(text)
  let id2 = nextId(blocks)
  const out = [...before]
  if (!skipGap && out.length && !out[out.length - 1].gap) out.push({ ...emptyGap(), id: id2++ })
  out.push(...freshIds(sub, id2))
  id2 += sub.length
  if (after.length && (!sub.length || !sub[sub.length - 1].gap)) out.push({ ...emptyGap(), id: id2 })
  return renumberLines([...out, ...after])
}

/** 某个块的结束行之后的位置（用于定位/滚动）。 */
export function blockAtLine(blocks: Block[], line: number): Block | null {
  for (const b of blocks) {
    if (line >= b.startLine && line <= b.endLine) return b
  }
  return null
}

export type BlockKind = 'p' | 'h1' | 'h2' | 'h3' | 'ul' | 'ol' | 'quote' | 'code'

export interface BlockPreset {
  kind: BlockKind
  label: string
  hint: string
}

export const BLOCK_PRESETS: BlockPreset[] = [
  { kind: 'p', label: '正文', hint: '普通段落' },
  { kind: 'h1', label: '一级标题', hint: '# 标题' },
  { kind: 'h2', label: '二级标题', hint: '## 标题' },
  { kind: 'h3', label: '三级标题', hint: '### 标题' },
  { kind: 'ul', label: '项目符号', hint: '- 列表项' },
  { kind: 'ol', label: '有序列表', hint: '1. 列表项' },
  { kind: 'quote', label: '引用', hint: '> 引用内容' },
  { kind: 'code', label: '代码块', hint: '``` 代码 ```' },
]

/** 去掉行首块级标记（标题 / 列表 / 引用）。 */
function stripMarkers(text: string): string {
  return text
    .split('\n')
    .map((l) => l.replace(HEAD_RE, '').replace(UL_RE, '').replace(OL_RE, '').replace(QUOTE_RE, ''))
    .join('\n')
}

/** 空文本套类型时给出的起始标记（方便接着写）。 */
function emptyPrefix(kind: BlockKind): string {
  switch (kind) {
    case 'h1':
      return '# '
    case 'h2':
      return '## '
    case 'h3':
      return '### '
    case 'ul':
      return '- '
    case 'ol':
      return '1. '
    case 'quote':
      return '> '
    case 'code':
      return '```\n\n```'
    default:
      return ''
  }
}

/** 把块文本套上某种块类型（对多行会逐行套）。 */
export function applyPreset(kind: BlockKind, text: string): string {
  const body = stripMarkers(text)
  if (!body.trim()) return emptyPrefix(kind)
  const lines = body.split('\n')
  switch (kind) {
    case 'p':
      return body
    case 'h1':
      return lines.map((l) => (l.trim() ? `# ${l}` : l)).join('\n')
    case 'h2':
      return lines.map((l) => (l.trim() ? `## ${l}` : l)).join('\n')
    case 'h3':
      return lines.map((l) => (l.trim() ? `### ${l}` : l)).join('\n')
    case 'ul':
      return lines.map((l) => (l.trim() ? `- ${l}` : l)).join('\n')
    case 'ol':
      return lines.map((l, i) => (l.trim() ? `${i + 1}. ${l}` : l)).join('\n')
    case 'quote':
      return lines.map((l) => `> ${l}`).join('\n')
    case 'code':
      return `\`\`\`\n${body}\n\`\`\``
    default:
      return body
  }
}

/** 看块文本当前是什么类型（菜单高亮 / 状态显示）。 */
export function detectKind(text: string): BlockKind {
  const first = text.split('\n')[0] ?? ''
  if (FENCE_RE.test(first)) return 'code'
  // `#` 后面要有空白或行尾（`#hashtag` 不算标题），也兼容只敲了 `##` 还没写内容
  const head = /^(#{1,6})(?:\s|$)/.exec(first)
  if (head) {
    const level = head[1].length
    return level === 1 ? 'h1' : level === 2 ? 'h2' : 'h3'
  }
  if (UL_RE.test(first)) return 'ul'
  if (OL_RE.test(first)) return 'ol'
  if (QUOTE_RE.test(first)) return 'quote'
  return 'p'
}
