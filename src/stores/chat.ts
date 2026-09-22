import { defineStore } from 'pinia'
import { chatHistoryAdapter } from '../platform/chatHistory'
import { retrievalAdapter } from '../platform/retrieval'
import {
  HISTORY_VERSION,
  capConversations,
  conversationTitle,
  groupConversations,
  newConversationId,
  normalizeHistory,
  trimForStore,
  type Conversation,
  type HistoryGroup,
  type StoredEntry,
} from '../core/chatHistory'
import type { QaStreamEvent, RetrievalStatus } from '../core/retrieval'

export interface ChatEntry extends StoredEntry {
  pending: boolean
  /** 正在逐字生成（可停止） */
  streaming?: boolean
  /** 思考过程（部分模型返回 reasoning_content） */
  reasoning?: string
  /** 追问建议（按需生成，不落盘） */
  suggestions?: string[]
  /** 追问建议请求中 */
  suggesting?: boolean
}

let seq = 1
/** 历史落盘防抖（模块级：不进响应式）。 */
let persistTimer = 0

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

    // ---- 会话历史 ----
    conversations: [] as Conversation[],
    currentConversationId: '',
    historyLoaded: false,
    historyError: '',
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
    currentConversation(state): Conversation | undefined {
      return state.conversations.find((c) => c.id === state.currentConversationId)
    },
    /** 左栏分组（置顶 / 今天 / 昨天 / 7 天内 / 更早）。 */
    historyGroups(state): HistoryGroup[] {
      return groupConversations(state.conversations)
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

    // ------------------------------------------------------------------ 历史

    /** 启动时读一次历史，并打开最近用过的会话。 */
    async loadHistory() {
      if (this.historyLoaded) return
      try {
        const payload = await chatHistoryAdapter().load()
        this.conversations = capConversations(normalizeHistory(payload))
        this.historyLoaded = true
        this.historyError = ''
        const recent = [...this.conversations].sort(
          (a, b) => (b.updatedAt || b.createdAt) - (a.updatedAt || a.createdAt),
        )[0]
        if (recent) {
          this.currentConversationId = recent.id
          this.entries = recent.entries.map((e) => ({ ...e, pending: false, streaming: false }))
        }
      } catch (e) {
        this.historyError = String(e)
      }
    },

    /** 拿到当前会话；没有就新建一个（提问前调用）。 */
    ensureConversation(): Conversation {
      const existing = this.currentConversation
      if (existing) return existing
      const now = Date.now()
      const conv: Conversation = {
        id: newConversationId(),
        title: '新会话',
        createdAt: now,
        updatedAt: now,
        pinned: false,
        entries: [],
      }
      this.conversations.unshift(conv)
      this.currentConversationId = conv.id
      return conv
    },

    /** 新会话：先把当前问答区同步回列表。 */
    newConversation() {
      this.flushCurrent()
      const now = Date.now()
      const conv: Conversation = {
        id: newConversationId(),
        title: '新会话',
        createdAt: now,
        updatedAt: now,
        pinned: false,
        entries: [],
      }
      this.conversations.unshift(conv)
      this.currentConversationId = conv.id
      this.entries = []
      this.persistSoon()
    },

    selectConversation(id: string) {
      if (id === this.currentConversationId) return
      this.flushCurrent()
      const conv = this.conversations.find((c) => c.id === id)
      if (!conv) return
      this.currentConversationId = id
      this.entries = conv.entries.map((e) => ({ ...e, pending: false, streaming: false }))
    },

    /** 把问答区写回当前会话（不落盘；标题在未改名时跟随首个问题）。 */
    flushCurrent() {
      const conv = this.currentConversation
      if (!conv) return
      conv.entries = this.entries.map((e) =>
        trimForStore({
          id: e.id,
          question: e.question,
          answer: e.answer,
          stopped: e.stopped,
          error: e.error,
        }),
      )
      conv.updatedAt = Date.now()
      if ((conv.title === '新会话' || !conv.title) && this.entries.length) {
        conv.title = conversationTitle(this.entries[0].question)
      }
    },

    renameConversation(id: string, title: string) {
      const conv = this.conversations.find((c) => c.id === id)
      if (!conv) return
      const next = title.trim()
      if (!next) return
      conv.title = next
      conv.updatedAt = Date.now()
      this.persistSoon()
    },

    togglePinConversation(id: string) {
      const conv = this.conversations.find((c) => c.id === id)
      if (!conv) return
      conv.pinned = !conv.pinned
      this.persistSoon()
    },

    removeConversation(id: string) {
      const conv = this.conversations.find((c) => c.id === id)
      if (!conv) return
      this.conversations = this.conversations.filter((c) => c.id !== id)
      if (this.currentConversationId === id) {
        this.currentConversationId = ''
        this.entries = []
      }
      this.persistSoon()
    },

    /** 清空当前问答区（空会话会在落盘时被丢掉）。 */
    clear() {
      this.entries = []
      this.flushCurrent()
      this.dropEmptyConversations()
      this.persistSoon()
    },

    /** 丢弃没有任何内容的会话（置顶的保留）。 */
    dropEmptyConversations() {
      this.conversations = this.conversations.filter((c) => c.pinned || c.entries.length > 0)
      if (
        this.currentConversationId &&
        !this.conversations.some((c) => c.id === this.currentConversationId)
      ) {
        this.currentConversationId = ''
      }
    },

    persistSoon() {
      if (persistTimer) window.clearTimeout(persistTimer)
      persistTimer = window.setTimeout(() => {
        persistTimer = 0
        void this.persistNow()
      }, 800)
    },

    async persistNow() {
      if (!this.historyLoaded) return
      const payload = {
        version: HISTORY_VERSION,
        conversations: capConversations(this.conversations),
      }
      try {
        await chatHistoryAdapter().save(payload)
        this.historyError = ''
      } catch (e) {
        this.historyError = String(e)
      }
    },

    // ------------------------------------------------------------------ 问答

    async ask(question: string) {
      const q = question.trim()
      if (!q || this.asking) return
      this.ensureConversation()
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
      entry.suggestions = undefined
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
        this.flushCurrent()
        this.persistSoon()
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

    /** 按需拉取追问建议（一次请求，注意上游限流）。 */
    async loadSuggestions(entry: ChatEntry) {
      const answer = entry.answer?.answer?.trim()
      if (!answer || entry.suggesting) return
      entry.suggesting = true
      try {
        entry.suggestions = await retrievalAdapter().suggest(entry.question, answer)
        if (!entry.suggestions.length) entry.suggestions = undefined
      } catch (e) {
        entry.suggestions = undefined
        entry.error = entry.error ? `${entry.error}\n${String(e)}` : String(e)
      } finally {
        entry.suggesting = false
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
  },
})
