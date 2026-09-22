import { invoke } from '@tauri-apps/api/core'
import type { HistoryPayload } from '../core/chatHistory'

/** 问答会话历史适配器：桌面写应用数据目录，浏览器预览写 localStorage。 */
export interface ChatHistoryAdapter {
  load(): Promise<unknown>
  save(payload: HistoryPayload): Promise<void>
}

class TauriChatHistoryAdapter implements ChatHistoryAdapter {
  load() {
    return invoke<unknown>('chat_history_load')
  }
  save(payload: HistoryPayload) {
    return invoke<void>('chat_history_save', { payload })
  }
}

const LOCAL_KEY = 'bnu-notes-chat-history'

class LocalChatHistoryAdapter implements ChatHistoryAdapter {
  async load() {
    try {
      const raw = localStorage.getItem(LOCAL_KEY)
      return raw ? (JSON.parse(raw) as unknown) : null
    } catch {
      return null
    }
  }

  async save(payload: HistoryPayload) {
    try {
      localStorage.setItem(LOCAL_KEY, JSON.stringify(payload))
    } catch {
      // 预览模式下存不了不影响使用
    }
  }
}

export function chatHistoryAdapter(): ChatHistoryAdapter {
  return '__TAURI_INTERNALS__' in window
    ? new TauriChatHistoryAdapter()
    : new LocalChatHistoryAdapter()
}
