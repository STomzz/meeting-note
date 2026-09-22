/** 会议相关类型（与 Rust 侧 camelCase 契约一致）。 */

export interface Meeting {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  durationMs: number
  status: string
  segments: number
  transcribedSegments: number
  hasTranscript: boolean
  hasMinutes: boolean
  noteId: string
  asrModel: string
  chatModel: string
  error: string
}

export interface MeetingSegment {
  id: number
  meetingId: string
  seq: number
  file: string
  srcRate: number
  durationMs: number
  bytes: number
  status: string
  transcript: string
  error: string
}

export interface MeetingDetail {
  meeting: Meeting
  segments: MeetingSegment[]
  transcript: string
  minutesMd: string
  dir: string
}

export interface SegmentStat {
  seq: number
  durationMs: number
  bytes: number
  status: string
}

export interface TranscribeProgress {
  meetingId: string
  segmentSeq: number
  segmentsTotal: number
  segmentsDone: number
  chunksTotal: number
  chunksDone: number
  currentText: string
  message: string
}

export interface TranscribeOutcome {
  transcript: string
  segmentsTotal: number
  segmentsDone: number
  failedSegments: number
  chunks: number
  elapsedMs: number
  cancelled: boolean
  errors: string[]
}

export interface MinutesOutcome {
  minutes: unknown
  markdown: string
  model: string
  elapsedMs: number
  completionTokens: number | null
  repaired: boolean
  usedMapReduce: boolean
}

export const MEETING_STATUS_LABEL: Record<string, string> = {
  recording: '录音中',
  recorded: '已录音',
  transcribing: '转写中',
  transcribed: '已转写',
  minutes: '已生成纪要',
  failed: '有失败项',
}

export const MEETING_STATUS_THEME: Record<string, string> = {
  recording: 'danger',
  recorded: 'default',
  transcribing: 'warning',
  transcribed: 'success',
  minutes: 'primary',
  failed: 'danger',
}

export const SEGMENT_STATUS_LABEL: Record<string, string> = {
  recording: '录制中',
  recorded: '待转写',
  done: '已转写',
  failed: '失败',
}

export function fmtDuration(ms: number): string {
  const total = Math.round(ms / 1000)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const p = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${p(m)}:${p(s)}`
}

export function fmtDateTime(secs: number): string {
  if (!secs) return ''
  const d = new Date(secs * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

export function localDateString(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}
