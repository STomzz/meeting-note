import { describe, expect, it } from 'vitest'
import { filterRefOptions, refLineFor, refOptions } from './refPicker'
import type { ClipInfo } from './meetingNote'

function clip(dir: string, file: string, modifiedAt: number, durationMs = 4000): ClipInfo {
  return {
    dir,
    file,
    path: `${dir}/${file}`,
    seq: Number(file.replace(/\D/g, '')) || 0,
    session: '',
    bytes: 1000,
    durationMs,
    sampleRate: 16000,
    modifiedAt,
  }
}

const clips: ClipInfo[] = [
  clip('会议音频/会议/周会', 'seg_0001.wav', 100),
  clip('会议音频/会议/周会', 'seg_0002.wav', 300),
  clip('会议音频/会议/周会', 'seg_0003.wav', 200),
  clip('会议音频/会议/评审', 'seg_0001.wav', 999),
]

describe('refOptions 排序', () => {
  it('本笔记的排前面、未引用的排前面，同档按修改时间新→旧', () => {
    const list = refOptions(clips, {
      ownDirs: new Set(['会议音频/会议/周会']),
      usedPaths: new Set(['会议音频/会议/周会/seg_0001.wav']),
    })
    expect(list.map((o) => o.path)).toEqual([
      '会议音频/会议/周会/seg_0002.wav', // 本笔记 + 未引用
      '会议音频/会议/周会/seg_0003.wav', // 本笔记 + 未引用
      '会议音频/会议/周会/seg_0001.wav', // 本笔记 + 已引用
      '会议音频/会议/评审/seg_0001.wav', // 别的笔记
    ])
  })

  it('没给上下文时按修改时间排', () => {
    const list = refOptions(clips)
    expect(list[0].path).toBe('会议音频/会议/评审/seg_0001.wav')
    expect(list.every((o) => !o.own && !o.used)).toBe(true)
  })

  it('标签是 `目录/文件名`，方便过滤与显示', () => {
    const list = refOptions([clips[0]])
    expect(list[0].label).toBe('会议音频/会议/周会/seg_0001.wav')
  })
})

describe('filterRefOptions', () => {
  const list = refOptions(clips, { ownDirs: new Set(['会议音频/会议/周会']) })

  it('空 query 不过滤（只截断）', () => {
    expect(filterRefOptions(list, '').length).toBe(4)
    expect(filterRefOptions(list, '', 2).length).toBe(2)
  })

  it('按子串过滤，大小写不敏感', () => {
    expect(filterRefOptions(list, 'seg_0002').map((o) => o.file)).toEqual(['seg_0002.wav'])
    expect(filterRefOptions(list, '评审').map((o) => o.path)).toEqual([
      '会议音频/会议/评审/seg_0001.wav',
    ])
    expect(filterRefOptions(list, 'SEG_0001')).toHaveLength(2)
  })

  it('过滤后仍保持排序（本笔记优先）', () => {
    expect(filterRefOptions(list, 'seg_0001').map((o) => o.path)).toEqual([
      '会议音频/会议/周会/seg_0001.wav',
      '会议音频/会议/评审/seg_0001.wav',
    ])
  })

  it('没有命中就是空列表', () => {
    expect(filterRefOptions(list, 'zzz')).toEqual([])
  })
})

describe('refLineFor', () => {
  it('生成 `/v <相对路径>`', () => {
    const [first] = refOptions(clips)
    expect(refLineFor(first)).toBe('/v 会议音频/会议/评审/seg_0001.wav')
  })
})
