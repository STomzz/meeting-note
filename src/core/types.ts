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

/** 文件夹（左侧目录树用；空文件夹也会返回）。 */
export interface FolderInfo {
  path: string
  /** 直接放在这个文件夹里的笔记数（不含子文件夹）。 */
  noteCount: number
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
  /** 文件夹树：含空文件夹，排除隐藏目录与 `会议音频/`。 */
  folders(): Promise<FolderInfo[]>
  createFolder(path: string): Promise<string>
  renameFolder(path: string, newName: string): Promise<string>
  /** 移动笔记到另一个文件夹，返回新 id（重名自动加序号）。 */
  moveNote(id: string, folder: string): Promise<string>
  /** 重命名笔记（同目录），返回新 id。 */
  renameNote(id: string, title: string): Promise<string>
}
