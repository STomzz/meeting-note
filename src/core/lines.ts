/**
 * 行号 ↔ 字符偏移的小工具（1-based 行号，含起止行）。
 *
 * 用于「问答引用 → 打开笔记并高亮第 X-Y 行」：textarea 没有行概念，
 * 只能把行号换成 selection 区间再滚动。
 */

/** 每一行行首在文本中的偏移（第 0 项是第 1 行行首 0）。 */
export function lineStarts(text: string): number[] {
  const starts = [0]
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\n') starts.push(i + 1)
  }
  return starts
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(Math.max(n, lo), hi)
}

/** start..end（1-based，含两端）对应的字符区间；越界自动收敛到合法范围。 */
export function lineRangeOffset(
  text: string,
  startLine: number,
  endLine: number,
): { start: number; end: number } {
  const starts = lineStarts(text)
  const s = clamp(Math.floor(startLine) || 1, 1, starts.length)
  const e = clamp(Math.floor(endLine) || s, s, starts.length)
  const start = starts[s - 1]
  // 第 e 行之后还有行时，取下一行行首；否则取文本末尾
  const end = e < starts.length ? starts[e] : text.length
  return { start, end }
}
