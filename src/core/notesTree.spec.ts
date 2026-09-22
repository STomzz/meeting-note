import { describe, expect, it } from 'vitest'
import { ancestorsOf, buildTreeRows } from './notesTree'
import type { NoteMeta } from './types'

const note = (id: string, updatedAt = 1): NoteMeta => ({
  id,
  title: id.replace(/\.md$/, '').split('/').pop() as string,
  relPath: id,
  folder: id.includes('/') ? id.slice(0, id.lastIndexOf('/')) : '',
  tags: '',
  mtime: 0,
  size: 0,
  updatedAt,
})

describe('buildTreeRows', () => {
  const folders = [
    { path: '工作', noteCount: 1 },
    { path: '工作/子', noteCount: 0 },
    { path: '会议', noteCount: 1 },
  ]
  const notes = [note('会议/周会.md'), note('工作/周报.md'), note('根.md')]

  it('默认折叠：只显示顶层文件夹与根笔记（文件夹在前）', () => {
    const rows = buildTreeRows(folders, notes, new Set())
    expect(rows.map((r) => `${r.kind}:${r.path}`)).toEqual([
      'folder:工作',
      'folder:会议',
      'note:根.md',
    ])
    expect(rows[0].count).toBe(1)
  })

  it('展开后输出子文件夹与笔记；空文件夹也在', () => {
    const rows = buildTreeRows(folders, notes, new Set(['工作']))
    const paths = rows.map((r) => `${r.kind}:${r.path}`)
    expect(paths).toContain('folder:工作/子')
    expect(paths).toContain('note:工作/周报.md')
    expect(paths.indexOf('folder:工作/子')).toBeLessThan(paths.indexOf('note:工作/周报.md'))
    expect(rows.find((r) => r.path === '工作/子')?.count).toBe(0)
    expect(rows.find((r) => r.path === '工作')?.expanded).toBe(true)
  })

  it('笔记按更新时间倒序，录音数进徽章', () => {
    const rows = buildTreeRows(
      [{ path: '会议', noteCount: 2 }],
      [note('会议/旧.md', 1), note('会议/新.md', 9)],
      new Set(['会议']),
      { '会议/新.md': 3 },
    )
    const noteRows = rows.filter((r) => r.kind === 'note')
    expect(noteRows.map((r) => r.path)).toEqual(['会议/新.md', '会议/旧.md'])
    expect(noteRows[0].count).toBe(3)
  })

  it('ancestorsOf 给出一路祖先文件夹', () => {
    expect(ancestorsOf('a/b/c.md')).toEqual(['a', 'a/b'])
    expect(ancestorsOf('根.md')).toEqual([])
  })
})
