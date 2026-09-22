import { defineStore } from 'pinia'
import { notesAdapter } from '../platform'
import type { FolderInfo, NoteMeta, SearchHit } from '../core/types'

const a = () => notesAdapter()

export type SaveState = 'idle' | 'dirty' | 'saving' | 'saved'

/** 自动保存防抖计时器放模块级：不进响应式，切笔记/卸载时可取消。 */
let autoSaveTimer = 0

/** 问答引用点击「打开并定位」时投递的目标行（编辑视图消费后高亮）。 */
export interface LocateTarget {
  noteId: string
  startLine: number
  endLine: number
}

export const useNotesStore = defineStore('notes', {
  state: () => ({
    vault: '',
    notes: [] as NoteMeta[],
    /** 文件夹树数据源（含空文件夹；由 Rust 扫目录返回） */
    folders: [] as FolderInfo[],
    currentId: '',
    content: '',
    dirty: false,
    /** 保存状态：编辑区角标显示「未保存 / 保存中 / 已保存」 */
    saveState: 'idle' as SaveState,
    lastSavedAt: 0,
    loading: false,
    scanning: false,
    query: '',
    hits: [] as SearchHit[],
    scanMessage: '',
    /** 待定位的引用位置（打开笔记后编辑视图滚动并高亮） */
    pendingLocate: null as LocateTarget | null,
  }),

  getters: {
    current(state): NoteMeta | undefined {
      return state.notes.find((n) => n.id === state.currentId)
    },
    /** 文件夹下拉选项：根目录 + 所有文件夹（含空文件夹）。 */
    folderOptions(state): Array<{ label: string; value: string }> {
      return [
        { label: '根目录', value: '' },
        ...state.folders.map((f) => ({ label: f.path, value: f.path })),
      ]
    },
  },

  actions: {
    async init() {
      this.vault = (await a().getVault()) ?? ''
      if (!this.vault) this.vault = '（未设置）'
      try {
        await this.refresh()
        await this.refreshFolders()
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

    async refreshFolders() {
      try {
        this.folders = await a().folders()
      } catch {
        this.folders = []
      }
    },

    async scan() {
      this.scanning = true
      this.scanMessage = ''
      try {
        const s = await a().scan()
        await this.refresh()
        await this.refreshFolders()
        this.scanMessage = `扫描完成：新增/更新 ${s.indexed}，跳过 ${s.skipped}，移除 ${s.removed}`
      } catch (e) {
        this.scanMessage = `扫描失败：${String(e)}`
      } finally {
        this.scanning = false
      }
    },

    async openNote(id: string) {
      cancelAutoSave()
      this.currentId = id
      this.dirty = false
      this.saveState = 'idle'
      this.content = await a().read(id)
    },

    setContent(value: string) {
      this.content = value
      this.dirty = true
      this.saveState = 'dirty'
      this.scheduleAutoSave()
    },

    /** 编辑后 0.8s 静默保存；手动保存/切笔记会取消本次排队。 */
    scheduleAutoSave(delay = 800) {
      cancelAutoSave()
      if (!this.currentId) return
      autoSaveTimer = window.setTimeout(() => {
        autoSaveTimer = 0
        void this.autoSave()
      }, delay)
    },

    async autoSave() {
      if (!this.dirty || !this.currentId) return
      if (this.saveState === 'saving') return
      this.saveState = 'saving'
      try {
        await this.save()
      } catch (e) {
        this.saveState = 'dirty'
        this.scanMessage = `自动保存失败：${String(e)}`
      }
    },

    async save() {
      if (!this.currentId) return
      cancelAutoSave()
      const meta = await a().write(this.currentId, this.content)
      this.dirty = false
      this.saveState = 'saved'
      this.lastSavedAt = Date.now()
      const idx = this.notes.findIndex((n) => n.id === meta.id)
      if (idx >= 0) this.notes[idx] = meta
      else await this.refresh()
    },

    /** 问答/图谱里点引用：打开目标笔记并高亮对应行。 */
    locateNote(noteId: string, startLine: number, endLine = startLine) {
      this.pendingLocate = { noteId, startLine, endLine }
    },

    clearLocate() {
      this.pendingLocate = null
    },

    async createNote(folder: string, title: string) {
      const meta = await a().create(folder, title)
      await this.refresh()
      await this.refreshFolders()
      await this.openNote(meta.id)
    },

    async createFolder(path: string): Promise<string> {
      const created = await a().createFolder(path)
      await this.refreshFolders()
      return created
    },

    async renameFolder(path: string, newName: string): Promise<string> {
      const next = await a().renameFolder(path, newName)
      await this.refresh()
      await this.refreshFolders()
      if (this.currentId === path || this.currentId.startsWith(`${path}/`)) {
        await this.openNote(`${next}${this.currentId.slice(path.length)}`)
      }
      return next
    },

    async moveNote(id: string, folder: string): Promise<string> {
      const next = await a().moveNote(id, folder)
      await this.refresh()
      if (this.currentId === id) await this.openNote(next)
      return next
    },

    async renameNote(id: string, title: string): Promise<string> {
      const next = await a().renameNote(id, title)
      await this.refresh()
      if (this.currentId === id) await this.openNote(next)
      return next
    },

    async removeNote(id: string) {
      cancelAutoSave()
      await a().remove(id)
      if (this.currentId === id) {
        this.currentId = ''
        this.content = ''
        this.dirty = false
        this.saveState = 'idle'
      }
      await this.refresh()
      await this.refreshFolders()
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

function cancelAutoSave() {
  if (autoSaveTimer) {
    window.clearTimeout(autoSaveTimer)
    autoSaveTimer = 0
  }
}
