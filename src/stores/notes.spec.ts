import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNotesStore } from './notes'

describe('notes store（浏览器预览适配器）', () => {
  beforeEach(() => setActivePinia(createPinia()))
  afterEach(() => vi.useRealTimers())

  it('初始化后能加载示例笔记与文件夹', async () => {
    const s = useNotesStore()
    await s.init()
    expect(s.notes.length).toBeGreaterThanOrEqual(4)
    const paths = s.folders.map((f) => f.path)
    expect(paths).toContain('工作')
    expect(paths).toContain('学习')
    expect(paths).toContain('会议')
    expect(s.vault).toBeTruthy()
  })

  it('新建 → 编辑 → 保存 → 搜索 → 删除 全链路', async () => {
    const s = useNotesStore()
    await s.init()

    await s.createNote('工作', '测试笔记')
    const id = s.currentId
    expect(id).toBe('工作/测试笔记.md')
    expect(s.current?.title).toBe('测试笔记')

    s.setContent('# 测试笔记\n\n包含关键词：知识图谱。\n')
    expect(s.dirty).toBe(true)
    await s.save()
    expect(s.dirty).toBe(false)

    await s.search('知识图谱')
    expect(s.hits.some((h) => h.noteId === id)).toBe(true)

    await s.removeNote(id)
    expect(s.notes.find((n) => n.id === id)).toBeUndefined()
  })

  it('文件夹：新建（含空文件夹）→ 重命名 → 移动 / 重命名笔记', async () => {
    const s = useNotesStore()
    await s.init()

    expect(await s.createFolder('项目/子目录')).toBe('项目/子目录')
    expect(s.folders.map((f) => f.path)).toContain('项目/子目录')

    expect(await s.renameFolder('项目', '新项目')).toBe('新项目')
    const paths = s.folders.map((f) => f.path)
    expect(paths).toContain('新项目/子目录')
    expect(paths).not.toContain('项目')

    const moved = await s.moveNote('学习/Rust 所有权.md', '新项目')
    expect(moved).toBe('新项目/Rust 所有权.md')
    expect(s.notes.some((n) => n.id === moved)).toBe(true)

    const renamed = await s.renameNote(moved, 'Rust 所有权（二）')
    expect(renamed).toBe('新项目/Rust 所有权（二）.md')
    expect(s.notes.some((n) => n.id === renamed)).toBe(true)
    expect(s.notes.some((n) => n.id === moved)).toBe(false)
  })

  it('文件夹下拉选项含根目录', async () => {
    const s = useNotesStore()
    await s.init()
    expect(s.folderOptions[0]).toEqual({ label: '根目录', value: '' })
    expect(s.folderOptions.some((o) => o.value === '工作')).toBe(true)
  })

  it('自动保存：编辑后防抖写盘，保存状态走 dirty → saved', async () => {
    const s = useNotesStore()
    await s.init()
    await s.openNote('学习/Rust 所有权.md')

    vi.useFakeTimers()
    s.setContent('# Rust 所有权\n\n自动保存测试。\n')
    expect(s.saveState).toBe('dirty')
    expect(s.dirty).toBe(true)

    await vi.advanceTimersByTimeAsync(900)
    expect(s.dirty).toBe(false)
    expect(s.saveState).toBe('saved')
    expect(s.lastSavedAt).toBeGreaterThan(0)
  })

  it('切走笔记会取消排队中的自动保存', async () => {
    const s = useNotesStore()
    await s.init()
    await s.openNote('学习/Rust 所有权.md')

    vi.useFakeTimers()
    s.setContent('# 改动\n')
    await s.openNote('工作/周会示例.md')
    expect(s.dirty).toBe(false)
    expect(s.saveState).toBe('idle')

    await vi.advanceTimersByTimeAsync(1200)
    // 排队已被取消：不会把上一篇的改动写进刚打开的笔记
    expect(s.content).not.toContain('改动')
  })

  it('引用定位：投递与消费目标行', async () => {
    const s = useNotesStore()
    await s.init()
    s.locateNote('学习/Rust 所有权.md', 3, 5)
    expect(s.pendingLocate).toEqual({ noteId: '学习/Rust 所有权.md', startLine: 3, endLine: 5 })
    s.clearLocate()
    expect(s.pendingLocate).toBeNull()
  })
})
