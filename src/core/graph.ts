/** 知识图谱的类型与纯函数（对应 Rust 侧 camelCase 契约）。 */

export interface GraphNode {
  id: number
  name: string
  /** person / org / place / concept / event / other */
  kind: string
  /** 关系条数（去重前的出现次数） */
  degree: number
  /** 这个实体出现在几篇笔记里 */
  noteCount: number
}

export interface GraphEdge {
  src: number
  dst: number
  /** 强度：出现次数 */
  weight: number
  /** 关系短语，多个用 `/` 分隔 */
  kinds: string
}

export interface GraphSnapshot {
  nodes: GraphNode[]
  edges: GraphEdge[]
  /** 因上限被截断的节点数（>0 表示图不完整） */
  truncated: number
}

export interface GraphStats {
  notesTotal: number
  notesExtracted: number
  notesFailed: number
  entities: number
  relations: number
  chunkLinks: number
  lastExtractedAt: number
  model: string
}

export interface EntityMention {
  noteId: string
  noteTitle: string
  chunkId: number
  startLine: number
  endLine: number
  snippet: string
}

export interface EntityNeighbor {
  entityId: number
  name: string
  kind: string
  relKind: string
  weight: number
  /** out = 本实体指向邻居；in = 邻居指向本实体 */
  direction: string
}

export interface NodeDetail {
  node: GraphNode
  mentions: EntityMention[]
  neighbors: EntityNeighbor[]
}

export interface GraphProgress {
  notesTotal: number
  notesDone: number
  notesSkipped: number
  notesFailed: number
  currentNote: string
  currentTitle: string
  batchesTotal: number
  batchesDone: number
  entities: number
  relations: number
  message: string
}

export interface ExtractOutcome {
  notesTotal: number
  notesDone: number
  notesSkipped: number
  notesFailed: number
  batches: number
  entities: number
  relations: number
  elapsedMs: number
  cancelled: boolean
  errors: string[]
}

export const KIND_LABEL: Record<string, string> = {
  person: '人物',
  org: '组织/产品',
  place: '地点',
  concept: '概念',
  event: '事件',
  other: '其它',
}

export const KIND_COLOR: Record<string, string> = {
  person: '#0ea5e9',
  org: '#8b5cf6',
  place: '#10b981',
  concept: '#f59e0b',
  event: '#ef4444',
  other: '#94a3b8',
}

export const ALL_KINDS = ['person', 'org', 'place', 'concept', 'event', 'other']

export function kindLabel(kind: string): string {
  return KIND_LABEL[kind] ?? kind
}

export function kindColor(kind: string): string {
  return KIND_COLOR[kind] ?? KIND_COLOR.other
}

/** 节点半径：度数越大越大（8~28）。 */
export function nodeSize(degree: number): number {
  return Math.min(28, 8 + Math.sqrt(Math.max(0, degree)) * 4)
}

/** 边线宽：强度越大越粗（1~5）。 */
export function edgeWidth(weight: number): number {
  return Math.min(5, 1 + Math.log2(Math.max(1, weight)))
}

/** 关系短语展示：最多 3 个，其余用 +N。 */
export function kindsLabel(kinds: string, max = 3): string {
  const list = kinds
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  if (!list.length) return ''
  if (list.length <= max) return list.join(' / ')
  return `${list.slice(0, max).join(' / ')} +${list.length - max}`
}

export interface GraphFilter {
  /** 空数组 = 不限类型 */
  kinds: string[]
  keyword: string
  minDegree: number
}

export const EMPTY_FILTER: GraphFilter = { kinds: [], keyword: '', minDegree: 0 }

/**
 * 前端筛选：只保留命中的节点，以及两端都保留的边。
 * 不改后端数据，随时可以清空条件恢复。
 */
export function filterSnapshot(snapshot: GraphSnapshot, filter: GraphFilter): GraphSnapshot {
  const kw = filter.keyword.trim().toLowerCase()
  const kinds = new Set(filter.kinds)
  const nodes = snapshot.nodes.filter((n) => {
    if (kinds.size && !kinds.has(n.kind)) return false
    if (n.degree < filter.minDegree) return false
    if (kw && !n.name.toLowerCase().includes(kw)) return false
    return true
  })
  const keep = new Set(nodes.map((n) => n.id))
  const edges = snapshot.edges.filter((e) => keep.has(e.src) && keep.has(e.dst))
  return { nodes, edges, truncated: snapshot.truncated }
}

/** 快照里出现过的实体类型（用于生成筛选项）。 */
export function kindsOf(snapshot: GraphSnapshot): string[] {
  const set = new Set<string>()
  snapshot.nodes.forEach((n) => set.add(n.kind))
  return ALL_KINDS.filter((k) => set.has(k))
}

/** 抽取结果的中文摘要（界面提示用）。 */
export function outcomeSummary(out: ExtractOutcome): string {
  const parts = [`完成 ${out.notesDone} 篇`]
  if (out.notesSkipped) parts.push(`跳过 ${out.notesSkipped} 篇`)
  if (out.notesFailed) parts.push(`失败 ${out.notesFailed} 篇`)
  parts.push(`实体 ${out.entities}`, `关系 ${out.relations}`)
  parts.push(`${(out.elapsedMs / 1000).toFixed(1)}s`)
  const text = parts.join(' · ')
  return out.cancelled ? `已中断（${text}）` : text
}
