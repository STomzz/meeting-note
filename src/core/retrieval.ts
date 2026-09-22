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
  /** fts | hybrid | hybrid+rerank */
  mode: string
  ftsHits: number
  vectorHits: number
  reranked: boolean
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
