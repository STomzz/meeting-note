import type { NoteMeta, NotesAdapter, ScanStats, SearchHit } from '../core/types'

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
