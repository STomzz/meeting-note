import { defineStore } from 'pinia'
import { notesAdapter } from '../platform'
import type { NoteMeta, SearchHit } from '../core/types'

const a = () => notesAdapter()

export const useNotesStore = defineStore('notes', {
  state: () => ({
    vault: '',
    notes: [] as NoteMeta[],
    currentId: '',
    content: '',
    dirty: false,
    loading: false,
    scanning: false,
    folderFilter: '',
    tagFilter: '',
    query: '',
    hits: [] as SearchHit[],
    scanMessage: '',
  }),

  getters: {
    folders(state): Array<{ path: string; count: number }> {
      const map = new Map<string, number>()
      for (const n of state.notes) {
        const f = n.folder || ''
        map.set(f, (map.get(f) ?? 0) + 1)
      }
      return [...map.entries()]
        .map(([path, count]) => ({ path, count }))
        .sort((x, y) => x.path.localeCompare(y.path))
    },
    tags(state): string[] {
      const set = new Set<string>()
      for (const n of state.notes) {
        for (const t of n.tags.split(',')) if (t.trim()) set.add(t.trim())
      }
      return [...set].sort()
    },
    visibleNotes(state): NoteMeta[] {
      return state.notes.filter((n) => {
        if (state.folderFilter && n.folder !== state.folderFilter) return false
        if (state.tagFilter && !n.tags.split(',').map((t) => t.trim()).includes(state.tagFilter)) return false
        return true
      })
    },
    current(state): NoteMeta | undefined {
      return state.notes.find((n) => n.id === state.currentId)
    },
  },

  actions: {
    async init() {
      this.vault = (await a().getVault()) ?? ''
      if (!this.vault) this.vault = '（未设置）'
      try {
        await this.refresh()
      } catch (e) {
        this.scanMessage = `读取失败：${String(e)}`
      }
    },

    async refresh() {
      this.loading = true
      try {
        this.notes = await a().list()
      } finally {
        this.loading = false
      }
    },

    async scan() {
      this.scanning = true
      this.scanMessage = ''
      try {
        const s = await a().scan()
        await this.refresh()
        this.scanMessage = `扫描完成：新增/更新 ${s.indexed}，跳过 ${s.skipped}，移除 ${s.removed}`
      } catch (e) {
        this.scanMessage = `扫描失败：${String(e)}`
      } finally {
        this.scanning = false
      }
    },

    async openNote(id: string) {
      this.currentId = id
      this.dirty = false
      this.content = await a().read(id)
    },

    setContent(value: string) {
      this.content = value
      this.dirty = true
    },

    async save() {
      if (!this.currentId) return
      const meta = await a().write(this.currentId, this.content)
      this.dirty = false
      const idx = this.notes.findIndex((n) => n.id === meta.id)
      if (idx >= 0) this.notes[idx] = meta
      else await this.refresh()
    },

    async createNote(folder: string, title: string) {
      const meta = await a().create(folder, title)
      await this.refresh()
      await this.openNote(meta.id)
    },

    async removeNote(id: string) {
      await a().remove(id)
      if (this.currentId === id) {
        this.currentId = ''
        this.content = ''
      }
      await this.refresh()
    },

    async search(q: string) {
      this.query = q
      if (!q.trim()) {
        this.hits = []
        return
      }
      this.hits = await a().search(q)
    },
  },
})
