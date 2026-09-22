<script setup lang="ts">
// P3：检索问答 —— 检索本地笔记片段，交给对话模型作答，附引用来源
import { computed, onMounted, ref } from 'vue'
import MarkdownIt from 'markdown-it'
import { MessagePlugin } from 'tdesign-vue-next'
import { useRouter } from 'vue-router'
import { useChatStore } from '../stores/chat'
import { useNotesStore } from '../stores/notes'
import { MODE_LABEL, type RetrievedChunk } from '../core/retrieval'

const chat = useChatStore()
const notes = useNotesStore()
const router = useRouter()
const md = new MarkdownIt({ html: false, linkify: true })

const question = ref('')

const samples = [
  '镜像拉取超时最后是怎么解决的？',
  'Rust 所有权有哪三条规则？',
  '最近的周会有哪些待办？',
]

const status = computed(() => chat.status)

onMounted(() => {
  void chat.loadStatus()
})

async function submit() {
  const q = question.value.trim()
  if (!q) return
  if (chat.status && !chat.status.hasChat) {
    void MessagePlugin.warning('还没配置对话模型，请先到「设置」里填写对话端点')
    return
  }
  question.value = ''
  await chat.ask(q)
}

function useSample(s: string) {
  question.value = s
  void submit()
}

/** Ctrl/Cmd + Enter 发送（挂在 footer 上，事件从输入框冒泡上来）。 */
function onComposerKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
    e.preventDefault()
    void submit()
  }
}

async function openSource(s: RetrievedChunk) {
  try {
    if (!notes.notes.length) await notes.init()
    await notes.openNote(s.noteId)
    await router.push({ path: '/notes' })
    void MessagePlugin.info(`已打开「${s.title}」（第 ${s.startLine}-${s.endLine} 行）`)
  } catch (e) {
    void MessagePlugin.error(String(e))
  }
}

function render(text: string) {
  return md.render(text || '')
}

function modeLabel(mode: string) {
  return MODE_LABEL[mode] ?? mode
}

function shorten(text: string, n = 160) {
  const t = text.replace(/\s+/g, ' ').trim()
  return t.length > n ? `${t.slice(0, n)}…` : t
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">问答</span>
      <span class="page-sub">基于本地笔记的检索增强问答</span>
      <span class="spacer" />
      <t-button
        v-if="chat.status?.hasEmbedding"
        size="small"
        :loading="chat.building"
        @click="chat.buildIndex()"
      >
        构建向量索引
      </t-button>
      <t-button v-if="chat.entries.length" size="small" variant="outline" @click="chat.clear()">
        清空
      </t-button>
    </header>

    <div class="chat-body">
      <div class="status-bar">
        <t-tag size="small" variant="light">{{ chat.modeText || '检索模式未知' }}</t-tag>
        <span v-if="status" class="stat">
          笔记块 {{ status.chunks }} · 向量 {{ status.vectors }}
          <template v-if="status.hasEmbedding && status.pending > 0">
            · 待构建 {{ status.pending }}
          </template>
        </span>
        <span v-if="status?.chatModel" class="stat">对话模型 {{ status.chatModel }}</span>
        <span v-if="status?.hasRerank" class="stat">重排 {{ status.rerankModel }}</span>
        <span v-if="chat.buildMessage" class="stat build-msg">{{ chat.buildMessage }}</span>
        <span v-if="chat.statusError" class="stat err">{{ chat.statusError }}</span>
      </div>

      <div v-if="!chat.entries.length" class="empty-state">
        <div class="empty-title">问点什么，从你的笔记里找答案</div>
        <div class="empty-sub">
          检索只在本机进行（全文 + 可选向量），只有命中的片段会发给对话模型。
        </div>
        <div class="samples">
          <span v-for="s in samples" :key="s" class="sample" @click="useSample(s)">{{ s }}</span>
        </div>
      </div>

      <div class="entries">
        <div v-for="e in chat.entries" :key="e.id" class="entry">
          <div class="q-row">
            <span class="q-mark">问</span>
            <span class="q-text">{{ e.question }}</span>
          </div>

          <div v-if="e.pending" class="pending">
            <t-loading size="small" />
            <span>正在检索并生成…</span>
          </div>

          <t-alert v-if="e.error" theme="error" :message="e.error" class="entry-alert" />

          <template v-if="e.answer">
            <div class="answer" v-html="render(e.answer.answer)" />

            <div v-for="(d, i) in e.answer.trace.degraded" :key="i" class="degraded">
              ⚠️ {{ d }}
            </div>

            <div class="meta">
              <t-tag size="small" theme="primary" variant="light">
                {{ modeLabel(e.answer.trace.mode) }}
              </t-tag>
              <span class="meta-item">全文命中 {{ e.answer.trace.ftsHits }}</span>
              <span v-if="e.answer.trace.vectorHits" class="meta-item">
                向量命中 {{ e.answer.trace.vectorHits }}
              </span>
              <span class="meta-item">{{ e.answer.elapsedMs }} ms</span>
              <span v-if="e.answer.completionTokens" class="meta-item">
                {{ e.answer.completionTokens }} tokens
              </span>
              <span class="meta-item">{{ e.answer.model }}</span>
            </div>

            <div v-if="e.answer.sources.length" class="sources">
              <div
                v-for="(s, i) in e.answer.sources"
                :key="s.chunkId"
                class="source"
                @click="openSource(s)"
              >
                <div class="source-head">
                  <span class="source-idx">[{{ i + 1 }}]</span>
                  <span class="source-title">{{ s.title }}</span>
                  <span class="source-path">{{ s.noteId }} · 第 {{ s.startLine }}-{{ s.endLine }} 行</span>
                  <span class="spacer" />
                  <t-tag v-for="src in s.sources" :key="src" size="small" variant="outline">
                    {{ src }}
                  </t-tag>
                </div>
                <div class="source-text">{{ shorten(s.text) }}</div>
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>

    <footer class="composer" @keydown="onComposerKey">
      <t-textarea
        v-model="question"
        :autosize="{ minRows: 1, maxRows: 5 }"
        placeholder="问点什么…（Ctrl/Cmd + Enter 发送）"
      />
      <t-button theme="primary" :loading="chat.asking" @click="submit">提问</t-button>
    </footer>
  </div>
</template>

<style scoped>
.chat-body {
  flex: 1;
  overflow: auto;
  padding: 16px 18px 0;
}

.status-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--text-3);
  margin-bottom: 12px;
}

.stat {
  color: var(--text-3);
}

.build-msg {
  color: var(--text-2);
}

.err {
  color: #d54941;
}

.empty-state {
  max-width: 640px;
  margin: 40px auto;
  text-align: center;
}

.empty-title {
  font-size: 16px;
  font-weight: 600;
  margin-bottom: 8px;
}

.empty-sub {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.8;
  margin-bottom: 16px;
}

.samples {
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: center;
}

.sample {
  font-size: 13px;
  color: var(--text-2);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 6px 14px;
  cursor: pointer;
  background: var(--panel);
}

.sample:hover {
  border-color: #0052d9;
  color: #0052d9;
}

.entries {
  max-width: 880px;
  margin: 0 auto;
  padding-bottom: 16px;
}

.entry {
  margin-bottom: 22px;
}

.q-row {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  margin-bottom: 10px;
}

.q-mark {
  flex: none;
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: #0052d9;
  color: #fff;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.q-text {
  font-size: 15px;
  font-weight: 600;
  line-height: 1.6;
}

.pending {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-3);
  padding: 8px 0;
}

.entry-alert {
  margin: 8px 0;
}

.answer {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  font-size: 14px;
  line-height: 1.85;
}

.answer :deep(p) {
  margin: 0 0 10px;
}

.answer :deep(p:last-child) {
  margin-bottom: 0;
}

.answer :deep(code) {
  background: #f2f3f5;
  border-radius: 4px;
  padding: 1px 4px;
  font-size: 13px;
}

.answer :deep(pre) {
  background: #f7f8fa;
  padding: 10px;
  border-radius: 8px;
  overflow: auto;
}

.degraded {
  font-size: 12px;
  color: #a8710b;
  margin-top: 8px;
  line-height: 1.7;
}

.meta {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
  margin-top: 10px;
  font-size: 12px;
  color: var(--text-3);
}

.meta-item {
  color: var(--text-3);
}

.sources {
  margin-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.source {
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 8px 10px;
  cursor: pointer;
  background: var(--panel);
}

.source:hover {
  border-color: #0052d9;
}

.source-head {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.source-idx {
  font-size: 12px;
  color: #0052d9;
}

.source-title {
  font-size: 13px;
  font-weight: 500;
}

.source-path {
  font-size: 12px;
  color: var(--text-3);
}

.source-text {
  font-size: 12px;
  color: var(--text-2);
  margin-top: 4px;
  line-height: 1.7;
}

.composer {
  display: flex;
  gap: 10px;
  align-items: flex-end;
  padding: 12px 18px;
  border-top: 1px solid var(--border);
  background: var(--panel);
}

.composer :deep(.t-textarea) {
  flex: 1;
}
</style>
