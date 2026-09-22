import { describe, expect, it } from 'vitest'
import {
  applyPreset,
  appendBlocks,
  appendParagraph,
  blockAtLine,
  detectKind,
  dropBlockTidy,
  insertBlockAfter,
  insertBlocksBefore,
  insertParagraphAfter,
  insertParagraphAtIndex,
  joinBlocks,
  replaceBlock,
  splitBlocks,
} from './blocks'

/** 会议笔记样例：包含 /v 引用、转写块、纪要标题等「必须原样保留」的结构。 */
const MEETING = [
  '# 周会',
  '',
  '讨论了镜像拉取超时的问题，先用镜像站顶一下。',
  '/v 会议音频/会议/周会/seg_0001.wav',
  '/v 会议音频/会议/周会/seg_0002.wav',
  '',
  '> 🎙 转写（qwen3-asr-1.7b）',
  '> 第一段：镜像拉取太慢。',
  '> 第二段：改用镜像站。',
  '',
  '## 会议纪要（AI 整理）',
  '',
  '- 结论：改用镜像站',
  '- 待办：写部署文档',
  '',
].join('\n')

const SAMPLES = [
  '',
  'a',
  'a\n',
  'a\n\nb',
  'a\nb\n\nc\n',
  '\n\n',
  'a\n\n\n\nb\n',
  MEETING,
  '```ts\nconst a = 1\n\nconst b = 2\n```\n\n后一段\n',
  '~~~\n不管里面是什么 # 都不是标题\n~~~\n',
  '| a | b |\n| - | - |\n| 1 | 2 |\n',
  '没有收尾的代码围栏\n```\nstill code\n',
]

describe('splitBlocks / joinBlocks', () => {
  it.each(SAMPLES.map((s) => [s.slice(0, 24).replace(/\n/g, '⏎') || '(空)', s] as const))(
    '切块后能逐字节还原：%s',
    (_label, md) => {
      expect(joinBlocks(splitBlocks(md))).toBe(md)
    },
  )

  it('行号连续且覆盖全文', () => {
    const blocks = splitBlocks(MEETING)
    const total = MEETING.split('\n').length
    expect(blocks[blocks.length - 1].endLine).toBe(total)
    for (let i = 1; i < blocks.length; i++) {
      expect(blocks[i].startLine).toBe(blocks[i - 1].endLine + 1)
    }
  })

  it('空行与代码围栏各自成块，引用块整体成块', () => {
    const blocks = splitBlocks(MEETING)
    expect(blocks.filter((b) => b.gap).length).toBeGreaterThan(0)
    const quote = blocks.find((b) => b.text.startsWith('> 🎙'))
    expect(quote?.text.split('\n')).toHaveLength(3)
    // `/v` 引用行各自成块
    expect(blocks.filter((b) => b.text.startsWith('/v '))).toHaveLength(2)
    const fences = splitBlocks('```\na\n\nb\n```')
    expect(fences.filter((b) => b.fence)).toHaveLength(1)
    expect(fences.find((b) => b.fence)?.text).toBe('```\na\n\nb\n```')
  })
})

describe('replaceBlock', () => {
  it('只改目标块，其它块逐字节不动（会议结构安全）', () => {
    const blocks = splitBlocks(MEETING)
    const target = blocks.find((b) => b.text.startsWith('讨论了镜像'))
    expect(target).toBeTruthy()
    const next = replaceBlock(blocks, target!.id, '镜像问题已解决，改用镜像站。')
    const out = joinBlocks(next)
    expect(out).toContain('/v 会议音频/会议/周会/seg_0001.wav')
    expect(out).toContain('> 🎙 转写（qwen3-asr-1.7b）')
    expect(out).toContain('## 会议纪要（AI 整理）')
    expect(out).toContain('镜像问题已解决，改用镜像站。')
    expect(out).not.toContain('先用镜像站顶一下')

    // 行号重算正确
    const updated = next.find((b) => b.text.includes('镜像问题已解决'))
    expect(updated?.startLine).toBe(3)
  })

  it('新文本里的空行会切成多块', () => {
    const blocks = splitBlocks('a\n\nb')
    const next = replaceBlock(blocks, 0, 'a1\n\na2')
    expect(joinBlocks(next)).toBe('a1\n\na2\n\nb')
  })

  it('改成空文本 = 清空该块', () => {
    const blocks = splitBlocks('a\n\nb')
    const next = replaceBlock(blocks, 0, '')
    expect(joinBlocks(next)).toBe('\n\nb')
  })

  it('编辑某一块不会改其它块的 id（编辑句柄稳定）', () => {
    const blocks = splitBlocks('a\n\nb\n\nc')
    const idOf = (text: string) => blocks.find((b) => b.text === text)!.id
    const next = replaceBlock(blocks, idOf('b'), 'b1\n\nb2')
    expect(next.find((b) => b.text === 'a')?.id).toBe(idOf('a'))
    expect(next.find((b) => b.text === 'c')?.id).toBe(idOf('c'))
    const ids = next.map((b) => b.id)
    expect(new Set(ids).size).toBe(ids.length)
  })
})

describe('appendBlocks / insertBlockAfter', () => {
  it('文末追加会自动补空行', () => {
    const blocks = splitBlocks('# 标题\n正文')
    const out = joinBlocks(appendBlocks(blocks, '/v a.wav'))
    expect(out).toBe('# 标题\n正文\n\n/v a.wav')
  })

  it('已有空行时不重复补', () => {
    const out = joinBlocks(appendBlocks(splitBlocks('正文\n'), '后续'))
    expect(out).toBe('正文\n\n后续')
  })

  it('在指定块后插入', () => {
    const blocks = splitBlocks('a\n\nb')
    const out = joinBlocks(insertBlockAfter(blocks, 0, '新增'))
    expect(out).toBe('a\n\n新增\n\nb')
  })
})

describe('空段落', () => {
  it('文末新起一段会补空行', () => {
    const { blocks: next, id } = appendParagraph(splitBlocks('a'))
    expect(joinBlocks(next)).toBe('a\n\n')
    const created = next.find((b) => b.id === id)
    expect(created?.gap).toBe(false)
    expect(created?.text).toBe('')
  })

  it('空文档也能新起一段', () => {
    const { blocks: next, id } = appendParagraph(splitBlocks(''))
    expect(next.find((b) => b.id === id)?.gap).toBe(false)
    expect(next.some((b) => b.gap)).toBe(true)
  })

  it('在某段后新起一段（跨过原有空行）', () => {
    const blocks = splitBlocks('a\n\nb')
    const { blocks: next, id } = insertParagraphAfter(blocks, blocks[0].id)
    expect(joinBlocks(next)).toBe('a\n\n\n\nb')
    expect(next.find((b) => b.id === id)?.gap).toBe(false)
  })

  it('可直接带内容插入（回车拆分段落）', () => {
    const blocks = splitBlocks('前半后半')
    const { blocks: next, id } = insertParagraphAfter(blocks, blocks[0].id, '后半')
    expect(joinBlocks(next)).toBe('前半后半\n\n后半')
    expect(next.find((b) => b.id === id)?.text).toBe('后半')
  })

  it('insertParagraphAtIndex 支持 index = -1（插到最前）', () => {
    const blocks = splitBlocks('a')
    const { blocks: next } = insertParagraphAtIndex(blocks, -1, '最前')
    expect(joinBlocks(next)).toBe('最前\n\na')
  })
})

describe('insertBlocksBefore（引用插到纪要标题之前）', () => {
  const doc = ['正文', '', '## 会议纪要（AI 整理）', '', '- 结论'].join('\n')

  it('插在标题前，前后各有空行', () => {
    const blocks = splitBlocks(doc)
    const idx = blocks.findIndex((b) => b.text.startsWith('## 会议纪要'))
    const out = joinBlocks(insertBlocksBefore(blocks, idx, '/v a.wav\n/v b.wav'))
    expect(out).toBe(
      ['正文', '', '/v a.wav', '/v b.wav', '', '## 会议纪要（AI 整理）', '', '- 结论'].join('\n'),
    )
    // 纪要段本身没被拆碎
    const next = insertBlocksBefore(blocks, idx, '/v a.wav')
    expect(joinBlocks(next)).toContain('## 会议纪要（AI 整理）\n\n- 结论')
  })

  it('标题在文首时也能插', () => {
    const blocks = splitBlocks('## 会议纪要（AI 整理）\n\n- 结论')
    const out = joinBlocks(insertBlocksBefore(blocks, 0, '/v a.wav'))
    expect(out).toBe('/v a.wav\n\n## 会议纪要（AI 整理）\n\n- 结论')
  })
})

describe('dropBlockTidy（删 /v 引用行）', () => {
  it('两侧各一个空行并成一个', () => {
    const blocks = splitBlocks('正文\n\n/v 会议音频/a.wav\n\n后文')
    const ref = blocks.find((b) => b.text.startsWith('/v'))!
    expect(joinBlocks(dropBlockTidy(blocks, ref.id))).toBe('正文\n\n后文')
  })

  it('引用在文首时不留开头的空行', () => {
    const blocks = splitBlocks('/v 会议音频/a.wav\n\n后文')
    const ref = blocks.find((b) => b.text.startsWith('/v'))!
    expect(joinBlocks(dropBlockTidy(blocks, ref.id))).toBe('后文')
  })

  it('引用在文末时保留结尾结构', () => {
    const blocks = splitBlocks('正文\n\n/v 会议音频/a.wav')
    const ref = blocks.find((b) => b.text.startsWith('/v'))!
    // 原文件行是「正文 / 空行 / 引用行」，删掉引用行后就是「正文 / 空行」→ `正文\n`
    expect(joinBlocks(dropBlockTidy(blocks, ref.id))).toBe('正文\n')
  })

  it('用户自己留的两个空行不会被吃掉', () => {
    const blocks = splitBlocks('正文\n\n\n/v 会议音频/a.wav\n后文')
    const ref = blocks.find((b) => b.text.startsWith('/v'))!
    expect(joinBlocks(dropBlockTidy(blocks, ref.id))).toBe('正文\n\n\n后文')
  })

  it('连着两个引用删一个，另一个与正文都不动', () => {
    const blocks = splitBlocks('正文\n\n/v a.wav\n/v b.wav\n\n后文')
    const refs = blocks.filter((b) => b.text.startsWith('/v'))
    const out = joinBlocks(dropBlockTidy(blocks, refs[0].id))
    expect(out).toBe('正文\n\n/v b.wav\n\n后文')
  })

  it('只有一行引用时删完是空文档', () => {
    const blocks = splitBlocks('/v a.wav')
    expect(joinBlocks(dropBlockTidy(blocks, blocks[0].id))).toBe('')
  })

  it('只删这一块，别的块逐字节不动', () => {
    const md = '会议\n\n/v a.wav\n\n## 会议纪要（AI 整理）\n\n- 结论'
    const blocks = splitBlocks(md)
    const ref = blocks.find((b) => b.text.startsWith('/v'))!
    const out = joinBlocks(dropBlockTidy(blocks, ref.id))
    expect(out).toContain('## 会议纪要（AI 整理）\n\n- 结论')
    expect(out).not.toContain('/v a.wav')
  })
})

describe('applyPreset / detectKind', () => {
  it('正文去掉标记', () => {
    expect(applyPreset('p', '## 标题')).toBe('标题')
    expect(applyPreset('p', '- 列表')).toBe('列表')
    expect(applyPreset('p', '> 引用')).toBe('引用')
  })

  it('标题/列表/引用/代码', () => {
    expect(applyPreset('h1', '标题')).toBe('# 标题')
    expect(applyPreset('h3', '正文\n第二行')).toBe('### 正文\n### 第二行')
    expect(applyPreset('ul', '一\n二')).toBe('- 一\n- 二')
    expect(applyPreset('ol', '一\n二')).toBe('1. 一\n2. 二')
    expect(applyPreset('quote', '一\n二')).toBe('> 一\n> 二')
    expect(applyPreset('code', 'x = 1')).toBe('```\nx = 1\n```')
  })

  it('切换类型不会叠加标记', () => {
    expect(applyPreset('h2', '# 旧标题')).toBe('## 旧标题')
    expect(applyPreset('ul', '> 引用过的')).toBe('- 引用过的')
  })

  it('detectKind 识别当前类型', () => {
    expect(detectKind('## 标题')).toBe('h2')
    expect(detectKind('##')).toBe('h2')
    expect(detectKind('###### 六级标题')).toBe('h3')
    expect(detectKind('#标签')).toBe('p')
    expect(detectKind('###### 六级')).toBe('h3')
    expect(detectKind('- a')).toBe('ul')
    expect(detectKind('2) b')).toBe('ol')
    expect(detectKind('> c')).toBe('quote')
    expect(detectKind('```\nx\n```')).toBe('code')
    expect(detectKind('普通')).toBe('p')
  })
})

describe('blockAtLine', () => {
  it('按行号找块', () => {
    const blocks = splitBlocks(MEETING)
    expect(blockAtLine(blocks, 1)?.text).toBe('# 周会')
    expect(blockAtLine(blocks, 4)?.text).toContain('/v 会议音频')
    expect(blockAtLine(blocks, 999)).toBeNull()
  })
})
