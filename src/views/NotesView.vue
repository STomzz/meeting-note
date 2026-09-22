<script setup lang="ts">
/**
 * 会议笔记：vault 里所有 md 都在这里（会议只是其中一类）。
 *
 * - 左侧是文件夹树：点开文件夹才看到里面的笔记，右键可新建 / 重命名 / 移动；
 * - 编辑器支持 `/v` 引用录音、预览播放、一键处理（转写 + 纪要）；
 * - 录音面板在右下角，停止录音只保存文件，引用由 `/v` 或「本笔记录音」插入；
 * - 处理前会提示还没写进笔记的录音，可一键插入或忽略。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { MessagePlugin } from 'tdesign-vue-next'
import MarkdownIt from 'markdown-it'
import { useNotesStore } from '../stores/notes'
import { useMeetingNoteStore } from '../stores/meetingNote'
import {
  clipCountsByNote,
  dirsForNote,
  dirForNote,
  formatBytes,
  formatDur,
  insertRefAtCursor,
  prepareAudioRefs,
  replaceAudioPlaceholders,
  parseRefs,
  slashCommandAt,
  unreferencedClips,
} from '../core/meetingNote'
import type { ClipInfo } from '../core/meetingNote'
import { ancestorsOf, buildTreeRows } from '../core/notesTree'
import { lineRangeOffset } from '../core/lines'
import type { NoteMeta } from '../core/types'

const store = useNotesStore()
const meeting = useMeetingNoteStore()
const md = new MarkdownIt({ html: false, linkify: true })

const query = ref('')
const preview = ref(false)
const editorEl = ref<HTMLTextAreaElement | null>(null)

/** 保存状态角标文案（自动保存 + 手动保存共用）。 */
const saveLabel = computed(() => {
  switch (store.saveState) {
    case 'dirty':
      return '未保存'
    case 'saving':
      return '保存中…'
    case 'saved':
      return '已保存'
    default:
      return ''
  }
})

// ---------------------------------------------------------------- 目录树
const expanded = ref<Set<string>>(new Set())
const clipCounts = computed(() => clipCountsByNote(store.notes, meeting.allClips))
const treeRows = computed(() =>
  buildTreeRows(store.folders, store.notes, expanded.value, clipCounts.value),
)

function toggleFolder(path: string) {
  const next = new Set(expanded.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  expanded.value = next
}

// ---------------------------------------------------------------- 右键菜单
const menu = ref<{ x: number; y: number; kind: 'folder' | 'note'; path: string; name: string } | null>(
  null,
)

function openMenu(e: MouseEvent, kind: 'folder' | 'note', path: string, name: string) {
  menu.value = { x: e.clientX, y: e.clientY, kind, path, name }
}

/** 行内「…」按钮：贴着按钮右下角弹菜单。 */
function openMenuAt(el: MouseEvent | HTMLElement, kind: 'folder' | 'note', path: string, name: string) {
  const target = el instanceof MouseEvent ? (el.currentTarget as HTMLElement) : el
  const rect = target.getBoundingClientRect()
  menu.value = { x: rect.left, y: rect.bottom + 4, kind, path, name }
}

// ---------------------------------------------------------------- 行内重命名
const editingPath = ref('')
const editingValue = ref('')

async function startInlineRename(row: { kind: 'folder' | 'note'; path: string; name: string }) {
  menu.value = null
  editingPath.value = row.path
  editingValue.value = row.name
  await nextTick()
  const el = document.querySelector<HTMLInputElement>('.tree-rename')
  el?.focus()
  el?.select()
}

async function commitInlineRename() {
  const path = editingPath.value
  const value = editingValue.value.trim()
  editingPath.value = ''
  if (!path || !value) return
  try {
    if (store.folders.some((f) => f.path === path)) await store.renameFolder(path, value)
    else await store.renameNote(path, value)
    await meeting.loadAllClips()
    MessagePlugin.success('已重命名')
  } catch (e) {
    MessagePlugin.error(String(e))
  }
}

function cancelInlineRename() {
  editingPath.value = ''
}

// ---------------------------------------------------------------- 拖拽移动
const dragNote = ref('')
const dragOverFolder = ref('')

function onNoteDragStart(e: DragEvent, row: { kind: string; path: string }) {
  if (row.kind !== 'note') return
  dragNote.value = row.path
  e.dataTransfer?.setData('text/plain', row.path)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onFolderDragOver(e: DragEvent, folderPath: string) {
  if (!dragNote.value) return
  e.preventDefault()
  dragOverFolder.value = folderPath
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
}

async function onDropToFolder(folderPath: string) {
  const noteId = dragNote.value
  dragNote.value = ''
  dragOverFolder.value = ''
  if (!noteId) return
  const note = store.notes.find((n) => n.id === noteId)
  if (!note || (note.folder ?? '') === folderPath) return
  try {
    const next = await store.moveNote(noteId, folderPath)
    await meeting.loadAllClips()
    MessagePlugin.success(`已移动到 ${folderPath || '根目录'}：${next}`)
  } catch (e) {
    MessagePlugin.error(String(e))
  }
}

async function onDropToRoot(e: DragEvent) {
  if (!dragNote.value) return
  e.preventDefault()
  await onDropToFolder('')
}

function onRootDragOver(e: DragEvent) {
  if (!dragNote.value) return
  e.preventDefault()
}

function onDragEnd() {
  dragNote.value = ''
  dragOverFolder.value = ''
}

// ---------------------------------------------------------------- 新建
const showNew = ref(false)
const newTitle = ref('')
const newFolder = ref('')

function startNewNote(folder = '') {
  newFolder.value = folder
  newTitle.value = ''
  showNew.value = true
  menu.value = null
}

async function createNote() {
  const title = newTitle.value.trim()
  if (!title) return
  await store.createNote(newFolder.value.trim(), title)
  const next = new Set(expanded.value)
  if (newFolder.value.trim()) next.add(newFolder.value.trim())
  expanded.value = next
  showNew.value = false
}

const showNewFolder = ref(false)
const newFolderName = ref('')
const newFolderParent = ref('')

function startNewFolder(parent = '') {
  newFolderParent.value = parent
  newFolderName.value = ''
  showNewFolder.value = true
  menu.value = null
}

async function createFolder() {
  const name = newFolderName.value.trim()
  if (!name) return
  const path = newFolderParent.value ? `${newFolderParent.value}/${name}` : name
  try {
    const created = await store.createFolder(path)
    const next = new Set(expanded.value)
    for (const p of ancestorsOf(`${created}/x.md`)) next.add(p)
    expanded.value = next
    showNewFolder.value = false
    MessagePlugin.success(`已新建文件夹 ${created}`)
  } catch (e) {
    MessagePlugin.error(String(e))
  }
}

// ---------------------------------------------------------------- 重命名 / 移动
const renameTarget = ref<{ kind: 'folder' | 'note'; path: string } | null>(null)
const renameValue = ref('')

function startRename() {
  const m = menu.value
  if (!m) return
  renameTarget.value = { kind: m.kind, path: m.path }
  renameValue.value = m.name
  menu.value = null
}

async function doRename() {
  const target = renameTarget.value
  const value = renameValue.value.trim()
  if (!target || !value) return
  try {
    if (target.kind === 'folder') await store.renameFolder(target.path, value)
    else await store.renameNote(target.path, value)
    renameTarget.value = null
    await meeting.loadAllClips()
    MessagePlugin.success('已重命名')
  } catch (e) {
    MessagePlugin.error(String(e))
  }
}

const moveTarget = ref<NoteMeta | null>(null)
const moveFolder = ref('')

function startMove() {
  const m = menu.value
  if (!m || m.kind !== 'note') return
  moveTarget.value = store.notes.find((n) => n.id === m.path) ?? null
  moveFolder.value = moveTarget.value?.folder ?? ''
  menu.value = null
}

async function doMove() {
  const note = moveTarget.value
  if (!note) return
  try {
    const next = await store.moveNote(note.id, moveFolder.value)
    moveTarget.value = null
    await meeting.loadAllClips()
    if (next === note.id) MessagePlugin.info('笔记已在目标文件夹')
    else MessagePlugin.success(`已移动到 ${next}`)
  } catch (e) {
    MessagePlugin.error(String(e))
  }
}

// ---------------------------------------------------------------- 打开笔记
async function open(id: string) {
  await store.openNote(id)
  await meeting.openNote(id)
  unrefDialog.value = false
  preview.value = false
  showClips.value = false
  const next = new Set(expanded.value)
  for (const p of ancestorsOf(id)) next.add(p)
  expanded.value = next
}

function openHit(noteId: string) {
  void open(noteId)
}

// ---------------------------------------------------------------- 录音片段
const showClips = ref(false)
const clipSrcMap = ref<Record<string, string | null>>({})
const playing = ref('')
const confirmDiscard = ref<ClipInfo | null>(null)

/** 本笔记录音（新目录 + 旧版目录）。 */
const currentClips = computed(() => {
  if (!store.currentId) return []
  const dirs = dirsForNote(store.currentId)
  return meeting.allClips
    .filter((c) => dirs.includes(c.dir))
    .sort((a, b) => a.dir.localeCompare(b.dir) || a.seq - b.seq)
})

const refPathSet = computed(() => new Set(meeting.refs.map((r) => r.path)))
const unrefClips = computed(() => unreferencedClips(currentClips.value, meeting.refs))

function isRef(c: ClipInfo): boolean {
  return refPathSet.value.has(c.path)
}

async function playClip(c: ClipInfo) {
  if (playing.value === c.path) {
    playing.value = ''
    return
  }
  if (!(c.path in clipSrcMap.value)) {
    clipSrcMap.value = { ...clipSrcMap.value, [c.path]: await meeting.clipSrc(c.path) }
  }
  if (!clipSrcMap.value[c.path]) {
    MessagePlugin.info('这段音频暂不可播放（浏览器预览模式或文件缺失）')
    return
  }
  playing.value = c.path
}

/** 直接在当前光标处插入若干引用行（编辑器打开时用）。 */
function insertLinesNow(lines: string[]) {
  if (!lines.length) return
  const el = editorEl.value
  let pos = el ? el.selectionStart : store.content.length
  let body = store.content
  for (const line of lines) {
    const res = insertRefAtCursor(body, line, pos)
    body = res.body
    pos = res.cursor
  }
  store.setContent(body)
  void nextTick(() => {
    if (!editorEl.value) return
    editorEl.value.focus()
    editorEl.value.setSelectionRange(pos, pos)
  })
}

async function insertClipNow(c: ClipInfo) {
  insertLinesNow([`/v ${c.path}`])
  await nextTick()
  await store.save()
  if (store.currentId) await meeting.openNote(store.currentId)
  MessagePlugin.success('已插入引用（已保存）')
}

async function insertAllUnref() {
  const lines = unrefClips.value.map((c) => `/v ${c.path}`)
  insertLinesNow(lines)
  await nextTick()
  await store.save()
  if (store.currentId) await meeting.openNote(store.currentId)
  MessagePlugin.success(`已插入 ${lines.length} 条引用（已保存）`)
}

async function discardClip() {
  const clip = confirmDiscard.value
  confirmDiscard.value = null
  if (!clip) return
  await meeting.discardClip(clip)
  await meeting.loadAllClips()
  if (store.currentId) await meeting.openNote(store.currentId)
}

// ---------------------------------------------------------------- 一键处理
const refCount = computed(() => parseRefs(store.content).length)
const unrefDialog = ref(false)
const unrefList = ref<ClipInfo[]>([])
/** 本次处理是否忽略转写缓存（按钮下拉里的「强制重新转写」） */
const processForce = ref(false)
const processOptions = [
  { content: '一键处理（缓存命中不重复转写）', value: 'normal' },
  { content: '强制重新转写（忽略缓存，较慢）', value: 'force' },
]

function onProcessPick(item: { value?: unknown }) {
  void processCurrent(item?.value === 'force')
}

async function processCurrent(force = processForce.value) {
  if (!store.currentId || meeting.processing) return
  processForce.value = force
  await store.save()
  await meeting.openNote(store.currentId)
  await meeting.loadAllClips()
  const unref = unreferencedClips(currentClips.value, meeting.refs)
  if (unref.length) {
    unrefList.value = unref
    unrefDialog.value = true
    return
  }
  await runProcess()
}

async function runProcess() {
  if (!store.currentId) return
  await meeting.openNote(store.currentId)
  await meeting.process(processForce.value)
}

async function insertUnrefAndProcess() {
  const lines = unrefList.value.map((c) => `/v ${c.path}`)
  insertLinesNow(lines)
  unrefDialog.value = false
  await nextTick()
  await store.save()
  await meeting.openNote(store.currentId)
  await runProcess()
}

async function ignoreUnref() {
  unrefDialog.value = false
  await runProcess()
}

// ---------------------------------------------------------------- 编辑器
const audioSrcMap = ref<Record<string, string | null>>({})
const loadingSrc = new Set<string>()

const slashOpen = ref(false)
const slashFilter = ref('')
const slashIndex = ref(0)

const slashMatches = computed(() => {
  const q = slashFilter.value.toLowerCase()
  const own = new Set(currentClips.value.map((c) => c.path))
  const unref = new Set(unrefClips.value.map((c) => c.path))
  let list = [...meeting.clips]
  list.sort((a, b) => {
    const rank = (c: ClipInfo) => (own.has(c.path) ? 0 : 2) + (unref.has(c.path) ? 0 : 1)
    return rank(a) - rank(b) || b.modifiedAt - a.modifiedAt
  })
  if (q) list = list.filter((c) => `${c.dir}/${c.file}`.toLowerCase().includes(q))
  return list.slice(0, 8)
})

const rendered = computed(() => renderMarkdown(store.content))

/** 预览：把 `/v 音频` 行渲染成播放器（src 异步取，取不到时先显示文件名）。 */
function renderMarkdown(body: string): string {
  const html = md.render(prepareAudioRefs(body))
  return replaceAudioPlaceholders(html, (raw) => {
    const src = resolveSrc(raw)
    if (!src) return `<p class="audio-ref"><code>${escapeHtml(raw)}</code>（暂不可播放）</p>`
    return `<p class="audio-ref"><audio controls preload="metadata" src="${escapeHtml(src)}"></audio><span class="audio-name">${escapeHtml(raw)}</span></p>`
  })
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function resolveSrc(raw: string): string | null {
  const cached = audioSrcMap.value[raw]
  if (cached !== undefined) return cached
  if (!loadingSrc.has(raw)) {
    loadingSrc.add(raw)
    void meeting.clipSrc(raw).then((src) => {
      audioSrcMap.value = { ...audioSrcMap.value, [raw]: src }
      loadingSrc.delete(raw)
    })
  }
  return null
}

onMounted(async () => {
  await store.init()
  await meeting.loadAllClips()
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))

/** 录音面板写入的引用：插到光标处（预览模式下追加到正文末尾）。 */
watch(
  () => meeting.pendingRef,
  async (pending) => {
    if (!pending || pending.noteId !== store.currentId) return
    const el = editorEl.value
    const cursor = el ? el.selectionStart : store.content.length
    const res = insertRefAtCursor(store.content, pending.text, cursor)
    store.setContent(res.body)
    meeting.clearPendingRef()
    await nextTick()
    if (editorEl.value) {
      editorEl.value.focus()
      editorEl.value.setSelectionRange(res.cursor, res.cursor)
    }
  },
)

/** 录音增删后：刷新徽章与引用状态。 */
watch(
  () => meeting.clipRevision,
  async () => {
    await meeting.loadAllClips()
    if (store.currentId) await meeting.openNote(store.currentId)
  },
)

/** 问答/图谱里点引用：打开笔记 → 切编辑视图 → 滚动并高亮对应行。 */
watch(
  () => store.pendingLocate,
  async (loc) => {
    if (!loc) return
    if (store.currentId !== loc.noteId) await open(loc.noteId)
    preview.value = false
    showClips.value = false
    await nextTick()
    const el = editorEl.value
    if (!el) {
      store.clearLocate()
      return
    }
    const { start, end } = lineRangeOffset(store.content, loc.startLine, loc.endLine)
    const lineHeight = Number.parseFloat(getComputedStyle(el).lineHeight || '') || 24
    el.focus()
    el.setSelectionRange(start, Math.max(start, end - 1))
    el.scrollTop = Math.max(0, (loc.startLine - 3) * lineHeight)
    store.clearLocate()
    MessagePlugin.info(`已定位到第 ${loc.startLine}-${loc.endLine} 行`)
  },
)

function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    void saveCurrent()
  }
}

/** 输入时检测行首 `/v`、`/video`，弹出录音选择器。 */
function onEditorInput(e: Event) {
  const el = e.target as HTMLTextAreaElement
  store.setContent(el.value)
  const hit = slashCommandAt(el.value, el.selectionStart)
  if (!hit) {
    slashOpen.value = false
    return
  }
  slashFilter.value = hit.filter
  slashIndex.value = 0
  slashOpen.value = true
  if (!meeting.clips.length) void meeting.loadClips()
}

function onEditorKeydown(e: KeyboardEvent) {
  if (!slashOpen.value) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    slashIndex.value = Math.min(slashIndex.value + 1, slashMatches.value.length - 1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    slashIndex.value = Math.max(slashIndex.value - 1, 0)
  } else if (e.key === 'Enter' || e.key === 'Tab') {
    const clip = slashMatches.value[slashIndex.value]
    if (clip) {
      e.preventDefault()
      applyClip(clip)
    }
  } else if (e.key === 'Escape') {
    slashOpen.value = false
  }
}

/** 用选中的录音替换刚输入的 `/v`，并把完整引用行插到光标处。 */
function applyClip(clip: ClipInfo) {
  const el = editorEl.value
  if (!el) return
  const cursor = el.selectionStart
  const hit = slashCommandAt(store.content, cursor)
  const start = hit ? hit.start : cursor
  const cleaned = store.content.slice(0, start) + store.content.slice(cursor)
  const res = insertRefAtCursor(cleaned, `/v ${clip.path}`, start)
  store.setContent(res.body)
  slashOpen.value = false
  void nextTick(() => {
    if (!editorEl.value) return
    editorEl.value.focus()
    editorEl.value.setSelectionRange(res.cursor, res.cursor)
  })
}

async function onSearch() {
  await store.search(query.value)
}

/** 保存后刷新引用状态（未引用计数/选择器标签依赖它）。 */
async function saveCurrent() {
  await store.save()
  if (store.currentId) await meeting.openNote(store.currentId)
}

async function removeCurrent() {
  if (!store.currentId) return
  const ok = window.confirm(`确定删除「${store.current?.title ?? store.currentId}」？文件会从磁盘删除。`)
  if (!ok) return
  await store.removeNote(store.currentId)
  await meeting.loadAllClips()
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">会议笔记</span>
      <span class="page-sub" :title="store.vault">{{ store.vault }}</span>
      <span v-if="store.scanMessage" class="scan-msg">{{ store.scanMessage }}</span>
      <span class="spacer" />
      <t-button size="small" variant="outline" @click="meeting.toggleRecorder(true)">
        <t-icon name="microphone-1" size="14px" />
        录音
      </t-button>
      <t-button size="small" :loading="store.scanning" @click="store.scan()">扫描 vault</t-button>
    </header>

    <div class="notes">
      <aside class="list-pane">
        <div class="list-head">
          <t-input
            v-model="query"
            size="small"
            placeholder="搜索标题 / 正文…"
            clearable
            @enter="onSearch"
            @clear="onSearch"
          />
        </div>
        <div class="list-actions">
          <t-button size="small" theme="primary" @click="startNewNote(store.current?.folder ?? '')">
            新建笔记
          </t-button>
          <t-button size="small" variant="outline" @click="startNewFolder(store.current?.folder ?? '')">
            新建文件夹
          </t-button>
        </div>

        <div class="note-list">
          <!-- 搜索结果 -->
          <template v-if="query.trim()">
            <div class="list-label">搜索「{{ query }}」 · {{ store.hits.length }} 条</div>
            <div
              v-for="h in store.hits"
              :key="`${h.noteId}-${h.startLine}`"
              class="note-item"
              @click="openHit(h.noteId)"
            >
              <div class="note-title">{{ h.title }}</div>
              <div class="hit-snippet">{{ h.snippet }}</div>
              <div class="note-meta">{{ h.noteId }} · L{{ h.startLine }}-{{ h.endLine }}</div>
            </div>
            <div v-if="!store.hits.length" class="empty">没有匹配的笔记</div>
          </template>

          <!-- 文件夹树 -->
          <template v-else>
            <div
              class="list-label"
              :class="{ 'drop-on': dragNote && !dragOverFolder }"
              @dragover="onRootDragOver"
              @drop.prevent="onDropToRoot"
            >
              {{ store.notes.length }} 篇笔记
              <span v-if="dragNote" class="drop-hint">放到这里 = 移动到根目录</span>
            </div>
            <div
              v-for="row in treeRows"
              :key="`${row.kind}:${row.path}`"
              class="tree-row"
              :class="{
                active: row.kind === 'note' && row.path === store.currentId,
                'drop-on': row.kind === 'folder' && dragOverFolder === row.path,
                dragging: dragNote === row.path,
              }"
              :style="{ paddingLeft: `${8 + row.depth * 14}px` }"
              :draggable="row.kind === 'note'"
              @click="row.kind === 'folder' ? toggleFolder(row.path) : open(row.path)"
              @contextmenu.prevent="openMenu($event, row.kind, row.path, row.name)"
              @dblclick.stop="startInlineRename(row)"
              @dragstart="onNoteDragStart($event, row)"
              @dragend="onDragEnd"
              @dragover="row.kind === 'folder' ? onFolderDragOver($event, row.path) : undefined"
              @drop.prevent="row.kind === 'folder' ? onDropToFolder(row.path) : undefined"
            >
              <input
                v-if="editingPath === row.path"
                v-model="editingValue"
                class="tree-rename"
                @click.stop
                @dblclick.stop
                @keydown.enter.stop="commitInlineRename"
                @keydown.esc.stop="cancelInlineRename"
                @blur="commitInlineRename"
              />
              <template v-else-if="row.kind === 'folder'">
                <span class="tree-caret">
                  <t-icon :name="row.expanded ? 'chevron-down' : 'chevron-right'" size="13px" />
                </span>
                <t-icon
                  :name="row.expanded ? 'folder-open' : 'folder'"
                  size="15px"
                  class="tree-icon folder-icon"
                />
                <span class="tree-name">{{ row.name }}</span>
                <span class="tree-count">{{ row.count }}</span>
                <span class="tree-actions">
                  <button
                    class="icon-btn"
                    title="在此新建笔记"
                    @click.stop="startNewNote(row.path)"
                  >
                    <t-icon name="file-add" size="13px" />
                  </button>
                  <button
                    class="icon-btn"
                    title="更多"
                    @click.stop="openMenuAt($event, 'folder', row.path, row.name)"
                  >
                    <t-icon name="ellipsis" size="14px" />
                  </button>
                </span>
              </template>
              <template v-else>
                <span class="tree-caret" />
                <t-icon name="file" size="14px" class="tree-icon" />
                <span class="tree-name">{{ row.name }}</span>
                <span v-if="row.count" class="tree-badge">
                  <t-icon name="microphone-1" size="12px" />
                  {{ row.count }}
                </span>
                <span class="tree-actions">
                  <button
                    class="icon-btn"
                    title="更多"
                    @click.stop="openMenuAt($event, 'note', row.path, row.name)"
                  >
                    <t-icon name="ellipsis" size="14px" />
                  </button>
                </span>
              </template>
            </div>
            <div v-if="!treeRows.length" class="empty-state">
              <t-icon name="edit-1" size="30px" class="empty-icon" />
              <div class="empty-title">还没有笔记</div>
              <div class="empty-desc">点上方「新建笔记」开始，或把 md 文件放进 vault 后「扫描 vault」。</div>
            </div>
          </template>
        </div>
      </aside>

      <section class="editor-pane">
        <template v-if="store.currentId">
          <div class="editor-bar">
            <span class="doc-title">{{ store.current?.title ?? store.currentId }}</span>
            <span class="doc-path">{{ store.currentId }}</span>
            <span v-if="saveLabel" class="save-state" :class="store.saveState">
              <t-icon :name="store.saveState === 'saved' ? 'check-circle' : 'time'" size="13px" />
              {{ saveLabel }}
            </span>
            <span class="spacer" />
            <t-button
              v-if="currentClips.length"
              size="small"
              variant="outline"
              @click="showClips = !showClips"
            >
              <t-icon name="microphone-1" size="14px" />
              {{ currentClips.length }} 段<template v-if="unrefClips.length">
                · {{ unrefClips.length }} 未引用</template
              >
            </t-button>
            <t-dropdown
              v-if="refCount"
              trigger="click"
              :options="processOptions"
              :disabled="meeting.processing"
              @click="onProcessPick"
            >
              <t-button size="small" variant="outline" :loading="meeting.processing">
                一键处理（{{ refCount }} 段录音）
              </t-button>
            </t-dropdown>
            <t-button size="small" :disabled="!store.dirty" theme="primary" @click="saveCurrent()"
              >保存</t-button
            >
            <t-button size="small" variant="outline" @click="preview = !preview">
              {{ preview ? '编辑' : '预览' }}
            </t-button>
            <t-button size="small" theme="danger" variant="text" @click="removeCurrent"
              >删除</t-button
            >
          </div>

          <!-- 本笔记录音 -->
          <div v-if="showClips && currentClips.length" class="clips-panel">
            <div class="clips-head">
              <span class="clips-title">
                本笔记录音 · <code>会议音频/{{ dirForNote(store.currentId) }}</code>
              </span>
              <span class="spacer" />
              <t-button
                v-if="unrefClips.length"
                size="small"
                theme="primary"
                variant="outline"
                @click="insertAllUnref"
              >
                插入全部未引用（{{ unrefClips.length }}）
              </t-button>
            </div>
            <div v-for="c in currentClips" :key="c.path" class="clip-row">
              <span class="clip-name">{{ c.file }}</span>
              <span class="clip-meta">
                {{ formatDur(c.durationMs) }} · {{ formatBytes(c.bytes) }} ·
                {{ isRef(c) ? '已引用' : '未引用' }}
              </span>
              <t-button size="small" variant="text" @click="playClip(c)">
                {{ playing === c.path ? '停止' : '试听' }}
              </t-button>
              <t-button v-if="!isRef(c)" size="small" variant="text" @click="insertClipNow(c)">
                插入
              </t-button>
              <t-button size="small" variant="text" theme="danger" @click="confirmDiscard = c">
                丢弃
              </t-button>
              <audio
                v-if="playing === c.path && clipSrcMap[c.path]"
                :src="clipSrcMap[c.path] ?? ''"
                controls
                autoplay
              />
            </div>
          </div>

          <div v-if="meeting.processing || meeting.message" class="editor-progress">
            <t-progress :percentage="meeting.progress" :label="false" />
            <span class="editor-progress-text">{{ meeting.message }}</span>
          </div>
          <div class="editor-body">
            <textarea
              v-if="!preview"
              ref="editorEl"
              class="editor"
              :value="store.content"
              spellcheck="false"
              @input="onEditorInput"
              @keydown="onEditorKeydown"
              @click="slashOpen = false"
              @blur="slashOpen = false"
            />
            <div v-else class="preview md-body" v-html="rendered" />
            <div v-if="slashOpen" class="slash">
              <div class="slash-head">
                插入录音引用（↑↓ 选择，Enter 插入，Esc 取消）
              </div>
              <div v-if="!slashMatches.length" class="slash-empty">
                还没有录音。用右下角「录音」录一段，或先录到 <code>会议音频/</code> 下。
              </div>
              <div
                v-for="(c, i) in slashMatches"
                :key="c.path"
                class="slash-item"
                :class="{ on: i === slashIndex }"
                @mousedown.prevent="applyClip(c)"
              >
                <span v-if="isRef(c)" class="slash-tag">已引用</span>
                <span v-else class="slash-tag new">未引用</span>
                <span class="slash-name" :title="`${c.dir}/${c.file}`">{{ c.dir }}/{{ c.file }}</span>
                <span class="slash-meta">{{ formatDur(c.durationMs) }}</span>
              </div>
            </div>
          </div>
        </template>
        <div v-else class="empty">从左侧选择一篇笔记，或新建一篇</div>
      </section>
    </div>

    <!-- 右键菜单 -->
    <div v-if="menu" class="ctx-backdrop" @click="menu = null" @contextmenu.prevent="menu = null" />
    <div v-if="menu" class="ctx" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
      <template v-if="menu.kind === 'folder'">
        <div class="ctx-item" @click="startNewNote(menu.path)">在此新建笔记</div>
        <div class="ctx-item" @click="startNewFolder(menu.path)">新建子文件夹</div>
      </template>
      <div class="ctx-item" @click="startRename">
        {{ menu.kind === 'folder' ? '重命名文件夹' : '重命名笔记' }}
      </div>
      <div v-if="menu.kind === 'note'" class="ctx-item" @click="startMove">移动到…</div>
    </div>

    <!-- 新建笔记 -->
    <t-dialog
      v-model:visible="showNew"
      header="新建笔记"
      :confirm-btn="{ content: '创建', disabled: !newTitle.trim() }"
      @confirm="createNote"
    >
      <div class="dialog-row">
        <span class="dialog-label">标题</span>
        <t-input v-model="newTitle" placeholder="例如：周会纪要" @enter="createNote" />
      </div>
      <div class="dialog-row">
        <span class="dialog-label">文件夹</span>
        <t-select v-model="newFolder" :options="store.folderOptions" size="small" />
      </div>
    </t-dialog>

    <!-- 新建文件夹 -->
    <t-dialog
      v-model:visible="showNewFolder"
      header="新建文件夹"
      :confirm-btn="{ content: '创建', disabled: !newFolderName.trim() }"
      @confirm="createFolder"
    >
      <div class="dialog-row">
        <span class="dialog-label">名称</span>
        <t-input v-model="newFolderName" placeholder="例如：项目 A" @enter="createFolder" />
      </div>
      <div class="dialog-row">
        <span class="dialog-label">上级</span>
        <t-select v-model="newFolderParent" :options="store.folderOptions" size="small" />
      </div>
    </t-dialog>

    <!-- 重命名 -->
    <t-dialog
      :visible="!!renameTarget"
      :header="renameTarget?.kind === 'folder' ? '重命名文件夹' : '重命名笔记'"
      :confirm-btn="{ content: '重命名', disabled: !renameValue.trim() }"
      @confirm="doRename"
      @cancel="renameTarget = null"
      @close="renameTarget = null"
    >
      <div class="dialog-row">
        <span class="dialog-label">新名字</span>
        <t-input v-model="renameValue" @enter="doRename" />
      </div>
      <div v-if="renameTarget?.kind === 'note'" class="dialog-hint">
        标题（正文第一个 <code>#</code> 行）一定会改；文件名与标题一致时文件名同步改
        （<code>会议/2026-09-22-周会.md</code> 这类带日期的文件名保持不变）。
        录音与 `/v` 引用不受影响，别处的链接不会自动更新。
      </div>
    </t-dialog>

    <!-- 移动 -->
    <t-dialog
      :visible="!!moveTarget"
      header="移动到文件夹"
      :confirm-btn="{ content: '移动' }"
      @confirm="doMove"
      @cancel="moveTarget = null"
      @close="moveTarget = null"
    >
      <div class="dialog-row">
        <span class="dialog-label">目标</span>
        <t-select v-model="moveFolder" :options="store.folderOptions" size="small" />
      </div>
      <div class="dialog-hint">重名时自动加序号；录音留在原目录，引用不会失效。</div>
    </t-dialog>

    <!-- 未引用录音提醒 -->
    <t-dialog
      v-model:visible="unrefDialog"
      header="还有录音没写进笔记"
      :footer="false"
      width="520px"
    >
      <p class="unref-text">
        这段笔记的目录里还有 <b>{{ unrefList.length }}</b> 段录音没有 `/v` 引用，
        直接处理的话它们不会被转写：
      </p>
      <ul class="unref-list">
        <li v-for="c in unrefList" :key="c.path">
          {{ c.dir }}/{{ c.file }}（{{ formatDur(c.durationMs) }}）
        </li>
      </ul>
      <div class="dialog-actions">
        <t-button theme="primary" @click="insertUnrefAndProcess">插入并处理</t-button>
        <t-button variant="outline" @click="ignoreUnref">忽略并继续</t-button>
        <t-button variant="text" @click="unrefDialog = false">取消</t-button>
      </div>
    </t-dialog>

    <!-- 丢弃录音 -->
    <t-dialog
      :visible="!!confirmDiscard"
      header="丢弃这段录音？"
      :confirm-btn="{ content: '丢弃', theme: 'danger' }"
      @confirm="discardClip"
      @cancel="confirmDiscard = null"
      @close="confirmDiscard = null"
    >
      <p>
        将删除文件 <code>{{ confirmDiscard?.path }}</code>
        （{{ confirmDiscard ? formatDur(confirmDiscard.durationMs) : '' }}），已写入笔记的引用需要自己删。
      </p>
    </t-dialog>
  </div>
</template>

<style scoped>
.page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.notes {
  flex: 1;
  min-height: 0;
  display: flex;
}

.list-pane {
  width: 320px;
  flex: 0 0 320px;
  border-right: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.list-head {
  display: flex;
  gap: 8px;
  padding: 12px 12px 8px;
}

.list-actions {
  display: flex;
  gap: 8px;
  padding: 0 12px 8px;
  border-bottom: 1px solid var(--border);
}

.note-list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 8px;
}

.list-label {
  font-size: 12px;
  color: var(--text-3);
  padding: 6px 6px 8px;
}

.tree-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 6px 6px 8px;
  border-radius: var(--radius-m);
  cursor: pointer;
  font-size: 13px;
  user-select: none;
  transition: background 0.12s ease;
}

.tree-row:hover {
  background: var(--hover);
}

.tree-row.active {
  background: var(--brand-weak);
}

.tree-row.active .tree-name {
  color: var(--primary);
  font-weight: 500;
}

.tree-row.dragging {
  opacity: 0.45;
}

.tree-row.drop-on,
.list-label.drop-on {
  background: var(--brand-weak);
  box-shadow: inset 0 0 0 1px var(--primary);
}

.list-label.drop-on {
  border-radius: var(--radius-m);
  padding: 6px;
}

.drop-hint {
  margin-left: 8px;
  color: var(--primary);
}

.tree-caret {
  width: 14px;
  flex: 0 0 14px;
  color: var(--text-3);
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.tree-icon {
  flex: none;
  color: var(--text-3);
}

.tree-icon.folder-icon {
  color: #f5a623;
}

:root[theme-mode='dark'] .tree-icon.folder-icon {
  color: #d9a13b;
}

.tree-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tree-count,
.tree-badge {
  font-size: 11px;
  color: var(--text-3);
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.tree-badge {
  color: var(--primary);
}

.tree-actions {
  flex: none;
  display: none;
  align-items: center;
  gap: 2px;
}

.tree-row:hover .tree-actions {
  display: inline-flex;
}

.tree-rename {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-family: var(--font-sans);
  color: var(--text);
  background: var(--panel);
  border: 1px solid var(--primary);
  border-radius: var(--radius-s);
  padding: 3px 6px;
  outline: none;
}

/* 触屏没有 hover：行内操作常驻，并把行高放大一点 */
@media (pointer: coarse) {
  .tree-actions {
    display: inline-flex;
  }

  .tree-row {
    padding-top: 9px;
    padding-bottom: 9px;
  }
}

.note-item {
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
}

.note-item:hover {
  background: var(--hover);
}

.note-item.active {
  background: var(--brand-weak);
}

.note-title {
  font-size: 14px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.note-meta {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 3px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hit-snippet {
  font-size: 12px;
  color: var(--text-2);
  margin-top: 3px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.editor-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--panel);
}

.editor-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
}

.clips-panel {
  border-bottom: 1px solid var(--border);
  padding: 8px 16px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--panel-2);
}

.clips-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.clips-title {
  font-size: 12px;
  color: var(--text-3);
}

.clip-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
}

.clip-name {
  font-family: ui-monospace, monospace;
}

.clip-meta {
  color: var(--text-3);
}

.editor-progress {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  border-bottom: 1px solid var(--border);
}

.editor-progress-text {
  font-size: 12px;
  color: var(--text-3);
  white-space: nowrap;
}

/* `/v` 录音选择器 */
.slash {
  position: absolute;
  left: 24px;
  bottom: 24px;
  width: 460px;
  max-width: calc(100% - 48px);
  max-height: 260px;
  overflow: auto;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--shadow-3);
  z-index: 5;
}

.slash-head {
  font-size: 11px;
  color: var(--text-3);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
}

.slash-empty {
  font-size: 12px;
  color: var(--text-3);
  padding: 10px;
  line-height: 1.8;
}

.slash-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
}

.slash-item.on,
.slash-item:hover {
  background: var(--brand-weak);
}

.slash-tag {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--text-3);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0 6px;
}

.slash-tag.new {
  color: var(--primary);
  border-color: var(--primary);
}

.slash-name {
  flex: 1;
  font-family: ui-monospace, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.slash-meta {
  color: var(--text-3);
}

.doc-title {
  font-weight: 600;
  font-size: 14px;
}

.doc-path {
  font-size: 12px;
  color: var(--text-3);
}

.save-state {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 12px;
  padding: 1px 8px;
  border-radius: 999px;
  color: var(--text-3);
  background: var(--panel-2);
}

.save-state.dirty {
  color: var(--warning);
  background: transparent;
}

.save-state.saving {
  color: var(--text-3);
}

.save-state.saved {
  color: var(--success);
  background: transparent;
}

.editor-body {
  flex: 1;
  min-height: 0;
  display: flex;
  position: relative;
}

.editor {
  flex: 1;
  border: none;
  outline: none;
  resize: none;
  padding: 16px 20px;
  font-size: 14px;
  line-height: 1.7;
  font-family: var(--font-mono);
  background: var(--panel);
  color: var(--text);
}

.preview {
  flex: 1;
  overflow: auto;
  padding: 16px 24px;
}

.scan-msg {
  font-size: 12px;
  color: var(--text-3);
}

/* 右键菜单 */
.ctx-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1000;
}

.ctx {
  position: fixed;
  z-index: 1001;
  min-width: 150px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: var(--shadow-3);
  padding: 4px;
}

.ctx-item {
  font-size: 13px;
  padding: 7px 10px;
  border-radius: 6px;
  cursor: pointer;
}

.ctx-item:hover {
  background: var(--hover);
}

.dialog-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}

.dialog-label {
  width: 52px;
  font-size: 13px;
  color: var(--text-2);
  flex: 0 0 52px;
}

.dialog-hint {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.7;
}

.dialog-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 4px;
}

.unref-text {
  font-size: 13px;
  line-height: 1.8;
  margin: 0 0 6px;
}

.unref-list {
  margin: 0 0 12px;
  padding-left: 18px;
  font-size: 12px;
  color: var(--text-2);
  font-family: ui-monospace, monospace;
  max-height: 180px;
  overflow: auto;
}
</style>
