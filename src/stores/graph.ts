import { defineStore } from 'pinia'
import { graphAdapter } from '../platform/graph'
import {
  EMPTY_FILTER,
  filterSnapshot,
  kindsOf,
  outcomeSummary,
  type ExtractOutcome,
  type GraphFilter,
  type GraphProgress,
  type GraphSnapshot,
  type GraphStats,
  type NodeDetail,
} from '../core/graph'

/** 进度事件订阅的取消函数（模块级：只订阅一次，不进响应式）。 */
let unsubscribeProgress: (() => void) | null = null

export const useGraphStore = defineStore('graph', {
  state: () => ({
    stats: null as GraphStats | null,
    snapshot: { nodes: [], edges: [], truncated: 0 } as GraphSnapshot,
    detail: null as NodeDetail | null,
    selectedId: 0,
    loading: false,
    detailLoading: false,
    /** 正在抽取（全部） */
    extracting: false,
    /** 正在抽取的笔记 id（单篇抽取时非空） */
    extractingNote: '',
    message: '',
    error: '',
    progress: null as GraphProgress | null,
    filter: { ...EMPTY_FILTER } as GraphFilter,
    lastOutcome: null as ExtractOutcome | null,
  }),

  getters: {
    /** 应用筛选后的图（画布与统计都用它）。 */
    visible(state): GraphSnapshot {
      return filterSnapshot(state.snapshot, state.filter)
    },
    /** 图里出现过的实体类型（筛选项）。 */
    kindOptions(state): string[] {
      return kindsOf(state.snapshot)
    },
    hasGraph(state): boolean {
      return state.snapshot.nodes.length > 0
    },
    filtering(state): boolean {
      return (
        state.filter.kinds.length > 0 || !!state.filter.keyword.trim() || state.filter.minDegree > 0
      )
    },
    selected(state) {
      return state.snapshot.nodes.find((n) => n.id === state.selectedId) ?? null
    },
    /** 抽取进度（0~100）：按「已处理的笔记数」估算。 */
    progressPercent(state): number {
      const p = state.progress
      if (!p || !p.notesTotal) return 0
      const done = p.notesDone + p.notesSkipped + p.notesFailed
      return Math.min(100, Math.round((done / p.notesTotal) * 100))
    },
  },

  actions: {
    // -------------------------------------------------------------- 读取
    /** 拉取统计与图数据（抽取完成后也会调用）。 */
    async refresh() {
      this.loading = true
      try {
        const a = graphAdapter()
        const [stats, snapshot] = await Promise.all([a.stats(), a.snapshot()])
        this.stats = stats
        this.snapshot = snapshot
        // 选中的节点被筛掉了（例如重建后消失）→ 清空详情
        if (this.selectedId && !snapshot.nodes.some((n) => n.id === this.selectedId)) {
          this.selectedId = 0
          this.detail = null
        }
        this.error = ''
      } catch (e) {
        this.error = String(e)
      } finally {
        this.loading = false
      }
    },

    /** 选中实体 → 拉取出处片段与邻居。 */
    async select(id: number) {
      this.selectedId = id
      this.detailLoading = true
      try {
        this.detail = await graphAdapter().nodeDetail(id)
        this.error = ''
      } catch (e) {
        this.error = String(e)
        this.detail = null
      } finally {
        this.detailLoading = false
      }
    },

    clearSelection() {
      this.selectedId = 0
      this.detail = null
    },

    resetFilter() {
      this.filter = { ...EMPTY_FILTER }
    },

    // -------------------------------------------------------------- 抽取
    /** 增量抽取全部笔记（`force = true` 全量重抽）。 */
    async extractAll(force = false) {
      this.extracting = true
      this.message = force ? '全量重抽中…' : '增量抽取中…'
      this.progress = null
      try {
        const out = await graphAdapter().extractAll(force)
        this.lastOutcome = out
        this.message = outcomeSummary(out)
        this.error = out.errors.length ? out.errors.slice(0, 3).join('；') : ''
        await this.refresh()
      } catch (e) {
        this.error = String(e)
        this.message = ''
      } finally {
        this.extracting = false
        this.progress = null
      }
    },

    /** 抽取单篇笔记（未改动会直接跳过）。 */
    async extractNote(noteId: string) {
      this.extractingNote = noteId
      this.message = '抽取这一篇…'
      try {
        const out = await graphAdapter().extractNote(noteId)
        this.lastOutcome = out
        this.message = out.notesSkipped ? '这篇笔记没有改动，已跳过' : outcomeSummary(out)
        this.error = out.errors.length ? out.errors.slice(0, 3).join('；') : ''
        await this.refresh()
        if (this.selectedId) await this.select(this.selectedId)
      } catch (e) {
        this.error = String(e)
        this.message = ''
      } finally {
        this.extractingNote = ''
      }
    },

    async cancel() {
      try {
        await graphAdapter().cancel()
        this.message = '已请求中断，当前批次结束后停止…'
      } catch (e) {
        this.error = String(e)
      }
    },

    // -------------------------------------------------------------- 进度
    /** 订阅后端进度事件（幂等：重复调用只订阅一次）。 */
    async bindProgress() {
      if (unsubscribeProgress) return
      try {
        unsubscribeProgress = await graphAdapter().onProgress((p) => {
          this.progress = p
          if (p.message) this.message = p.message
        })
      } catch {
        unsubscribeProgress = null
      }
    },

    unbindProgress() {
      unsubscribeProgress?.()
      unsubscribeProgress = null
    },
  },
})
