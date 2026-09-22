import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  Meeting,
  MeetingDetail,
  MeetingSegment,
  MinutesOutcome,
  SegmentStat,
  TranscribeOutcome,
  TranscribeProgress,
} from '../core/meetings'
import { MEETING_STATUS_LABEL } from '../core/meetings'

/** 会议适配器：录音落盘、转写、纪要、播放地址。 */
export interface MeetingsAdapter {
  list(): Promise<Meeting[]>
  create(title: string): Promise<Meeting>
  detail(id: string): Promise<MeetingDetail>
  rename(id: string, title: string): Promise<void>
  remove(id: string, deleteFiles: boolean): Promise<void>
  startSegment(id: string, seq: number, sampleRate: number): Promise<MeetingSegment>
  appendPcm(id: string, seq: number, sampleRate: number, pcmBase64: string): Promise<SegmentStat>
  closeSegment(id: string, seq: number): Promise<MeetingSegment>
  transcribe(id: string): Promise<TranscribeOutcome>
  cancelTranscribe(): Promise<void>
  generateMinutes(id: string, date?: string): Promise<MinutesOutcome>
  /** 分段音频的可播放地址（预览模式返回 null）。 */
  segmentSrc(dir: string, file: string): string | null
  /** 会议音频目录（用于「打开文件夹」）。 */
  dirOf(id: string): Promise<string>
  /** 订阅转写进度，返回取消订阅函数。 */
  onProgress(cb: (p: TranscribeProgress) => void): Promise<() => void>
}

class TauriMeetingsAdapter implements MeetingsAdapter {
  list() {
    return invoke<Meeting[]>('meeting_list')
  }
  create(title: string) {
    return invoke<Meeting>('meeting_create', { title })
  }
  detail(id: string) {
    return invoke<MeetingDetail>('meeting_detail', { id })
  }
  rename(id: string, title: string) {
    return invoke<void>('meeting_rename', { id, title })
  }
  remove(id: string, deleteFiles: boolean) {
    return invoke<void>('meeting_delete', { id, deleteFiles })
  }
  startSegment(id: string, seq: number, sampleRate: number) {
    return invoke<MeetingSegment>('meeting_start_segment', { id, seq, sampleRate })
  }
  appendPcm(id: string, seq: number, sampleRate: number, pcmBase64: string) {
    return invoke<SegmentStat>('meeting_append_pcm', { id, seq, sampleRate, pcmBase64 })
  }
  closeSegment(id: string, seq: number) {
    return invoke<MeetingSegment>('meeting_close_segment', { id, seq })
  }
  transcribe(id: string) {
    return invoke<TranscribeOutcome>('meeting_transcribe', { id })
  }
  cancelTranscribe() {
    return invoke<void>('meeting_cancel_transcribe')
  }
  generateMinutes(id: string, date?: string) {
    return invoke<MinutesOutcome>('meeting_generate_minutes', { id, date: date ?? null })
  }
  segmentSrc(dir: string, file: string): string | null {
    if (!dir || !file) return null
    const base = dir.endsWith('/') ? dir : `${dir}/`
    return convertFileSrc(`${base}${file}`)
  }
  dirOf(id: string) {
    return invoke<string>('meeting_dir', { id })
  }
  async onProgress(cb: (p: TranscribeProgress) => void) {
    const un = await listen<TranscribeProgress>('meeting-progress', (e) => cb(e.payload))
    return un
  }
}

/** 预览模式：内存会议 + 模拟进度，不伪造真实转写结果。 */
class MockMeetingsAdapter implements MeetingsAdapter {
  private items: MeetingDetail[] = [
    {
      meeting: {
        id: 'demo-1',
        title: '示例会议（预览模式）',
        createdAt: Math.floor(Date.now() / 1000) - 3600,
        updatedAt: Math.floor(Date.now() / 1000) - 3000,
        durationMs: 252000,
        status: 'transcribed',
        segments: 2,
        transcribedSegments: 2,
        hasTranscript: true,
        hasMinutes: false,
        noteId: '',
        asrModel: 'qwen3-asr-1.7b',
        chatModel: 'Qwen-Inno-35B-v1',
        error: '',
      },
      segments: [
        {
          id: 1,
          meetingId: 'demo-1',
          seq: 1,
          file: 'seg_0001.wav',
          srcRate: 16000,
          durationMs: 126000,
          bytes: 4032000,
          status: 'done',
          transcript: '[00:00:00] 预览模式：这里是示例转写文本。',
          error: '',
        },
        {
          id: 2,
          meetingId: 'demo-1',
          seq: 2,
          file: 'seg_0002.wav',
          srcRate: 16000,
          durationMs: 126000,
          bytes: 4032000,
          status: 'done',
          transcript: '[00:02:06] 预览模式：真实转写需要桌面客户端。',
          error: '',
        },
      ],
      transcript:
        '[00:00:00] 预览模式：这里是示例转写文本。\n\n[00:02:06] 预览模式：真实转写需要桌面客户端。',
      minutesMd: '',
      dir: '/（预览模式无真实文件）',
    },
  ]

  private find(id: string) {
    const item = this.items.find((m) => m.meeting.id === id)
    if (!item) throw new Error('会议不存在（预览模式）')
    return item
  }

  async list() {
    return this.items.map((m) => m.meeting)
  }
  async create(title: string): Promise<Meeting> {
    const id = `demo-${Date.now()}`
    const detail: MeetingDetail = {
      meeting: {
        id,
        title: title.trim() || '未命名会议',
        createdAt: Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
        durationMs: 0,
        status: 'recording',
        segments: 0,
        transcribedSegments: 0,
        hasTranscript: false,
        hasMinutes: false,
        noteId: '',
        asrModel: '',
        chatModel: '',
        error: '',
      },
      segments: [],
      transcript: '',
      minutesMd: '',
      dir: '/（预览模式无真实文件）',
    }
    this.items.unshift(detail)
    return detail.meeting
  }
  async detail(id: string) {
    return this.find(id)
  }
  async rename(id: string, title: string) {
    this.find(id).meeting.title = title
  }
  async remove(id: string) {
    this.items = this.items.filter((m) => m.meeting.id !== id)
  }
  async startSegment(id: string, seq: number): Promise<MeetingSegment> {
    const item = this.find(id)
    const seg: MeetingSegment = {
      id: item.segments.length + 1,
      meetingId: id,
      seq,
      file: `seg_${String(seq).padStart(4, '0')}.wav`,
      srcRate: 16000,
      durationMs: 0,
      bytes: 0,
      status: 'recording',
      transcript: '',
      error: '',
    }
    item.segments.push(seg)
    return seg
  }
  async appendPcm(id: string, seq: number, _rate: number, pcmBase64: string): Promise<SegmentStat> {
    const item = this.find(id)
    const seg = item.segments.find((s) => s.seq === seq)
    if (!seg) throw new Error('分段不存在（预览模式）')
    const bytes = Math.floor((pcmBase64.length * 3) / 4)
    seg.bytes += bytes
    seg.durationMs = Math.round((seg.bytes / 2 / 16000) * 1000)
    item.meeting.durationMs = item.segments.reduce((sum, s) => sum + s.durationMs, 0)
    return { seq, durationMs: seg.durationMs, bytes: seg.bytes, status: seg.status }
  }
  async closeSegment(id: string, seq: number): Promise<MeetingSegment> {
    const item = this.find(id)
    const seg = item.segments.find((s) => s.seq === seq)
    if (!seg) throw new Error('分段不存在（预览模式）')
    seg.status = 'recorded'
    item.meeting.status = 'recorded'
    item.meeting.segments = item.segments.length
    return seg
  }
  async transcribe(id: string): Promise<TranscribeOutcome> {
    const item = this.find(id)
    for (let i = 0; i < item.segments.length; i++) {
      await new Promise((r) => setTimeout(r, 400))
    }
    item.meeting.status = 'transcribed'
    item.meeting.transcribedSegments = item.segments.length
    item.meeting.hasTranscript = true
    return {
      transcript: item.transcript,
      segmentsTotal: item.segments.length,
      segmentsDone: item.segments.length,
      failedSegments: 0,
      chunks: item.segments.length,
      elapsedMs: item.segments.length * 400,
      cancelled: false,
      errors: ['预览模式：未调用真实模型'],
    }
  }
  async cancelTranscribe() {
    /* 预览模式无操作 */
  }
  async generateMinutes(id: string): Promise<MinutesOutcome> {
    const item = this.find(id)
    await new Promise((r) => setTimeout(r, 500))
    const markdown = `# ${item.meeting.title}\n\n## 一、会议摘要\n\n（预览模式：这是示例纪要，真实生成需要桌面客户端 + 已配置的对话模型。）\n\n## 二、议题与讨论\n\n（无）\n\n## 三、决议事项\n\n（无）\n\n## 四、待办事项\n\n（无）\n`
    item.minutesMd = markdown
    item.meeting.hasMinutes = true
    item.meeting.status = 'minutes'
    return {
      minutes: {},
      markdown,
      model: 'preview',
      elapsedMs: 500,
      completionTokens: null,
      repaired: false,
      usedMapReduce: false,
    }
  }
  segmentSrc() {
    return null
  }
  async dirOf() {
    return '/（预览模式无真实文件）'
  }
  async onProgress() {
    return () => undefined
  }
}

let adapter: MeetingsAdapter | null = null

/** 与 notes/retrieval 一致：适配器单例（预览模式的会议也就能在页面间共享）。 */
export function meetingsAdapter(): MeetingsAdapter {
  if (!adapter) {
    adapter = '__TAURI_INTERNALS__' in window ? new TauriMeetingsAdapter() : new MockMeetingsAdapter()
  }
  return adapter
}

/** 状态文案（供 UI 复用）。 */
export function statusLabel(status: string): string {
  return MEETING_STATUS_LABEL[status] ?? status
}
