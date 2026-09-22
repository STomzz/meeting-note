/** 检索与问答相关类型（对应 Rust 侧 camelCase 契约）。 */

export interface RetrievedChunk {
  chunkId: number
  noteId: string
  title: string
  startLine: number
  endLine: number
  text: string
  score: number
  /** 命中来源：fts / vector / rerank */
  sources: string[]
}

export interface RetrievalTrace {
  /** fts | hybrid | hybrid+rerank（后缀 +graph 表示图谱邻居扩展生效） */
  mode: string
  ftsHits: number
  vectorHits: number
  reranked: boolean
  /** 图谱扩展召回的候选片段数 */
  graphHits: number
  /** 最终结果里来自图谱扩展的片段数 */
  graphAdded: number
  /** 用到的种子实体数 */
  graphEntities: number
  /** 降级原因（用户可见） */
  degraded: string[]
  elapsedMs: number
}

export interface Answer {
  question: string
  answer: string
  sources: RetrievedChunk[]
  trace: RetrievalTrace
  model: string
  elapsedMs: number
  completionTokens: number | null
}

/** 流式问答事件（对应 Rust 侧 `QaEvent`，serde 的 tag = type）。 */
export type QaStreamEvent =
  | {
      type: 'retrieved'
      question: string
      model: string
      sources: RetrievedChunk[]
      trace: RetrievalTrace
    }
  | { type: 'delta'; text: string }
  | { type: 'reasoning'; text: string }
  | { type: 'done'; answer: Answer }

export interface RetrievalStatus {
  chunks: number
  vectors: number
  pending: number
  embedModel: string
  rerankModel: string
  chatModel: string
  hasChat: boolean
  hasEmbedding: boolean
  hasRerank: boolean
  vectorReady: boolean
  /** 图谱实体/关系数（0 表示还没抽取过） */
  graphEntities: number
  graphRelations: number
}

export interface EmbedProgress {
  embedded: number
  remaining: number
  dim: number
  elapsedMs: number
}

export const MODE_LABEL: Record<string, string> = {
  fts: '全文检索',
  hybrid: '混合检索',
  'hybrid+rerank': '混合检索 + 重排',
  mock: '预览模式',
}

/** 检索模式标签：`+graph` 后缀表示图谱邻居扩展补到了片段。 */
export function modeLabel(mode: string): string {
  const graph = mode.includes('+graph')
  const base = mode.replace('+graph', '')
  const label = MODE_LABEL[base] ?? base
  return graph ? `${label} + 图谱扩展` : label
}
