import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useGraphStore } from './graph'

describe('graph store（浏览器预览适配器）', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('refresh 拉取统计与图数据', async () => {
    const s = useGraphStore()
    await s.refresh()
    expect(s.hasGraph).toBe(true)
    expect(s.stats?.entities).toBe(s.snapshot.nodes.length)
    expect(s.visible.edges.length).toBe(s.snapshot.edges.length)
    expect(s.kindOptions.length).toBeGreaterThan(1)
    expect(s.filtering).toBe(false)
    expect(s.error).toBe('')
  })

  it('筛选只在内存生效，清空后恢复', async () => {
    const s = useGraphStore()
    await s.refresh()
    const total = s.snapshot.nodes.length
    expect(total).toBeGreaterThan(2)

    s.filter.kinds = ['person']
    expect(s.visible.nodes.length).toBeLessThan(total)
    expect(s.filtering).toBe(true)
    // 边只保留两端都在的
    const keep = new Set(s.visible.nodes.map((n) => n.id))
    expect(s.visible.edges.every((e) => keep.has(e.src) && keep.has(e.dst))).toBe(true)

    s.filter.kinds = []
    s.filter.keyword = '不存在的实体'
    expect(s.visible.nodes.length).toBe(0)
    expect(s.visible.edges.length).toBe(0)

    s.resetFilter()
    expect(s.visible.nodes.length).toBe(total)
    expect(s.filtering).toBe(false)
  })

  it('选中实体后能拿到出处与邻居，点邻居可继续跳转', async () => {
    const s = useGraphStore()
    await s.refresh()
    const start = s.snapshot.nodes.find((n) => n.kind === 'person')!
    await s.select(start.id)
    expect(s.detail?.node.id).toBe(start.id)
    expect(s.detail?.mentions.length).toBeGreaterThan(0)
    expect(s.detail?.neighbors.length).toBeGreaterThan(0)

    const next = s.detail!.neighbors[0].entityId
    await s.select(next)
    expect(s.selectedId).toBe(next)
    expect(s.detail?.node.id).toBe(next)

    s.clearSelection()
    expect(s.selectedId).toBe(0)
    expect(s.detail).toBeNull()
  })

  it('预览模式抽取给出明确错误而不是崩溃', async () => {
    const s = useGraphStore()
    await s.extractAll(false)
    expect(s.extracting).toBe(false)
    expect(s.progress).toBeNull()
    expect(s.error).toContain('预览模式')
    expect(s.message).toBe('')

    await s.extractNote('工作/周会示例.md')
    expect(s.extractingNote).toBe('')
    expect(s.error).toContain('预览模式')
  })

  it('进度百分比按已处理笔记数计算', () => {
    const s = useGraphStore()
    s.progress = {
      notesTotal: 4,
      notesDone: 1,
      notesSkipped: 1,
      notesFailed: 0,
      currentNote: 'a.md',
      currentTitle: 'A',
      batchesTotal: 2,
      batchesDone: 0,
      entities: 3,
      relations: 2,
      message: '处理中',
    }
    expect(s.progressPercent).toBe(50)
    s.progress = null
    expect(s.progressPercent).toBe(0)
  })

  it('进度订阅幂等，可解绑', async () => {
    const s = useGraphStore()
    await s.bindProgress()
    await s.bindProgress()
    s.unbindProgress()
    // 解绑后再订阅一次也应正常
    await s.bindProgress()
    s.unbindProgress()
    expect(s.error).toBe('')
  })
})
