import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useChatStore } from './chat'

/** 轮询等条件成立（避免依赖具体实现节奏）。 */
async function until(cond: () => boolean, timeoutMs = 2000) {
  const started = Date.now()
  while (!cond() && Date.now() - started < timeoutMs) {
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}

describe('chat store（流式，浏览器预览适配器）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
  })

  it('流式问答：先给来源，再逐字拼出完整回答', async () => {
    const s = useChatStore()
    await s.ask('镜像拉取超时')
    const entry = s.entries[0]
    expect(entry.error).toBeFalsy()
    expect(entry.pending).toBe(false)
    expect(entry.streaming).toBe(false)
    expect(entry.answer?.sources.length).toBeGreaterThan(0)
    expect(entry.answer?.answer).toContain('预览模式')
    expect(s.streaming).toBe(false)
    expect(s.asking).toBe(false)
  })

  it('生成中途可停止：保留已生成的部分并标记 stopped', async () => {
    const s = useChatStore()
    const running = s.ask('镜像拉取超时')
    await until(() => s.entries[0]?.streaming === true)
    await s.stop()
    await running

    const entry = s.entries[0]
    expect(entry.stopped).toBe(true)
    expect(entry.streaming).toBe(false)
    expect(entry.answer?.answer.length).toBeGreaterThan(0)
    expect(entry.answer?.answer).toContain('预览模式')
  })

  it('重新生成：原地重跑，条目数不变', async () => {
    const s = useChatStore()
    await s.ask('镜像拉取超时')
    const entry = s.entries[0]
    const first = entry.answer?.answer
    await s.askAgain(entry)
    expect(s.entries).toHaveLength(1)
    expect(entry.answer?.answer).toBe(first)
    expect(entry.pending).toBe(false)
  })

  it('没有命中时给出兜底回答（不调用模型）', async () => {
    const s = useChatStore()
    await s.ask('量子纠缠退相干时间')
    const entry = s.entries[0]
    expect(entry.answer?.sources).toHaveLength(0)
    expect(entry.answer?.answer).toContain('没有与')
  })

  it('会话历史：新会话 → 提问 → 切走再切回能看到记录', async () => {
    const s = useChatStore()
    await s.loadHistory()
    expect(s.conversations).toHaveLength(0)

    await s.ask('镜像拉取超时')
    const firstId = s.currentConversationId
    expect(firstId).toBeTruthy()
    expect(s.conversations).toHaveLength(1)
    expect(s.conversations[0].title).toContain('镜像')

    s.newConversation()
    expect(s.entries).toHaveLength(0)
    expect(s.conversations).toHaveLength(2)

    s.selectConversation(firstId)
    expect(s.entries).toHaveLength(1)
    expect(s.entries[0].question).toBe('镜像拉取超时')
    expect(s.entries[0].answer?.answer).toContain('预览模式')
  })

  it('历史：重命名 / 置顶 / 删除', async () => {
    const s = useChatStore()
    await s.loadHistory()
    await s.ask('镜像拉取超时')
    const id = s.currentConversationId

    s.renameConversation(id, '  镜像问题  ')
    expect(s.conversations[0].title).toBe('镜像问题')

    s.togglePinConversation(id)
    expect(s.conversations[0].pinned).toBe(true)

    s.removeConversation(id)
    expect(s.conversations).toHaveLength(0)
    expect(s.entries).toHaveLength(0)
  })

  it('历史落盘后可重新读出（预览模式走 localStorage）', async () => {
    const s = useChatStore()
    await s.loadHistory()
    await s.ask('镜像拉取超时')
    await s.persistNow()

    const raw = localStorage.getItem('bnu-notes-chat-history')
    expect(raw).toContain('镜像拉取超时')

    // 新 store（模拟重启）应能读回会话与问答
    setActivePinia(createPinia())
    const s2 = useChatStore()
    await s2.loadHistory()
    expect(s2.conversations).toHaveLength(1)
    expect(s2.entries).toHaveLength(1)
    expect(s2.entries[0].answer?.answer).toContain('预览模式')
  })
})
