import { describe, expect, it } from 'vitest'
import {
  fromPermille,
  globalPosition,
  locateGlobal,
  progressPermille,
  segmentStarts,
  totalDuration,
} from './playlist'

describe('播放列表（一条长语音）时间轴', () => {
  const durs = [10_000, 20_000, 5_000] // 0–10s / 10–30s / 30–35s

  it('前缀和与总时长', () => {
    expect(segmentStarts(durs)).toEqual([0, 10_000, 30_000])
    expect(totalDuration(durs)).toBe(35_000)
    expect(totalDuration([])).toBe(0)
    // 时长为 0 / 负数按 0 处理，不产生负进度
    expect(totalDuration([1000, 0, -5])).toBe(1000)
  })

  it('整场位置 → 第几段 + 段内偏移', () => {
    expect(locateGlobal(0, durs)).toEqual({ index: 0, offsetMs: 0 })
    expect(locateGlobal(9_999, durs)).toEqual({ index: 0, offsetMs: 9_999 })
    expect(locateGlobal(10_000, durs)).toEqual({ index: 1, offsetMs: 0 })
    expect(locateGlobal(25_000, durs)).toEqual({ index: 1, offsetMs: 15_000 })
    expect(locateGlobal(30_000, durs)).toEqual({ index: 2, offsetMs: 0 })
    // 超出末尾 → 停在最后一段末尾
    expect(locateGlobal(99_000, durs)).toEqual({ index: 2, offsetMs: 5_000 })
    expect(locateGlobal(-5, durs)).toEqual({ index: 0, offsetMs: 0 })
    expect(locateGlobal(5_000, [])).toEqual({ index: 0, offsetMs: 0 })
  })

  it('第几段 + 段内偏移 → 整场位置（locate 的逆运算）', () => {
    expect(globalPosition(0, 3_000, durs)).toBe(3_000)
    expect(globalPosition(1, 0, durs)).toBe(10_000)
    expect(globalPosition(2, 1_500, durs)).toBe(31_500)
    // 越界钳制
    expect(globalPosition(9, 0, durs)).toBe(30_000)
    expect(globalPosition(-1, 0, durs)).toBe(0)
    expect(globalPosition(0, 99_000, durs)).toBe(10_000)
    for (const ms of [0, 999, 10_000, 29_999, 35_000]) {
      const { index, offsetMs } = locateGlobal(ms, durs)
      expect(globalPosition(index, offsetMs, durs)).toBe(ms)
    }
  })

  it('进度条千分比换算', () => {
    expect(progressPermille(0, durs)).toBe(0)
    expect(progressPermille(17_500, durs)).toBe(500)
    expect(progressPermille(35_000, durs)).toBe(1000)
    expect(progressPermille(99_000, durs)).toBe(1000)
    expect(progressPermille(0, [])).toBe(0)
    expect(fromPermille(500, durs)).toBe(17_500)
    expect(fromPermille(1000, durs)).toBe(35_000)
    expect(fromPermille(2000, durs)).toBe(35_000)
    expect(fromPermille(500, [])).toBe(0)
  })
})
