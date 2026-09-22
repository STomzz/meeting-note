/**
 * 问答会话历史：结构与纯函数。
 *
 * 落盘格式（应用数据目录 `chat-history.json`）：
 * `{ version: 1, conversations: Conversation[] }`。
 * Rust 侧只做原子读写，schema 以这里为准。
 */

import type { Answer } from './retrieval'

/** 历史文件版本：结构不兼容时前端自行丢弃旧数据。 */
export const HISTORY_VERSION = 1

/** 落盘时来源片段的截断长度（历史只是为了回看与定位，不需要全文）。 */
export const MAX_SOURCE_TEXT = 400

/** 最多保留的会话数（超出按更新时间丢最旧的、非置顶的）。 */
export const MAX_CONVERSATIONS = 60

export interface StoredEntry {
  id: number
  question: string
  answer?: Answer
  /** 用户中途停止过 */
  stopped?: boolean
  error?: string
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  pinned: boolean
  entries: StoredEntry[]
}

export interface HistoryPayload {
  version: number
  conversations: Conversation[]
}

export function newConversationId(): string {
  try {
    if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID()
  } catch {
    // 老 WebView 没有 randomUUID：退回时间戳 + 随机数
  }
  return `c-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
}

/** 用首个问题当标题。 */
export function conversationTitle(question: string, max = 22): string {
  const text = question.replace(/\s+/g, ' ').trim()
  if (!text) return '新会话'
  return text.length > max ? `${text.slice(0, max)}…` : text
}

/** 落盘前瘦身：来源片段只留前若干字符。 */
export function trimForStore(entry: StoredEntry): StoredEntry {
  if (!entry.answer) return entry
  return {
    ...entry,
    answer: {
      ...entry.answer,
      sources: entry.answer.sources.map((s) =>
        s.text.length > MAX_SOURCE_TEXT ? { ...s, text: `${s.text.slice(0, MAX_SOURCE_TEXT)}…` } : s,
      ),
    },
  }
}

/** 读取历史时的健壮化：坏数据丢掉而不是让页面崩。 */
export function normalizeHistory(payload: unknown): Conversation[] {
  const raw = (payload as { conversations?: unknown } | null)?.conversations
  if (!Array.isArray(raw)) return []
  const out: Conversation[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const c = item as Partial<Conversation>
    const id = typeof c.id === 'string' ? c.id : ''
    if (!id || !Array.isArray(c.entries)) continue
    const createdAt = Number(c.createdAt) || Date.now()
    out.push({
      id,
      title: typeof c.title === 'string' && c.title.trim() ? c.title : '未命名会话',
      createdAt,
      updatedAt: Number(c.updatedAt) || createdAt,
      pinned: Boolean(c.pinned),
      entries: c.entries.filter(
        (e): e is StoredEntry => Boolean(e) && typeof e === 'object' && typeof (e as StoredEntry).question === 'string',
      ),
    })
  }
  return out
}

/** 超出上限时裁剪：置顶保留，其余按更新时间保留最新的。 */
export function capConversations(list: Conversation[], max = MAX_CONVERSATIONS): Conversation[] {
  if (list.length <= max) return list
  const pinned = list.filter((c) => c.pinned)
  const rest = list
    .filter((c) => !c.pinned)
    .sort((a, b) => (b.updatedAt || b.createdAt) - (a.updatedAt || a.createdAt))
  return [...pinned, ...rest.slice(0, Math.max(0, max - pinned.length))]
}

export interface HistoryGroup {
  label: string
  items: Conversation[]
}

function startOfDay(ts: number): number {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

const DAY = 24 * 60 * 60 * 1000

/** 按时间分组：置顶 / 今天 / 昨天 / 7 天内 / 更早，组内按更新时间倒序。 */
export function groupConversations(list: Conversation[], now = Date.now()): HistoryGroup[] {
  const byTime = (a: Conversation, b: Conversation) =>
    (b.updatedAt || b.createdAt) - (a.updatedAt || a.createdAt)
  const today = startOfDay(now)
  const buckets: Record<string, Conversation[]> = {
    今天: [],
    昨天: [],
    '7 天内': [],
    更早: [],
  }
  const pinned: Conversation[] = []
  for (const c of list) {
    if (c.pinned) {
      pinned.push(c)
      continue
    }
    const day = startOfDay(c.updatedAt || c.createdAt)
    if (day >= today) buckets['今天'].push(c)
    else if (day === today - DAY) buckets['昨天'].push(c)
    else if (day > today - 7 * DAY) buckets['7 天内'].push(c)
    else buckets['更早'].push(c)
  }

  const groups: HistoryGroup[] = []
  if (pinned.length) groups.push({ label: '置顶', items: pinned.sort(byTime) })
  for (const label of ['今天', '昨天', '7 天内', '更早']) {
    if (buckets[label].length) groups.push({ label, items: buckets[label].sort(byTime) })
  }
  return groups
}
