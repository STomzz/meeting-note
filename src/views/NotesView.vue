<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import MarkdownIt from 'markdown-it'
import { useNotesStore } from '../stores/notes'
import { useMeetingNoteStore } from '../stores/meetingNote'
import {
  formatDur,
  insertRefAtCursor,
  prepareAudioRefs,
  replaceAudioPlaceholders,
  parseRefs,
  slashCommandAt,
} from '../core/meetingNote'
import type { ClipInfo } from '../core/meetingNote'

const store = useNotesStore()
const meeting = useMeetingNoteStore()
const md = new MarkdownIt({ html: false, linkify: true })

const query = ref('')
const preview = ref(false)
const showNew = ref(false)
const newTitle = ref('')
const newFolder = ref('')

const editorEl = ref<HTMLTextAreaElement | null>(null)

/** vault 内音频的播放地址缓存（key = `/v` 里的路径原文） */
const audioSrcMap = ref<Record<string, string | null>>({})
const loadingSrc = new Set<string>()

/** `/v` 选择器状态 */
const slashOpen = ref(false)
const slashFilter = ref('')
const slashIndex = ref(0)

const refCount = computed(() => parseRefs(store.content).length)

const slashMatches = computed(() => {
  const q = slashFilter.value.toLowerCase()
  const list = meeting.clipOptions
  if (!q) return list.slice(0, 8)
  return list.filter((c) => `${c.dir}/${c.file}`.toLowerCase().includes(q)).slice(0, 8)
})

const rendered = computed(() => renderMarkdown(store.content))

/** 预览：把 `/v 音频` 行渲染成播放器（src 异步取，取不到时先显示文件名）。 */
function renderMarkdown(body: string): string {
  const html = md.render(prepareAudioRefs(body))
  return replaceAudioPlaceholders(html, (raw) => {
    const src = resolveSrc(raw)
    if (!src) return `<p class="audio-ref">🎧 <code>${escapeHtml(raw)}</code>（暂不可播放）</p>`
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

function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    void store.save()
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

/** 编辑器里直接「一键处理」：先保存再处理，避免处理到旧内容。 */
async function processCurrent() {
  if (!store.currentId || meeting.processing) return
  await store.save()
  await meeting.openNote(store.currentId)
  await meeting.process(false)
}

function fmt(ts: number): string {
  if (!ts) return ''
  const d = new Date(ts * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

async function onSearch() {
  await store.search(query.value)
}

function openHit(noteId: string) {
  void store.openNote(noteId)
}

async function open(id: string) {
  await store.openNote(id)
  preview.value = false
}

async function create() {
  const title = newTitle.value.trim()
  if (!title) return
  await store.createNote(newFolder.value.trim(), title)
  showNew.value = false
  newTitle.value = ''
  newFolder.value = ''
}

async function removeCurrent() {
  if (!store.currentId) return
  const ok = window.confirm(`确定删除「${store.current?.title ?? store.currentId}」？文件会从磁盘删除。`)
  if (!ok) return
  await store.removeNote(store.currentId)
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">笔记</span>
      <span class="page-sub" :title="store.vault">{{ store.vault }}</span>
      <span v-if="store.scanMessage" class="scan-msg">{{ store.scanMessage }}</span>
      <span class="spacer" />
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
          <t-button size="small" theme="primary" @click="showNew = true">新建</t-button>
        </div>

        <div class="chips">
          <span
            class="chip"
            :class="{ on: store.folderFilter === '' }"
            @click="store.folderFilter = ''"
            >全部</span
          >
          <span
            v-for="f in store.folders"
            :key="f.path || '(根)'"
            class="chip"
            :class="{ on: store.folderFilter === f.path }"
            @click="store.folderFilter = f.path"
            >{{ f.path || '根目录' }} {{ f.count }}</span
          >
        </div>
        <div v-if="store.tags.length" class="chips">
          <span
            v-for="t in store.tags"
            :key="t"
            class="chip tag"
            :class="{ on: store.tagFilter === t }"
            @click="store.tagFilter = store.tagFilter === t ? '' : t"
            >#{{ t }}</span
          >
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
          </template>

          <!-- 笔记列表 -->
          <template v-else>
            <div class="list-label">{{ store.visibleNotes.length }} 篇笔记</div>
            <div
              v-for="n in store.visibleNotes"
              :key="n.id"
              class="note-item"
              :class="{ active: n.id === store.currentId }"
              @click="open(n.id)"
            >
              <div class="note-title">{{ n.title }}</div>
              <div class="note-meta">
                {{ n.folder || '根目录' }} · {{ fmt(n.updatedAt) }}
              </div>
            </div>
            <div v-if="!store.visibleNotes.length" class="empty">还没有笔记，点「新建」开始</div>
          </template>
        </div>
      </aside>

      <section class="editor-pane">
        <template v-if="store.currentId">
          <div class="editor-bar">
            <span class="doc-title">{{ store.current?.title ?? store.currentId }}</span>
            <span class="doc-path">{{ store.currentId }}</span>
            <span v-if="store.dirty" class="dirty">未保存</span>
            <span class="spacer" />
            <t-button
              v-if="refCount"
              size="small"
              variant="outline"
              :loading="meeting.processing"
              @click="processCurrent"
            >
              一键处理（{{ refCount }} 段录音）
            </t-button>
            <t-button size="small" :disabled="!store.dirty" theme="primary" @click="store.save()"
              >保存</t-button
            >
            <t-button size="small" variant="outline" @click="preview = !preview">
              {{ preview ? '编辑' : '预览' }}
            </t-button>
            <t-button size="small" theme="danger" variant="text" @click="removeCurrent"
              >删除</t-button
            >
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
            <div v-else class="preview markdown-body" v-html="rendered" />
            <div v-if="slashOpen" class="slash">
              <div class="slash-head">
                插入录音引用（↑↓ 选择，Enter 插入，Esc 取消）
              </div>
              <div v-if="!slashMatches.length" class="slash-empty">
                还没有录音。用右下角 🎙 录一段，或先录到 <code>会议音频/</code> 下。
              </div>
              <div
                v-for="(c, i) in slashMatches"
                :key="c.path"
                class="slash-item"
                :class="{ on: i === slashIndex }"
                @mousedown.prevent="applyClip(c)"
              >
                <span class="slash-name">{{ c.dir }}/{{ c.file }}</span>
                <span class="slash-meta">{{ formatDur(c.durationMs) }}</span>
              </div>
            </div>
          </div>
        </template>
        <div v-else class="empty">从左侧选择一篇笔记，或新建一篇</div>
      </section>
    </div>

    <t-dialog
      v-model:visible="showNew"
      header="新建笔记"
      :confirm-btn="{ content: '创建', disabled: !newTitle.trim() }"
      @confirm="create"
    >
      <div class="dialog-row">
        <span class="dialog-label">标题</span>
        <t-input v-model="newTitle" placeholder="例如：周会纪要" />
      </div>
      <div class="dialog-row">
        <span class="dialog-label">文件夹</span>
        <t-input v-model="newFolder" placeholder="可留空，例如：工作" />
      </div>
    </t-dialog>
  </div>
</template>

<style scoped>
.notes {
  flex: 1;
  min-height: 0;
  display: flex;
}

.list-pane {
  width: 300px;
  flex: 0 0 300px;
  border-right: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.list-head {
  display: flex;
  gap: 8px;
  padding: 12px;
  border-bottom: 1px solid var(--border);
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 8px 12px 0;
}

.chip {
  font-size: 12px;
  color: var(--text-2);
  background: #f2f3f5;
  border-radius: 10px;
  padding: 2px 8px;
  cursor: pointer;
  user-select: none;
}

.chip.on {
  background: var(--primary);
  color: #fff;
}

.chip.tag {
  background: #eef3ff;
  color: var(--primary);
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

.note-item {
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
}

.note-item:hover {
  background: #f5f6f8;
}

.note-item.active {
  background: #eef3ff;
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
  width: 420px;
  max-width: calc(100% - 48px);
  max-height: 260px;
  overflow: auto;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.14);
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
  background: rgba(0, 82, 217, 0.08);
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

.preview :deep(.audio-ref) {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 8px 0;
}

.preview :deep(.audio-ref audio) {
  width: 100%;
  max-width: 520px;
}

.preview :deep(.audio-ref .audio-name) {
  font-size: 11px;
  color: var(--text-3);
  font-family: ui-monospace, monospace;
}

.doc-title {
  font-weight: 600;
  font-size: 14px;
}

.doc-path {
  font-size: 12px;
  color: var(--text-3);
}

.dirty {
  font-size: 12px;
  color: #e37318;
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
  font-family: 'JetBrains Mono', Consolas, 'Courier New', monospace;
  background: var(--panel);
  color: var(--text);
}

.preview {
  flex: 1;
  overflow: auto;
  padding: 16px 24px;
  line-height: 1.75;
  font-size: 14px;
}

.preview :deep(h1) {
  font-size: 22px;
  border-bottom: 1px solid var(--border);
  padding-bottom: 6px;
}
.preview :deep(h2) {
  font-size: 18px;
}
.preview :deep(code) {
  background: #f2f3f5;
  padding: 1px 5px;
  border-radius: 4px;
  font-size: 13px;
}
.preview :deep(pre) {
  background: #f7f8fa;
  padding: 12px;
  border-radius: 8px;
  overflow: auto;
}
.preview :deep(blockquote) {
  margin: 0;
  padding: 4px 12px;
  border-left: 3px solid var(--border);
  color: var(--text-2);
}

.scan-msg {
  font-size: 12px;
  color: var(--text-3);
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
</style>
