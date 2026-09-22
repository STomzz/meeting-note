import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { NoteMeta, NotesAdapter, ScanStats, SearchHit } from '../core/types'

/** 真正的客户端实现：调用 src-tauri 里的 Rust 命令。 */
export class TauriNotesAdapter implements NotesAdapter {
  getVault(): Promise<string | null> {
    return invoke('get_vault')
  }
  setVault(path: string): Promise<void> {
    return invoke('set_vault', { path })
  }
  async pickVault(): Promise<string | null> {
    const dir = await open({ directory: true, multiple: false, title: '选择笔记文件夹' })
    return typeof dir === 'string' ? dir : null
  }
  scan(): Promise<ScanStats> {
    return invoke('scan_vault')
  }
  list(): Promise<NoteMeta[]> {
    return invoke('list_notes')
  }
  read(id: string): Promise<string> {
    return invoke('read_note', { id })
  }
  write(id: string, content: string): Promise<NoteMeta> {
    return invoke('write_note', { id, content })
  }
  create(folder: string, title: string): Promise<NoteMeta> {
    return invoke('create_note', { folder, title })
  }
  remove(id: string): Promise<void> {
    return invoke('delete_note', { id })
  }
  search(query: string, limit = 30): Promise<SearchHit[]> {
    return invoke('search_notes', { query, limit })
  }
}
