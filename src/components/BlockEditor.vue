<script setup lang="ts">
/**
 * 块编辑器（所见即所得）。
 *
 * - 正文按「块」渲染成排版后的样子：点某一段进编辑，点别处 / Ctrl+Enter 写回；
 * - 编辑空段时输入 `#` / `##` / `/` 弹出块类型菜单（正文 / 一级标题 / …），选完回车就是排版结果；
 * - 只替换被编辑的那一块：其它块（会议笔记里的 `/v` 引用、`> 🎙 转写`、纪要段）逐字节不动。
 */
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
  type ComponentPublicInstance,
} from 'vue'
import { MINUTES_HEADING, formatDur, parseRefLine, slashCommandAt } from '../core/meetingNote'
import { filterRefOptions, refLineFor, type RefOption } from '../core/refPicker'
import {
  BLOCK_PRESETS,
  appendBlocks,
  appendParagraph,
  applyPreset,
  blockAtLine,
  detectKind,
  dropBlock,
  dropBlockTidy,
  insertBlockAfter,
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
    /** `/v` 选择器候选（父组件已按「本笔记优先 / 未引用优先」排好序） */
    refOptions?: RefOption[]
  }>(),
  { editable: true, refOptions: () => [] },
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
/** 当前这一轮编辑的输入框元素：只有它的 blur 才算「用户点到别处」。 */
let sessionEl: HTMLTextAreaElement | null = null

/** `/v` 录音选择器（和块类型菜单互斥） */
const refOpen = ref(false)
const refIndex = ref(0)
const refFilter = ref('')
const refMatches = computed(() => filterRefOptions(props.refOptions, refFilter.value))

/** 引用行右侧「⋯」菜单 */
const refMenuId = ref<number | null>(null)
const copied = ref(false)
/** 下方空间不够时把「⋯」菜单翻到引用行上方 */
const menuAboveRef = ref(false)
let copiedTimer = 0

/** 操作提示只在本次会话第一次进编辑时露一下 */
let hintShown = false
const showHint = ref(false)
let hintTimer = 0

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

/**
 * 让输入框高度跟着「折行之后的内容」长。
 *
 * 只按 `:rows="逻辑行数"` 算高度的话，长行折行时会少算一行：光标走到第二个视觉行，
 * 浏览器就把输入框内部滚动一下，行首被顶出去——`overflow: hidden` 还拉不回来
 * （真机上的表现就是「打了几个字，前面的就不见了，回车之后才又能看到」）。
 * 进编辑 / 每次输入 / 文档宽度变化时都要重算。
 */
function autosizeTa(el: HTMLTextAreaElement | null) {
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight}px`
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
    refOpen.value = false
    refMenuId.value = null
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
  refOpen.value = false
  refMenuId.value = null
  menuIndex.value = 0
  sessionEl = null
  // 提示只在本次会话第一次编辑时出现，之后不打扰
  if (!hintShown) {
    hintShown = true
    showHint.value = true
    window.clearTimeout(hintTimer)
    hintTimer = window.setTimeout(() => {
      showHint.value = false
    }, 8000)
  }
  void nextTick(() => {
    const el = currentTa()
    if (!el) return
    autosizeTa(el)
    el.focus()
    const pos = caret === 'start' ? markerOffset(el.value) : el.value.length
    el.setSelectionRange(pos, pos)
    el.scrollIntoView({ block: 'nearest' })
    sessionEl = el
  })
}

/** 写回当前编辑的块（没改动就什么都不做）。 */
function commit() {
  sessionEl = null
  if (editingId.value === null) return
  const id = editingId.value
  const block = blocks.value.find((b) => b.id === id)
  editingId.value = null
  menuOpen.value = false
  refOpen.value = false
  refMenuId.value = null
  if (!block || draft.value === block.text) return
  const next = replaceBlock(blocks.value, id, draft.value)
  blocks.value = next
  emitContent(joinBlocks(next))
}

function cancel() {
  sessionEl = null
  editingId.value = null
  menuOpen.value = false
  refOpen.value = false
  refMenuId.value = null
  draft.value = ''
}

/** 这一块是不是 `/v` 引用行（渲染出来是一个播放器）。 */
function isRefBlock(block: Block): boolean {
  return parseRefLine(block.text) !== null
}

/**
 * 失焦 = 写回。
 *
 * 注意：Chrome 在「旧的编辑框被移除」时也会补发 blur，而那时候新的编辑会话已经开了
 * （回车拆段、点另一段都会这样），所以只处理当前输入框的 blur，别把新会话关掉。
 */
function onBlur(e: FocusEvent) {
  // 只认当前这一轮编辑的输入框：换段、回车拆段时旧输入框被移除 / 新输入框刚挂载，
  // 浏览器补发的 blur 不能当成「用户点到别处」（否则会把刚开的新会话关掉）。
  if ((e.target as HTMLTextAreaElement | null) !== sessionEl) return
  commit()
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
    autosizeTa(el)
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
  })
}

function onInput() {
  const el = currentTa()
  autosizeTa(el)
  const cursor = el ? el.selectionStart : draft.value.length
  // 行首敲 `/v` / `/video` → 录音选择器（与源码模式同一套判定）
  const hit = slashCommandAt(draft.value, cursor)
  if (hit) {
    refFilter.value = hit.filter
    refIndex.value = 0
    refOpen.value = true
    menuOpen.value = false
    return
  }
  if (refOpen.value) refOpen.value = false
  const text = draft.value.trim()
  // 空段里输入 `#` / `##` / `/` 就召唤块类型菜单
  if (text === '/' || /^#{1,6}$/.test(text)) {
    openMenu()
    return
  }
  // 继续打字就把块类型菜单收起来
  if (menuOpen.value) menuOpen.value = false
}

/** 选中一条录音：把正在输入的 `/v …` 换成完整引用行。 */
function pickRef(opt: RefOption) {
  const el = currentTa()
  const cursor = el ? el.selectionStart : draft.value.length
  const hit = slashCommandAt(draft.value, cursor)
  const line = refLineFor(opt)
  draft.value = hit ? draft.value.slice(0, hit.start) + line + draft.value.slice(cursor) : line
  refOpen.value = false
  commit()
}

/** 打开 / 收起引用行的「⋯」菜单。 */
function toggleRefMenu(e: MouseEvent, block: Block) {
  if (refMenuId.value === block.id) {
    refMenuId.value = null
    return
  }
  const el = e.currentTarget as HTMLElement | null
  if (el) menuAboveRef.value = window.innerHeight - el.getBoundingClientRect().bottom < 140
  copied.value = false
  refMenuId.value = block.id
}

/** 删掉文档里的这一行引用（音频文件本身不动）。 */
function deleteRef(block: Block) {
  refMenuId.value = null
  const next = dropBlockTidy(blocks.value, block.id)
  blocks.value = next
  emitContent(joinBlocks(next))
}

/** 复制音频路径（vault 相对路径，粘到资源管理器里能用）。 */
async function copyRefPath(block: Block) {
  const ref = parseRefLine(block.text)
  if (!ref) return
  try {
    await navigator.clipboard.writeText(ref.raw)
  } catch {
    const ta = document.createElement('textarea')
    ta.value = ref.raw
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    try {
      document.execCommand('copy')
    } catch {
      // 复制不了就算了，源码模式里还能看到路径
    }
    ta.remove()
  }
  copied.value = true
  window.clearTimeout(copiedTimer)
  copiedTimer = window.setTimeout(() => {
    copied.value = false
  }, 1400)
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
  if (refOpen.value) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const total = refMatches.value.length
      if (total) {
        const dir = e.key === 'ArrowDown' ? 1 : -1
        refIndex.value = (refIndex.value + dir + total) % total
      }
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      const opt = refMatches.value[refIndex.value]
      if (opt) pickRef(opt)
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      refOpen.value = false
      return
    }
  }
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

/** 文档宽度变了（转屏 / 拉侧栏 / 窗口缩放）→ 折行位置变 → 高度要重算。 */
let widthWatcher: ResizeObserver | null = null
let lastWidth = 0

onMounted(() => {
  if (typeof ResizeObserver === 'undefined' || !rootEl.value) return
  widthWatcher = new ResizeObserver(() => {
    const width = rootEl.value?.clientWidth ?? 0
    // 只看宽度：高度变化（正是输入框长高引起的）不能再触发一轮，否则来回抖
    if (width === lastWidth) return
    lastWidth = width
    autosizeTa(currentTa())
  })
  widthWatcher.observe(rootEl.value)
})

onBeforeUnmount(() => {
  widthWatcher?.disconnect()
  widthWatcher = null
  window.clearTimeout(flashTimer)
  window.clearTimeout(hintTimer)
  window.clearTimeout(copiedTimer)
})

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
            :class="`k-${draftKind}`"
            :rows="rows"
            spellcheck="false"
            @input="onInput"
            @keydown="onKeydown"
            @blur="onBlur"
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
          <div v-if="refOpen" class="blk-menu ref-menu" :class="{ above: menuAbove }">
            <div class="menu-head">插入录音引用（↑↓ 选择 · Enter 插入 · Esc 取消）</div>
            <div v-if="!refMatches.length" class="ref-empty">
              还没有可选录音：用右下角「录音」录一段，或把 wav 放进 <code>会议音频/</code> 下。
            </div>
            <button
              v-for="(o, i) in refMatches"
              :key="o.path"
              class="ref-item"
              :class="{ on: i === refIndex }"
              @mousedown.prevent="pickRef(o)"
            >
              <span class="ref-tag" :class="{ used: o.used }">{{ o.used ? '已引用' : '未引用' }}</span>
              <span class="ref-name" :title="o.label">{{ o.label }}</span>
              <span class="ref-meta">{{ formatDur(o.durationMs) }}</span>
            </button>
          </div>
          <div v-if="showHint" class="blk-hint">
            Enter 新起一段 · Esc 取消 · Ctrl+Enter 写回 · 输入 <code>#</code> 换块类型、<code>/v</code> 插录音引用
          </div>
        </div>
        <template v-else>
          <div
            class="blk md-body"
            :class="{ flash: flashId === b.id, 'has-ref': isRefBlock(b) }"
            :data-block-id="b.id"
            :title="editable ? '点一下改这一段' : ''"
            @click="onBlockClick($event, b)"
            v-html="render(b.text)"
          />
          <div v-if="editable && isRefBlock(b)" class="blk-tools">
            <button class="tool-btn" title="引用操作" @mousedown.prevent @click.stop="toggleRefMenu($event, b)">
              <t-icon name="ellipsis" size="14px" />
            </button>
          </div>
          <div v-if="refMenuId === b.id" class="ref-actions" :class="{ above: menuAboveRef }">
            <button class="act-item" @mousedown.prevent @click.stop="deleteRef(b)">删除引用（音频文件保留）</button>
            <button class="act-item" @mousedown.prevent @click.stop="copyRefPath(b)">
              {{ copied ? '已复制' : '复制音频路径' }}
            </button>
          </div>
        </template>
      </div>
    </template>

    <div
      v-if="refMenuId !== null"
      class="ref-backdrop"
      @mousedown.prevent
      @click="refMenuId = null"
      @contextmenu.prevent="refMenuId = null"
    />

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
  /* `.editor-body` 是横向 flex，本节点是它的 flex item：
     不 fill 的话宽度会被算成「内容 max-content」——编辑时长行会被 textarea 的固有宽度
     （cols=20 ≈ 412px）拖成半屏宽，写回后又弹回原宽，观感就是「半屏就换行、回车又变一行」。
     这里让它始终占满编辑列（宽度只由面板决定，不由内容决定）。 */
  flex: 1 1 auto;
  min-width: 0;
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

.blocks.readonly .blk {
  cursor: default;
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
  padding: 2px 6px;
}

/* 编辑态不画「输入框」：透明、无边框，只有光标在闪；字号跟着块类型走，与渲染结果一致 */
.blk-ta {
  display: block;
  width: 100%;
  box-sizing: border-box;
  margin: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: inherit;
  font-family: var(--font-sans);
  font-size: 14.5px;
  line-height: 1.85;
  resize: none;
  overflow: hidden;
  caret-color: var(--primary);
}

.blk-ta.k-h1 {
  font-size: 21.75px;
  font-weight: 600;
  line-height: 1.4;
}

.blk-ta.k-h2 {
  font-size: 18.1px;
  font-weight: 600;
  line-height: 1.4;
}

.blk-ta.k-h3 {
  font-size: 15.95px;
  font-weight: 600;
  line-height: 1.4;
}

.blk-ta.k-quote {
  color: var(--text-2);
}

.blk-ta.k-code {
  font-family: var(--font-mono);
  font-size: 13px;
}

.blk-hint {
  margin-top: 4px;
  font-size: 11.5px;
  color: var(--text-3);
}

.blk-hint code,
.ref-empty code,
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

/* ---- `/v` 录音选择器 ---- */

.ref-menu {
  width: min(440px, 92vw);
  max-height: 300px;
  overflow: auto;
}

.ref-empty {
  padding: 8px;
  font-size: 12px;
  line-height: 1.8;
  color: var(--text-3);
}

.ref-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  border: 0;
  border-radius: var(--radius-s);
  background: transparent;
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
}

.ref-item.on {
  background: var(--brand-weak);
}

.ref-tag {
  flex: none;
  padding: 0 6px;
  border: 1px solid var(--primary);
  border-radius: 8px;
  color: var(--primary);
  font-size: 10px;
}

.ref-tag.used {
  border-color: var(--border);
  color: var(--text-3);
}

.ref-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ref-meta {
  flex: none;
  color: var(--text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

/* ---- 引用行右侧的「⋯」：桌面悬停出现，触屏常显 ---- */

.blk.has-ref {
  padding-right: 34px;
}

.blk-tools {
  position: absolute;
  top: 50%;
  right: 6px;
  display: none;
  transform: translateY(-50%);
}

.blk-slot:hover .blk-tools,
.blk-tools:focus-within {
  display: block;
}

@media (pointer: coarse) {
  .blk-tools {
    display: block;
  }
}

.tool-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-s);
  background: var(--panel);
  color: var(--text-3);
  cursor: pointer;
}

.tool-btn:hover {
  border-color: var(--primary);
  color: var(--primary);
}

.ref-actions {
  position: absolute;
  z-index: 21;
  top: 100%;
  right: 6px;
  min-width: 200px;
  margin-top: 2px;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  background: var(--panel);
  box-shadow: var(--shadow-2);
}

.ref-actions.above {
  top: auto;
  bottom: 100%;
  margin-top: 0;
  margin-bottom: 2px;
}

.act-item {
  display: block;
  width: 100%;
  padding: 6px 8px;
  border: 0;
  border-radius: var(--radius-s);
  background: transparent;
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
}

.act-item:hover {
  background: var(--hover);
}

.ref-backdrop {
  position: fixed;
  z-index: 20;
  inset: 0;
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
