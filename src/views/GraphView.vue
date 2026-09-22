<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { MessagePlugin } from 'tdesign-vue-next'
import { Graph } from '@antv/g6'
import { useRouter } from 'vue-router'
import { useGraphStore } from '../stores/graph'
import { useNotesStore } from '../stores/notes'
import {
  ALL_KINDS,
  edgeWidth,
  kindColor,
  kindLabel,
  nodeSize,
  type EntityMention,
  type GraphSnapshot,
} from '../core/graph'

const store = useGraphStore()
const notes = useNotesStore()
const router = useRouter()

const container = ref<HTMLDivElement | null>(null)
let graph: Graph | null = null

const kindChips = computed(() => (store.kindOptions.length ? store.kindOptions : ALL_KINDS))

/** 出处片段按笔记去重（一个实体常在同一篇里出现多次）。 */
const mentionNotes = computed(() => {
  const map = new Map<string, { noteId: string; title: string; count: number; line: number }>()
  for (const m of store.detail?.mentions ?? []) {
    const hit = map.get(m.noteId)
    if (hit) hit.count += 1
    else map.set(m.noteId, { noteId: m.noteId, title: m.noteTitle, count: 1, line: m.startLine })
  }
  return [...map.values()]
})

function toggleKind(kind: string) {
  const list = store.filter.kinds
  store.filter.kinds = list.includes(kind) ? list.filter((k) => k !== kind) : [...list, kind]
}

/** 转成 G6 数据：保留已有坐标，筛选/重排时画面不整体跳动。 */
function toG6Data(snapshot: GraphSnapshot) {
  const nodes = snapshot.nodes.map((n) => {
    let x: number | undefined
    let y: number | undefined
    try {
      const pos = graph?.getElementPosition(String(n.id))
      if (pos) {
        x = pos[0]
        y = pos[1]
      }
    } catch {
      // 新节点还没有坐标：交给力导布局
    }
    return {
      id: String(n.id),
      data: { label: n.name, kind: n.kind, degree: n.degree, noteCount: n.noteCount },
      style: {
        size: nodeSize(n.degree),
        fill: kindColor(n.kind),
        stroke: '#ffffff',
        lineWidth: 1.5,
        x,
        y,
        labelText: n.name.length > 14 ? `${n.name.slice(0, 14)}…` : n.name,
      },
    }
  })
  const edges = snapshot.edges.map((e, i) => ({
    id: `e${i}-${e.src}-${e.dst}`,
    source: String(e.src),
    target: String(e.dst),
    data: { weight: e.weight, kinds: e.kinds },
    style: {
      lineWidth: edgeWidth(e.weight),
      endArrow: true,
      labelText: e.weight > 1 ? String(e.weight) : '',
    },
  }))
  return { nodes, edges }
}

function cssVar(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}

/** 主题切换（浅色/深色）后重建画布：G6 的 canvas 样式不会跟着 CSS 变量走。 */
let themeObserver: MutationObserver | null = null

async function remountGraph() {
  const old = graph
  graph = null
  old?.destroy()
  await nextTick()
  await mountGraph()
}

async function mountGraph() {
  if (graph || !container.value) return
  const labelFill = cssVar('--text-2', '#475569')
  const edgeLabelFill = cssVar('--text-3', '#94a3b8')
  const edgeStroke = cssVar('--border-strong', '#cbd5e1')
  const selectedStroke = cssVar('--text', '#0f172a')
  const labelBg = cssVar('--panel', '#ffffff')
  graph = new Graph({
    container: container.value,
    autoResize: true,
    data: toG6Data(store.visible),
    layout: {
      type: 'd3-force',
      preventOverlap: true,
      collideStrength: 0.9,
      linkDistance: 130,
      nodeStrength: -220,
      edgeStrength: 0.5,
      alphaDecay: 0.03,
      alphaMin: 0.02,
      centerStrength: 0.05,
    },
    node: {
      style: {
        cursor: 'pointer',
        labelPlacement: 'bottom',
        labelFontSize: 11,
        labelFill,
        labelBackgroundFill: labelBg,
        labelBackground: true,
        labelBackgroundRadius: 4,
        labelPadding: [1, 4],
      },
      state: {
        selected: { lineWidth: 3, stroke: selectedStroke },
      },
    },
    edge: {
      style: {
        stroke: edgeStroke,
        endArrowSize: 6,
        labelFontSize: 10,
        labelFill: edgeLabelFill,
        labelBackground: true,
        labelBackgroundFill: labelBg,
        labelBackgroundRadius: 3,
      },
    },
    behaviors: [
      'drag-canvas',
      'zoom-canvas',
      { type: 'drag-element-force', fixed: false },
      'click-select',
    ],
    animation: false,
  })
  graph.on('node:click', (evt: unknown) => {
    const id = Number((evt as { target?: { id?: string } })?.target?.id)
    if (Number.isFinite(id) && id > 0) void store.select(id)
  })
  graph.on('canvas:click', () => store.clearSelection())
  await graph.render()
}

async function renderGraph() {
  if (!graph) {
    await mountGraph()
    return
  }
  graph.setData(toG6Data(store.visible))
  await graph.render()
}

async function zoom(ratio: number) {
  await graph?.zoomBy(ratio)
}

async function fit() {
  await graph?.fitView({ when: 'always' }).catch(() => {})
}

async function focusSelected() {
  if (graph && store.selectedId) await graph.focusElement(String(store.selectedId))
}

/** 点详情里的邻居时把视图挪过去；直接点画布则不缩放（避免画面乱跳）。 */
let zoomOnSelect = false

function selectAndFocus(id: number) {
  zoomOnSelect = true
  void store.select(id)
}

async function extractAll(force: boolean) {
  await store.extractAll(force)
  if (store.error) MessagePlugin.error(store.error)
  else MessagePlugin.success(store.message)
}

async function openNote(noteId: string, line?: number) {
  try {
    if (!notes.notes.length) await notes.init()
    await notes.openNote(noteId)
    if (line) notes.locateNote(noteId, line)
    await router.push('/notes')
  } catch (e) {
    void MessagePlugin.error(String(e))
  }
}

async function reextract(noteId: string, title: string) {
  await store.extractNote(noteId)
  if (store.error) MessagePlugin.error(store.error)
  else MessagePlugin.success(`「${title}」：${store.message}`)
}

onMounted(async () => {
  await store.bindProgress()
  await store.refresh()
  await mountGraph()
  themeObserver = new MutationObserver(() => void remountGraph())
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['theme-mode'],
  })
})

onBeforeUnmount(() => {
  themeObserver?.disconnect()
  themeObserver = null
  graph?.destroy()
  graph = null
  store.unbindProgress()
})

watch(
  () => store.visible,
  async () => {
    await renderGraph()
  },
)

/** 点详情里的邻居后，把视图挪到新选中的节点。 */
watch(
  () => store.selectedId,
  async (id) => {
    if (id && zoomOnSelect) {
      zoomOnSelect = false
      await focusSelected()
    }
  },
)

function openMention(m: EntityMention) {
  return openNote(m.noteId, m.startLine)
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">知识图谱</span>
      <span class="page-sub">实体与关系都存在本机；问答时会用图谱邻居扩展补召回</span>
      <span class="spacer" />
      <t-button size="small" theme="primary" :loading="store.extracting" @click="extractAll(false)">
        增量抽取
      </t-button>
      <t-button size="small" variant="outline" :disabled="store.extracting" @click="extractAll(true)">
        全量重抽
      </t-button>
      <t-button v-if="store.extracting" size="small" theme="danger" variant="outline" @click="store.cancel()">
        中断
      </t-button>
      <t-button size="small" variant="text" :loading="store.loading" @click="store.refresh()">刷新</t-button>
    </header>

    <div v-if="store.error || store.message" class="banner" :class="{ err: !!store.error }">
      <span class="banner-text">{{ store.error || store.message }}</span>
      <t-progress
        v-if="store.extracting"
        theme="line"
        size="small"
        :percentage="store.progressPercent"
        class="banner-bar"
      />
    </div>

    <div class="toolbar">
      <span class="tb-label">类型</span>
      <t-check-tag
        v-for="k in kindChips"
        :key="k"
        size="small"
        :checked="store.filter.kinds.includes(k)"
        @change="toggleKind(k)"
      >
        <i class="dot" :style="{ background: kindColor(k) }" />{{ kindLabel(k) }}
      </t-check-tag>
      <t-input
        v-model="store.filter.keyword"
        size="small"
        placeholder="搜索实体名…"
        clearable
        class="tb-kw"
      />
      <t-select v-model="store.filter.minDegree" size="small" class="tb-degree">
        <t-option :value="0" label="不限度数" />
        <t-option :value="1" label="度数 ≥ 1" />
        <t-option :value="2" label="度数 ≥ 2" />
        <t-option :value="5" label="度数 ≥ 5" />
      </t-select>
      <t-button v-if="store.filtering" size="small" variant="text" @click="store.resetFilter()">
        清空筛选
      </t-button>
      <span class="spacer" />
      <span class="tb-stat">
        节点 {{ store.visible.nodes.length }}/{{ store.snapshot.nodes.length }} · 边
        {{ store.visible.edges.length }}
      </span>
      <t-button size="small" variant="text" @click="zoom(1.2)">放大</t-button>
      <t-button size="small" variant="text" @click="zoom(0.8)">缩小</t-button>
      <t-button size="small" variant="text" @click="fit()">适应窗口</t-button>
      <t-button size="small" variant="text" :disabled="!store.selectedId" @click="focusSelected()">
        定位选中
      </t-button>
    </div>

    <div class="body">
      <div ref="container" class="canvas" />

      <aside class="detail">
        <div v-if="!store.hasGraph" class="empty">
          <p>还没有图谱数据。</p>
          <p>点右上角「增量抽取」，让对话模型读一遍笔记，抽出实体与关系。</p>
          <p>只处理新增 / 改动过的笔记，可随时中断；图数据存在本机 SQLite，不会上传。</p>
        </div>

        <div v-else-if="!store.detail" class="empty">
          <p>点击左侧任意节点，查看它的邻居与出处片段。</p>
          <p v-if="store.snapshot.truncated" class="hint">
            当前只画了度数最高的 {{ store.snapshot.nodes.length }} 个实体（其余
            {{ store.snapshot.truncated }} 个未显示）。
          </p>
        </div>

        <template v-else>
          <div class="det-head">
            <span class="det-name">{{ store.detail.node.name }}</span>
            <t-tag size="small" variant="light">{{ kindLabel(store.detail.node.kind) }}</t-tag>
          </div>
          <div class="det-meta">
            度数 {{ store.detail.node.degree }} · 出现在 {{ store.detail.node.noteCount }} 篇笔记
          </div>

          <div class="det-sec">邻居（{{ store.detail.neighbors.length }}）</div>
          <div class="neighbors">
            <div
              v-for="nb in store.detail.neighbors"
              :key="`${nb.direction}-${nb.entityId}-${nb.relKind}`"
              class="nb"
              @click="selectAndFocus(nb.entityId)"
            >
              <span class="dir">{{ nb.direction === 'out' ? '→' : '←' }}</span>
              <i class="dot" :style="{ background: kindColor(nb.kind) }" />
              <span class="nb-name">{{ nb.name }}</span>
              <span class="nb-rel">
                {{ nb.relKind }}<template v-if="nb.weight > 1"> ×{{ nb.weight }}</template>
              </span>
            </div>
            <div v-if="!store.detail.neighbors.length" class="empty small">这个实体还没有关系</div>
          </div>

          <div class="det-sec">出处笔记（{{ mentionNotes.length }}）</div>
          <div class="notes">
            <div v-for="n in mentionNotes" :key="n.noteId" class="note-row">
              <span class="note-name" @click="openNote(n.noteId, n.line)">{{ n.title }}</span>
              <span class="note-count">{{ n.count }} 处</span>
              <t-button
                size="small"
                variant="text"
                :loading="store.extractingNote === n.noteId"
                @click="reextract(n.noteId, n.title)"
              >
                重抽
              </t-button>
            </div>
          </div>

          <div class="det-sec">出处片段（{{ store.detail.mentions.length }}）</div>
          <div class="mentions">
            <div
              v-for="m in store.detail.mentions"
              :key="m.chunkId"
              class="mention"
              @click="openMention(m)"
            >
              <div class="m-head">{{ m.noteTitle }} · 第 {{ m.startLine }}-{{ m.endLine }} 行</div>
              <div class="m-snippet">{{ m.snippet }}</div>
            </div>
          </div>
        </template>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.banner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 18px;
  font-size: 12px;
  color: var(--text-2, #475569);
  border-bottom: 1px solid var(--border);
  background: var(--panel);
}
.banner.err {
  color: var(--danger);
  background: var(--brand-weak);
}
.banner-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.banner-bar {
  width: 180px;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 18px;
  border-bottom: 1px solid var(--border);
  flex-wrap: wrap;
}
.tb-label {
  font-size: 12px;
  color: var(--text-3);
}
.tb-kw {
  width: 170px;
}
.tb-degree {
  width: 120px;
}
.tb-stat {
  font-size: 12px;
  color: var(--text-3);
}
.dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 5px;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.canvas {
  flex: 1;
  min-width: 0;
  background: var(--panel);
}
.detail {
  width: 320px;
  border-left: 1px solid var(--border);
  background: var(--panel);
  overflow: auto;
  padding: 12px 14px;
}
.det-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.det-name {
  font-size: 15px;
  font-weight: 600;
  word-break: break-all;
}
.det-meta {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 4px;
}
.det-sec {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-2, #475569);
  margin: 14px 0 6px;
}
.neighbors,
.notes,
.mentions {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.nb {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  padding: 3px 4px;
  border-radius: 4px;
  cursor: pointer;
}
.nb:hover {
  background: var(--hover);
}
.nb .dir {
  color: var(--text-3);
}
.nb-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nb-rel {
  color: var(--text-3);
}
.note-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}
.note-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
  color: var(--primary);
}
.note-count {
  color: var(--text-3);
}
.mention {
  font-size: 12px;
  padding: 5px 6px;
  border: 1px solid var(--border);
  border-radius: 4px;
  cursor: pointer;
}
.mention:hover {
  background: var(--hover);
}
.m-head {
  color: var(--text-3);
  margin-bottom: 3px;
}
.m-snippet {
  color: var(--text-2);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.empty.small {
  padding: 8px;
  font-size: 12px;
}
.hint {
  font-size: 12px;
  color: var(--text-3);
}
</style>
