/**
 * `/v` 音频选择器的候选列表（源码模式弹层与块编辑器共用）。
 *
 * 排序：本笔记音频目录的、还没引用过的排前面，同档按修改时间新 → 旧。
 */

import type { ClipInfo } from './meetingNote'

export interface RefOption {
  /** vault 相对路径：写进引用行的就是它 */
  path: string
  dir: string
  file: string
  /** `目录/文件名`：列表显示与过滤都用这个 */
  label: string
  durationMs: number
  /** 正文里已经引用过 */
  used: boolean
  /** 属于当前这篇笔记的音频目录 */
  own: boolean
  modifiedAt: number
}

export interface RefOptionContext {
  /** 当前笔记的音频目录集合（`ClipInfo.dir`） */
  ownDirs?: Set<string>
  /** 正文里已引用的 vault 相对路径 */
  usedPaths?: Set<string>
}

function rank(o: RefOption): number {
  return (o.own ? 0 : 2) + (o.used ? 1 : 0)
}

/** 把录音列表转成选择器候选（已排好序，未截断）。 */
export function refOptions(clips: ClipInfo[], ctx: RefOptionContext = {}): RefOption[] {
  const ownDirs = ctx.ownDirs ?? new Set<string>()
  const usedPaths = ctx.usedPaths ?? new Set<string>()
  return clips
    .map((c) => ({
      path: c.path,
      dir: c.dir,
      file: c.file,
      label: `${c.dir}/${c.file}`,
      durationMs: c.durationMs,
      used: usedPaths.has(c.path),
      own: ownDirs.has(c.dir),
      modifiedAt: c.modifiedAt,
    }))
    .sort((a, b) => rank(a) - rank(b) || b.modifiedAt - a.modifiedAt)
}

/** 按 `目录/文件名` 子串过滤（空 query = 不过滤）并截断。 */
export function filterRefOptions(list: RefOption[], query: string, limit = 8): RefOption[] {
  const q = query.trim().toLowerCase()
  const hit = q ? list.filter((o) => o.label.toLowerCase().includes(q)) : list
  return hit.slice(0, limit)
}

/** 引用行文本：`/v <相对路径>`。 */
export function refLineFor(option: RefOption): string {
  return `/v ${option.path}`
}
