import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ExtractOutcome,
  GraphEdge,
  GraphNode,
  GraphProgress,
  GraphSnapshot,
  GraphStats,
  NodeDetail,
} from '../core/graph'

/** 图谱适配器：抽取、图数据查询、节点详情。 */
export interface GraphAdapter {
  stats(): Promise<GraphStats>
  snapshot(opts?: { limit?: number; minDegree?: number }): Promise<GraphSnapshot>
  nodeDetail(entityId: number): Promise<NodeDetail>
  /** 增量抽取全部笔记；`force = true` 忽略哈希全量重抽。 */
  extractAll(force?: boolean): Promise<ExtractOutcome>
  /** 抽取单篇笔记。 */
  extractNote(noteId: string, force?: boolean): Promise<ExtractOutcome>
  cancel(): Promise<void>
  /** 订阅抽取进度，返回取消订阅函数。 */
  onProgress(cb: (p: GraphProgress) => void): Promise<() => void>
}

class TauriGraphAdapter implements GraphAdapter {
  stats() {
    return invoke<GraphStats>('graph_stats')
  }
  snapshot(opts?: { limit?: number; minDegree?: number }) {
    return invoke<GraphSnapshot>('graph_snapshot', {
      limit: opts?.limit ?? 1500,
      minDegree: opts?.minDegree ?? 0,
    })
  }
  nodeDetail(entityId: number) {
    return invoke<NodeDetail>('graph_node_detail', { entityId })
  }
  extractAll(force = false) {
    return invoke<ExtractOutcome>('graph_extract_all', { force })
  }
  extractNote(noteId: string, force = false) {
    return invoke<ExtractOutcome>('graph_extract_note', { noteId, force })
  }
  cancel() {
    return invoke<void>('graph_cancel_extract')
  }
  async onProgress(cb: (p: GraphProgress) => void) {
    return await listen<GraphProgress>('graph-progress', (e) => cb(e.payload))
  }
}

/** 预览模式的演示图：形状与真实数据一致，内容固定，不调用模型。 */
const DEMO_NODES: GraphNode[] = [
  { id: 1, name: '镜像站', kind: 'concept', degree: 4, noteCount: 1 },
  { id: 2, name: '镜像拉取超时', kind: 'concept', degree: 2, noteCount: 1 },
  { id: 3, name: '张伟', kind: 'person', degree: 3, noteCount: 2 },
  { id: 4, name: 'gpu-node3', kind: 'place', degree: 2, noteCount: 1 },
  { id: 5, name: '部署文档', kind: 'concept', degree: 1, noteCount: 1 },
  { id: 6, name: '周会', kind: 'event', degree: 2, noteCount: 1 },
  { id: 7, name: 'Rust 所有权', kind: 'concept', degree: 2, noteCount: 1 },
  { id: 8, name: '借用检查器', kind: 'concept', degree: 1, noteCount: 1 },
  { id: 9, name: 'BNU Notes', kind: 'org', degree: 2, noteCount: 1 },
  { id: 10, name: '本地优先', kind: 'concept', degree: 1, noteCount: 1 },
]

const DEMO_EDGES: GraphEdge[] = [
  { src: 2, dst: 1, weight: 2, kinds: '改用' },
  { src: 3, dst: 2, weight: 1, kinds: '跟进' },
  { src: 3, dst: 4, weight: 1, kinds: '验证于' },
  { src: 4, dst: 1, weight: 1, kinds: '使用' },
  { src: 6, dst: 2, weight: 1, kinds: '讨论' },
  { src: 6, dst: 5, weight: 1, kinds: '安排' },
  { src: 7, dst: 8, weight: 2, kinds: '依赖' },
  { src: 9, dst: 10, weight: 1, kinds: '采用' },
  { src: 9, dst: 7, weight: 1, kinds: '包含' },
]

class MockGraphAdapter implements GraphAdapter {
  async stats(): Promise<GraphStats> {
    return {
      notesTotal: 3,
      notesExtracted: 3,
      notesFailed: 0,
      entities: DEMO_NODES.length,
      relations: DEMO_EDGES.reduce((sum, e) => sum + e.weight, 0),
      chunkLinks: 12,
      lastExtractedAt: Math.floor(Date.now() / 1000),
      model: 'mock',
    }
  }

  async snapshot(): Promise<GraphSnapshot> {
    return { nodes: [...DEMO_NODES], edges: [...DEMO_EDGES], truncated: 0 }
  }

  async nodeDetail(entityId: number): Promise<NodeDetail> {
    const node = DEMO_NODES.find((n) => n.id === entityId)
    if (!node) throw new Error('实体不存在（预览模式）')
    const neighbors = DEMO_EDGES.filter((e) => e.src === entityId || e.dst === entityId).map((e) => {
      const out = e.src === entityId
      const other = DEMO_NODES.find((n) => n.id === (out ? e.dst : e.src))!
      return {
        entityId: other.id,
        name: other.name,
        kind: other.kind,
        relKind: e.kinds,
        weight: e.weight,
        direction: out ? 'out' : 'in',
      }
    })
    return {
      node,
      mentions: [
        {
          noteId: '工作/周会示例.md',
          noteTitle: '周会示例',
          chunkId: 1,
          startLine: 1,
          endLine: 8,
          snippet: '预览模式：这里是演示用的出处片段，真实数据来自你的笔记。',
        },
      ],
      neighbors,
    }
  }

  async extractAll(): Promise<ExtractOutcome> {
    throw new Error('浏览器预览模式不能抽取图谱，请在桌面客户端里操作')
  }

  async extractNote(): Promise<ExtractOutcome> {
    throw new Error('浏览器预览模式不能抽取图谱，请在桌面客户端里操作')
  }

  async cancel(): Promise<void> {
    /* 预览模式没有可取消的任务 */
  }

  async onProgress(): Promise<() => void> {
    return () => {}
  }
}

export function graphAdapter(): GraphAdapter {
  return '__TAURI_INTERNALS__' in window ? new TauriGraphAdapter() : new MockGraphAdapter()
}
