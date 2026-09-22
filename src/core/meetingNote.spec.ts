import { describe, expect, it } from 'vitest'
import MarkdownIt from 'markdown-it'
import {
  AUDIO_DIR,
  MINUTES_HEADING,
  MINUTES_HINT,
  appendRefLine,
  clipCountsByNote,
  dirForNote,
  dirForNoteLegacy,
  dirsForNote,
  formatBytes,
  formatDur,
  insertRefAtCursor,
  minutesReady,
  outcomeSummary,
  parseRefLine,
  parseRefs,
  prepareAudioRefs,
  progressPercent,
  replaceAudioPlaceholders,
  slashCommandAt,
  splitMinutes,
  unreferencedClips,
  type ClipInfo,
  type ProcessOutcome,
  type ProcessProgress,
} from './meetingNote'

describe('parseRefLine / parseRefs', () => {
  it('识别 /v 与 /video，且必须行首', () => {
    expect(parseRefLine('/v a.wav')).toEqual({ token: '/v', raw: 'a.wav' })
    expect(parseRefLine('  /video 会议音频/周会/seg_0001.wav ')).toEqual({
      token: '/video',
      raw: '会议音频/周会/seg_0001.wav',
    })
    expect(parseRefLine('/v')).toBeNull()
    expect(parseRefLine('/very good')).toBeNull()
    expect(parseRefLine('可以看 /v a.wav')).toBeNull()
    expect(parseRefLine('/v `a.wav`')).toEqual({ token: '/v', raw: 'a.wav' })
  })

  it('按行号收集引用', () => {
    const refs = parseRefs('# 会\n\n手写\n/v a.wav\n补充\n/video b.wav\n')
    expect(refs.map((r) => [r.line, r.raw])).toEqual([
      [4, 'a.wav'],
      [6, 'b.wav'],
    ])
  })
})

describe('splitMinutes / minutesReady', () => {
  const body = ['# 会', '', '手写', '', MINUTES_HEADING, '', '旧纪要', ''].join('\n')

  it('切出正文与纪要段', () => {
    const { head, minutes } = splitMinutes(body)
    expect(head).toBe('# 会\n\n手写')
    expect(minutes).toContain('旧纪要')
    expect(minutesReady(minutes)).toBe(true)
  })

  it('没有纪要段时 head 为全文', () => {
    const { head, minutes } = splitMinutes('只有正文\n')
    expect(head).toBe('只有正文')
    expect(minutes).toBeNull()
    expect(minutesReady(null)).toBe(false)
  })

  it('占位提示不算已生成', () => {
    expect(minutesReady(`${MINUTES_HEADING}\n\n${MINUTES_HINT}`)).toBe(false)
  })

  it('取最后一次出现的分界标题（正文里提到标题也不怕）', () => {
    const tricky = `# 会\n\n我说：${MINUTES_HEADING} 是程序生成的\n\n${MINUTES_HEADING}\n\n真纪要\n`
    const { head, minutes } = splitMinutes(tricky)
    expect(head).toContain('是程序生成的')
    expect(minutes).toContain('真纪要')
  })
})

describe('appendRefLine / insertRefAtCursor', () => {
  it('引用插到纪要段之前', () => {
    const out = appendRefLine(`# 会\n\n手写\n\n${MINUTES_HEADING}\n\n纪要\n`, '/v 会议音频/周会/seg_0001.wav')
    expect(out.indexOf('/v 会议音频')).toBeLessThan(out.indexOf(MINUTES_HEADING))
    expect(out).toContain('手写')
    expect(out.trimEnd().endsWith('纪要')).toBe(true)
  })

  it('没有纪要段时追加到末尾', () => {
    const out = appendRefLine('# 会\n\n手写\n', '/v a.wav')
    expect(out).toBe('# 会\n\n手写\n\n/v a.wav\n')
  })

  it('光标在正文时插到光标处', () => {
    const body = '# 会\n\n第一行\n第二行\n'
    const res = insertRefAtCursor(body, '/v a.wav', '# 会\n\n第一行\n'.length)
    expect(res.body).toBe('# 会\n\n第一行\n/v a.wav\n第二行\n')
    expect(res.body.slice(0, res.cursor).endsWith('/v a.wav\n')).toBe(true)
  })

  it('光标落在纪要段里时退回正文末尾', () => {
    const body = `# 会\n\n手写\n\n${MINUTES_HEADING}\n\n纪要\n`
    const res = insertRefAtCursor(body, '/v a.wav', body.length)
    expect(res.body.indexOf('/v a.wav')).toBeLessThan(res.body.indexOf(MINUTES_HEADING))
  })
})

describe('slashCommandAt（编辑器 `/v` 选择器）', () => {
  it('识别行首的 /v 与已输入的部分路径', () => {
    const text = '# 会\n\n手写\n/v 会议音频/周'
    const hit = slashCommandAt(text, text.length)
    expect(hit).not.toBeNull()
    expect(text.slice(hit!.start, text.length)).toBe('/v 会议音频/周')
    expect(hit!.token).toBe('/v')
    expect(hit!.filter).toBe('会议音频/周')
  })

  it('支持 /video 与缩进行（缩进一并被替换）', () => {
    const text = '  /video seg_0001'
    const hit = slashCommandAt(text, text.length)
    expect(hit!.token).toBe('/video')
    expect(hit!.start).toBe(0)
    expect(hit!.filter).toBe('seg_0001')
  })

  it('不是行首命令就不弹（/very good、正文里的 /v）', () => {
    expect(slashCommandAt('/very good', 6)).toBeNull()
    expect(slashCommandAt('请参考 /v a.wav', 10)).toBeNull()
    expect(slashCommandAt('/v 路径\n下一行', 12)).toBeNull()
  })

  it('配合 insertRefAtCursor：替换输入的 token 并落到光标处', () => {
    const text = '# 会\n\n手写\n/v 会议音频/周'
    const hit = slashCommandAt(text, text.length)!
    const cursor = text.length
    const cleaned = text.slice(0, hit.start) + text.slice(cursor)
    const res = insertRefAtCursor(cleaned, '/v 会议音频/周会/seg_0001.wav', hit.start)
    expect(res.body).toBe('# 会\n\n手写\n/v 会议音频/周会/seg_0001.wav\n')
    expect(res.body.slice(0, res.cursor)).toContain('seg_0001.wav')
  })
})

describe('预览渲染（prepareAudioRefs + replaceAudioPlaceholders）', () => {
  // 用真实的 markdown-it 跑一遍：验证占位符不会被转义 / 拆块
  const md = new MarkdownIt({ html: false, linkify: true })

  it('引用行渲染成播放器，转写块保持引用块样式', () => {
    const body = [
      '# 会',
      '',
      '手写一句。',
      '/v 会议音频/周会/seg_0001.wav',
      '> 🎙 转写 00:00:00–00:01:00 · seg_0001.wav',
      '>',
      '> 甲：内容',
      '',
      MINUTES_HEADING,
      '',
      '纪要正文',
      '',
    ].join('\n')
    const html = replaceAudioPlaceholders(
      md.render(prepareAudioRefs(body)),
      (raw) => `<p class="audio-ref"><audio src="asset://${raw}"></audio></p>`,
    )
    expect(html).toContain('<audio src="asset://会议音频/周会/seg_0001.wav">')
    expect(html).toContain('<blockquote>')
    expect(html).toContain('甲：内容')
    expect(html).toContain('手写一句。')
    expect(html).toContain('纪要正文')
    expect(html).not.toContain('§§AUDIO')
    expect(html).not.toContain('&lt;audio')
  })

  it('没有引用时原样渲染', () => {
    const html = md.render(prepareAudioRefs('# 标题\n\n正文\n'))
    expect(html).toContain('<h1>标题</h1>')
  })
})

describe('dirForNote / 展示辅助', () => {
  it('笔记 id 映射成音频目录（与 Rust 对齐：与笔记路径一一对应）', () => {
    expect(dirForNote('会议/2026-09-22 周会.md')).toBe('会议/2026-09-22 周会')
    expect(dirForNote('会议/子目录/周会.md')).toBe('会议/子目录/周会')
    expect(dirForNote('随手记.md')).toBe('随手记')
    expect(dirForNote('.md')).toBe('未命名')
    expect(AUDIO_DIR).toBe('会议音频')
  })

  it('旧版目录名保留（兼容 0.1.x 已录的音频）', () => {
    expect(dirForNoteLegacy('会议/2026-09-22 周会.md')).toBe('2026-09-22 周会')
    expect(dirForNoteLegacy('会议/子目录/周会.md')).toBe('子目录_周会')
    expect(dirsForNote('会议/周会.md')).toEqual(['会议/周会', '周会'])
    expect(dirsForNote('周会.md')).toEqual(['周会'])
  })

  it('未引用录音与列表徽章（新旧目录都算）', () => {
    const mk = (dir: string, seq: number): ClipInfo => ({
      dir,
      file: `seg_${String(seq).padStart(4, '0')}.wav`,
      path: `会议音频/${dir}/seg_${String(seq).padStart(4, '0')}.wav`,
      seq,
      bytes: 100,
      durationMs: 1000,
      sampleRate: 16000,
      modifiedAt: seq,
    })
    const clips = [mk('会议/周会', 1), mk('会议/周会', 2), mk('周会', 1), mk('其它', 1)]
    const unref = unreferencedClips(clips, [{ path: '会议音频/会议/周会/seg_0001.wav' }])
    expect(unref.map((c) => c.path)).toEqual([
      '会议音频/会议/周会/seg_0002.wav',
      '会议音频/周会/seg_0001.wav',
      '会议音频/其它/seg_0001.wav',
    ])
    expect(clipCountsByNote([{ id: '会议/周会.md' }, { id: '其它.md' }], clips)).toEqual({
      '会议/周会.md': 3,
      '其它.md': 1,
    })
  })

  it('时长与体积格式化', () => {
    expect(formatDur(0)).toBe('0:00')
    expect(formatDur(65_000)).toBe('1:05')
    expect(formatDur(3_725_000)).toBe('1:02:05')
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(2048)).toBe('2 KB')
    expect(formatBytes(5 * 1024 * 1024)).toBe('5 MB')
  })
})

describe('progressPercent / outcomeSummary', () => {
  const base: ProcessProgress = {
    noteId: '会议/a.md',
    phase: 'parse',
    audioTotal: 4,
    audioDone: 0,
    audioSkipped: 0,
    audioFailed: 0,
    current: '',
    message: '',
  }

  it('按阶段与进度推进，单调不减', () => {
    expect(progressPercent({ ...base, phase: 'parse' })).toBe(2)
    const mid = progressPercent({ ...base, phase: 'transcribe', audioDone: 2, audioSkipped: 1 })
    const end = progressPercent({ ...base, phase: 'transcribe', audioSkipped: 4 })
    expect(mid).toBeGreaterThan(10)
    expect(end).toBe(85)
    expect(progressPercent({ ...base, phase: 'minutes' })).toBe(96)
    expect(progressPercent({ ...base, phase: 'done' })).toBe(100)
    expect(end).toBeLessThan(progressPercent({ ...base, phase: 'write' }))
  })

  it('没有音频引用时 parse 阶段也不为 0', () => {
    expect(progressPercent({ ...base, audioTotal: 0 })).toBe(5)
  })

  it('结果总结带复用/失败与纪要信息', () => {
    const out: ProcessOutcome = {
      noteId: '会议/a.md',
      audioTotal: 3,
      audioDone: 1,
      audioSkipped: 1,
      audioFailed: 1,
      transcriptChars: 120,
      minutesChars: 300,
      model: 'Qwen-Inno-35B-v1',
      elapsedMs: 20_400,
      cancelled: false,
      errors: [],
    }
    const text = outcomeSummary(out)
    expect(text).toContain('转写 1 段')
    expect(text).toContain('复用缓存 1 段')
    expect(text).toContain('失败 1 段')
    expect(text).toContain('Qwen-Inno-35B-v1')
    expect(text).toContain('20.4s')
    expect(outcomeSummary({ ...out, cancelled: true })).toContain('已取消')
  })
})
