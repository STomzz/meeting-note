import { describe, expect, it } from 'vitest'
import { lineRangeOffset, lineStarts } from './lines'

describe('lineStarts', () => {
  it('单行文本只有行首 0', () => {
    expect(lineStarts('abc')).toEqual([0])
  })

  it('空文本也算一行', () => {
    expect(lineStarts('')).toEqual([0])
  })

  it('多行给出每行行首', () => {
    expect(lineStarts('a\nbb\nccc')).toEqual([0, 2, 5])
  })

  it('末尾换行会多出一行空行', () => {
    expect(lineStarts('a\n')).toEqual([0, 2])
  })
})

describe('lineRangeOffset', () => {
  const text = 'one\ntwo\nthree\nfour'

  it('取单行区间（不含换行符后的下一行内容）', () => {
    const { start, end } = lineRangeOffset(text, 2, 2)
    expect(text.slice(start, end).trimEnd()).toBe('two')
  })

  it('取跨行区间', () => {
    const { start, end } = lineRangeOffset(text, 2, 3)
    expect(text.slice(start, end).trimEnd()).toBe('two\nthree')
  })

  it('末行取到文本末尾', () => {
    const { start, end } = lineRangeOffset(text, 4, 4)
    expect(text.slice(start, end)).toBe('four')
    expect(end).toBe(text.length)
  })

  it('越界自动收敛', () => {
    expect(lineRangeOffset(text, 0, 99)).toEqual({ start: 0, end: text.length })
    const last = lineRangeOffset(text, 99, 99)
    expect(text.slice(last.start, last.end)).toBe('four')
  })

  it('endLine 小于 startLine 时按单行处理', () => {
    const { start, end } = lineRangeOffset(text, 2, 1)
    expect(text.slice(start, end).trimEnd()).toBe('two')
  })
})
