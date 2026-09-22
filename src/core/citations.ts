/** 回答里的行内引用角标：把 `[1] [2]` 变成可点的小角标。 */

import type { RetrievedChunk } from './retrieval'

/**
 * 处理**已转义**的正文文本：
 * - `[n]` 且存在第 n 个来源 → `<sup class="cite" data-cite="n" title="…">n</sup>`；
 * - 没有对应来源（模型多标了编号）→ 原样保留，避免误导；
 * - 只匹配 1~2 位数字，不会碰到普通正文或 `[文字](链接)`。
 */
export function citeMarkup(escapedText: string, sources: RetrievedChunk[]): string {
  return escapedText.replace(/\[(\d{1,2})\]/g, (raw, digits: string) => {
    const n = Number(digits)
    const source = sources[n - 1]
    if (!source) return raw
    const tip = escapeAttr(`${source.title} · 第 ${source.startLine}-${source.endLine} 行`)
    return `<sup class="cite" data-cite="${n}" title="${tip}">${n}</sup>`
  })
}

/** 属性值转义（title 里可能有引号）。 */
function escapeAttr(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}
