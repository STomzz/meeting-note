/** 与 Rust 侧（serde camelCase）共享的数据契约。 */

export interface NoteMeta {
  id: string
  title: string
  relPath: string
  folder: string
  tags: string // 逗号分隔
  mtime: number
  size: number
  updatedAt: number
}

export interface SearchHit {
  noteId: string
  title: string
  startLine: number
  endLine: number
  snippet: string
}

export interface ScanStats {
  indexed: number
  skipped: number
  removed: number
}

/** 笔记存储适配器：Tauri 版走 Rust，浏览器版是开发预览用的内存实现。 */
export interface NotesAdapter {
  getVault(): Promise<string | null>
  setVault(path: string): Promise<void>
  pickVault(): Promise<string | null>
  scan(): Promise<ScanStats>
  list(): Promise<NoteMeta[]>
  read(id: string): Promise<string>
  write(id: string, content: string): Promise<NoteMeta>
  create(folder: string, title: string): Promise<NoteMeta>
  remove(id: string): Promise<void>
  search(query: string, limit?: number): Promise<SearchHit[]>
}
