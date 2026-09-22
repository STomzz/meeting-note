import { defineStore } from 'pinia'
import { retrievalAdapter } from '../platform/retrieval'
import type { Answer, RetrievalStatus } from '../core/retrieval'

export interface ChatEntry {
  id: number
  question: string
  pending: boolean
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
      this.asking = true
      try {
        entry.answer = await retrievalAdapter().ask(q)
      } catch (e) {
        entry.error = String(e)
      } finally {
        entry.pending = false
        this.asking = false
        await this.loadStatus()
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
