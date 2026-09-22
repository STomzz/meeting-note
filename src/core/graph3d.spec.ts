import { describe, expect, it } from 'vitest'
import { build3DData, endpointId, neighborMap, nodeValFor } from './graph3d'
import type { GraphSnapshot } from './graph'

const snapshot: GraphSnapshot = {
  nodes: [
    { id: 1, name: 'Milvus', kind: 'concept', degree: 9, noteCount: 3 },
    { id: 2, name: '向量检索', kind: 'concept', degree: 4, noteCount: 2 },
    { id: 3, name: '北师大', kind: 'org', degree: 1, noteCount: 1 },
  ],
  edges: [
    { src: 1, dst: 2, weight: 3, kinds: '属于' },
    { src: 2, dst: 3, weight: 1, kinds: '位于' },
    // 悬空边：节点 99 不在节点集里（被上限截断时会出现）
    { src: 2, dst: 99, weight: 1, kinds: '相关' },
  ],
  truncated: 1,
}

describe('nodeValFor', () => {
  it('随度数单调增并封顶', () => {
    expect(nodeValFor(0)).toBe(1)
    expect(nodeValFor(1)).toBeGreaterThan(1)
    expect(nodeValFor(4)).toBeGreaterThan(nodeValFor(1))
    expect(nodeValFor(10000)).toBe(24)
  })

  it('异常输入按 0 处理', () => {
    expect(nodeValFor(Number.NaN)).toBe(1)
    expect(nodeValFor(-5)).toBe(1)
  })
})

describe('build3DData', () => {
  it('节点 id 转字符串、带上 val', () => {
    const { nodes } = build3DData(snapshot)
    expect(nodes.map((n) => n.id)).toEqual(['1', '2', '3'])
    expect(nodes[0].val).toBe(nodeValFor(9))
    expect(nodes[0].name).toBe('Milvus')
  })

  it('丢掉悬空边', () => {
    const { links } = build3DData(snapshot)
    expect(links).toHaveLength(2)
    expect(links.map((l) => `${l.source}-${l.target}`)).toEqual(['1-2', '2-3'])
  })
})

describe('neighborMap', () => {
  it('双向且忽略自环', () => {
    const map = neighborMap([
      { source: '1', target: '2' },
      { source: '2', target: '2' },
    ])
    expect([...map.get('1')!]).toEqual(['2'])
    expect([...map.get('2')!]).toEqual(['1'])
  })
})

describe('endpointId', () => {
  it('兼容字符串 id 与节点对象', () => {
    expect(endpointId('7')).toBe('7')
    expect(endpointId(7)).toBe('7')
    expect(endpointId({ id: 7 })).toBe('7')
    expect(endpointId(null)).toBe('')
  })
})
