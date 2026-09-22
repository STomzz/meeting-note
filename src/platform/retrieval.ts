import { Channel, invoke } from '@tauri-apps/api/core'
import type {
  Answer,
  EmbedProgress,
  QaStreamEvent,
  RetrievalStatus,
  RetrievedChunk,
} from '../core/retrieval'
import { SEED, splitTitle } from './mock'

/** 检索 + 问答适配器。 */
export interface RetrievalAdapter {
  status(): Promise<RetrievalStatus>
  buildIndex(maxChunks?: number): Promise<EmbedProgress>
  ask(question: string, topK?: number): Promise<Answer>
  /** 流式问答：检索完成后逐字回调；返回完整答案（被停止时返回已生成的部分）。 */
  askStream(question: string, onEvent: (ev: QaStreamEvent) => void, topK?: number): Promise<Answer>
  /** 请求停止当前流式问答（下一次增量检查时生效）。 */
  cancel(): Promise<void>
  /** 追问建议（按需触发，一次请求最多 3 条）。 */
  suggest(question: string, answer: string): Promise<string[]>
}

class TauriRetrievalAdapter implements RetrievalAdapter {
  status() {
    return invoke<RetrievalStatus>('retrieval_status')
  }
  buildIndex(maxChunks?: number) {
    return invoke<EmbedProgress>('build_vector_index', { batch: 16, maxChunks: maxChunks ?? 256 })
  }
  ask(question: string, topK?: number) {
    return invoke<Answer>('ask_question', { question, topK: topK ?? 6 })
  }
  askStream(question: string, onEvent: (ev: QaStreamEvent) => void, topK?: number) {
    const channel = new Channel<QaStreamEvent>()
    channel.onmessage = (ev) => onEvent(ev)
    return invoke<Answer>('ask_question_stream', { question, topK: topK ?? 6, onEvent: channel })
  }
  cancel() {
    return invoke<void>('qa_cancel')
  }
  suggest(question: string, answer: string) {
    return invoke<string[]>('qa_suggest', { question, answer })
  }
}

/**
 * 浏览器预览实现：对内置示例笔记做朴素子串匹配，
 * 明确标注"预览模式"，不伪造模型回答。
 */
class MockRetrievalAdapter implements RetrievalAdapter {
  async status(): Promise<RetrievalStatus> {
    return {
      chunks: SEED.length,
      vectors: 0,
      pending: SEED.length,
      embedModel: '',
      rerankModel: '',
      chatModel: 'mock',
      hasChat: false,
      hasEmbedding: false,
      hasRerank: false,
      vectorReady: false,
      graphEntities: 0,
      graphRelations: 0,
    }
  }

  async buildIndex(): Promise<EmbedProgress> {
    throw new Error('浏览器预览模式不能构建向量索引，请在桌面客户端里操作')
  }

  async ask(question: string): Promise<Answer> {
    const q = question.trim().toLowerCase()
    const sources: RetrievedChunk[] = []
    SEED.forEach(([id, content], i) => {
      content.split('\n').forEach((line, li) => {
        if (q && line.toLowerCase().includes(q)) {
          sources.push({
            chunkId: i + 1,
            noteId: id,
            title: splitTitle(content, id).title,
            startLine: li + 1,
            endLine: li + 1,
            text: line,
            score: 0.5,
            sources: ['mock'],
          })
        }
      })
    })
    const top = sources.slice(0, 5)
    const answer = top.length
      ? `**（浏览器预览模式）** 在示例笔记里找到 ${top.length} 处与「${question}」相关的内容：\n\n${top
          .map((s, i) => `${i + 1}. 《${s.title}》第 ${s.startLine} 行`)
          .join('\n')}\n\n真正的问答需要桌面客户端 + 设置里配置的对话模型。`
      : `**（浏览器预览模式）** 示例笔记里没有与「${question}」相关的内容。\n\n桌面客户端里会用本地全文 + 向量检索召回片段，再交给对话模型作答。`
    return {
      question,
      answer,
      sources: top,
      trace: {
        mode: 'mock',
        ftsHits: top.length,
        vectorHits: 0,
        reranked: false,
        graphHits: 0,
        graphAdded: 0,
        graphEntities: 0,
        degraded: ['浏览器预览模式：未调用真实模型，未使用向量索引'],
        elapsedMs: 0,
      },
      model: 'mock',
      elapsedMs: 0,
      completionTokens: null,
    }
  }

  /** 浏览器预览：把整段回答按小步长"吐"出来，方便验证流式交互（不发真实请求）。 */
  async askStream(question: string, onEvent: (ev: QaStreamEvent) => void): Promise<Answer> {
    this.cancelled = false
    const full = await this.ask(question)
    onEvent({
      type: 'retrieved',
      question: full.question,
      model: full.model,
      sources: full.sources,
      trace: full.trace,
    })
    const text = full.answer
    let cut = 0
    while (cut < text.length) {
      if (this.cancelled) break
      const step = 6 + Math.floor(Math.random() * 8)
      onEvent({ type: 'delta', text: text.slice(cut, cut + step) })
      cut += step
      await new Promise((resolve) => setTimeout(resolve, 18))
    }
    const final: Answer = { ...full, answer: text.slice(0, cut) }
    onEvent({ type: 'done', answer: final })
    return final
  }

  async cancel() {
    this.cancelled = true
  }

  /** 预览模式：给出与问题相关的示例追问（不调用模型）。 */
  async suggest(question: string): Promise<string[]> {
    const short = question.replace(/\s+/g, ' ').slice(0, 12)
    return [
      `关于「${short}」还有哪些细节？`,
      '相关笔记里有哪些待办？',
      '这个结论的依据是什么？',
    ]
  }

  private cancelled = false
}

export function retrievalAdapter(): RetrievalAdapter {
  return '__TAURI_INTERNALS__' in window
    ? new TauriRetrievalAdapter()
    : new MockRetrievalAdapter()
}
