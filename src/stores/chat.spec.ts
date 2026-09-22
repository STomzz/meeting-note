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
  beforeEach(() => setActivePinia(createPinia()))

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
})
