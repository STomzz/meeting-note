import { describe, expect, it } from 'vitest'
import { citeMarkup } from './citations'
import type { RetrievedChunk } from './retrieval'

function chunk(noteId: string, title: string, start: number, end: number): RetrievedChunk {
  return {
    chunkId: start,
    noteId,
    title,
    startLine: start,
    endLine: end,
    text: 'x',
    score: 0.1,
    sources: ['fts'],
  }
}

const sources = [
  chunk('工作/周会.md', '周会纪要', 3, 9),
  chunk('学习/Rust.md', 'Rust 所有权', 5, 5),
]

describe('citeMarkup', () => {
  it('把 [n] 换成可点角标并带来源提示', () => {
    const html = citeMarkup('镜像问题改用镜像站 [1]。', sources)
    expect(html).toContain('data-cite="1"')
    expect(html).toContain('title="周会纪要 · 第 3-9 行"')
    expect(html).toContain('>1</sup>')
  })

  it('连续编号各自替换', () => {
    const html = citeMarkup('结论 [1][2]', sources)
    expect(html.match(/class="cite"/g)).toHaveLength(2)
    expect(html).toContain('data-cite="2"')
    expect(html).toContain('Rust 所有权')
  })

  it('没有对应来源时原样保留', () => {
    expect(citeMarkup('依据不足 [9]', sources)).toBe('依据不足 [9]')
  })

  it('不碰普通正文与链接语法', () => {
    expect(citeMarkup('[文字](https://x)', sources)).toBe('[文字](https://x)')
    expect(citeMarkup('数组下标 a[0] 与年份 [2024]', sources)).toBe('数组下标 a[0] 与年份 [2024]')
  })

  it('标题里的引号会被转义', () => {
    const html = citeMarkup('[1]', [chunk('n.md', '他说"你好"', 1, 1)])
    expect(html).toContain('&quot;你好&quot;')
    expect(html).not.toContain('title="他说"你好""')
  })
})
