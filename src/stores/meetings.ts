import { defineStore } from 'pinia'
import { meetingsAdapter } from '../platform/meetings'
import type { Meeting, MeetingDetail, MeetingSegment } from '../core/meetings'
import { MicRecorder, pcmToBase64, pcmToWavUrl } from '../core/recorder'
import type { RecorderInfo } from '../core/recorder'

/** 单个录音分段的时长上限（默认 4 分钟，可在此调整）。 */
export const SEGMENT_MAX_MS = 4 * 60 * 1000
/** PCM 落盘间隔（毫秒）：越小越不容易丢数据，IPC 调用越频繁。 */
const FLUSH_MS = 1000

/** 录音器与 PCM 缓冲放在模块级：大数组不进响应式，避免每帧代理开销。 */
const recorder = new MicRecorder()
let pcmBuffer: Int16Array[] = []
let bufferedSamples = 0
let ticker = 0
/** 采集回调不等待 promise，必须自己严格串行，避免并发写同一分段 / 重复切段。 */
let pushChain: Promise<void> = Promise.resolve()

export interface SelfCheckState {
  notes: string[]
  running: boolean
  recording: boolean
  seconds: number
  info: RecorderInfo | null
  peak: number
  playbackUrl: string
}

export const useMeetingsStore = defineStore('meetings', {
  state: () => ({
    meetings: [] as Meeting[],
    current: null as MeetingDetail | null,
    listLoading: false,
    listError: '',
    error: '',

    // 录音状态
    recording: false,
    recordMeetingId: '',
    recordSeq: 1,
    recordElapsedMs: 0,
    recordSegmentMs: 0,
    level: 0,
    recordInfo: null as RecorderInfo | null,

    // 转写状态
    transcribing: false,
    transcribeMessage: '',
    transcribeProgress: 0,

    // 纪要
    generating: false,
    minutesMessage: '',

    selfCheck: {
      notes: [],
      running: false,
      recording: false,
      seconds: 0,
      info: null,
      peak: 0,
      playbackUrl: '',
    } as SelfCheckState,
  }),

  getters: {
    /** 录音中的草稿分段（UI 展示"录制中"那一行）。 */
    draftSegment(state): { seq: number } | null {
      if (!state.recording || !state.recordMeetingId) return null
      return { seq: state.recordSeq }
    },
    /** 展示用总时长（录音中把当前段也算进去）。 */
    displayDurationMs(state): number {
      const base = state.current?.meeting.durationMs ?? 0
      return state.recording ? base + state.recordElapsedMs : base
    },
    levelPercent(state): number {
      // RMS 到 0~100：约 6000 已接近满格
      return Math.min(100, Math.round((state.level / 6000) * 100))
    },
  },

  actions: {
    // ---------------------------------------------------------------- 列表
    async loadList() {
      this.listLoading = true
      try {
        this.meetings = await meetingsAdapter().list()
        this.listError = ''
      } catch (e) {
        this.listError = String(e)
      } finally {
        this.listLoading = false
      }
    },

    async open(id: string) {
      try {
        this.current = await meetingsAdapter().detail(id)
        this.error = ''
      } catch (e) {
        this.error = String(e)
      }
    },

    async rename(id: string, title: string) {
      try {
        await meetingsAdapter().rename(id, title)
        await this.loadList()
        if (this.current?.meeting.id === id) await this.open(id)
      } catch (e) {
        this.error = String(e)
      }
    },

    async remove(id: string, deleteFiles: boolean) {
      try {
        await meetingsAdapter().remove(id, deleteFiles)
        if (this.current?.meeting.id === id) this.current = null
        await this.loadList()
      } catch (e) {
        this.error = String(e)
      }
    },

    async openDir(id: string): Promise<string> {
      try {
        return await meetingsAdapter().dirOf(id)
      } catch (e) {
        this.error = String(e)
        return ''
      }
    },

    // ---------------------------------------------------------------- 录音
    /** 开始录音；没有会议时先新建一场。返回会议 id（失败为 null）。 */
    async startRecording(title?: string): Promise<string | null> {
      if (this.recording) return this.recordMeetingId
      this.error = ''
      let meetingId = this.current?.meeting.id ?? ''
      if (!meetingId) {
        try {
          const meeting = await meetingsAdapter().create(title?.trim() || defaultTitle())
          meetingId = meeting.id
          this.current = await meetingsAdapter().detail(meetingId)
        } catch (e) {
          this.error = String(e)
          return null
        }
      }
      const existing = this.current?.segments ?? []
      const seq = existing.length ? Math.max(...existing.map((s) => s.seq)) + 1 : 1
      try {
        const info = await recorder.start({
          onChunk: (pcm) => void this.pushPcm(pcm),
          onLevel: (rms) => {
            this.level = rms
          },
          onError: (message) => {
            this.error = message
          },
        })
        this.recordInfo = info
        await meetingsAdapter().startSegment(meetingId, seq, info.sampleRate)
        this.recordMeetingId = meetingId
        this.recordSeq = seq
        this.recordElapsedMs = 0
        this.recordSegmentMs = 0
        pcmBuffer = []
        bufferedSamples = 0
        pushChain = Promise.resolve()
        this.recording = true
        this.startTicker()
        await this.refreshCurrent()
        await this.loadList()
        return meetingId
      } catch (e) {
        this.error = String(e)
        return null
      }
    },

    async stopRecording() {
      if (!this.recording) return
      const meetingId = this.recordMeetingId
      const seq = this.recordSeq
      this.stopTicker()
      try {
        await recorder.stop()
        // 先等排队中的 PCM 全部落盘（此时 recording 仍为 true），再收尾分段
        await pushChain
        await this.flushBuffer(true)
        this.recording = false
        if (meetingId && seq > 0) {
          await meetingsAdapter().closeSegment(meetingId, seq)
        }
      } catch (e) {
        this.error = String(e)
      } finally {
        this.level = 0
        await this.refreshCurrent()
        await this.loadList()
      }
    },

    /** 采集回调：排队串行处理（约每 1 秒落盘一次，每 4 分钟切段）。 */
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
      if (bufferedSamples >= (rate * FLUSH_MS) / 1000) {
        await this.flushBuffer(false)
      }
      if (this.recordSegmentMs >= SEGMENT_MAX_MS) {
        await this.rotateSegment()
      }
    },

    async flushBuffer(force: boolean) {
      if (!force && !bufferedSamples) return
      if (!pcmBuffer.length) return
      const rate = this.recordInfo?.sampleRate || 16000
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
        await meetingsAdapter().appendPcm(
          this.recordMeetingId,
          this.recordSeq,
          rate,
          pcmToBase64(merged),
        )
      } catch (e) {
        this.error = `写入录音失败：${String(e)}`
      }
    },

    async rotateSegment() {
      const meetingId = this.recordMeetingId
      const seq = this.recordSeq
      await this.flushBuffer(true)
      try {
        await meetingsAdapter().closeSegment(meetingId, seq)
        const next = seq + 1
        await meetingsAdapter().startSegment(meetingId, next, this.recordInfo?.sampleRate || 16000)
        this.recordSeq = next
        this.recordSegmentMs = 0
        await this.refreshCurrent()
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

    async refreshCurrent() {
      if (this.current?.meeting.id) await this.open(this.current.meeting.id)
    },

    // ---------------------------------------------------------------- 转写
    async transcribe() {
      const id = this.current?.meeting.id
      if (!id || this.transcribing) return
      this.transcribing = true
      this.error = ''
      this.transcribeProgress = 0
      this.transcribeMessage = '准备中…'
      let unlisten: (() => void) | null = null
      try {
        unlisten = await meetingsAdapter().onProgress((p) => {
          this.transcribeMessage = p.message
          const total = Math.max(p.segmentsTotal, 1)
          const chunkPart = p.chunksTotal ? p.chunksDone / p.chunksTotal : 0
          this.transcribeProgress = Math.min(
            99,
            Math.round(((p.segmentsDone + chunkPart) / total) * 100),
          )
        })
        const out = await meetingsAdapter().transcribe(id)
        const summary = out.cancelled
          ? `已取消（完成 ${out.segmentsDone}/${out.segmentsTotal} 段）`
          : `完成：${out.segmentsDone}/${out.segmentsTotal} 段，${out.chunks} 块，${out.elapsedMs} ms`
        this.transcribeProgress = 100
        await this.refreshCurrent()
        await this.loadList()
        this.transcribeMessage = summary
        if (out.errors.length) this.error = out.errors.join('；')
      } catch (e) {
        this.error = String(e)
        this.transcribeMessage = `转写失败：${String(e)}`
      } finally {
        unlisten?.()
        this.transcribing = false
      }
    },

    async cancelTranscribe() {
      try {
        await meetingsAdapter().cancelTranscribe()
        this.transcribeMessage = '已请求取消，等待当前请求结束…'
      } catch (e) {
        this.error = String(e)
      }
    },

    // ---------------------------------------------------------------- 纪要
    async generateMinutes() {
      const id = this.current?.meeting.id
      if (!id || this.generating) return
      this.generating = true
      this.error = ''
      this.minutesMessage = '正在生成纪要…'
      try {
        const out = await meetingsAdapter().generateMinutes(id, localDate())
        this.minutesMessage = `完成：${out.model}，${out.elapsedMs} ms${
          out.repaired ? '（修复重试 1 次）' : ''
        }${out.usedMapReduce ? '（长会议 map-reduce）' : ''}`
        await this.refreshCurrent()
        await this.loadList()
      } catch (e) {
        this.error = String(e)
        this.minutesMessage = `生成失败：${String(e)}`
      } finally {
        this.generating = false
      }
    },

    // ---------------------------------------------------------------- 自检
    async runSelfCheck() {
      this.selfCheck.running = true
      try {
        this.selfCheck.notes = await MicRecorder.selfCheck()
      } finally {
        this.selfCheck.running = false
      }
    },

    /** 录音自检：只在内存里做，不写会议库；用于 Android 录音 spike 验证。 */
    async startSelfCheckRecording(seconds = 5) {
      if (this.selfCheck.recording) return
      this.selfCheck.recording = true
      this.selfCheck.seconds = 0
      this.selfCheck.peak = 0
      this.selfCheck.playbackUrl = ''
      const parts: Int16Array[] = []
      let rate = 16000
      try {
        const info = await recorder.start({
          onChunk: (pcm) => {
            parts.push(pcm)
            let peak = this.selfCheck.peak
            for (let i = 0; i < pcm.length; i += 8) {
              const v = Math.abs(pcm[i])
              if (v > peak) peak = v
            }
            this.selfCheck.peak = peak
            this.level = peak
          },
          onLevel: () => undefined,
          onError: (m) => {
            this.error = m
          },
        })
        this.selfCheck.info = info
        rate = info.sampleRate
        const timer = window.setInterval(() => {
          this.selfCheck.seconds += 1
          if (this.selfCheck.seconds >= seconds) {
            window.clearInterval(timer)
            void (async () => {
              await recorder.stop()
              const total = parts.reduce((sum, p) => sum + p.length, 0)
              const merged = new Int16Array(total)
              let offset = 0
              for (const part of parts) {
                merged.set(part, offset)
                offset += part.length
              }
              this.selfCheck.playbackUrl = merged.length ? pcmToWavUrl(merged, rate) : ''
              this.selfCheck.recording = false
              this.level = 0
            })()
          }
        }, 1000)
      } catch (e) {
        this.error = String(e)
        this.selfCheck.recording = false
      }
    },

    // ---------------------------------------------------------------- 播放
    segmentSrc(seg: MeetingSegment): string | null {
      if (!this.current) return null
      return meetingsAdapter().segmentSrc(this.current.dir, seg.file)
    },
  },
})

function defaultTitle(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `会议 ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

function localDate(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}
