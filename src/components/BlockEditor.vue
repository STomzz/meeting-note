<script setup lang="ts">
/**
 * 块编辑器（所见即所得）。
 *
 * - 正文按「块」渲染成排版后的样子：点某一段进编辑，点别处 / Ctrl+Enter 写回；
 * - 编辑空段时输入 `#` 或 `/` 弹出块类型菜单（正文 / 一级标题 / …），选完回车就是排版结果；
 * - 只替换被编辑的那一块：其它块（会议笔记里的 `/v` 引用、`> 🎙 转写`、纪要段）逐字节不动。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch, type ComponentPublicInstance } from 'vue'
import { MINUTES_HEADING } from '../core/meetingNote'
import {
  BLOCK_PRESETS,
  appendBlocks,
  appendParagraph,
  applyPreset,
  blockAtLine,
  detectKind,
  insertBlockAfter,
  dropBlock,
  insertBlocksBefore,
  insertParagraphAfter,
  insertParagraphAtIndex,
  joinBlocks,
  replaceBlock,
  splitBlocks,
  type Block,
  type BlockKind,
} from '../core/blocks'

const props = withDefaults(
  defineProps<{
    content: string
    /** 单块 markdown → HTML（父组件提供，含 `/v` 播放器替换等） */
    render: (md: string) => string
    editable?: boolean
  }>(),
  { editable: true },
)

const emit = defineEmits<{ (e: 'update:content', value: string): void }>()

const blocks = ref<Block[]>(splitBlocks(props.content))
const editingId = ref<number | null>(null)
const draft = ref('')
const menuOpen = ref(false)
const menuIndex = ref(0)
const flashId = ref<number | null>(null)
/** 下方空间不够时把块类型菜单翻到输入框上方 */
const menuAbove = ref(false)
/** 正在编辑的输入框。注意它在 v-for 里，模板 ref 会变成数组，所以用函数 ref。 */
const taEl = ref<HTMLTextAreaElement | null>(null)
const rootEl = ref<HTMLDivElement | null>(null)

function setTaRef(el: Element | ComponentPublicInstance | null) {
  taEl.value = el instanceof HTMLTextAreaElement ? el : null
}

/**
 * 当前输入框。
 *
 * 函数 ref 在「旧的编辑框卸载、新的挂载」同一个 patch 里可能被后置的 null 覆盖，
 * 所以再兜一层：在本组件根节点里找那个唯一的 .blk-ta。
 */
function currentTa(): HTMLTextAreaElement | null {
  return taEl.value ?? rootEl.value?.querySelector<HTMLTextAreaElement>('textarea.blk-ta') ?? null
}

/** 自己刚发出去的内容不要再回灌，否则打字中途会被重置。 */
let lastEmitted = props.content
let flashTimer = 0

watch(
  () => props.content,
  (value) => {
    // 外部改动（切笔记、一键处理写回）直接重新切块
    if (value === lastEmitted) return
    blocks.value = splitBlocks(value)
    editingId.value = null
    menuOpen.value = false
  },
)

const hasBody = computed(() => blocks.value.some((b) => !b.gap))
const rows = computed(() => Math.max(1, draft.value.split('\n').length))
const draftKind = computed(() => detectKind(draft.value))

function emitContent(next: string) {
  lastEmitted = next
  emit('update:content', next)
}

/** 行首块标记的长度（把光标放到标记之后，接着写内容）。 */
function markerOffset(text: string): number {
  const m = /^(\s*(?:[-*+]|\d+[.)])\s+|#{1,6}\s+|>\s?)/.exec(text)
  return m ? m[1].length : 0
}

function startEdit(block: Block, caret: 'start' | 'end' = 'end') {
  if (!props.editable || block.gap) return
  editingId.value = block.id
  draft.value = block.text
  menuOpen.value = false
  menuIndex.value = 0
  void nextTick(() => {
    const el = currentTa()
    if (!el) return
    el.focus()
    const pos = caret === 'start' ? markerOffset(el.value) : el.value.length
    el.setSelectionRange(pos, pos)
    el.scrollIntoView({ block: 'nearest' })
  })
}

/** 写回当前编辑的块（没改动就什么都不做）。 */
function commit() {
  if (editingId.value === null) return
  const id = editingId.value
  const block = blocks.value.find((b) => b.id === id)
  editingId.value = null
  menuOpen.value = false
  if (!block || draft.value === block.text) return
  const next = replaceBlock(blocks.value, id, draft.value)
  blocks.value = next
  emitContent(joinBlocks(next))
}

function cancel() {
  editingId.value = null
  menuOpen.value = false
  draft.value = ''
}

/**
 * 点渲染出来的某一段 → 进编辑。
 *
 * 播放器 / 链接 / 按钮不拦；正在拖选文字（要复制）时也不进编辑。
 */
function onBlockClick(e: MouseEvent, block: Block) {
  const el = e.target as HTMLElement | null
  if (el?.closest('audio, a, button, [data-no-edit]')) return
  const sel = window.getSelection()
  if (sel && !sel.isCollapsed) return
  commit()
  startEdit(block)
}

function openMenu() {
  menuOpen.value = true
  menuIndex.value = Math.max(
    0,
    BLOCK_PRESETS.findIndex((p) => p.kind === draftKind.value),
  )
  const el = currentTa()
  if (el) menuAbove.value = window.innerHeight - el.getBoundingClientRect().bottom < 340
}

function choose(kind: BlockKind) {
  // 触发菜单的 `#` / `/` 本身不算内容
  const base = /^[#/]+$/.test(draft.value.trim()) ? '' : draft.value
  draft.value = applyPreset(kind, base)
  menuOpen.value = false
  void nextTick(() => {
    const el = currentTa()
    if (!el) return
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
  })
}

function onInput() {
  const text = draft.value.trim()
  // 空段里输入 `#` / `##` / `/` 就召唤块类型菜单
  if (text === '/' || /^#{1,6}$/.test(text)) {
    openMenu()
    return
  }
  // 继续打字（例如写成 `/v xxx`）就把块类型菜单收起来
  if (menuOpen.value) menuOpen.value = false
}

/** 在文末新起一段（「＋ 新起一段」按钮 / 空文档入口）。 */
function addParagraph(afterId: number | null) {
  commit()
  // 空文档里开写：不要留一个开头的空行
  const base = blocks.value.every((b) => b.gap && !b.text) ? [] : blocks.value
  const res = afterId === null ? appendParagraph(base) : insertParagraphAfter(base, afterId)
  blocks.value = res.blocks
  emitContent(joinBlocks(res.blocks))
  const created = blocks.value.find((b) => b.id === res.id)
  if (created) startEdit(created)
}

/** 在下标 index 的块后面新起一段（回车拆分段落）并把光标放在新段开头。 */
function addParagraphAt(index: number, text = '') {
  commit()
  const res = insertParagraphAtIndex(blocks.value, index, text)
  blocks.value = res.blocks
  emitContent(joinBlocks(res.blocks))
  const created = blocks.value.find((b) => b.id === res.id)
  if (created) startEdit(created, 'start')
}

/**
 * 单行块里按回车：以光标为界拆成两段。
 *
 * 列表 / 引用会自动延续标记，只写了标记就回车则退出该标记（普通段落）。
 */
function splitForEnter(el: HTMLTextAreaElement | null): { head: string; tail: string } {
  const text = draft.value
  const pos = el ? el.selectionStart : text.length
  const before = text.slice(0, pos)
  const after = text.slice(pos)
  const ul = /^(\s*)([-*+]|\d+[.)])(\s+)/.exec(text)
  if (ul) {
    const num = /^\d+$/.test(ul[2])
    const marker = num ? `${Number(ul[2]) + 1}. ` : `${ul[2]} `
    return { head: before, tail: marker + after }
  }
  if (/^\s*>/.test(text)) return { head: before, tail: `> ${after}` }
  return { head: before, tail: after }
}

function onKeydown(e: KeyboardEvent) {
  if (menuOpen.value) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const dir = e.key === 'ArrowDown' ? 1 : -1
      menuIndex.value = (menuIndex.value + dir + BLOCK_PRESETS.length) % BLOCK_PRESETS.length
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      choose(BLOCK_PRESETS[menuIndex.value].kind)
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      menuOpen.value = false
      return
    }
  }

  if (e.key === 'Escape') {
    e.preventDefault()
    cancel()
    return
  }
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault()
    commit()
    return
  }
  if (e.key !== 'Enter' || e.shiftKey) return
  // 多行块里回车 = 块内换行（Shift+Enter 同为软换行）
  if (draft.value.includes('\n')) return
  e.preventDefault()
  const id = editingId.value
  if (id === null) return
  const index = blocks.value.findIndex((b) => b.id === id)
  if (index < 0) return

  // 只写了标记（`- `、`1. `、`# `、`> `）就回车 = 退出这个标记，继续当普通段落写
  // 注意：这里用未 trim 的原值（列表标记后面的空格是标记的一部分）
  const markerOnly = /^(\s*([-*+]|\d+[.)])\s+|#{1,6}\s*|>\s*)$/.test(draft.value)
  if (markerOnly) {
    const rest = draft.value.replace(/^(\s*(?:[-*+]|\d+[.)])\s+|#{1,6}\s*|>\s?)/, '').trim()
    const next = dropBlock(blocks.value, id)
    blocks.value = next
    emitContent(joinBlocks(next))
    addParagraphAt(index - 1, rest)
    return
  }

  const { head, tail } = splitForEnter(e.target as HTMLTextAreaElement | null)
  draft.value = head
  commit()
  addParagraphAt(index, tail)
}

/** 供父组件用：插入引用行等（正在编辑就插到这段后面，否则插到纪要标题前 / 文末）。 */
function insertLines(lines: string[]) {
  if (!lines.length) return
  const text = lines.join('\n')
  const id = editingId.value
  const index = id === null ? -1 : blocks.value.findIndex((b) => b.id === id)
  if (id !== null && index >= 0) {
    const next = insertBlockAfter(blocks.value, id, text)
    blocks.value = next
    emitContent(joinBlocks(next))
    return
  }
  // 没在编辑哪一段：插到「会议纪要」标题之前，避免落进纪要段（下次处理会整段覆盖）
  const minutesIdx = blocks.value.findIndex((b) => b.text.split('\n')[0].trim() === MINUTES_HEADING)
  const next =
    minutesIdx >= 0 ? insertBlocksBefore(blocks.value, minutesIdx, text) : appendBlocks(blocks.value, text)
  blocks.value = next
  emitContent(joinBlocks(next))
}

/** 供父组件用：定位到某几行所在的块（问答/图谱点引用）。 */
function locate(startLine: number, endLine: number) {
  const block = blockAtLine(blocks.value, startLine) ?? blockAtLine(blocks.value, endLine)
  if (!block) return
  flashId.value = block.id
  void nextTick(() => {
    const el = rootEl.value?.querySelector<HTMLElement>(`[data-block-id="${block.id}"]`)
    el?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  })
  window.clearTimeout(flashTimer)
  flashTimer = window.setTimeout(() => {
    if (flashId.value === block.id) flashId.value = null
  }, 1800)
}

onBeforeUnmount(() => window.clearTimeout(flashTimer))

defineExpose({ insertLines, locate })
</script>

<template>
  <div ref="rootEl" class="blocks" :class="{ readonly: !editable }">
    <template v-for="b in blocks" :key="b.id">
      <div v-if="b.gap" class="blk-gap" />
      <div v-else class="blk-slot">
        <div v-if="editingId === b.id" class="blk-editing">
          <textarea
            :ref="setTaRef"
            v-model="draft"
            class="blk-ta"
            :rows="rows"
            spellcheck="false"
            @input="onInput"
            @keydown="onKeydown"
            @blur="commit"
          />
          <div v-if="menuOpen" class="blk-menu" :class="{ above: menuAbove }">
            <div class="menu-head">块类型（↑↓ 选择 · Enter 应用 · Esc 关掉）</div>
            <button
              v-for="(p, i) in BLOCK_PRESETS"
              :key="p.kind"
              class="menu-item"
              :class="{ on: i === menuIndex, current: p.kind === draftKind }"
              @mousedown.prevent="choose(p.kind)"
            >
              <span class="menu-label">{{ p.label }}</span>
              <span class="menu-hint">{{ p.hint }}</span>
            </button>
          </div>
          <div class="blk-tip">
            Esc 取消 · Ctrl+Enter 写回 · 输入 <code>#</code> 或 <code>/</code> 换块类型
          </div>
        </div>
        <div
          v-else
          class="blk md-body"
          :class="{ flash: flashId === b.id }"
          :data-block-id="b.id"
          :title="editable ? '点一下改这一段' : ''"
          @click="onBlockClick($event, b)"
          v-html="render(b.text)"
        />
      </div>
    </template>

    <div v-if="!hasBody" class="empty-tip" @mousedown.prevent="addParagraph(null)">
      还没有内容，点这里开始写（输入 <code>#</code> 或 <code>/</code> 可换块类型）
    </div>
    <button v-else-if="editable" class="add-row" title="在末尾新起一段" @mousedown.prevent="addParagraph(null)">
      ＋ 新起一段
    </button>
  </div>
</template>

<style scoped>
.blocks {
  padding: 6px 18px 40px;
}

.blk-slot {
  position: relative;
}

.blk-gap {
  height: 10px;
}

.blk {
  padding: 2px 6px;
  border-radius: var(--radius-s);
  cursor: text;
}

.blk:hover {
  background: var(--hover);
}

.blk.flash {
  animation: blk-flash 1.6s ease-out;
}

@keyframes blk-flash {
  0% {
    background: var(--brand-weak);
  }

  100% {
    background: transparent;
  }
}

.blk-editing {
  position: relative;
  padding: 2px 0;
}

.blk-ta {
  display: block;
  width: 100%;
  box-sizing: border-box;
  padding: 6px 8px;
  border: 1px solid var(--primary);
  border-radius: var(--radius-m);
  background: var(--panel);
  color: var(--text);
  font-family: var(--font-sans);
  font-size: inherit;
  line-height: inherit;
  resize: vertical;
  outline: none;
}

.blk-tip {
  margin-top: 4px;
  font-size: 11.5px;
  color: var(--text-3);
}

.blk-tip code,
.empty-tip code {
  padding: 0 3px;
  border-radius: 3px;
  background: var(--panel-2);
}

.blk-menu {
  position: absolute;
  z-index: 30;
  top: 100%;
  left: 0;
  margin-top: 4px;
  width: 268px;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  background: var(--panel);
  box-shadow: var(--shadow-2);
}

.blk-menu.above {
  top: auto;
  bottom: 100%;
  margin-top: 0;
  margin-bottom: 4px;
}

.menu-head {
  padding: 4px 8px 6px;
  font-size: 11.5px;
  color: var(--text-3);
}

.menu-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  border: 0;
  border-radius: var(--radius-s);
  background: transparent;
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.menu-item.on {
  background: var(--brand-weak);
}

.menu-item.current .menu-label {
  color: var(--primary);
  font-weight: 600;
}

.menu-label {
  flex: none;
}

.menu-hint {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: var(--text-3);
}

.empty-tip {
  padding: 24px 8px;
  color: var(--text-3);
  font-size: 13px;
  cursor: text;
}

.add-row {
  margin-top: 10px;
  padding: 4px 10px;
  border: 1px dashed var(--border);
  border-radius: var(--radius-m);
  background: transparent;
  color: var(--text-3);
  font-family: var(--font-sans);
  font-size: 12px;
  cursor: pointer;
}

.add-row:hover {
  color: var(--primary);
  border-color: var(--primary);
}
</style>
