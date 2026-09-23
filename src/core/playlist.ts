/**
 * 「一条长语音」的播放列表算法（纯函数，便于单测）。
 *
 * 一场录音被切成多段 WAV 存盘，但笔记里只写一条场次引用：
 * 播放器把各段按顺序连播，并把总时长当一条时间轴——拖动进度条要能跨段定位。
 */

/** 各段时长（毫秒）→ 每段的起始位置（前缀和）。 */
export function segmentStarts(durations: number[]): number[] {
  const out: number[] = []
  let acc = 0
  for (const d of durations) {
    out.push(acc)
    acc += Math.max(0, d || 0)
  }
  return out
}

/** 整场总时长（毫秒）。 */
export function totalDuration(durations: number[]): number {
  return durations.reduce((sum, d) => sum + Math.max(0, d || 0), 0)
}

/**
 * 整场时间轴上的位置 → (第几段, 段内偏移)。
 *
 * - 空列表 → (0, 0)；
 * - 超出末尾 → 最后一段的末尾（播放器按「播完了」处理）。
 */
export function locateGlobal(
  ms: number,
  durations: number[],
): { index: number; offsetMs: number } {
  if (!durations.length) return { index: 0, offsetMs: 0 }
  const pos = Math.max(0, ms || 0)
  const starts = segmentStarts(durations)
  const total = totalDuration(durations)
  if (pos >= total) {
    const last = durations.length - 1
    return { index: last, offsetMs: Math.max(0, durations[last] || 0) }
  }
  let index = 0
  for (let i = 0; i < starts.length; i++) {
    if (pos >= starts[i]) index = i
    else break
  }
  return { index, offsetMs: pos - starts[index] }
}

/** (第几段, 段内偏移) → 整场时间轴上的位置。 */
export function globalPosition(index: number, offsetMs: number, durations: number[]): number {
  if (!durations.length) return 0
  const i = Math.min(Math.max(0, Math.floor(index)), durations.length - 1)
  const starts = segmentStarts(durations)
  const dur = Math.max(0, durations[i] || 0)
  const offset = Math.min(Math.max(0, offsetMs || 0), dur)
  return starts[i] + offset
}

/** 进度百分比（0–1000，给 `<input type=range>` 用，避免浮点抖动）。 */
export function progressPermille(ms: number, durations: number[]): number {
  const total = totalDuration(durations)
  if (!total) return 0
  return Math.round((Math.min(Math.max(0, ms || 0), total) / total) * 1000)
}

/** 百分比（0–1000）→ 整场毫秒。 */
export function fromPermille(value: number, durations: number[]): number {
  const total = totalDuration(durations)
  return (Math.min(1000, Math.max(0, value)) / 1000) * total
}
