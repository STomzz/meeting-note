import { describe, expect, it } from 'vitest'
import {
  capConversations,
  conversationTitle,
  groupConversations,
  normalizeHistory,
  trimForStore,
  type Conversation,
} from './chatHistory'

function conv(partial: Partial<Conversation> & { id: string }): Conversation {
  return {
    title: partial.id,
    createdAt: 0,
    updatedAt: 0,
    pinned: false,
    entries: [],
    ...partial,
  }
}

const DAY = 24 * 60 * 60 * 1000

describe('conversationTitle', () => {
  it('压缩空白并截断', () => {
    expect(conversationTitle('  镜像   拉取超时  ')).toBe('镜像 拉取超时')
    expect(conversationTitle('一'.repeat(40))).toHaveLength(23)
    expect(conversationTitle('   ')).toBe('新会话')
  })
})

describe('trimForStore', () => {
  it('来源正文超长时截断', () => {
    const entry = {
      id: 1,
      question: 'q',
      answer: {
        question: 'q',
        answer: 'a',
        sources: [
          {
            chunkId: 1,
            noteId: 'n.md',
            title: 't',
            startLine: 1,
            endLine: 1,
            text: 'x'.repeat(1000),
            score: 0.1,
            sources: ['fts'],
          },
        ],
        trace: {
          mode: 'fts',
          ftsHits: 1,
          vectorHits: 0,
          reranked: false,
          graphHits: 0,
          graphAdded: 0,
          graphEntities: 0,
          degraded: [],
          elapsedMs: 1,
        },
        model: 'm',
        elapsedMs: 1,
        completionTokens: null,
      },
    }
    const stored = trimForStore(entry)
    expect(stored.answer?.sources[0].text.length).toBeLessThan(500)
    expect(stored.answer?.sources[0].text.endsWith('…')).toBe(true)
    // 没有答案的条目原样返回
    expect(trimForStore({ id: 2, question: 'q' })).toEqual({ id: 2, question: 'q' })
  })
})

describe('groupConversations', () => {
  const now = new Date('2026-09-22T15:00:00').getTime()

  it('按置顶/今天/昨天/7 天内/更早分组并各自倒序', () => {
    const list = [
      conv({ id: 'p', pinned: true, updatedAt: now - 30 * DAY }),
      conv({ id: 't1', updatedAt: now - 1000 }),
      conv({ id: 't2', updatedAt: now - 5000 }),
      conv({ id: 'y', updatedAt: now - DAY }),
      conv({ id: 'w', updatedAt: now - 5 * DAY }),
      conv({ id: 'old', updatedAt: now - 20 * DAY }),
    ]
    const groups = groupConversations(list, now)
    expect(groups.map((g) => g.label)).toEqual(['置顶', '今天', '昨天', '7 天内', '更早'])
    expect(groups[0].items.map((c) => c.id)).toEqual(['p'])
    expect(groups[1].items.map((c) => c.id)).toEqual(['t1', 't2'])
    expect(groups[2].items.map((c) => c.id)).toEqual(['y'])
    expect(groups[3].items.map((c) => c.id)).toEqual(['w'])
    expect(groups[4].items.map((c) => c.id)).toEqual(['old'])
  })

  it('空列表没有分组', () => {
    expect(groupConversations([], now)).toEqual([])
  })
})

describe('capConversations', () => {
  it('保留置顶，其余按更新时间留最新', () => {
    const list = [
      conv({ id: 'old', updatedAt: 1 }),
      conv({ id: 'p', pinned: true, updatedAt: 0 }),
      conv({ id: 'new', updatedAt: 3 }),
      conv({ id: 'mid', updatedAt: 2 }),
    ]
    const capped = capConversations(list, 3)
    expect(capped.map((c) => c.id).sort()).toEqual(['new', 'p', 'mid'].sort())
  })
})

describe('normalizeHistory', () => {
  it('丢掉坏数据并补默认值', () => {
    const list = normalizeHistory({
      version: 1,
      conversations: [
        { id: 'ok', title: '  ', entries: [{ question: 'q' }, null], pinned: 1 },
        { id: '', entries: [] },
        { id: 'no-entries' },
        null,
        'x',
      ],
    })
    expect(list).toHaveLength(1)
    expect(list[0].title).toBe('未命名会话')
    expect(list[0].pinned).toBe(true)
    expect(list[0].entries).toHaveLength(1)
  })

  it('非对象输入返回空数组', () => {
    expect(normalizeHistory(null)).toEqual([])
    expect(normalizeHistory({ conversations: 'x' })).toEqual([])
  })
})
