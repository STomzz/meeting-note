import { defineStore } from 'pinia'
import { meetingNoteAdapter } from '../platform/meetingNote'
import type {
  AudioRef,
  ClipInfo,
  ClipStat,
  MeetingNoteBrief,
  ProcessOutcome,
  ProcessProgress,
} from '../core/meetingNote'
import { appendRefLine, outcomeSummary, progressPercent } from '../core/meetingNote'
import { MicRecorder, pcmToBase64 } from '../core/recorder'
import type { RecorderInfo } from '../core/recorder'
import { useNotesStore } from './notes'

/** 单个录音分段时长上限（默认 4 分钟）。 */
export const SEGMENT_MAX_MS = 4 * 60 * 1000
/** PCM 落盘间隔：越小越不容易丢数据，IPC 越频繁。 */
const FLUSH_MS = 1000

/** 录音器与大缓冲放模块级：大数组不进响应式，避免每帧代理开销。 */
const recorder = new MicRecorder()
let pcmBuffer: Int16Array[] = []
let bufferedSamples = 0
let ticker = 0
/** 采集回调不等待 promise，必须自己串行，避免并发写同一分段 / 重复切段。 */
let pushChain: Promise<void> = Promise.resolve()

export const useMeetingNoteStore = defineStore('meetingNote', {
  state: () => ({
    // 列表
    notes: [] as MeetingNoteBrief[],
    scanAll: false,
    loading: false,
    listError: '',

    // 当前选中
    currentId: '',
    refs: [] as AudioRef[],

    // 一键处理
    processing: false,
    force: false,
    progress: 0,
    message: '',
    outcome: null as ProcessOutcome | null,

    // 录音面板
    recorderOpen: false,
    targetNoteId: '',
    recording: false,
    recordSeq: 0,
    recordElapsedMs: 0,
    recordSegmentMs: 0,
    level: 0,
    recordInfo: null as RecorderInfo | null,
    recentClips: [] as ClipStat[],
    lastClip: null as ClipStat | null,
    insertHint: '',
    /** 录音增删版本号：界面用它触发「本笔记录音」徽章与列表刷新 */
    clipRevision: 0,
    /** 等待编辑器插到光标处的引用（笔记正打开时走这条路，避免覆盖未保存编辑） */
    pendingRef: null as { noteId: string; text: string } | null,

    /** vault 里所有录音片段（`/v` 选择器用） */
    clips: [] as ClipInfo[],
    /** vault 里所有录音片段（列表徽章用，独立于选择器状态） */
    allClips: [] as ClipInfo[],

    error: '',
  }),

  getters: {
    current(state): MeetingNoteBrief | null {
      return state.notes.find((n) => n.noteId === state.currentId) ?? null
    },
    levelPercent(state): number {
      return Math.min(100, Math.round((state.level / 6000) * 100))
    },
    /** 录音面板的目标笔记下拉项：vault 里所有笔记（随笔记树一起更新）。 */
    targetOptions(): { label: string; value: string }[] {
      const notesStore = useNotesStore()
      return notesStore.notes.map((n) => ({
        label: n.folder ? `${n.title}（${n.folder}）` : n.title,
        value: n.id,
      }))
    },
    /** `/v` 选择器按剩余录制时长从新到旧排序。 */
    clipOptions(state): ClipInfo[] {
      return [...state.clips].sort((a, b) => b.modifiedAt - a.modifiedAt)
    },
  },

  actions: {
    // ---------------------------------------------------------------- 列表
    async loadList(scanAll?: boolean) {
      if (scanAll !== undefined) this.scanAll = scanAll
      this.loading = true
      try {
        this.notes = await meetingNoteAdapter().list(this.scanAll)
        this.listError = ''
      } catch (e) {
        this.listError = String(e)
      } finally {
        this.loading = false
      }
    },

    async openNote(noteId: string) {
      // 切到别的笔记才清空处理结果；处理完的刷新（同一篇）要保留进度与结论
      const changed = noteId !== this.currentId
      this.currentId = noteId
      if (changed) {
        this.outcome = null
        this.message = ''
        this.progress = 0
      }
      try {
        this.refs = await meetingNoteAdapter().refs(noteId)
        this.error = ''
      } catch (e) {
        this.error = String(e)
        this.refs = []
      }
    },

    /** 新建会议笔记（`会议/<日期>-<标题>.md`），并选中它。 */
    async create(title: string): Promise<string> {
      const id = await meetingNoteAdapter().create(title, todayLocal())
      await this.loadList()
      await this.openNote(id)
      this.targetNoteId = id
      return id
    },

    /** 把旧会议（meetings 表）导出为会议笔记。 */
    async migrateLegacy(): Promise<string[]> {
      const ids = await meetingNoteAdapter().migrateLegacy()
      await this.loadList()
      return ids
    },

    // ---------------------------------------------------------------- 处理
    async process(force = false): Promise<ProcessOutcome | null> {
      const noteId = this.currentId
      if (!noteId || this.processing) return null
      // 处理的是磁盘上的文件：正打开且未保存时先存盘，别让模型读到旧内容
      const notesStore = useNotesStore()
      if (notesStore.currentId === noteId && notesStore.dirty) {
        try {
          await notesStore.save()
        } catch (e) {
          this.error = `保存笔记失败：${String(e)}`
          return null
        }
      }
      this.processing = true
      this.force = force
      this.progress = 0
      this.message = '准备中…'
      this.outcome = null
      this.error = ''
      let unlisten: (() => void) | null = null
      try {
        unlisten = await meetingNoteAdapter().onProgress((p: ProcessProgress) => {
          if (p.noteId && p.noteId !== noteId) return
          this.progress = progressPercent(p)
          if (p.message) this.message = p.message
        })
        const out = await meetingNoteAdapter().process(noteId, force)
        this.outcome = out
        this.progress = 100
        this.message = outcomeSummary(out)
        if (out.errors.length) this.error = out.errors.join('；')
        await this.openNote(noteId)
        await this.loadList()
        // 笔记内容被改写了：正打开同一篇且没有未保存修改时刷新编辑区
        if (notesStore.currentId === noteId && !notesStore.dirty) await notesStore.openNote(noteId)
        return out
      } catch (e) {
        this.error = String(e)
        this.message = `处理失败：${String(e)}`
        return null
      } finally {
        unlisten?.()
        this.processing = false
      }
    },

    async cancelProcess() {
      try {
        await meetingNoteAdapter().cancel()
        this.message = '已请求取消，等待当前请求结束…'
      } catch (e) {
        this.error = String(e)
      }
    },

    /** vault 内音频的播放地址。 */
    async clipSrc(relPath: string): Promise<string | null> {
      return meetingNoteAdapter().audioSrc(relPath)
    },

    // ---------------------------------------------------------------- 录音
    /** 打开/收起浮动录音面板；打开时顺带刷新片段列表。 */
    async toggleRecorder(open?: boolean) {
      this.recorderOpen = open ?? !this.recorderOpen
      if (this.recorderOpen) {
        const notesStore = useNotesStore()
        if (!notesStore.notes.length) await notesStore.refresh()
        if (!this.targetNoteId) this.targetNoteId = notesStore.currentId || this.currentId || ''
        try {
          this.clips = await meetingNoteAdapter().clipList()
        } catch (e) {
          this.error = String(e)
        }
      }
    },

    setTarget(noteId: string) {
      this.targetNoteId = noteId
    },

    /** 开始录音；没有目标笔记时自动新建一场会议。 */
    async startRecording() {
      if (this.recording) return
      this.error = ''
      const notesStore = useNotesStore()
      let target = this.targetNoteId || notesStore.currentId || this.currentId
      try {
        if (!target) {
          // 没有目标笔记：自动新建一场会议（`会议/<日期>-<时间>.md`）
          target = await this.create(defaultTitle())
        }
        if (!notesStore.notes.some((n) => n.id === target)) await notesStore.refresh()
        this.targetNoteId = target
        const info = await recorder.start({
          onChunk: (pcm) => void this.pushPcm(pcm),
          onLevel: (rms) => {
            this.level = rms
          },
          onError: (message) => {
            this.error = message
          },
        })
        const stat = await meetingNoteAdapter().clipStart(target, info.sampleRate)
        this.recordInfo = info
        this.recordSeq = stat.seq
        this.recordElapsedMs = 0
        this.recordSegmentMs = 0
        this.insertHint = ''
        pcmBuffer = []
        bufferedSamples = 0
        pushChain = Promise.resolve()
        this.recording = true
        this.startTicker()
      } catch (e) {
        this.error = String(e)
      }
    },

    /**
     * 停止录音：只保存分段并提示路径，**不自动写引用**。
     * 引用由用户在笔记里用 `/v` 选择器插入（或「本笔记录音」里一键插入）。
     */
    async stopRecording() {
      if (!this.recording) return
      const target = this.targetNoteId
      const seq = this.recordSeq
      this.stopTicker()
      try {
        await recorder.stop()
        // 先把排队中的 PCM 落盘（此时 recording 仍为 true），再收尾分段
        await pushChain
        await this.flushBuffer(true)
        this.recording = false
        if (target && seq > 0) {
          const stat = await meetingNoteAdapter().clipClose(target, seq)
          this.recentClips = [stat, ...this.recentClips.filter((c) => c.path !== stat.path)]
          this.lastClip = stat
          this.insertHint = `第 ${stat.seq} 段已保存：${stat.path}（用 /v 插入引用）`
          this.clipRevision += 1
          await this.loadClips(target)
          if (this.currentId === target) await this.openNote(target)
        }
      } catch (e) {
        this.error = String(e)
      } finally {
        this.level = 0
      }
    },

    /** 采集回调：串行处理（约 1 秒落盘一次，每 4 分钟切段）。 */
    pushPcm(pcm: Int16Array): Promise<void> {
      pushChain = pushChain
        .then(() => this.handlePcm(pcm))
        .catch((e) => {
          this.error = `处理录音数据失败：${String(e)}`
        })
      return pushChain
    },

    async handlePcm(pcm: Int16Array) {
      if (!this.recording || !pcm.length) return
      const rate = this.recordInfo?.sampleRate || 16000
      pcmBuffer.push(pcm)
      bufferedSamples += pcm.length
      const ms = (pcm.length / rate) * 1000
      this.recordElapsedMs += ms
      this.recordSegmentMs += ms
      if (bufferedSamples >= (rate * FLUSH_MS) / 1000) await this.flushBuffer(false)
      if (this.recordSegmentMs >= SEGMENT_MAX_MS) await this.rotateSegment()
    },

    async flushBuffer(force: boolean) {
      if (!force && !bufferedSamples) return
      if (!pcmBuffer.length) return
      const total = pcmBuffer.reduce((sum, p) => sum + p.length, 0)
      const merged = new Int16Array(total)
      let offset = 0
      for (const part of pcmBuffer) {
        merged.set(part, offset)
        offset += part.length
      }
      pcmBuffer = []
      bufferedSamples = 0
      if (!merged.length) return
      try {
        await meetingNoteAdapter().clipAppend(this.targetNoteId, this.recordSeq, pcmToBase64(merged))
      } catch (e) {
        this.error = `写入录音失败：${String(e)}`
      }
    },

    /** 到 4 分钟：收尾当前分段并自动开始下一段。 */
    async rotateSegment() {
      const target = this.targetNoteId
      const seq = this.recordSeq
      await this.flushBuffer(true)
      try {
        const stat = await meetingNoteAdapter().clipClose(target, seq)
        this.recentClips = [stat, ...this.recentClips]
        const next = await meetingNoteAdapter().clipStart(
          target,
          this.recordInfo?.sampleRate || 16000,
        )
        this.recordSeq = next.seq
        this.recordSegmentMs = 0
        this.insertHint = `第 ${stat.seq} 段已保存，继续录第 ${next.seq} 段`
        this.clipRevision += 1
        await this.loadClips(target)
      } catch (e) {
        this.error = String(e)
      }
    },

    startTicker() {
      this.stopTicker()
      ticker = window.setInterval(() => {
        if (!this.recording) this.stopTicker()
      }, 500)
    },

    stopTicker() {
      if (ticker) {
        window.clearInterval(ticker)
        ticker = 0
      }
    },

    /** 丢弃一段录坏的录音（界面上要二次确认）。 */
    async discardClip(clip: { path: string }) {
      try {
        await meetingNoteAdapter().clipDiscard(clip.path)
        this.recentClips = this.recentClips.filter((c) => c.path !== clip.path)
        this.clipRevision += 1
        await this.loadClips(this.targetNoteId || this.currentId)
      } catch (e) {
        this.error = String(e)
      }
    },

    /** 重新把某段录音的引用插进笔记。 */
    async insertClip(clip: ClipStat) {
      const target = this.targetNoteId || this.currentId
      if (!target) return
      const where = await this.insertRef(clip.refLine, target)
      this.insertHint = where === 'cursor' ? '已插到光标处（记得保存）' : '已追加到笔记末尾'
    },

    /** 一次性插入多条引用行（「插入全部未引用」）。 */
    async insertRefs(refLines: string[], noteId: string): Promise<'cursor' | 'end'> {
      const text = refLines
        .map((l) => l.trim())
        .filter(Boolean)
        .join('\n')
      if (!text) return 'end'
      return this.insertRef(text, noteId)
    },

    /**
     * 插入引用行：笔记正打开时交给编辑器插到光标处（避免覆盖未保存的编辑），
     * 否则直接追加到笔记正文末尾（纪要段之前）。
     */
    async insertRef(refLine: string, noteId: string): Promise<'cursor' | 'end'> {
      const notesStore = useNotesStore()
      if (notesStore.currentId === noteId) {
        this.pendingRef = { noteId, text: refLine }
        return 'cursor'
      }
      const body = await meetingNoteAdapter().read(noteId)
      await meetingNoteAdapter().write(noteId, appendRefLine(body, refLine))
      return 'end'
    },

    /** 编辑器完成光标插入后回调。 */
    clearPendingRef() {
      this.pendingRef = null
    },

    /** 刷新 `/v` 选择器用的片段列表（编辑器弹选择器时调用）。 */
    async loadClips(noteId?: string) {
      try {
        this.clips = await meetingNoteAdapter().clipList(noteId)
      } catch (e) {
        this.error = String(e)
      }
    },

    /** 刷新全部录音片段（列表徽章 / 未引用提示用）。 */
    async loadAllClips() {
      try {
        this.allClips = await meetingNoteAdapter().clipList()
      } catch (e) {
        this.error = String(e)
      }
    },
  },
})

function todayLocal(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

function defaultTitle(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `会议 ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}
