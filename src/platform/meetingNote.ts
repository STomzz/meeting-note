import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AudioRef,
  ClipInfo,
  ClipStat,
  MeetingNoteBrief,
  ProcessOutcome,
  ProcessProgress,
} from '../core/meetingNote'

/** 会议笔记适配器：md 即会议，音频在 vault 里，一键处理跑在 Rust 侧。 */
export interface MeetingNoteAdapter {
  list(scanAll: boolean): Promise<MeetingNoteBrief[]>
  /** 新建会议笔记，返回笔记 id。 */
  create(title: string, date: string): Promise<string>
  refs(noteId: string): Promise<AudioRef[]>
  process(noteId: string, force: boolean): Promise<ProcessOutcome>
  cancel(): Promise<void>
  /** 订阅处理进度，返回取消订阅函数。 */
  onProgress(cb: (p: ProcessProgress) => void): Promise<() => void>
  /** 旧会议（meetings 表）一次性导出为会议笔记，返回导出的笔记 id。 */
  migrateLegacy(): Promise<string[]>
  read(noteId: string): Promise<string>
  write(noteId: string, content: string): Promise<void>
  clipStart(noteId: string, sampleRate: number): Promise<ClipStat>
  clipAppend(noteId: string, seq: number, pcmBase64: string): Promise<ClipStat>
  clipClose(noteId: string, seq: number): Promise<ClipStat>
  /** 按 vault 相对路径丢弃一段录音（只允许 `会议音频/` 下的分段）。 */
  clipDiscard(path: string): Promise<void>
  clipList(noteId?: string): Promise<ClipInfo[]>
  /** vault 内音频的播放地址（预览模式返回 null）。 */
  audioSrc(relPath: string): Promise<string | null>
}

function joinVault(vault: string, rel: string): string {
  const base = vault.replace(/\\/g, '/').replace(/\/+$/, '')
  return `${base}/${rel.replace(/^\/+/, '')}`
}

class TauriMeetingNoteAdapter implements MeetingNoteAdapter {
  private vault: string | null = null

  list(scanAll: boolean) {
    return invoke<MeetingNoteBrief[]>('meeting_notes_list', { scanAll })
  }
  create(title: string, date: string) {
    return invoke<string>('meeting_note_new', { title, date })
  }
  refs(noteId: string) {
    return invoke<AudioRef[]>('meeting_note_refs', { noteId })
  }
  process(noteId: string, force: boolean) {
    return invoke<ProcessOutcome>('meeting_note_process', { noteId, force })
  }
  cancel() {
    return invoke<void>('meeting_note_cancel')
  }
  async onProgress(cb: (p: ProcessProgress) => void) {
    const un = await listen<ProcessProgress>('meeting-note-progress', (e) => cb(e.payload))
    return un
  }
  migrateLegacy() {
    return invoke<string[]>('meeting_note_migrate_legacy')
  }
  read(noteId: string) {
    return invoke<string>('read_note', { id: noteId })
  }
  write(noteId: string, content: string) {
    return invoke<void>('write_note', { id: noteId, content })
  }
  clipStart(noteId: string, sampleRate: number) {
    return invoke<ClipStat>('audio_clip_start', { noteId, sampleRate })
  }
  clipAppend(noteId: string, seq: number, pcmBase64: string) {
    return invoke<ClipStat>('audio_clip_append', { noteId, seq, pcmBase64 })
  }
  clipClose(noteId: string, seq: number) {
    return invoke<ClipStat>('audio_clip_close', { noteId, seq })
  }
  clipDiscard(relPath: string) {
    return invoke<void>('audio_clip_discard', { path: relPath })
  }
  clipList(noteId?: string) {
    return invoke<ClipInfo[]>('audio_clip_list', { noteId: noteId ?? null })
  }
  async audioSrc(relPath: string): Promise<string | null> {
    if (!relPath) return null
    try {
      if (!this.vault) this.vault = await invoke<string>('get_vault')
      if (!this.vault) return null
      return convertFileSrc(joinVault(this.vault, relPath))
    } catch {
      return null
    }
  }
}

/** 预览模式：内存里的示例会议笔记，用来在浏览器里看 UI（不产生真实文件与请求）。 */
class MockMeetingNoteAdapter implements MeetingNoteAdapter {
  private notes: { brief: MeetingNoteBrief; body: string }[] = [
    {
      brief: {
        noteId: '会议/2026-09-22-示例周会.md',
        title: '示例周会',
        folder: '会议',
        updatedAt: Math.floor(Date.now() / 1000) - 600,
        audioTotal: 2,
        audioMissing: 0,
        transcribed: 1,
        hasMinutes: false,
        durationMs: 252000,
        audioBytes: 8064000,
      },
      body: [
        '# 示例周会',
        '',
        '> 预览模式：真实数据需要桌面客户端。',
        '',
        '## 我的记录',
        '',
        '张三说镜像拉取超时，先换镜像站解决。',
        '/v 会议音频/会议/2026-09-22-示例周会/seg_0001.wav',
        '> 🎙 转写 00:00:00–00:02:06 · seg_0001.wav',
        '>',
        '> [00:00:00] 预览模式：这里是示例转写文本。',
        '',
        '我又补了一句：下周一补部署文档。',
        '/v 会议音频/会议/2026-09-22-示例周会/seg_0002.wav',
        '',
        '## 会议纪要（AI 整理）',
        '',
        '_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_',
        '',
      ].join('\n'),
    },
  ]

  private clips: ClipInfo[] = [
    {
      dir: '会议/2026-09-22-示例周会',
      file: 'seg_0001.wav',
      path: '会议音频/会议/2026-09-22-示例周会/seg_0001.wav',
      seq: 1,
      bytes: 4032000,
      durationMs: 126000,
      sampleRate: 16000,
      modifiedAt: Math.floor(Date.now() / 1000) - 630,
    },
    {
      dir: '会议/2026-09-22-示例周会',
      file: 'seg_0002.wav',
      path: '会议音频/会议/2026-09-22-示例周会/seg_0002.wav',
      seq: 2,
      bytes: 4032000,
      durationMs: 126000,
      sampleRate: 16000,
      modifiedAt: Math.floor(Date.now() / 1000) - 600,
    },
  ]

  private progress: ((p: ProcessProgress) => void) | null = null

  async list() {
    return this.notes.map((n) => n.brief)
  }
  async create(title: string, date: string) {
    const noteId = `会议/${date}-${(title.trim() || '未命名会议').replace(/[\\/:*?"<>|]/g, '-')}.md`
    const body = [
      `# ${title.trim() || '未命名会议'}`,
      '',
      `> ${date} 会议记录：随手写下要点；用 \`/v 音频文件名\` 引用录音。`,
      '',
      '## 会议纪要（AI 整理）',
      '',
      '_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_',
      '',
    ].join('\n')
    this.notes.unshift({
      brief: {
        noteId,
        title: title.trim() || '未命名会议',
        folder: '会议',
        updatedAt: Math.floor(Date.now() / 1000),
        audioTotal: 0,
        audioMissing: 0,
        transcribed: 0,
        hasMinutes: false,
        durationMs: 0,
        audioBytes: 0,
      },
      body,
    })
    return noteId
  }
  async refs(noteId: string) {
    const note = this.notes.find((n) => n.brief.noteId === noteId)
    if (!note) return []
    return this.clips
      .filter((c) => note.body.includes(c.path))
      .map<AudioRef>((c) => ({
        line: note.body.split('\n').findIndex((l) => l.includes(c.path)) + 1,
        token: '/v',
        raw: c.path,
        path: c.path,
        fileName: c.file,
        exists: true,
        bytes: c.bytes,
        durationMs: c.durationMs,
        hasTranscript: note.body.includes(c.file) && note.body.includes('🎙 转写'),
        transcript: '',
      }))
  }
  async process(noteId: string, force: boolean): Promise<ProcessOutcome> {
    void force
    const note = this.notes.find((n) => n.brief.noteId === noteId)
    const total = note ? this.clips.filter((c) => note.body.includes(c.path)).length : 0
    const emit = (p: Partial<ProcessProgress>) =>
      this.progress?.({
        noteId,
        phase: 'parse',
        audioTotal: total,
        audioDone: 0,
        audioSkipped: 0,
        audioFailed: 0,
        current: '',
        message: '',
        ...p,
      } as ProcessProgress)
    emit({ phase: 'parse', message: `找到 ${total} 处音频引用（预览模式不会真的转写）` })
    for (let i = 0; i < total; i++) {
      await new Promise((r) => setTimeout(r, 300))
      emit({ phase: 'transcribe', audioSkipped: i + 1, message: `复用缓存 ${i + 1}/${total}（预览）` })
    }
    emit({ phase: 'minutes', message: '生成会议纪要…（预览模式）' })
    await new Promise((r) => setTimeout(r, 400))
    const minutes = [
      '# 示例周会',
      '',
      '## 一、会议摘要',
      '',
      '预览模式生成的示意纪要：真实纪要由对话模型依据「手写记录 + 录音转写」整理。',
      '',
      '## 二、决议事项',
      '',
      '1. 更换镜像站解决拉取超时。',
      '2. 下周补充部署文档。',
      '',
    ].join('\n')
    if (note) {
      note.body = `${note.body.split('## 会议纪要（AI 整理）')[0]}## 会议纪要（AI 整理）\n\n${minutes}`
      note.brief.hasMinutes = true
    }
    emit({ phase: 'done', message: '处理结束（预览模式）' })
    return {
      noteId,
      audioTotal: total,
      audioDone: 0,
      audioSkipped: total,
      audioFailed: 0,
      transcriptChars: 40,
      minutesChars: minutes.length,
      model: '预览模式',
      elapsedMs: 1200,
      cancelled: false,
      errors: [],
    }
  }
  async cancel() {
    /* 预览模式无需取消 */
  }
  async onProgress(cb: (p: ProcessProgress) => void) {
    this.progress = cb
    return () => {
      this.progress = null
    }
  }
  async migrateLegacy() {
    return []
  }
  async read(noteId: string) {
    return this.notes.find((n) => n.brief.noteId === noteId)?.body ?? ''
  }
  async write(noteId: string, content: string) {
    const note = this.notes.find((n) => n.brief.noteId === noteId)
    if (note) note.body = content
  }
  async clipStart(noteId: string, sampleRate: number) {
    const dir = mockDirFor(noteId)
    const dirs = [dir, mockLegacyDirFor(noteId)]
    const seq = this.clips.filter((c) => dirs.includes(c.dir)).reduce((m, c) => Math.max(m, c.seq), 0) + 1
    const clip: ClipInfo = {
      dir,
      file: `seg_${String(seq).padStart(4, '0')}.wav`,
      path: `会议音频/${dir}/seg_${String(seq).padStart(4, '0')}.wav`,
      seq,
      bytes: 44,
      durationMs: 0,
      sampleRate,
      modifiedAt: Math.floor(Date.now() / 1000),
    }
    this.clips.push(clip)
    return { ...clip, refLine: `/v ${clip.path}` } as ClipStat
  }
  async clipAppend(noteId: string, seq: number) {
    void noteId
    const clip = this.clips.find((c) => c.seq === seq)
    if (!clip) throw new Error('录音分段不存在（预览模式）')
    clip.durationMs += 1000
    return { ...clip, refLine: `/v ${clip.path}` } as ClipStat
  }
  async clipClose(noteId: string, seq: number) {
    return this.clipAppend(noteId, seq)
  }
  async clipDiscard(path: string) {
    this.clips = this.clips.filter((c) => c.path !== path)
  }
  async clipList(noteId?: string) {
    if (!noteId) return this.clips
    const dirs = [mockDirFor(noteId), mockLegacyDirFor(noteId)]
    return this.clips.filter((c) => dirs.includes(c.dir))
  }
  async audioSrc() {
    return null
  }
}

/** 预览模式：镜像 Rust 的音频目录规则（`会议/周会.md` → `会议/周会`）。 */
function mockDirFor(noteId: string): string {
  return noteId.trim().replace(/^\/+/, '').replace(/\.md$/, '')
}

/** 预览模式：旧版扁平目录（兼容展示历史录音）。 */
function mockLegacyDirFor(noteId: string): string {
  return mockDirFor(noteId).replace(/^会议\//, '').replace(/[\\/]/g, '_')
}

let adapter: MeetingNoteAdapter | null = null

/** Tauri 壳里走 Rust；浏览器预览里自动落到内存实现。 */
export function meetingNoteAdapter(): MeetingNoteAdapter {
  if (!adapter) {
    adapter =
      '__TAURI_INTERNALS__' in window ? new TauriMeetingNoteAdapter() : new MockMeetingNoteAdapter()
  }
  return adapter
}

/** 测试用：替换适配器。 */
export function setMeetingNoteAdapter(next: MeetingNoteAdapter | null) {
  adapter = next
}
