/**
 * 3D 图谱的数据映射与纯函数（渲染在 Graph3D.vue，依赖 three/3d-force-graph 懒加载）。
 *
 * 这里只做「图快照 → 3D 需要的形状」的转换，方便单测；
 * 颜色仍复用 core/graph.ts 的 kindColor。
 */

import type { GraphSnapshot } from './graph'

export interface Node3D {
  id: string
  name: string
  kind: string
  /** 关系条数 */
  degree: number
  /** 出现在几篇笔记里 */
  noteCount: number
  /** 球体体积参数（3d-force-graph 的 nodeVal） */
  val: number
}

export interface Link3D {
  source: string
  target: string
  weight: number
  kinds: string
}

/** 度数 → 球体大小：开方压缩并封顶，避免枢纽节点吞掉整个画面。 */
export function nodeValFor(degree: number): number {
  const d = Number.isFinite(degree) ? Math.max(0, degree) : 0
  return Math.min(24, Math.max(1, 1 + Math.sqrt(d) * 1.8))
}

/** 图快照 → 3D 数据；丢掉两端不在节点集里的悬空边（节点可能被上限截断）。 */
export function build3DData(snapshot: GraphSnapshot): { nodes: Node3D[]; links: Link3D[] } {
  const ids = new Set(snapshot.nodes.map((n) => String(n.id)))
  const nodes: Node3D[] = snapshot.nodes.map((n) => ({
    id: String(n.id),
    name: n.name,
    kind: n.kind,
    degree: n.degree,
    noteCount: n.noteCount,
    val: nodeValFor(n.degree),
  }))
  const links: Link3D[] = snapshot.edges
    .filter((e) => ids.has(String(e.src)) && ids.has(String(e.dst)))
    .map((e) => ({
      source: String(e.src),
      target: String(e.dst),
      weight: e.weight,
      kinds: e.kinds,
    }))
  return { nodes, links }
}

/**
 * 双向邻接表，用于悬停高亮邻居。
 * 注意：3d-force-graph 会在内部把 link.source/target 从 id 换成节点对象，
 * 所以这个表要用**传给库之前**的数据来建。
 */
export function neighborMap(
  links: Array<{ source: string; target: string }>,
): Map<string, Set<string>> {
  const map = new Map<string, Set<string>>()
  const add = (a: string, b: string) => {
    const set = map.get(a)
    if (set) set.add(b)
    else map.set(a, new Set([b]))
  }
  for (const l of links) {
    if (l.source === l.target) continue
    add(l.source, l.target)
    add(l.target, l.source)
  }
  return map
}

/** 链接端点归一化：库会把端点替换成节点对象。 */
export function endpointId(value: unknown): string {
  if (value && typeof value === 'object') {
    const id = (value as { id?: unknown }).id
    return id === undefined || id === null ? '' : String(id)
  }
  return value === undefined || value === null ? '' : String(value)
}
