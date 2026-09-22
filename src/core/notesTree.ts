/**
 * 左侧目录树：把「文件夹列表 + 笔记列表」铺平成可渲染的行。
 *
 * - 文件夹优先、笔记在后；文件夹按名字排序，笔记按更新时间倒序；
 * - 只有展开的文件夹才输出子行（`expanded` 集合控制）；
 * - 文件夹计数含子文件夹里的笔记（一眼知道下面有多少东西）。
 */
import type { FolderInfo, NoteMeta } from './types'

export interface TreeRow {
  kind: 'folder' | 'note'
  /** 文件夹路径（如 `会议/旧会议`）或笔记 id（如 `会议/周会.md`） */
  path: string
  /** 显示名：文件夹用最后一段，笔记用标题 */
  name: string
  depth: number
  /** 文件夹：含子文件夹的笔记总数；笔记：该笔记的录音段数（徽章用） */
  count: number
  expanded?: boolean
  hasChildren?: boolean
  note?: NoteMeta
}

interface Node {
  path: string
  name: string
  children: Map<string, Node>
  notes: NoteMeta[]
}

export function buildTreeRows(
  folders: FolderInfo[],
  notes: NoteMeta[],
  expanded: Set<string>,
  clipCounts: Record<string, number> = {},
): TreeRow[] {
  const root: Node = { path: '', name: '', children: new Map(), notes: [] }
  const ensure = (path: string): Node => {
    if (!path) return root
    let node = root
    let cur = ''
    for (const seg of path.split('/')) {
      cur = cur ? `${cur}/${seg}` : seg
      let child = node.children.get(seg)
      if (!child) {
        child = { path: cur, name: seg, children: new Map(), notes: [] }
        node.children.set(seg, child)
      }
      node = child
    }
    return node
  }

  for (const f of folders) ensure(f.path)
  for (const n of notes) ensure(n.folder || '').notes.push(n)

  const totalNotes = (n: Node): number =>
    n.notes.length + [...n.children.values()].reduce((sum, c) => sum + totalNotes(c), 0)

  const rows: TreeRow[] = []
  const walk = (n: Node, depth: number) => {
    const subFolders = [...n.children.values()].sort((a, b) => a.name.localeCompare(b.name, 'zh'))
    for (const f of subFolders) {
      const isOpen = expanded.has(f.path)
      rows.push({
        kind: 'folder',
        path: f.path,
        name: f.name,
        depth,
        count: totalNotes(f),
        expanded: isOpen,
        hasChildren: f.children.size > 0 || f.notes.length > 0,
      })
      if (isOpen) walk(f, depth + 1)
    }
    const sorted = [...n.notes].sort(
      (a, b) => b.updatedAt - a.updatedAt || a.title.localeCompare(b.title, 'zh'),
    )
    for (const note of sorted) {
      rows.push({
        kind: 'note',
        path: note.id,
        name: note.title,
        depth,
        count: clipCounts[note.id] ?? 0,
        note,
      })
    }
  }
  walk(root, 0)
  return rows
}

/** 一篇笔记的所有祖先文件夹（用于选中笔记时自动展开路径）。 */
export function ancestorsOf(id: string): string[] {
  const segs = id.split('/')
  segs.pop()
  const out: string[] = []
  let cur = ''
  for (const s of segs) {
    cur = cur ? `${cur}/${s}` : s
    out.push(cur)
  }
  return out
}
