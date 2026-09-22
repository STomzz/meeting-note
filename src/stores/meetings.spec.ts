import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

/** 录音器替身：测试里手动触发采集回调，不依赖真实麦克风。 */
const rec = vi.hoisted(() => ({
  chunk: null as ((pcm: Int16Array) => void) | null,
  level: null as ((rms: number) => void) | null,
}))

vi.mock('../core/recorder', () => {
  class MicRecorder {
    info = { sampleRate: 16000, channelCount: 1, bitsPerSample: 16 }
    recording = false
    static async selfCheck() {
      return ['✅ mock 自检']
    }
    async start(cb: {
      onChunk: (pcm: Int16Array) => void
      onLevel: (rms: number) => void
    }) {
      rec.chunk = cb.onChunk
      rec.level = cb.onLevel
      this.recording = true
      return this.info
    }
    async stop() {
      this.recording = false
    }
  }
  return {
    MicRecorder,
    // 模拟真实 base64 的长度：bytes ≈ samples * 2
    pcmToBase64: (pcm: Int16Array) => 'A'.repeat(4 * Math.ceil((pcm.length * 2) / 3)),
    pcmToWavUrl: () => 'blob:mock',
  }
})

import { useMeetingsStore } from './meetings'

/** 一秒钟 16k 单声道的 PCM16。 */
function secondOfPcm(): Int16Array {
  return new Int16Array(16000).fill(1000)
}

describe('meetings store（浏览器预览适配器 + 录音器替身）', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('开始录音 → 落盘 → 停止后分段标记为待转写', async () => {
    const s = useMeetingsStore()
    const id = await s.startRecording('测试会议')
    expect(id).toBeTruthy()
    expect(s.recording).toBe(true)
    expect(s.current?.segments.length).toBe(1)
    expect(s.recordSeq).toBe(1)

    for (let i = 0; i < 3; i++) {
      rec.chunk?.(secondOfPcm())
    }
    // pushPcm 是 async，等微任务队列清空
    await new Promise((r) => setTimeout(r, 0))

    await s.stopRecording()
    expect(s.recording).toBe(false)
    const seg = s.current?.segments[0]
    expect(seg?.status).toBe('recorded')
    expect(seg?.durationMs).toBeGreaterThanOrEqual(2900)
    expect(s.current?.meeting.durationMs).toBeGreaterThanOrEqual(2900)
  })

  it('到达 4 分钟自动分段，录音不中断', async () => {
    const s = useMeetingsStore()
    await s.startRecording('长会议')
    // 245 秒：第 240 秒处应切段，第 2 段继续录
    for (let i = 0; i < 245; i++) {
      rec.chunk?.(secondOfPcm())
      if (i % 25 === 0) await new Promise((r) => setTimeout(r, 0))
    }
    await new Promise((r) => setTimeout(r, 0))

    expect(s.recordSeq).toBe(2)
    expect(s.current?.segments.length).toBe(2)
    const first = s.current?.segments[0]
    expect(first?.durationMs).toBeGreaterThanOrEqual(239000)
    expect(first?.durationMs).toBeLessThanOrEqual(241000)
    expect(s.recordSegmentMs).toBeLessThan(6000) // 新段刚开不久
    await s.stopRecording()
    expect(s.current?.segments[1]?.status).toBe('recorded')
  })

  it('转写走适配器并保留失败信息（预览模式不伪造模型结果）', async () => {
    const s = useMeetingsStore()
    await s.startRecording('转写测试')
    rec.chunk?.(secondOfPcm())
    await new Promise((r) => setTimeout(r, 0))
    await s.stopRecording()

    await s.transcribe()
    expect(s.transcribing).toBe(false)
    expect(s.current?.meeting.status).toBe('transcribed')
    expect(s.current?.meeting.hasTranscript).toBe(true)
    // 预览模式的错误提示会带「预览模式」字样，方便识别
    expect(s.error).toContain('预览模式')
  })

  it('录音自检能取到采样率并生成回放地址', async () => {
    const s = useMeetingsStore()
    await s.runSelfCheck()
    expect(s.selfCheck.notes.length).toBeGreaterThan(0)

    void s.startSelfCheckRecording(1)
    await new Promise((r) => setTimeout(r, 10))
    rec.chunk?.(secondOfPcm())
    await new Promise((r) => setTimeout(r, 1200))
    expect(s.selfCheck.info?.sampleRate).toBe(16000)
    expect(s.selfCheck.playbackUrl).toBe('blob:mock')
  })
})
