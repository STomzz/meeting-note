<script setup lang="ts">
/**
 * 3D 知识图谱（「星球」视图）。
 *
 * - three.js / 3d-force-graph 只在切到 3D 时才随组件加载（vite 动态 chunk）；
 * - 自动缓慢旋转，拖拽 / 滚轮可接管视角；单击实体 → 打开右侧详情面板；
 * - 悬停高亮邻居、其余变淡；颜色按实体类型（与 2D 一致），大小按度数；
 * - 主题切换、筛选变化都会同步。
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ForceGraph3D, { type ForceGraph3DInstance } from '3d-force-graph'
import { kindColor } from '../core/graph'
import { build3DData, endpointId, neighborMap, type Link3D, type Node3D } from '../core/graph3d'
import { useGraphStore } from '../stores/graph'

const store = useGraphStore()
const container = ref<HTMLDivElement | null>(null)
const autoRotate = ref(true)
const supported = ref(true)
const booting = ref(true)

type Graph = ForceGraph3DInstance<Node3D, Link3D>

let graph: Graph | null = null
let hoverId = ''
let neighbors = new Map<string, Set<string>>()
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null

/** 当前主题下的画布配色（访问器会被高频调用，取一次缓存起来）。 */
let colors = { link: '#cbd5e1', linkHl: '#0052d9', selected: '#0052d9', dim: '#e7e9ee' }

function cssVar(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}

function refreshColors() {
  colors = {
    link: cssVar('--border-strong', '#cbd5e1'),
    linkHl: cssVar('--primary', '#0052d9'),
    selected: cssVar('--primary', '#0052d9'),
    dim: cssVar('--border', '#e7e9ee'),
  }
}

function isSelected(id: string): boolean {
  return store.selectedId !== null && String(store.selectedId) === id
}

function isDimmed(id: string): boolean {
  if (!hoverId || id === hoverId) return false
  return !neighbors.get(hoverId)?.has(id)
}

function nodeColor(n: Node3D): string {
  if (isSelected(n.id)) return colors.selected
  if (isDimmed(n.id)) return colors.dim
  return kindColor(n.kind)
}

function linkColor(l: Link3D): string {
  if (!hoverId) return colors.link
  const s = endpointId(l.source)
  const t = endpointId(l.target)
  return s === hoverId || t === hoverId ? colors.linkHl : colors.dim
}

function nodeLabel(n: Node3D): string {
  return `${n.name} · ${n.kind} · 度数 ${n.degree}`
}

/** 重新应用颜色访问器（库里改访问器后要重新 set 一次才会重绘）。 */
function repaint() {
  refreshColors()
  if (!graph) return
  graph.nodeColor(nodeColor).linkColor(linkColor)
}

function applyAutoRotate() {
  if (!graph) return
  const controls = graph.controls() as { autoRotate?: boolean; autoRotateSpeed?: number }
  controls.autoRotate = autoRotate.value
  controls.autoRotateSpeed = 0.7
}

function toggleRotate() {
  autoRotate.value = !autoRotate.value
  applyAutoRotate()
}

function fitView() {
  graph?.zoomToFit(600, 60)
}

function webglSupported(): boolean {
  try {
    const canvas = document.createElement('canvas')
    return Boolean(canvas.getContext('webgl2') || canvas.getContext('webgl'))
  } catch {
    return false
  }
}

function mount() {
  const el = container.value
  if (graph || !el) return

  refreshColors()
  const data = build3DData(store.visible)
  neighbors = neighborMap(data.links)

  // 构造函数不带泛型：拿到手后按我们的节点/链接类型收窄
  const g = new ForceGraph3D(el, { controlType: 'orbit' }) as unknown as Graph
  graph = g

  g.backgroundColor('rgba(0,0,0,0)')
    .showNavInfo(false)
    .nodeRelSize(3.4)
    .nodeVal((n) => n.val)
    .nodeColor(nodeColor)
    .nodeOpacity(0.92)
    .nodeResolution(14)
    .nodeLabel(nodeLabel)
    .linkColor(linkColor)
    .linkWidth(1)
    .linkOpacity(0.35)
    .linkDirectionalArrowLength(3)
    .linkDirectionalArrowRelPos(1)
    .warmupTicks(20)
    .cooldownTicks(180)
    .d3AlphaDecay(0.028)
    .d3VelocityDecay(0.35)
    .onNodeHover((node) => {
      hoverId = node ? node.id : ''
      if (el) el.style.cursor = node ? 'pointer' : 'grab'
      repaint()
    })
    .onNodeClick((node) => {
      const id = Number(node.id)
      if (Number.isFinite(id)) void store.select(id)
    })
    .onBackgroundClick(() => store.clearSelection())
    .onEngineStop(() => fitView())

  g.graphData(data)
  applyAutoRotate()

  // 初始镜头：节点越多站得越远
  g.cameraPosition({ x: 0, y: 0, z: 260 + data.nodes.length * 1.5 })

  resizeObserver = new ResizeObserver(() => {
    if (container.value && graph) {
      graph.width(container.value.clientWidth).height(container.value.clientHeight)
    }
  })
  resizeObserver.observe(el)
  booting.value = false
}

onMounted(() => {
  supported.value = webglSupported()
  if (!supported.value) {
    booting.value = false
    return
  }
  mount()
  themeObserver = new MutationObserver(() => repaint())
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['theme-mode'],
  })
})

onBeforeUnmount(() => {
  themeObserver?.disconnect()
  themeObserver = null
  resizeObserver?.disconnect()
  resizeObserver = null
  try {
    graph?._destructor()
  } catch {
    // 卸载失败不影响页面切换
  }
  graph = null
})

watch(
  () => store.visible,
  (snapshot) => {
    if (!graph) return
    const data = build3DData(snapshot)
    neighbors = neighborMap(data.links)
    graph.graphData(data)
  },
)

watch(
  () => store.selectedId,
  () => repaint(),
)
</script>

<template>
  <div class="g3d">
    <div v-if="!supported" class="fallback">
      <t-icon name="relation" size="26px" />
      <div class="fb-title">当前环境不支持 WebGL</div>
      <div class="fb-desc">3D 视图需要 WebGL。可以继续用左侧「力导图」2D 视图，功能完全一致。</div>
    </div>

    <template v-else>
      <div ref="container" class="canvas3d" />
      <div v-if="booting" class="loading">
        <t-loading size="small" />
        <span>正在加载 3D 引擎…</span>
      </div>

      <div class="overlay">
        <button class="ov-btn" :title="autoRotate ? '暂停旋转' : '继续旋转'" @click="toggleRotate">
          <t-icon :name="autoRotate ? 'pause' : 'play'" size="14px" />
        </button>
        <button class="ov-btn" title="适应窗口" @click="fitView">
          <t-icon name="refresh" size="14px" />
        </button>
      </div>

      <div class="hint">
        拖拽旋转 · 滚轮缩放 · 单击实体看详情
        <span v-if="store.visible.nodes.length > 800" class="warn">
          （节点较多，低端设备可能卡顿，可切回 2D）
        </span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.g3d {
  position: relative;
  flex: 1;
  min-width: 0;
  background: var(--panel);
}

.canvas3d {
  position: absolute;
  inset: 0;
}

.loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text-3);
  background: var(--panel);
}

.overlay {
  position: absolute;
  top: 12px;
  right: 12px;
  display: flex;
  gap: 6px;
}

.ov-btn {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  background: var(--panel);
  color: var(--text-2);
  cursor: pointer;
  box-shadow: var(--shadow-1);
}

.ov-btn:hover {
  color: var(--primary);
  border-color: var(--primary);
}

.hint {
  position: absolute;
  left: 12px;
  bottom: 10px;
  font-size: 11.5px;
  color: var(--text-3);
  pointer-events: none;
}

.warn {
  color: var(--warning);
}

.fallback {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--text-3);
  padding: 24px;
  text-align: center;
}

.fb-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-2);
}

.fb-desc {
  font-size: 12.5px;
  line-height: 1.8;
  max-width: 380px;
}
</style>
