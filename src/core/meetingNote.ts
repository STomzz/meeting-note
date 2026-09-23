/**
 * 会议笔记（P7）类型与纯函数。
 *
 * 与 Rust 侧 `bnu_core::meeting_note` / `bnu_core::audio_clip` 的约定逐字对齐：
 * - `/v <路径>` 或 `/video <路径>` 独占一行 = 音频引用；
 * - 引用行后面紧跟的 `>` 块 = 程序生成的转写，重跑整块替换；
 * - `## 会议纪要（AI 整理）`（取最后一次出现）= 分界线，之后由 LLM 生成并整段重写。
 */

export interface MeetingNoteBrief {
  noteId: string
  title: string
  folder: string
  updatedAt: number
  audioTotal: number
  audioMissing: number
  transcribed: number
  hasMinutes: boolean
  durationMs: number
  audioBytes: number
}

export interface AudioRef {
  line: number
  token: string
  raw: string
  /** `file` = 单个音频；`session` = 整场（一条长语音） */
  kind: 'file' | 'session'
  /** `file` = 文件路径；`session` = 相对 `会议音频/` 的场次目录（与 `ClipInfo.dir` 一致） */
  path: string
  fileName: string
  exists: boolean
  bytes: number
  durationMs: number
  /** 场次里的分段（单文件引用只有一项） */
  segments: AudioRefSegment[]
  hasTranscript: boolean
  transcript: string
}

/** 场次引用里的一个分段。 */
export interface AudioRefSegment {
  file: string
  path: string
  seq: number
  bytes: number
  durationMs: number
}

export interface ClipInfo {
  dir: string
  file: string
  path: string
  seq: number
  /** 场次 id（`20260923-1430`）；旧版扁平目录为空串 */
  session: string
  bytes: number
  durationMs: number
  sampleRate: number
  modifiedAt: number
}

export interface ClipStat {
  dir: string
  file: string
  path: string
  seq: number
  /** 场次 id（`20260923-1430`）；旧版扁平目录为空串 */
  session: string
  bytes: number
  durationMs: number
  sampleRate: number
  /** 单段引用行，如 `/v 会议音频/周会/20260923-1430/seg_0001.wav` */
  refLine: string
  /** 整场引用行（一条长语音），如 `/v 会议音频/周会/20260923-1430/` */
  sessionRefLine: string
}

export interface ProcessProgress {
  noteId: string
  phase: string
  audioTotal: number
  audioDone: number
  audioSkipped: number
  audioFailed: number
  current: string
  message: string
}

export interface ProcessOutcome {
  noteId: string
  audioTotal: number
  audioDone: number
  audioSkipped: number
  audioFailed: number
  transcriptChars: number
  minutesChars: number
  model: string
  elapsedMs: number
  cancelled: boolean
  errors: string[]
}

/** 单段转写的结果（录音过程中「每段结束就转写」用）。 */
export interface ClipTranscribeOutcome {
  /** cached / transcribed / empty / failed */
  status: string
  /** vault 相对路径 */
  path: string
  chars: number
  error: string
}

export const MEETING_DIR = '会议'
export const AUDIO_DIR = '会议音频'
export const MINUTES_HEADING = '## 会议纪要（AI 整理）'
export const MINUTES_HINT = '_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_'
export const REF_TOKENS = ['/v', '/video']

export interface RawRef {
  line: number
  token: string
  raw: string
}

/** 解析一行是否为音频引用（镜像 Rust `parse_ref_line`）。 */
export function parseRefLine(line: string): { token: string; raw: string } | null {
  const trimmed = line.trim()
  for (const token of REF_TOKENS) {
    if (!trimmed.startsWith(token)) continue
    const rest = trimmed.slice(token.length)
    // 关键词后面必须是空白或行尾，避免把 `/very` 认成 `/v`
    if (rest && !/^\s/.test(rest)) continue
    const raw = rest.trim().replace(/^[`"'<>]+/, '').replace(/[`"'<>]+$/, '')
    if (raw) return { token, raw }
  }
  return null
}

/** 扫出正文里所有音频引用（行号从 1 开始）。 */
export function parseRefs(body: string): RawRef[] {
  const out: RawRef[] = []
  body.split('\n').forEach((line, i) => {
    const hit = parseRefLine(line)
    if (hit) out.push({ line: i + 1, token: hit.token, raw: hit.raw })
  })
  return out
}

/** 切出「正文」与「已有纪要」（以最后一次出现的分界标题为界）。 */
export function splitMinutes(body: string): { head: string; minutes: string | null } {
  const lines = body.split('\n')
  let idx = -1
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].trim() === MINUTES_HEADING) {
      idx = i
      break
    }
  }
  if (idx < 0) return { head: body.replace(/\s+$/, ''), minutes: null }
  return {
    head: lines.slice(0, idx).join('\n').replace(/\s+$/, ''),
    minutes: lines.slice(idx).join('\n').replace(/\s+$/, ''),
  }
}

/** 纪要段是否有真实内容（不是新建时的占位提示）。 */
export function minutesReady(minutes: string | null): boolean {
  if (!minutes) return false
  const body = minutes
    .split('\n')
    .filter((l) => l.trim() !== MINUTES_HEADING)
    .join('\n')
    .trim()
  return !!body && !body.includes(MINUTES_HINT)
}

/** 在纪要段之前追加一行音频引用（录音面板在笔记未打开时的兜底插入）。 */
export function appendRefLine(body: string, refLine: string): string {
  const { head, minutes } = splitMinutes(body)
  const line = refLine.trim()
  if (!minutes) return `${head}\n\n${line}\n`
  return `${head}\n\n${line}\n\n${minutes}\n`
}

/**
 * 把引用插到光标处（编辑器里用）。
 *
 * 光标落在纪要段里时退回「追加到正文末尾」——纪要段是程序生成区，引用写进去下次就被覆盖。
 */
export function insertRefAtCursor(
  body: string,
  refLine: string,
  cursor: number,
): { body: string; cursor: number } {
  const { head, minutes } = splitMinutes(body)
  const limit = head.length
  const pos = minutes && cursor > limit ? limit : Math.max(0, Math.min(cursor, body.length))
  const before = body.slice(0, pos)
  const after = body.slice(pos)
  const pad = before === '' || before.endsWith('\n') ? '' : '\n'
  const insert = `${pad}${refLine.trim()}\n`
  return { body: `${before}${insert}${after}`, cursor: pos + insert.length }
}

/**
 * 找光标前正在输入的 `/v` 命令（编辑器里弹录音选择器用）。
 *
 * 返回 `start`（**替换范围起点**：含行首缩进、不含换行符，从这里到光标整段会被引用行替换）
 * 与 `filter`（已输入的部分路径）。
 */
export function slashCommandAt(
  text: string,
  cursor: number,
): { start: number; token: string; filter: string } | null {
  const pos = Math.max(0, Math.min(cursor, text.length))
  const before = text.slice(0, pos)
  const hit = /(?:^|\n)[ \t]*\/(v|video)(?=[ \t]|$)([^\n]*)$/.exec(before)
  if (!hit) return null
  const start = before.length - hit[0].length + (hit[0].startsWith('\n') ? 1 : 0)
  return { start, token: `/${hit[1]}`, filter: hit[2].trim() }
}

/** `/v` 音频引用的渲染占位符（markdown-it 渲染完再替换成播放器）。 */
export const AUDIO_PLACEHOLDER_RE = /§§AUDIO:([^§]+)§§/g

/**
 * 预览渲染前的准备：把 `/v 路径` 行换成占位符并拼回纪要段。
 *
 * 返回可以直接喂给 markdown-it 的文本；渲染后用 `replaceAudioPlaceholders` 换成播放器。
 */
export function prepareAudioRefs(body: string): string {
  const { head, minutes } = splitMinutes(body)
  const prepared = head
    .split('\n')
    .map((line) => {
      const hit = parseRefLine(line)
      return hit ? `\n\n§§AUDIO:${hit.raw}§§\n\n` : line
    })
    .join('\n')
  return minutes ? `${prepared}\n\n${minutes}` : prepared
}

/** 把渲染结果里的占位符换成 `<audio>`（`tag` 返回要插入的 HTML）。 */
export function replaceAudioPlaceholders(html: string, tag: (raw: string) => string): string {
  return html.replace(AUDIO_PLACEHOLDER_RE, (_m, raw: string) => tag(raw))
}

/** 会议笔记 id → 音频目录（镜像 Rust `audio_clip::dir_for_note`，与笔记路径一一对应）。 */
export function dirForNote(noteId: string): string {
  const id = noteId.trim().replace(/^\/+/, '')
  const stem = id.endsWith('.md') ? id.slice(0, -3) : id
  const parts = stem
    .split(/[\\/]/)
    .map(sanitizeSegment)
    .filter(Boolean)
  if (!parts.length) return '未命名'
  return parts.join('/')
}

/** 旧版（0.1.x）目录名（镜像 Rust `dir_for_note_legacy`，用于兼容列出历史录音）。 */
export function dirForNoteLegacy(noteId: string): string {
  const id = noteId.trim().replace(/^\/+/, '')
  let stem = id.endsWith('.md') ? id.slice(0, -3) : id
  if (stem.startsWith(`${MEETING_DIR}/`)) stem = stem.slice(MEETING_DIR.length + 1)
  const safe = stem
    .replace(/[\\/]/g, '_')
    .trim()
    .replace(/^\.+|\.+$/g, '')
    .trim()
  if (!safe) return '未命名会议'
  return Array.from(safe).slice(0, 60).join('')
}

/** 一篇笔记可能存在的音频目录（新目录 + 旧版目录，去重）。 */
export function dirsForNote(noteId: string): string[] {
  const dirs = [dirForNote(noteId)]
  const legacy = dirForNoteLegacy(noteId)
  if (!dirs.includes(legacy)) dirs.push(legacy)
  return dirs
}

function sanitizeSegment(part: string): string {
  const s = part
    .replace(/[<>:"|?*\\]/g, '-')
    .replace(/[\u0000-\u001f]/g, '')
    .trim()
    .replace(/^\.+|\.+$/g, '')
    .trim()
  return Array.from(s).slice(0, 60).join('')
}

/** 判断一段录音是否已经被笔记引用（单文件按 path 匹配，整场按目录匹配）。 */
export function isClipReferenced(
  clip: Pick<ClipInfo, 'path' | 'dir'>,
  refs: Array<Pick<AudioRef, 'kind' | 'path'>>,
): boolean {
  const path = normalizeRel(clip.path)
  const dir = normalizeRel(clip.dir)
  return refs.some((r) =>
    r.kind === 'session' ? normalizeRel(r.path) === dir : normalizeRel(r.path) === path,
  )
}

/** 还没写进笔记的录音（按 vault 相对路径 / 场次目录比对）。 */
export function unreferencedClips(
  clips: ClipInfo[],
  refs: Array<Pick<AudioRef, 'kind' | 'path'>>,
): ClipInfo[] {
  return clips.filter((c) => !isClipReferenced(c, refs))
}

/** 属于某篇笔记的录音（新目录 + 其下所有场次 + 旧版扁平目录）。 */
export function clipsBelongToNote(clips: ClipInfo[], noteId: string): ClipInfo[] {
  const dirs = dirsForNote(noteId)
  return clips.filter((c) => dirs.some((d) => c.dir === d || c.dir.startsWith(`${d}/`)))
}

/** 一场录音（同一个场次目录里的全部分段）。 */
export interface ClipSession {
  dir: string
  session: string
  clips: ClipInfo[]
  durationMs: number
  bytes: number
  modifiedAt: number
}

/** 按场次（目录）分组：组内按序号，组间新的在前。 */
export function groupClipsByDir(clips: ClipInfo[]): ClipSession[] {
  const map = new Map<string, ClipInfo[]>()
  for (const c of clips) {
    const list = map.get(c.dir)
    if (list) list.push(c)
    else map.set(c.dir, [c])
  }
  return [...map.entries()]
    .map(([dir, list]) => {
      const sorted = [...list].sort((a, b) => a.seq - b.seq)
      return {
        dir,
        session: sorted[0]?.session ?? '',
        clips: sorted,
        durationMs: sorted.reduce((s, c) => s + c.durationMs, 0),
        bytes: sorted.reduce((s, c) => s + c.bytes, 0),
        modifiedAt: sorted.reduce((m, c) => Math.max(m, c.modifiedAt), 0),
      }
    })
    .sort((a, b) => b.modifiedAt - a.modifiedAt)
}

/** 场次 id：本地时间 `YYYYMMDD-HHMM`（一次录音 = 一个场次目录）。 */
export function sessionIdNow(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`
}

/** 场次展示名：`09-23 14:30`；没有场次的旧录音 → `早期录音`。 */
export function sessionLabel(sessionOrDir: string): string {
  const last = sessionOrDir.split('/').filter(Boolean).pop() ?? ''
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})$/.exec(last)
  return m ? `${m[2]}-${m[3]} ${m[4]}:${m[5]}` : '早期录音'
}

/** 整场引用行（一条长语音）：`/v 会议音频/<目录>/`。 */
export function sessionRefLine(dir: string): string {
  return `/v ${AUDIO_DIR}/${normalizeRel(dir)}/`
}

/**
 * 引用路径里的音频目录（相对 `会议音频/`，等于 `ClipInfo.dir`）。
 *
 * `会议音频/会议/周会/20260922-0930/` → `会议/周会/20260922-0930`；
 * 看着像文件（带音频扩展名）或捞不出目录时返回 ''，调用方按文件处理。
 */
export function dirRelOfRef(raw: string): string {
  const inner = normalizeRel(raw).replace(new RegExp(`^${AUDIO_DIR}/`), '')
  if (!inner || /\.(wav|mp3|m4a|ogg|flac|aac|opus)$/i.test(inner)) return ''
  return inner
}

function normalizeRel(p: string): string {
  return p.trim().replace(/\\/g, '/').replace(/^\.?\/+/, '').replace(/\/+$/, '')
}

/** 每篇笔记的录音数量（含场次与旧版目录），用于列表徽章。 */
export function clipCountsByNote(
  notes: Array<{ id: string }>,
  clips: ClipInfo[],
): Record<string, number> {
  const out: Record<string, number> = {}
  for (const n of notes) {
    const count = clipsBelongToNote(clips, n.id).length
    if (count) out[n.id] = count
  }
  return out
}

/** 一键处理的进度百分比（粗略，按音频条数 + 阶段估算）。 */
export function progressPercent(p: ProcessProgress): number {
  const done = p.audioDone + p.audioSkipped + p.audioFailed
  const total = Math.max(p.audioTotal, 0)
  const audioRatio = total ? done / total : 1
  switch (p.phase) {
    case 'parse':
      return total ? 2 : 5
    case 'transcribe':
      return Math.min(90, Math.round(5 + audioRatio * 80))
    case 'write':
      return 92
    case 'minutes':
      return 96
    case 'done':
      return 100
    default:
      return 0
  }
}

/** 处理结果的一句话总结。 */
export function outcomeSummary(o: ProcessOutcome): string {
  if (o.cancelled) return `已取消：转写 ${o.audioDone}、复用 ${o.audioSkipped}、失败 ${o.audioFailed}`
  const parts = [`转写 ${o.audioDone} 段`]
  if (o.audioSkipped) parts.push(`复用缓存 ${o.audioSkipped} 段`)
  if (o.audioFailed) parts.push(`失败 ${o.audioFailed} 段`)
  if (o.minutesChars) parts.push(`纪要 ${o.minutesChars} 字（${o.model}）`)
  parts.push(`${(o.elapsedMs / 1000).toFixed(1)}s`)
  return parts.join('，')
}

/** 时长格式化：`m:ss`，超过 1 小时用 `h:mm:ss`。 */
export function formatDur(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const p = (n: number) => String(n).padStart(2, '0')
  return h ? `${h}:${p(m)}:${p(s)}` : `${m}:${p(s)}`
}

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  const text = Number.isInteger(v) || v >= 10 ? String(Math.round(v)) : v.toFixed(1)
  return `${text} ${units[i]}`
}
