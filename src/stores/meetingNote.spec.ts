import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

/** 录音器替身：测试里手动触发采集回调，不依赖真实麦克风。 */
const rec = vi.hoisted(() => ({
  chunk: null as ((pcm: Int16Array) => void) | null,
  level: null as ((rms: number) => void) | null,
}))

vi.mock('../core/recorder', () => {
  class MicRecorder {
    info = { sampleRate: 16000, channelCount: 1, bitsPerSample: 16 }
    recording = false
    static async selfCheck() {
      return ['✅ mock 自检']
    }
    async start(cb: { onChunk: (pcm: Int16Array) => void; onLevel: (rms: number) => void }) {
      rec.chunk = cb.onChunk
      rec.level = cb.onLevel
      this.recording = true
      return this.info
    }
    async stop() {
      this.recording = false
    }
  }
  return {
    MicRecorder,
    pcmToBase64: (pcm: Int16Array) => `b64:${pcm.length}`,
    pcmToWavUrl: () => 'blob:mock',
  }
})

import { setMeetingNoteAdapter, type MeetingNoteAdapter } from '../platform/meetingNote'
import { useMeetingNoteStore } from './meetingNote'
import { useNotesStore } from './notes'
import type { AudioRef, ClipInfo, ClipStat, ProcessProgress } from '../core/meetingNote'

const NOTE_ID = '会议/2026-09-22 周会.md'
const NOTE_BODY = [
  '# 周会',
  '',
  '手写一句。',
  '',
  '## 会议纪要（AI 整理）',
  '',
  '_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_',
  '',
].join('\n')

/** 内存适配器：记录调用，供断言。 */
function fakeAdapter() {
  const notes = new Map<string, string>([[NOTE_ID, NOTE_BODY]])
  const clips: ClipInfo[] = []
  const stat = (c: ClipInfo): ClipStat => ({ ...c, refLine: `/v ${c.path}` })
  const calls = { closed: [] as number[], appended: 0, discarded: [] as number[], processed: [] as string[] }
  let progressCb: ((p: ProcessProgress) => void) | null = null

  const adapter: MeetingNoteAdapter = {
    list: async () =>
      [...notes.keys()].map((id) => ({
        noteId: id,
        title: id.replace(/^会议\//, '').replace(/\.md$/, ''),
        folder: '会议',
        updatedAt: 1,
        audioTotal: id === NOTE_ID ? 1 : 0,
        audioMissing: 0,
        transcribed: 0,
        hasMinutes: false,
        durationMs: 0,
        audioBytes: 0,
      })),
    create: async (title, date) => {
      const id = `会议/${date}-${title}.md`
      notes.set(id, `# ${title}\n\n## 会议纪要（AI 整理）\n`)
      return id
    },
    refs: async (noteId): Promise<AudioRef[]> =>
      noteId === NOTE_ID
        ? [
            {
              line: 4,
              token: '/v',
              raw: '会议音频/2026-09-22 周会/seg_0001.wav',
              path: '会议音频/2026-09-22 周会/seg_0001.wav',
              fileName: 'seg_0001.wav',
              exists: true,
              bytes: 32000,
              durationMs: 1000,
              hasTranscript: false,
              transcript: '',
            },
          ]
        : [],
    process: async (noteId) => {
      calls.processed.push(noteId)
      progressCb?.({
        noteId,
        phase: 'transcribe',
        audioTotal: 2,
        audioDone: 1,
        audioSkipped: 0,
        audioFailed: 0,
        current: 'seg_0001.wav',
        message: '转写中 1/2',
      })
      return {
        noteId,
        audioTotal: 2,
        audioDone: 1,
        audioSkipped: 1,
        audioFailed: 0,
        transcriptChars: 100,
        minutesChars: 200,
        model: 'mock-model',
        elapsedMs: 5000,
        cancelled: false,
        errors: [],
      }
    },
    cancel: async () => undefined,
    onProgress: async (cb) => {
      progressCb = cb
      return () => {
        progressCb = null
      }
    },
    migrateLegacy: async () => ['会议/旧会议/2026-09-01-老周会.md'],
    read: async (id) => notes.get(id) ?? '',
    write: async (id, content) => {
      notes.set(id, content)
    },
    clipStart: async (_noteId, sampleRate) => {
      const seq = clips.length + 1
      const clip: ClipInfo = {
        dir: '2026-09-22 周会',
        file: `seg_${String(seq).padStart(4, '0')}.wav`,
        path: `会议音频/2026-09-22 周会/seg_${String(seq).padStart(4, '0')}.wav`,
        seq,
        bytes: 44,
        durationMs: 0,
        sampleRate,
        modifiedAt: 1,
      }
      clips.push(clip)
      return stat(clip)
    },
    clipAppend: async (_noteId, seq) => {
      calls.appended += 1
      const clip = clips.find((c) => c.seq === seq)!
      clip.bytes += 32000
      clip.durationMs += 1000
      return stat(clip)
    },
    clipClose: async (_noteId, seq) => {
      calls.closed.push(seq)
      return stat(clips.find((c) => c.seq === seq)!)
    },
    clipDiscard: async (_noteId, seq) => {
      calls.discarded.push(seq)
    },
    clipList: async () => clips,
    audioSrc: async (p) => `asset://${p}`,
  }
  return { adapter, notes, clips, calls }
}

let fake: ReturnType<typeof fakeAdapter>

beforeEach(() => {
  setActivePinia(createPinia())
  fake = fakeAdapter()
  setMeetingNoteAdapter(fake.adapter)
})

describe('会议笔记 store（列表 / 处理）', () => {
  it('加载列表并选中第一篇', async () => {
    const store = useMeetingNoteStore()
    await store.loadList()
    expect(store.notes.map((n) => n.noteId)).toEqual([NOTE_ID])
    await store.openNote(NOTE_ID)
    expect(store.refs).toHaveLength(1)
    expect(store.current?.title).toBe('2026-09-22 周会')
  })

  it('新建会议：创建 md、刷新列表并设为录音目标', async () => {
    const store = useMeetingNoteStore()
    const id = await store.create('周会')
    expect(id).toMatch(/^会议\/\d{4}-\d{2}-\d{2}-周会\.md$/)
    expect(store.currentId).toBe(id)
    expect(store.targetNoteId).toBe(id)
    expect(store.notes.some((n) => n.noteId === id)).toBe(true)
  })

  it('一键处理：进度事件映射到百分比，完成后刷新笔记与列表', async () => {
    const store = useMeetingNoteStore()
    await store.loadList()
    await store.openNote(NOTE_ID)
    const out = await store.process(false)
    expect(out?.minutesChars).toBe(200)
    expect(store.progress).toBe(100)
    expect(store.message).toContain('mock-model')
    expect(store.outcome?.audioDone).toBe(1)
    expect(fake.calls.processed).toEqual([NOTE_ID])
  })

  it('迁移旧会议：返回导出的笔记 id', async () => {
    const store = useMeetingNoteStore()
    const ids = await store.migrateLegacy()
    expect(ids).toEqual(['会议/旧会议/2026-09-01-老周会.md'])
  })
})

describe('会议笔记 store（引用插入）', () => {
  it('笔记没打开时：把引用追加到纪要段之前并落盘', async () => {
    const store = useMeetingNoteStore()
    const where = await store.insertRef('/v 会议音频/周会/seg_0002.wav', NOTE_ID)
    expect(where).toBe('end')
    const body = fake.notes.get(NOTE_ID)!
    expect(body).toContain('/v 会议音频/周会/seg_0002.wav')
    expect(body.indexOf('/v 会议音频')).toBeLessThan(body.indexOf('## 会议纪要（AI 整理）'))
  })

  it('笔记正打开时：交给编辑器插到光标处，不覆盖未保存内容', async () => {
    const store = useMeetingNoteStore()
    const notes = useNotesStore()
    notes.currentId = NOTE_ID
    notes.setContent('# 周会\n\n我还没保存的草稿\n')
    const where = await store.insertRef('/v 会议音频/周会/seg_0003.wav', NOTE_ID)
    expect(where).toBe('cursor')
    expect(store.pendingRef).toEqual({ noteId: NOTE_ID, text: '/v 会议音频/周会/seg_0003.wav' })
    expect(fake.notes.get(NOTE_ID)).toBe(NOTE_BODY)
    expect(notes.content).toContain('草稿')
    store.clearPendingRef()
    expect(store.pendingRef).toBeNull()
  })
})

describe('会议笔记 store（录音面板）', () => {
  it('开始 → 落盘 → 停止：自动收尾分段并插入引用', async () => {
    const store = useMeetingNoteStore()
    await store.loadList()
    await store.openNote(NOTE_ID)
    store.setTarget(NOTE_ID)
    await store.toggleRecorder(true)
    expect(store.recorderOpen).toBe(true)

    await store.startRecording()
    expect(store.recording).toBe(true)
    expect(store.recordSeq).toBe(1)

    // 模拟 1 秒采集（触发一次落盘）+ 音量回调
    rec.level?.(1234)
    expect(store.levelPercent).toBeGreaterThan(0)
    rec.chunk?.(new Int16Array(16000))
    await store.pushPcm(new Int16Array(16000))
    await store.flushBuffer(true)
    expect(fake.calls.appended).toBeGreaterThan(0)

    await store.stopRecording()
    expect(store.recording).toBe(false)
    expect(fake.calls.closed).toEqual([1])
    expect(store.recentClips).toHaveLength(1)
    expect(store.insertHint).toContain('引用')
    expect(fake.notes.get(NOTE_ID)!).toContain('seg_0001.wav')
  })

  it('丢弃录坏的片段', async () => {
    const store = useMeetingNoteStore()
    await store.loadList()
    await store.openNote(NOTE_ID)
    store.setTarget(NOTE_ID)
    await store.startRecording()
    await store.stopRecording()
    const clip = store.recentClips[0]
    await store.discardClip(clip)
    expect(fake.calls.discarded).toEqual([1])
    expect(store.recentClips).toHaveLength(0)
  })

  it('音频播放地址走适配器', async () => {
    const store = useMeetingNoteStore()
    expect(await store.clipSrc('会议音频/周会/seg_0001.wav')).toBe(
      'asset://会议音频/周会/seg_0001.wav',
    )
  })
})
