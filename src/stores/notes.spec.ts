import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNotesStore } from './notes'

describe('notes store（浏览器预览适配器）', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('初始化后能加载示例笔记与文件夹', async () => {
    const s = useNotesStore()
    await s.init()
    expect(s.notes.length).toBeGreaterThanOrEqual(3)
    expect(s.folders.length).toBeGreaterThanOrEqual(2)
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

  it('文件夹与标签筛选', async () => {
    const s = useNotesStore()
    await s.init()
    s.folderFilter = '工作'
    expect(s.visibleNotes.every((n) => n.folder === '工作')).toBe(true)
    s.folderFilter = ''
    s.tagFilter = 'Rust'
    expect(s.visibleNotes.every((n) => n.tags.includes('Rust'))).toBe(true)
  })
})
