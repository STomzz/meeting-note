import type { NotesAdapter } from '../core/types'
import { MockNotesAdapter } from './mock'
import { TauriNotesAdapter } from './tauri'

let adapter: NotesAdapter | null = null

/** 在 Tauri 壳里走 Rust；浏览器里自动落到内存实现，方便开发预览。 */
export function notesAdapter(): NotesAdapter {
  if (!adapter) {
    adapter = '__TAURI_INTERNALS__' in window ? new TauriNotesAdapter() : new MockNotesAdapter()
  }
  return adapter
}

export function isTauri(): boolean {
  return '__TAURI_INTERNALS__' in window
}
