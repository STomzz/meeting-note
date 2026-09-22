<script setup lang="ts">
/**
 * 问答：检索本地笔记片段，交给对话模型作答，附引用来源。
 *
 * 排版参考 WeKnora：提问是右侧气泡，回答是正文直排（.md-body），
 * 上面一行「检索完成 · 引用 N 篇」可展开看检索步骤；来源点击可打开笔记并定位到行。
 */
import { computed, nextTick, onMounted, ref } from 'vue'
import MarkdownIt from 'markdown-it'
import { DialogPlugin, MessagePlugin } from 'tdesign-vue-next'
import { useRouter } from 'vue-router'
import { useChatStore, type ChatEntry } from '../stores/chat'
import { useNotesStore } from '../stores/notes'
import { citeMarkup } from '../core/citations'
import { modeLabel, type RetrievedChunk } from '../core/retrieval'

const chat = useChatStore()
const notes = useNotesStore()
const router = useRouter()
const md = new MarkdownIt({ html: false, linkify: true })

const question = ref('')
/** ↑/↓ 找回最近提问 */
const asked: string[] = []
let askedIndex = -1
const expandedTrace = ref<Set<number>>(new Set())
const copiedId = ref(0)
/** 会话历史栏：窄屏默认收起（点标题栏「历史」展开） */
const historyOpen = ref(typeof window !== 'undefined' && window.innerWidth > 900)
const renamingId = ref('')
const renameValue = ref('')

const samples = [
  '镜像拉取超时最后是怎么解决的？',
  'Rust 所有权有哪三条规则？',
  '最近的周会有哪些待办？',
]

const status = computed(() => chat.status)

const indexDetail = computed(() => {
  const s = status.value
  if (!s) return chat.statusError || '检索状态未知'
  const parts = [chat.modeText, `笔记块 ${s.chunks}`, `向量 ${s.vectors}`]
  if (s.hasEmbedding && s.pending > 0) parts.push(`待构建 ${s.pending}`)
  if (s.chatModel) parts.push(`对话 ${s.chatModel}`)
  if (s.hasRerank) parts.push(`重排 ${s.rerankModel}`)
  if (chat.buildMessage) parts.push(chat.buildMessage)
  return parts.join(' · ')
})

onMounted(() => {
  void chat.loadStatus()
  void chat.loadHistory()
})

function startNewConversation() {
  chat.newConversation()
  historyOpen.value = window.innerWidth > 900
}

function openConversation(id: string) {
  chat.selectConversation(id)
  if (window.innerWidth <= 900) historyOpen.value = false
}

function startRename(c: { id: string; title: string }) {
  renamingId.value = c.id
  renameValue.value = c.title
  void nextTick(() => document.querySelector<HTMLInputElement>('.history-rename')?.select())
}

function commitRename(id: string) {
  if (renamingId.value !== id) return
  const value = renameValue.value
  renamingId.value = ''
  chat.renameConversation(id, value)
}

function confirmRemove(c: { id: string; title: string }) {
  const dialog = DialogPlugin.confirm({
    header: '删除会话',
    body: `确定删除「${c.title}」吗？该会话的问答记录会一起删除（笔记与录音不受影响）。`,
    confirmBtn: '删除',
    theme: 'danger',
    onConfirm: () => {
      chat.removeConversation(c.id)
      dialog.hide()
    },
  })
}

async function submit() {
  const q = question.value.trim()
  if (!q) return
  if (chat.status && !chat.status.hasChat) {
    void MessagePlugin.warning('还没配置对话模型，请先到「设置」里填写对话端点')
    return
  }
  question.value = ''
  asked.unshift(q)
  if (asked.length > 20) asked.pop()
  askedIndex = -1
  await chat.ask(q)
}

function useSample(s: string) {
  question.value = s
  void submit()
}

/** Enter 发送，Shift+Enter 换行；空输入时 ↑/↓ 找回历史提问。 */
function onComposerKey(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    if (e.isComposing || e.keyCode === 229) return
    e.preventDefault()
    void submit()
    return
  }
  if (e.key === 'ArrowUp' && !question.value.includes('\n')) {
    if (!asked.length) return
    e.preventDefault()
    askedIndex = Math.min(askedIndex + 1, asked.length - 1)
    question.value = asked[askedIndex]
    return
  }
  if (e.key === 'ArrowDown' && !question.value.includes('\n') && askedIndex >= 0) {
    e.preventDefault()
    askedIndex -= 1
    question.value = askedIndex >= 0 ? asked[askedIndex] : ''
  }
}

async function openSource(s: RetrievedChunk) {
  try {
    if (!notes.notes.length) await notes.init()
    await notes.openNote(s.noteId)
    notes.locateNote(s.noteId, s.startLine, s.endLine)
    await router.push({ path: '/notes' })
  } catch (e) {
    void MessagePlugin.error(String(e))
  }
}

function toggleTrace(id: number) {
  const next = new Set(expandedTrace.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expandedTrace.value = next
}

/**
 * 回答里的 [1] [2] 渲染成可点的引用角标：
 * markdown-it 的 text 规则只作用于正文（代码块、链接语法不受影响），
 * 悬停有来源提示，点击跳到对应来源行。
 */
md.renderer.rules.text = (tokens, idx, _options, env) => {
  const sources = ((env as { sources?: RetrievedChunk[] }).sources ?? []) as RetrievedChunk[]
  return citeMarkup(md.utils.escapeHtml(tokens[idx].content), sources)
}

function render(text: string, sources: RetrievedChunk[] = []) {
  return md.render(text || '', { sources })
}

/** 点回答里的引用角标 → 打开对应来源并定位到行。 */
function onStreamClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  const cite = target.closest('.cite') as HTMLElement | null
  if (!cite) return
  const entryEl = target.closest('[data-entry-id]') as HTMLElement | null
  const entry = chat.entries.find((x) => x.id === Number(entryEl?.dataset.entryId))
  const source = entry?.answer?.sources[Number(cite.dataset.cite) - 1]
  if (source) void openSource(source)
}

function shorten(text: string, n = 140) {
  const t = text.replace(/\s+/g, ' ').trim()
  return t.length > n ? `${t.slice(0, n)}…` : t
}

async function copyAnswer(e: ChatEntry) {
  if (!e.answer) return
  const ok = await copyText(e.answer.answer)
  if (!ok) {
    void MessagePlugin.warning('复制失败，请手动选择文本')
    return
  }
  copiedId.value = e.id
  void MessagePlugin.success('已复制回答')
  window.setTimeout(() => {
    if (copiedId.value === e.id) copiedId.value = 0
  }, 1600)
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    // WebView 里 clipboard 可能被拒：退回 execCommand
    try {
      const el = document.createElement('textarea')
      el.value = text
      el.style.position = 'fixed'
      el.style.opacity = '0'
      document.body.appendChild(el)
      el.select()
      const ok = document.execCommand('copy')
      document.body.removeChild(el)
      return ok
    } catch {
      return false
    }
  }
}

function exportEntry(e: ChatEntry) {
  if (!e.answer) return
  const lines = [`# ${e.question}`, '', e.answer.answer, '']
  if (e.answer.sources.length) {
    lines.push('## 引用来源', '')
    e.answer.sources.forEach((s, i) => {
      lines.push(`${i + 1}. ${s.title}（${s.noteId} 第 ${s.startLine}-${s.endLine} 行）`)
    })
  }
  const blob = new Blob([lines.join('\n')], { type: 'text/markdown;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${e.question.slice(0, 20) || '问答'}.md`
  a.click()
  URL.revokeObjectURL(url)
}

/** 把这条问答存成 vault 里的笔记（`问答/<问题>.md`）。 */
async function saveAsNote(e: ChatEntry) {
  if (!e.answer) return
  const title = e.question.length > 20 ? `${e.question.slice(0, 20)}…` : e.question
  try {
    await notes.createNote('问答', title)
    const parts = [`# ${title}`, '', `> ${e.question}`, '', e.answer.answer, '']
    if (e.answer.sources.length) {
      parts.push('## 引用来源', '')
      e.answer.sources.forEach((s, i) => {
        parts.push(`${i + 1}. ${s.title}（\`${s.noteId}\` 第 ${s.startLine}-${s.endLine} 行）`)
      })
    }
    notes.setContent(parts.join('\n'))
    await notes.save()
    void MessagePlugin.success('已存为笔记「问答/' + title + '」')
    await router.push({ path: '/notes' })
  } catch (err) {
    void MessagePlugin.error(String(err))
  }
}

function stepList(e: ChatEntry): string[] {
  const a = e.answer
  if (!a) return []
  const steps: string[] = []
  steps.push(`全文检索命中 ${a.trace.ftsHits} 段`)
  if (a.trace.vectorHits) steps.push(`向量检索命中 ${a.trace.vectorHits} 段`)
  if (a.trace.graphAdded) steps.push(`图谱邻居补召回 ${a.trace.graphAdded} 段`)
  steps.push(`重排后取前 ${a.sources.length} 段作为依据`)
  steps.push(`生成完成：${a.model}${a.completionTokens ? ` · ${a.completionTokens} tokens` : ''}`)
  return steps
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <button class="icon-btn history-toggle" title="会话历史" @click="historyOpen = !historyOpen">
        <t-icon name="history" size="16px" />
      </button>
      <span class="page-title">问答</span>
      <span class="page-sub">基于本地笔记的检索增强问答</span>
      <span class="spacer" />
      <t-popup trigger="hover" :content="indexDetail" placement="bottom-right">
        <span class="chip muted index-chip">
          <t-icon name="search" size="13px" />
          {{ chat.modeText || '检索模式未知' }}
          <template v-if="status && status.hasEmbedding && status.pending > 0">
            · 待构建 {{ status.pending }}
          </template>
        </span>
      </t-popup>
      <t-button
        v-if="chat.status?.hasEmbedding"
        size="small"
        variant="text"
        :loading="chat.building"
        @click="chat.buildIndex()"
      >
        构建索引
      </t-button>
      <t-button v-if="chat.entries.length" size="small" variant="text" @click="chat.clear()">
        清空
      </t-button>
    </header>

    <div class="chat-shell">
      <aside class="history" :class="{ open: historyOpen }">
        <div class="history-head">
          <span class="history-title">会话</span>
          <button class="icon-btn" title="新会话" @click="startNewConversation">
            <t-icon name="add" size="15px" />
          </button>
        </div>
        <div class="history-list">
          <template v-for="g in chat.historyGroups" :key="g.label">
            <div class="history-group">{{ g.label }}</div>
            <div
              v-for="c in g.items"
              :key="c.id"
              class="history-item"
              :class="{ active: c.id === chat.currentConversationId }"
              @click="openConversation(c.id)"
              @dblclick.stop="startRename(c)"
            >
              <input
                v-if="renamingId === c.id"
                v-model="renameValue"
                class="history-rename"
                @click.stop
                @keydown.enter.stop="commitRename(c.id)"
                @keydown.esc.stop="renamingId = ''"
                @blur="commitRename(c.id)"
              />
              <template v-else>
                <t-icon v-if="c.pinned" name="pin" size="12px" class="history-pin" />
                <span class="history-name">{{ c.title }}</span>
                <span class="history-actions">
                  <button class="icon-btn" title="重命名" @click.stop="startRename(c)">
                    <t-icon name="edit-2" size="13px" />
                  </button>
                  <button
                    class="icon-btn"
                    :title="c.pinned ? '取消置顶' : '置顶'"
                    @click.stop="chat.togglePinConversation(c.id)"
                  >
                    <t-icon name="pin" size="13px" />
                  </button>
                  <button class="icon-btn" title="删除" @click.stop="confirmRemove(c)">
                    <t-icon name="delete" size="13px" />
                  </button>
                </span>
              </template>
            </div>
          </template>
          <div v-if="!chat.conversations.length" class="history-empty">还没有会话</div>
        </div>
        <div v-if="chat.historyError" class="history-error">{{ chat.historyError }}</div>
      </aside>

      <section class="chat-main">
        <div class="chat-body" @click="onStreamClick">
      <div class="stream">
        <div v-if="!chat.entries.length" class="empty-state">
          <t-icon name="chat-bubble" size="30px" class="empty-icon" />
          <div class="empty-title">问点什么，从你的笔记里找答案</div>
          <div class="empty-desc">
            检索只在本机进行（全文 + 可选向量，再叠加图谱邻居），只有命中的片段会发给对话模型。
          </div>
          <div class="samples">
            <button v-for="s in samples" :key="s" class="sample" @click="useSample(s)">
              {{ s }}
            </button>
          </div>
        </div>

        <div v-for="e in chat.entries" :key="e.id" class="entry fade-in" :data-entry-id="e.id">
          <div class="q-row">
            <div class="q-bubble">{{ e.question }}</div>
          </div>

          <div v-if="e.pending" class="pending">
            <t-loading size="small" />
            <span>正在检索…</span>
          </div>
          <div v-else-if="e.streaming" class="pending">
            <span class="pulse" />
            <span>正在生成…</span>
            <button class="stop-inline" @click="chat.stop()">
              <t-icon name="stop-circle" size="14px" />
              停止
            </button>
          </div>
          <div v-else-if="e.stopped" class="pending stopped">
            <t-icon name="stop-circle" size="14px" />
            <span>已停止生成（可复制已有内容，或点「重新生成」）</span>
          </div>

          <t-alert v-if="e.error" theme="error" :message="e.error" class="entry-alert" />

          <template v-if="e.answer">
            <!-- 检索过程：一行摘要，点开看步骤 -->
            <button v-if="!e.streaming" class="trace-line" @click="toggleTrace(e.id)">
              <t-icon name="check-circle" size="15px" class="trace-icon" />
              <span>
                检索完成 · 引用 {{ e.answer.sources.length }} 篇 · {{ e.answer.elapsedMs }} ms
              </span>
              <t-icon :name="expandedTrace.has(e.id) ? 'chevron-down' : 'chevron-right'" size="14px" />
            </button>
            <ol v-if="!e.streaming && expandedTrace.has(e.id)" class="trace-steps fade-in">
              <li v-for="(s, i) in stepList(e)" :key="i">{{ s }}</li>
            </ol>

            <details v-if="e.reasoning" class="reasoning">
              <summary>思考过程（{{ e.reasoning.length }} 字）</summary>
              <pre>{{ e.reasoning }}</pre>
            </details>

            <div class="answer md-body" v-html="render(e.answer.answer, e.answer.sources)" />

            <div v-for="(d, i) in e.answer.trace.degraded" :key="i" class="degraded">
              ⚠️ {{ d }}
            </div>

            <div v-if="!e.streaming" class="answer-bar">
              <button class="icon-btn" title="复制回答" @click="copyAnswer(e)">
                <t-icon :name="copiedId === e.id ? 'check' : 'copy'" size="14px" />
                <span>{{ copiedId === e.id ? '已复制' : '复制' }}</span>
              </button>
              <button class="icon-btn" title="重新生成" @click="chat.askAgain(e)">
                <t-icon name="refresh" size="14px" />
                <span>重新生成</span>
              </button>
              <button class="icon-btn" title="导出 Markdown" @click="exportEntry(e)">
                <t-icon name="download" size="14px" />
                <span>导出</span>
              </button>
              <button class="icon-btn" title="存为笔记" @click="saveAsNote(e)">
                <t-icon name="file-add" size="14px" />
                <span>存为笔记</span>
              </button>
              <span class="spacer" />
              <span class="answer-meta">
                {{ modeLabel(e.answer.trace.mode) }}
                <template v-if="e.answer.completionTokens">
                  · {{ e.answer.completionTokens }} tokens
                </template>
              </span>
            </div>

            <div v-if="e.answer.sources.length" class="sources">
              <div class="sources-head">引用来源（{{ e.answer.sources.length }}）</div>
              <div
                v-for="(s, i) in e.answer.sources"
                :key="s.chunkId"
                class="source-row"
                @click="openSource(s)"
              >
                <span class="src-idx">{{ i + 1 }}</span>
                <div class="src-main">
                  <div class="src-title">
                    {{ s.title }}
                    <span class="src-loc">{{ s.noteId }} · 第 {{ s.startLine }}-{{ s.endLine }} 行</span>
                  </div>
                  <div class="src-snippet">{{ shorten(s.text) }}</div>
                </div>
                <span class="src-open">
                  <t-icon name="link" size="13px" />
                  打开定位
                </span>
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>

        <footer class="composer">
      <div class="composer-card">
        <textarea
          v-model="question"
          class="composer-input"
          rows="1"
          placeholder="基于你的笔记提问…（Enter 发送，Shift+Enter 换行）"
          @keydown="onComposerKey"
        />
        <div class="composer-bar">
          <span class="chip muted">{{ chat.modeText || '检索' }}</span>
          <span v-if="status?.chatModel" class="chip muted">{{ status.chatModel }}</span>
          <span class="spacer" />
          <t-button
            v-if="chat.streaming"
            theme="danger"
            variant="outline"
            shape="round"
            size="small"
            @click="chat.stop()"
          >
            <t-icon name="stop-circle" size="14px" />
            停止生成
          </t-button>
          <t-button
            v-else
            theme="primary"
            shape="round"
            size="small"
            :disabled="!question.trim()"
            @click="submit"
          >
            提问
          </t-button>
        </div>
          </div>
        </footer>
      </section>
    </div>
  </div>
</template>

<style scoped>
.chat-shell {
  flex: 1;
  min-height: 0;
  display: flex;
  position: relative;
}

.chat-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

/* ---------------- 会话历史栏 ---------------- */
.history {
  display: none;
  width: 216px;
  flex: none;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--panel-2);
}

.history.open {
  display: flex;
}

.history-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 8px 6px 12px;
}

.history-title {
  flex: 1;
  font-size: 12px;
  color: var(--text-3);
}

.history-list {
  flex: 1;
  overflow: auto;
  padding: 0 8px 10px;
}

.history-group {
  font-size: 11px;
  color: var(--text-3);
  padding: 10px 6px 4px;
}

.history-item {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 4px 6px 8px;
  border-radius: var(--radius-m);
  cursor: pointer;
  font-size: 13px;
}

.history-item:hover {
  background: var(--hover);
}

.history-item.active {
  background: var(--brand-weak);
}

.history-item.active .history-name {
  color: var(--primary);
  font-weight: 500;
}

.history-pin {
  flex: none;
  color: var(--warning);
}

.history-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-actions {
  flex: none;
  display: none;
  align-items: center;
  gap: 1px;
}

.history-item:hover .history-actions {
  display: inline-flex;
}

.history-rename {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-family: var(--font-sans);
  color: var(--text);
  background: var(--panel);
  border: 1px solid var(--primary);
  border-radius: var(--radius-s);
  padding: 2px 6px;
  outline: none;
}

.history-empty,
.history-error {
  font-size: 12px;
  color: var(--text-3);
  padding: 10px 8px;
}

.history-error {
  color: var(--danger);
  word-break: break-all;
}

@media (pointer: coarse) {
  .history-actions {
    display: inline-flex;
  }
}

/* 窄屏：历史栏变抽屉浮层 */
@media (max-width: 900px) {
  .history.open {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 30;
    box-shadow: var(--shadow-3);
    width: 240px;
  }
}

.chat-body {
  flex: 1;
  overflow: auto;
  padding: 20px 20px 0;
}

.stream {
  max-width: 780px;
  margin: 0 auto;
  padding-bottom: 24px;
}

.index-chip {
  max-width: 42vw;
  overflow: hidden;
  text-overflow: ellipsis;
}

.empty-state {
  padding-top: 64px;
}

.samples {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: center;
  margin-top: 14px;
  max-width: 560px;
}

.sample {
  font-size: 13px;
  color: var(--text-2);
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 6px 14px;
  cursor: pointer;
  background: var(--panel);
  transition: border-color 0.15s ease, color 0.15s ease, background 0.15s ease;
}

.sample:hover {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--brand-weak);
}

.entry {
  margin-bottom: 28px;
}

.q-row {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 10px;
}

.q-bubble {
  max-width: 86%;
  background: var(--brand-weak);
  color: var(--text);
  border-radius: 14px 14px 4px 14px;
  padding: 9px 14px;
  font-size: 14.5px;
  line-height: 1.6;
  white-space: pre-wrap;
}

/* 行内引用角标（[1] [2]） */
.answer :deep(.cite) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 15px;
  height: 15px;
  margin: 0 1px;
  padding: 0 3px;
  border-radius: 4px;
  background: var(--brand-weak);
  color: var(--primary);
  font-size: 10.5px;
  font-weight: 600;
  line-height: 1;
  vertical-align: super;
  cursor: pointer;
  user-select: none;
}

.answer :deep(.cite:hover) {
  background: var(--primary);
  color: #fff;
}

.pending {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-3);
  padding: 6px 2px;
}

.entry-alert {
  margin: 8px 0;
}

.pulse {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--primary);
  animation: pulse 1.1s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.35;
    transform: scale(0.8);
  }
}

.stop-inline {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin-left: 4px;
  padding: 2px 8px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
}

.stop-inline:hover {
  color: var(--danger);
  border-color: var(--danger);
}

.pending.stopped {
  color: var(--text-3);
}

.trace-line {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 12.5px;
  padding: 4px 0;
  cursor: pointer;
}

.trace-line:hover {
  color: var(--primary);
}

.trace-icon {
  color: var(--success);
}

.trace-steps {
  margin: 4px 0 10px;
  padding-left: 22px;
  font-size: 12.5px;
  color: var(--text-2);
  line-height: 1.9;
}

.answer {
  padding: 2px 0 0;
}

.degraded {
  font-size: 12px;
  color: var(--warning);
  margin-top: 8px;
  line-height: 1.7;
}

.reasoning {
  margin: 4px 0 8px;
  font-size: 12.5px;
  color: var(--text-3);
}

.reasoning summary {
  cursor: pointer;
}

.reasoning pre {
  margin: 6px 0 0;
  padding: 8px 12px;
  background: var(--panel-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  white-space: pre-wrap;
  font-family: var(--font-sans);
  line-height: 1.7;
  color: var(--text-2);
}

.answer-bar {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-top: 10px;
  padding-top: 8px;
  border-top: 1px dashed var(--border);
}

.answer-meta {
  font-size: 11.5px;
  color: var(--text-3);
}

.sources {
  margin-top: 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sources-head {
  font-size: 12px;
  color: var(--text-3);
}

.source-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  padding: 8px 12px;
  cursor: pointer;
  background: var(--panel);
  transition: border-color 0.15s ease, background 0.15s ease;
}

.source-row:hover {
  border-color: var(--primary);
  background: var(--brand-weak);
}

.src-idx {
  flex: none;
  width: 20px;
  height: 20px;
  border-radius: 6px;
  background: var(--brand-weak);
  color: var(--primary);
  font-size: 11.5px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-top: 1px;
}

.src-main {
  flex: 1;
  min-width: 0;
}

.src-title {
  font-size: 13px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.src-loc {
  font-weight: 400;
  font-size: 11.5px;
  color: var(--text-3);
  margin-left: 6px;
}

.src-snippet {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 3px;
  line-height: 1.6;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.src-open {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 12px;
  color: var(--primary);
  opacity: 0;
  transition: opacity 0.15s ease;
  margin-top: 2px;
}

.source-row:hover .src-open {
  opacity: 1;
}

.composer {
  padding: 10px 20px 16px;
  background: linear-gradient(to top, var(--bg) 70%, transparent);
}

.composer-card {
  max-width: 780px;
  margin: 0 auto;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-l);
  box-shadow: var(--shadow-2);
  padding: 10px 12px 8px;
  transition: border-color 0.15s ease;
}

.composer-card:focus-within {
  border-color: var(--primary);
}

.composer-input {
  width: 100%;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 14.5px;
  line-height: 1.6;
  max-height: 160px;
  min-height: 24px;
  padding: 2px 2px 6px;
}

.composer-bar {
  display: flex;
  align-items: center;
  gap: 6px;
}
</style>
