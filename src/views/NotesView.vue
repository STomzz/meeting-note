<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import MarkdownIt from 'markdown-it'
import { useNotesStore } from '../stores/notes'

const store = useNotesStore()
const md = new MarkdownIt({ html: false, linkify: true })

const query = ref('')
const preview = ref(false)
const showNew = ref(false)
const newTitle = ref('')
const newFolder = ref('')

const rendered = computed(() => md.render(store.content || ''))

onMounted(async () => {
  await store.init()
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))

function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    void store.save()
  }
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
          <div class="editor-body">
            <textarea
              v-if="!preview"
              class="editor"
              :value="store.content"
              spellcheck="false"
              @input="store.setContent(($event.target as HTMLTextAreaElement).value)"
            />
            <div v-else class="preview markdown-body" v-html="rendered" />
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
