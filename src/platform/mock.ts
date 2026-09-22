import type {
  FolderInfo,
  NoteMeta,
  NotesAdapter,
  ScanStats,
  SearchHit,
} from '../core/types'

/**
 * 浏览器预览用的内存实现（仅开发期）。
 * 让 `npm run dev` 在浏览器里也能点开笔记模块看 UI，不参与打包后的真实数据。
 */

export const SEED: Array<[string, string]> = [
  [
    '欢迎使用 BNU Notes.md',
    `# 欢迎使用 BNU Notes\n\n这是一个**纯本地**的知识库客户端：笔记、图谱、会议数据都在你本机。\n\n## 快速上手\n\n- 左侧列表可以搜索、按文件夹筛选\n- 右侧编辑，\`Ctrl/Cmd + S\` 保存\n- 云端只用来调用模型（嵌入 / 重排 / 对话 / 转写），可在设置里配置\n\n> 当前是浏览器预览模式，数据存在内存里。`,
  ],
  [
    '工作/周会示例.md',
    `---\ntags: [会议, 工作]\n---\n# 周会示例\n\n## 结论\n\n- 镜像拉取超时问题改用镜像站解决\n- 下周完成部署文档\n\n## 待办\n\n- [ ] 补充监控告警\n- [ ] 整理回滚步骤`,
  ],
  [
    '学习/Rust 所有权.md',
    `---\ntags: [学习, Rust]\n---\n# Rust 所有权\n\n三条规则：\n\n1. 每个值都有一个所有者\n2. 同一时间只有一个所有者\n3. 所有者离开作用域，值被丢弃\n\n借用检查器在编译期保证内存安全，这也是向量检索库可以用 Rust 写的原因之一。`,
  ],
  [
    '会议/2026-09-22-示例周会.md',
    `# 示例周会\n\n随手写的要点：镜像拉取超时，改用镜像站。\n\n/v 会议音频/会议/2026-09-22-示例周会/seg_0001.wav\n\n## 会议纪要（AI 整理）\n\n_（点「一键处理」后由对话模型整理生成，整段会被覆盖）_\n`,
  ],
]

export function splitTitle(content: string, fallback: string): { title: string; tags: string } {
  const h1 = content.match(/^#\s+(.+)$/m)
  const fm = content.match(/^---\n([\s\S]*?)\n---/)
  let tags = ''
  if (fm) {
    const m = fm[1].match(/tags:\s*\[(.*?)\]/)
    if (m) tags = m[1].split(',').map((s) => s.trim()).filter(Boolean).join(',')
  }
  return { title: (h1?.[1] ?? fallback).trim(), tags }
}

export class MockNotesAdapter implements NotesAdapter {
  private files = new Map<string, string>()
  private extraFolders = new Set<string>()
  private vault = '浏览器预览模式（内存）'

  constructor() {
    for (const [id, content] of SEED) this.files.set(id, content)
  }

  async getVault() {
    return this.vault
  }
  async setVault(path: string) {
    this.vault = path
  }
  async pickVault() {
    return null
  }
  async scan(): Promise<ScanStats> {
    return { indexed: this.files.size, skipped: 0, removed: 0 }
  }
  async list(): Promise<NoteMeta[]> {
    return [...this.files.entries()].map(([id, content]) => this.meta(id, content))
  }
  async read(id: string) {
    return this.files.get(id) ?? ''
  }
  async write(id: string, content: string) {
    this.files.set(id, content)
    return this.meta(id, content)
  }
  async create(folder: string, title: string) {
    const prefix = folder ? `${folder.replace(/\/+$/, '')}/` : ''
    let id = `${prefix}${title}.md`
    let n = 2
    while (this.files.has(id)) id = `${prefix}${title}-${n++}.md`
    const content = `# ${title}\n\n`
    this.files.set(id, content)
    return this.meta(id, content)
  }
  async remove(id: string) {
    this.files.delete(id)
  }
  async search(query: string, limit = 30): Promise<SearchHit[]> {
    const q = query.trim().toLowerCase()
    if (!q) return []
    const out: SearchHit[] = []
    for (const [id, content] of this.files.entries()) {
      const lines = content.split('\n')
      const idx = lines.findIndex((l) => l.toLowerCase().includes(q))
      if (idx >= 0) {
        out.push({
          noteId: id,
          title: splitTitle(content, id).title,
          startLine: idx + 1,
          endLine: idx + 1,
          snippet: lines[idx].slice(0, 160),
        })
      }
      if (out.length >= limit) break
    }
    return out
  }

  async folders(): Promise<FolderInfo[]> {
    const set = new Set<string>(this.extraFolders)
    for (const id of this.files.keys()) {
      const parts = id.split('/')
      parts.pop()
      let cur = ''
      for (const p of parts) {
        cur = cur ? `${cur}/${p}` : p
        set.add(cur)
      }
    }
    return [...set].sort().map((path) => ({
      path,
      noteCount: [...this.files.keys()].filter(
        (id) => id.startsWith(`${path}/`) && !id.slice(path.length + 1).includes('/'),
      ).length,
    }))
  }

  async createFolder(path: string): Promise<string> {
    const clean = path.trim().replace(/^\/+|\/+$/g, '')
    if (!clean || clean.split('/').some((p) => !p || p === '.' || p === '..' || p.startsWith('.'))) {
      throw new Error(`文件夹名非法：${path}`)
    }
    if (clean === '会议音频' || clean.startsWith('会议音频/')) {
      throw new Error('「会议音频」是录音数据目录，不能作为笔记文件夹')
    }
    // 父级也登记为文件夹（模拟真实目录结构）
    let cur = ''
    for (const p of clean.split('/')) {
      cur = cur ? `${cur}/${p}` : p
      this.extraFolders.add(cur)
    }
    return clean
  }

  async renameFolder(path: string, newName: string): Promise<string> {
    const clean = path.trim().replace(/^\/+|\/+$/g, '')
    const parent = clean.includes('/') ? clean.slice(0, clean.lastIndexOf('/')) : ''
    const name = newName.trim()
    const next = parent ? `${parent}/${name}` : name
    const clash =
      this.extraFolders.has(next) || [...this.files.keys()].some((id) => id.startsWith(`${next}/`))
    if (!name || name.includes('/') || name.startsWith('.') || clash) {
      throw new Error(`重命名失败：${newName}`)
    }
    for (const id of [...this.files.keys()]) {
      if (id.startsWith(`${clean}/`)) {
        const moved = `${next}/${id.slice(clean.length + 1)}`
        this.files.set(moved, this.files.get(id) as string)
        this.files.delete(id)
      }
    }
    for (const f of [...this.extraFolders]) {
      if (f === clean || f.startsWith(`${clean}/`)) {
        this.extraFolders.delete(f)
        this.extraFolders.add(`${next}${f.slice(clean.length)}`)
      }
    }
    return next
  }

  async moveNote(id: string, folder: string): Promise<string> {
    const content = this.files.get(id)
    if (content === undefined) throw new Error(`笔记不存在：${id}`)
    const clean = folder.trim().replace(/^\/+|\/+$/g, '')
    const file = id.split('/').pop() as string
    const stem = file.replace(/\.md$/i, '')
    let next = clean ? `${clean}/${file}` : file
    let n = 2
    while (this.files.has(next)) {
      next = clean ? `${clean}/${stem}-${n}.md` : `${stem}-${n}.md`
      n += 1
    }
    if (next === id) return id
    this.files.delete(id)
    this.files.set(next, content)
    if (clean) await this.createFolder(clean)
    return next
  }

  async renameNote(id: string, title: string): Promise<string> {
    const content = this.files.get(id)
    if (content === undefined) throw new Error(`笔记不存在：${id}`)
    const cleanTitle = title.trim()
    if (!cleanTitle) throw new Error('标题不能为空')
    const file = id.split('/').pop() as string
    const stem = file.replace(/\.md$/i, '')
    const h1 = content.match(/^[ \t]*# (.+)$/m)?.[1]?.trim()
    const inSync = h1 === undefined || h1 === stem

    let next = id
    if (inSync) {
      const folder = id.includes('/') ? id.slice(0, id.lastIndexOf('/')) : ''
      const base = cleanTitle.replace(/[\\/:*?"<>|]/g, '-').replace(/^[.\s]+|[.\s]+$/g, '')
      next = folder ? `${folder}/${base}.md` : `${base}.md`
      let n = 2
      while (this.files.has(next) && next !== id) {
        next = folder ? `${folder}/${base}-${n}.md` : `${base}-${n}.md`
        n += 1
      }
    }
    const updated =
      h1 === undefined ? content : content.replace(/^([ \t]*)# .+$/m, `$1# ${cleanTitle}`)
    if (next !== id) this.files.delete(id)
    this.files.set(next, updated)
    return next
  }

  private meta(id: string, content: string): NoteMeta {
    const { title, tags } = splitTitle(content, id.replace(/\.md$/i, ''))
    const folder = id.includes('/') ? id.slice(0, id.lastIndexOf('/')) : ''
    return {
      id,
      title,
      relPath: id,
      folder,
      tags,
      mtime: Date.now() / 1000,
      size: content.length,
      updatedAt: Date.now() / 1000,
    }
  }
}
