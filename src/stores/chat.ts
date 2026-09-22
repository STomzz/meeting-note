import { defineStore } from 'pinia'
import { retrievalAdapter } from '../platform/retrieval'
import type { Answer, QaStreamEvent, RetrievalStatus } from '../core/retrieval'

export interface ChatEntry {
  id: number
  question: string
  pending: boolean
  /** 正在逐字生成（可停止） */
  streaming?: boolean
  /** 用户手动停止过 */
  stopped?: boolean
  /** 思考过程（部分模型返回 reasoning_content） */
  reasoning?: string
  answer?: Answer
  error?: string
}

let seq = 1

export const useChatStore = defineStore('chat', {
  state: () => ({
    entries: [] as ChatEntry[],
    status: null as RetrievalStatus | null,
    statusError: '',
    asking: false,
    /** 有任意一条正在流式生成 */
    streaming: false,
    building: false,
    buildMessage: '',
  }),

  getters: {
    canBuildIndex(state): boolean {
      return Boolean(state.status?.hasEmbedding) && (state.status?.pending ?? 0) > 0 && !state.building
    },
    modeText(state): string {
      const s = state.status
      if (!s) return ''
      if (s.vectorReady && s.hasRerank) return '混合检索 + 重排'
      if (s.vectorReady) return '混合检索'
      return '全文检索'
    },
  },

  actions: {
    async loadStatus() {
      try {
        this.status = await retrievalAdapter().status()
        this.statusError = ''
      } catch (e) {
        this.statusError = String(e)
      }
    },

    async ask(question: string) {
      const q = question.trim()
      if (!q || this.asking) return
      const entry: ChatEntry = { id: seq++, question: q, pending: true }
      this.entries.push(entry)
      await this.runStream(entry)
    },

    /** 重新生成：清掉该条答案原地重跑（历史与顺序不变）。 */
    async askAgain(entry: ChatEntry) {
      if (this.asking) return
      await this.runStream(entry)
    },

    /** 流式跑一次问答：检索 → 逐字 → 收尾；期间可 `stop()`。 */
    async runStream(entry: ChatEntry) {
      this.asking = true
      this.streaming = true
      entry.pending = true
      entry.streaming = false
      entry.stopped = false
      entry.error = ''
      entry.reasoning = ''
      entry.answer = undefined
      try {
        entry.answer = await retrievalAdapter().askStream(entry.question, (ev) =>
          this.applyEvent(entry, ev),
        )
      } catch (e) {
        entry.error = String(e)
      } finally {
        entry.pending = false
        entry.streaming = false
        this.streaming = false
        this.asking = false
        await this.loadStatus()
      }
    },

    /** 把流式事件折进条目：回答边生成边渲染。 */
    applyEvent(entry: ChatEntry, ev: QaStreamEvent) {
      switch (ev.type) {
        case 'retrieved':
          entry.pending = false
          entry.streaming = true
          entry.answer = {
            question: ev.question,
            answer: '',
            sources: ev.sources,
            trace: ev.trace,
            model: ev.model,
            elapsedMs: 0,
            completionTokens: null,
          }
          return
        case 'delta':
          entry.pending = false
          entry.streaming = true
          if (entry.answer) entry.answer.answer += ev.text
          return
        case 'reasoning':
          entry.reasoning = `${entry.reasoning ?? ''}${ev.text}`
          return
        default:
          entry.answer = ev.answer
          entry.pending = false
          entry.streaming = false
      }
    },

    /** 停止生成：保留已生成的部分，标记 stopped。 */
    async stop() {
      if (!this.streaming) return
      const last = [...this.entries].reverse().find((e) => e.streaming)
      if (last) last.stopped = true
      try {
        await retrievalAdapter().cancel()
      } catch {
        // 取消失败不影响前端收尾（响应结束后会自然停止）
      }
    },

    /** 循环调用增量构建，直到没有待构建的块（或达到轮数上限）。 */
    async buildIndex(maxRounds = 40) {
      if (this.building) return false
      this.building = true
      this.buildMessage = '开始构建向量索引…'
      let total = 0
      try {
        for (let i = 0; i < maxRounds; i++) {
          const p = await retrievalAdapter().buildIndex(256)
          total += p.embedded
          if (p.remaining <= 0) {
            this.buildMessage = `完成：写入 ${total} 块，维度 ${p.dim}，耗时 ${p.elapsedMs} ms`
            break
          }
          this.buildMessage = `已写入 ${total} 块，剩余 ${p.remaining}…`
          if (p.embedded === 0) {
            this.buildMessage = `已写入 ${total} 块，剩余 ${p.remaining}（无进展，已停止）`
            break
          }
        }
        await this.loadStatus()
        return true
      } catch (e) {
        this.buildMessage = `构建失败：${String(e)}`
        return false
      } finally {
        this.building = false
      }
    },

    clear() {
      this.entries = []
    },
  },
})
