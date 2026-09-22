import { describe, expect, it } from 'vitest'
import {
  ALL_KINDS,
  edgeWidth,
  filterSnapshot,
  kindColor,
  kindLabel,
  kindsLabel,
  kindsOf,
  nodeSize,
  outcomeSummary,
  type GraphSnapshot,
} from './graph'

const snapshot: GraphSnapshot = {
  nodes: [
    { id: 1, name: '张伟', kind: 'person', degree: 6, noteCount: 2 },
    { id: 2, name: 'gpu-node3', kind: 'place', degree: 2, noteCount: 1 },
    { id: 3, name: 'Rust', kind: 'concept', degree: 0, noteCount: 1 },
    { id: 4, name: 'BNU Notes', kind: 'org', degree: 1, noteCount: 1 },
  ],
  edges: [
    { src: 1, dst: 2, weight: 3, kinds: '验证于' },
    { src: 1, dst: 4, weight: 1, kinds: '包含,采用' },
    { src: 3, dst: 4, weight: 1, kinds: '依赖' },
  ],
  truncated: 5,
}

describe('core/graph 纯函数', () => {
  it('按类型筛选时丢掉两端被筛掉的边', () => {
    const out = filterSnapshot(snapshot, { kinds: ['person'], keyword: '', minDegree: 0 })
    expect(out.nodes.map((n) => n.id)).toEqual([1])
    expect(out.edges).toEqual([])
    expect(out.truncated).toBe(5)
  })

  it('关键词不区分大小写、匹配子串', () => {
    const out = filterSnapshot(snapshot, { kinds: [], keyword: 'rust', minDegree: 0 })
    expect(out.nodes.map((n) => n.name)).toEqual(['Rust'])
    const upper = filterSnapshot(snapshot, { kinds: [], keyword: 'BNU', minDegree: 0 })
    expect(upper.nodes.map((n) => n.id)).toEqual([4])
  })

  it('度数下限与类型条件可叠加', () => {
    const out = filterSnapshot(snapshot, { kinds: [], keyword: '', minDegree: 2 })
    expect(out.nodes.map((n) => n.id)).toEqual([1, 2])
    expect(out.edges.map((e) => e.src)).toEqual([1])
  })

  it('类型选项按固定顺序输出', () => {
    expect(kindsOf(snapshot)).toEqual(ALL_KINDS.filter((k) => ['person', 'place', 'concept', 'org'].includes(k)))
  })

  it('尺寸映射单调递增且有上限', () => {
    expect(nodeSize(0)).toBeLessThan(nodeSize(4))
    expect(nodeSize(4)).toBeLessThanOrEqual(28)
    expect(nodeSize(999)).toBe(28)
    expect(edgeWidth(1)).toBe(1)
    expect(edgeWidth(8)).toBeGreaterThan(edgeWidth(2))
    expect(edgeWidth(999)).toBe(5)
  })

  it('未知类型有兜底标签与颜色', () => {
    expect(kindLabel('person')).toBe('人物')
    expect(kindLabel('unknown')).toBe('unknown')
    expect(kindColor('nope')).toBe(kindColor('other'))
  })

  it('关系短语最多展示 3 个', () => {
    expect(kindsLabel('a,b')).toBe('a / b')
    expect(kindsLabel('a,b,c,d')).toBe('a / b / c +1')
    expect(kindsLabel('')).toBe('')
  })

  it('抽取结果摘要带中断标记', () => {
    const text = outcomeSummary({
      notesTotal: 3,
      notesDone: 2,
      notesSkipped: 1,
      notesFailed: 0,
      batches: 2,
      entities: 10,
      relations: 8,
      elapsedMs: 2000,
      cancelled: false,
      errors: [],
    })
    expect(text).toContain('完成 2 篇')
    expect(text).toContain('跳过 1 篇')
    expect(text).toContain('实体 10')
    expect(text).toContain('2.0s')
    expect(
      outcomeSummary({
        notesTotal: 3,
        notesDone: 1,
        notesSkipped: 0,
        notesFailed: 0,
        batches: 1,
        entities: 1,
        relations: 0,
        elapsedMs: 100,
        cancelled: true,
        errors: [],
      }),
    ).toContain('已中断')
  })
})
