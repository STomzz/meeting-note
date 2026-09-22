<script setup lang="ts">
/**
 * 会议（P7）：会议 = vault 里的一篇 Markdown 笔记。
 *
 * - 新建会议就是新建一篇 md（`会议/<日期>-<标题>.md`），随手写、随时插入 `/v 音频` 引用；
 * - 点「一键处理」：转写引用的音频（静音切段 + 限流）→ 写回转写块 → 生成纪要段；
 * - 音频存在 vault 的 `会议音频/` 下，跟着笔记一起备份，可随时试听；
 * - 旧版会议（录音在前的那种）可以一次性导出成会议笔记。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useMeetingNoteStore } from '../stores/meetingNote'
import { useMeetingsStore } from '../stores/meetings'
import { useNotesStore } from '../stores/notes'
import { formatBytes, formatDur } from '../core/meetingNote'
import type { AudioRef } from '../core/meetingNote'

const store = useMeetingNoteStore()
const legacy = useMeetingsStore()
const notes = useNotesStore()
const router = useRouter()

const showNew = ref(false)
const newTitle = ref('')
const showLegacy = ref(false)
const showSelfCheck = ref(false)
const migrating = ref(false)
const migrateMsg = ref('')
const force = ref(false)
const playing = ref('')
const srcCache = ref<Record<string, string | null>>({})

const refs = computed(() => store.refs)
const canProcess = computed(() => !!store.currentId && !store.processing)
const missing = computed(() => refs.value.filter((r) => !r.exists).length)

onMounted(async () => {
  await store.loadList()
  if (!store.currentId && store.notes.length) await store.openNote(store.notes[0].noteId)
})

async function create() {
  const title = newTitle.value.trim()
  if (!title) return
  await store.create(title)
  showNew.value = false
  newTitle.value = ''
}

async function openInEditor(noteId: string) {
  await notes.openNote(noteId)
  await router.push('/notes')
}

async function play(refItem: AudioRef) {
  if (!refItem.exists || !refItem.path) return
  if (playing.value === refItem.path) {
    playing.value = ''
    return
  }
  if (!(refItem.path in srcCache.value)) {
    srcCache.value[refItem.path] = await store.clipSrc(refItem.path)
  }
  playing.value = refItem.path
  window.setTimeout(() => {
    if (playing.value === refItem.path) playing.value = ''
  }, Math.min(Math.max(refItem.durationMs, 4000), 15 * 60 * 1000))
}

async function migrate() {
  migrating.value = true
  migrateMsg.value = ''
  try {
    const ids = await store.migrateLegacy()
    migrateMsg.value = ids.length
      ? `已导出 ${ids.length} 场旧会议到「会议/旧会议/」，录音也复制进了 会议音频/。`
      : '没有可导出的旧会议（或都已导出过）。'
    await legacy.loadList()
  } catch (e) {
    migrateMsg.value = `导出失败：${String(e)}`
  } finally {
    migrating.value = false
  }
}

function fmtTime(ts: number): string {
  if (!ts) return ''
  const d = new Date(ts * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">会议</span>
      <span class="page-sub">
        会议就是一篇 Markdown：随手写、用 <code>/v 音频文件名</code> 引用录音，会后一键处理
      </span>
      <span class="spacer" />
      <t-button size="small" variant="outline" @click="store.toggleRecorder(true)">🎙 录音</t-button>
      <t-button size="small" variant="outline" @click="store.loadList()">刷新</t-button>
      <t-button size="small" theme="primary" @click="showNew = true">新建会议</t-button>
    </header>

    <div class="body">
      <aside class="list">
        <div v-if="store.listError" class="list-err">{{ store.listError }}</div>
        <label class="scan-all">
          <t-checkbox
            :checked="store.scanAll"
            @change="(v: unknown) => store.loadList(!!v)"
          />
          <span>同时显示别处含录音的笔记</span>
        </label>
        <div v-if="!store.notes.length && !store.loading" class="list-empty">
          还没有会议笔记。<br />点右上角「新建会议」开始，或直接在笔记里写
          <code>/v 音频文件名</code>。
        </div>
        <div
          v-for="n in store.notes"
          :key="n.noteId"
          class="item"
          :class="{ active: n.noteId === store.currentId }"
          @click="store.openNote(n.noteId)"
        >
          <div class="item-title">{{ n.title }}</div>
          <div class="item-meta">
            <span>{{ n.audioTotal }} 段录音</span>
            <span>已转写 {{ n.transcribed }}</span>
            <span v-if="n.audioMissing" class="warn">缺 {{ n.audioMissing }}</span>
          </div>
          <div class="item-meta">
            <span :class="{ ok: n.hasMinutes }">{{ n.hasMinutes ? '已有纪要' : '待处理' }}</span>
            <span>{{ formatDur(n.durationMs) }}</span>
            <span>{{ formatBytes(n.audioBytes) }}</span>
          </div>
        </div>
      </aside>

      <section class="detail">
        <div v-if="!store.current" class="detail-empty">
          <div class="empty-title">选择左侧会议笔记</div>
          <div class="empty-sub">
            一场会议 = 一篇 md。开会时用右下角的 🎙 录音，录音引用会自动写进笔记；
            会后点「一键处理」自动转写并生成纪要。
          </div>
        </div>

        <template v-else>
          <div class="detail-head">
            <div class="title-row">
              <span class="meeting-title">{{ store.current.title }}</span>
              <span class="path">{{ store.current.noteId }}</span>
              <span class="spacer" />
              <t-button size="small" variant="outline" @click="openInEditor(store.current.noteId)">
                打开笔记
              </t-button>
            </div>
            <div class="stat-row">
              <span>{{ refs.length }} 段引用</span>
              <span>{{ refs.filter((r) => r.hasTranscript).length }} 段已转写</span>
              <span v-if="missing" class="warn">{{ missing }} 段找不到文件</span>
              <span>{{ formatDur(store.current.durationMs) }}</span>
              <span :class="{ ok: store.current.hasMinutes }">
                {{ store.current.hasMinutes ? '已有纪要' : '还没有纪要' }}
              </span>
            </div>
          </div>

          <t-alert v-if="store.error" theme="error" :message="store.error" class="alert" />

          <div class="section">
            <div class="section-head">
              <span class="section-title">音频引用</span>
              <span class="spacer" />
              <t-checkbox v-model="force">强制重新转写（忽略缓存）</t-checkbox>
              <t-button
                v-if="!store.processing"
                size="small"
                theme="primary"
                :disabled="!canProcess"
                @click="store.process(force)"
              >
                一键处理
              </t-button>
              <template v-else>
                <t-button size="small" theme="danger" variant="outline" @click="store.cancelProcess()">
                  取消
                </t-button>
              </template>
            </div>

            <div v-if="store.processing || store.message" class="progress">
              <t-progress :percentage="store.progress" :label="false" />
              <span class="progress-text">{{ store.message }}</span>
            </div>

            <div v-if="!refs.length" class="hint">
              这篇笔记还没有音频引用。用 <code>/v 会议音频/…</code> 或右下角录音面板录制，
              停止时会自动插入引用。
            </div>
            <div v-for="r in refs" :key="`${r.line}-${r.raw}`" class="clip-row">
              <span class="line">L{{ r.line }}</span>
              <span class="clip-path" :class="{ bad: !r.exists }">
                {{ r.raw }}
                <span v-if="!r.exists" class="warn">（找不到文件）</span>
              </span>
              <span class="clip-meta">
                {{ r.exists ? `${formatDur(r.durationMs)} · ${formatBytes(r.bytes)}` : '—' }}
              </span>
              <span class="tag" :class="{ ok: r.hasTranscript }">
                {{ r.hasTranscript ? '已转写' : '未转写' }}
              </span>
              <t-button size="small" variant="text" :disabled="!r.exists" @click="play(r)">
                {{ playing === r.path ? '停止' : '试听' }}
              </t-button>
              <audio
                v-if="playing === r.path && srcCache[r.path]"
                :src="srcCache[r.path] as string"
                autoplay
                controls
                class="player"
              />
            </div>
          </div>

          <div v-if="store.outcome" class="section">
            <div class="section-head">
              <span class="section-title">上次处理结果</span>
            </div>
            <div class="outcome">
              <div>
                转写 {{ store.outcome.audioDone }} 段、复用缓存 {{ store.outcome.audioSkipped }} 段、失败
                {{ store.outcome.audioFailed }} 段；转写 {{ store.outcome.transcriptChars }} 字，纪要
                {{ store.outcome.minutesChars }} 字
                <span v-if="store.outcome.model">（{{ store.outcome.model }}）</span>
                ，用时 {{ (store.outcome.elapsedMs / 1000).toFixed(1) }}s
              </div>
              <div v-if="store.outcome.cancelled" class="warn">处理被取消</div>
              <div v-for="(e, i) in store.outcome.errors" :key="i" class="warn">{{ e }}</div>
            </div>
          </div>

          <div class="section">
            <div class="section-head" @click="showLegacy = !showLegacy">
              <span class="section-title">旧版会议（录音在前的那种）</span>
              <span class="spacer" />
              <t-button
                size="small"
                variant="outline"
                :loading="migrating"
                @click.stop="migrate()"
              >
                导出为笔记
              </t-button>
              <span class="chev">{{ showLegacy ? '收起' : '展开' }}</span>
            </div>
            <div v-if="migrateMsg" class="hint">{{ migrateMsg }}</div>
            <div v-if="showLegacy">
              <div v-if="!legacy.meetings.length" class="hint">
                没有旧版会议。迁移会把旧的录音复制进 <code>会议音频/</code>，转写按时间戳分摊到各分段，
                旧纪要进「会议纪要」段（已存在的笔记会跳过）。
              </div>
              <div v-for="m in legacy.meetings" :key="m.id" class="legacy-row">
                <span class="legacy-title">{{ m.title }}</span>
                <span class="clip-meta">{{ m.segments }} 段 · {{ m.status }}</span>
                <span class="clip-meta">{{ fmtTime(m.createdAt) }}</span>
              </div>
            </div>
          </div>

          <div class="section">
            <div class="section-head" @click="showSelfCheck = !showSelfCheck">
              <span class="section-title">录音自检（排障用）</span>
              <span class="spacer" />
              <span class="chev">{{ showSelfCheck ? '收起' : '展开' }}</span>
            </div>
            <div v-if="showSelfCheck" class="selfcheck">
              <t-button size="small" variant="outline" @click="legacy.runSelfCheck()">
                检查环境
              </t-button>
              <t-button
                size="small"
                variant="outline"
                :disabled="legacy.selfCheck.recording"
                @click="legacy.startSelfCheckRecording(5)"
              >
                录 5 秒试听
              </t-button>
              <div v-for="(n, i) in legacy.selfCheck.notes" :key="i" class="hint">{{ n }}</div>
              <div v-if="legacy.selfCheck.info" class="hint">
                采样率 {{ legacy.selfCheck.info.sampleRate }} Hz ·
                峰值 {{ legacy.selfCheck.peak }}
              </div>
              <audio v-if="legacy.selfCheck.playbackUrl" :src="legacy.selfCheck.playbackUrl" controls />
            </div>
          </div>
        </template>
      </section>
    </div>

    <t-dialog
      v-model:visible="showNew"
      header="新建会议"
      :confirm-btn="{ content: '创建', disabled: !newTitle.trim() }"
      @confirm="create"
    >
      <div class="dialog-row">
        <span class="dialog-label">标题</span>
        <t-input v-model="newTitle" placeholder="例如：周会" @enter="create" />
      </div>
      <div class="dialog-hint">
        会创建 <code>会议/&lt;日期&gt;-&lt;标题&gt;.md</code>，并在文件末尾留好「会议纪要」段。
      </div>
    </t-dialog>
  </div>
</template>

<style scoped>
.body {
  flex: 1;
  display: flex;
  min-height: 0;
}

.list {
  width: 280px;
  flex: none;
  border-right: 1px solid var(--border);
  overflow: auto;
  padding: 10px 8px;
}

.list-empty,
.list-err {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.9;
  padding: 12px 10px;
}

.list-err {
  color: #d54941;
}

.scan-all {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-3);
  padding: 4px 10px 8px;
}

.item {
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  margin-bottom: 4px;
}

.item:hover {
  background: rgba(0, 82, 217, 0.06);
}

.item.active {
  background: rgba(0, 82, 217, 0.1);
}

.item-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: var(--text-3);
  flex-wrap: wrap;
}

.detail {
  flex: 1;
  overflow: auto;
  padding: 14px 18px 24px;
  min-width: 0;
}

.detail-empty {
  max-width: 540px;
  margin: 60px auto;
  text-align: center;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.empty-sub {
  font-size: 12px;
  color: var(--text-3);
  line-height: 2;
}

.detail-head {
  margin-bottom: 12px;
}

.title-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.meeting-title {
  font-size: 16px;
  font-weight: 600;
}

.path {
  font-size: 11px;
  color: var(--text-3);
  font-family: ui-monospace, monospace;
}

.stat-row {
  display: flex;
  gap: 12px;
  font-size: 12px;
  color: var(--text-3);
  margin-top: 6px;
  flex-wrap: wrap;
}

.ok {
  color: #2ba471;
}

.warn {
  color: #d54941;
}

.alert {
  margin-bottom: 12px;
}

.section {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px;
  margin-bottom: 12px;
}

.section-head {
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: default;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
}

.chev {
  font-size: 12px;
  color: var(--text-3);
  cursor: pointer;
}

.progress {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
}

.progress-text {
  font-size: 12px;
  color: var(--text-3);
  white-space: nowrap;
}

.hint {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.9;
  margin-top: 8px;
}

.clip-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  padding: 6px 0;
  border-bottom: 1px dashed var(--border);
  flex-wrap: wrap;
}

.clip-row:last-child {
  border-bottom: none;
}

.line {
  font-family: ui-monospace, monospace;
  color: var(--text-3);
  width: 32px;
  flex: none;
}

.clip-path {
  flex: 1;
  min-width: 200px;
  font-family: ui-monospace, monospace;
  word-break: break-all;
}

.clip-path.bad {
  color: #d54941;
  text-decoration: line-through;
}

.clip-meta {
  color: var(--text-3);
  white-space: nowrap;
}

.tag {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 6px;
  background: rgba(134, 144, 156, 0.15);
  color: var(--text-3);
  white-space: nowrap;
}

.tag.ok {
  background: rgba(43, 164, 113, 0.12);
  color: #2ba471;
}

.player {
  width: 100%;
  margin-top: 6px;
}

.outcome {
  font-size: 12px;
  color: var(--text-3);
  line-height: 2;
  margin-top: 6px;
}

.legacy-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  padding: 4px 0;
}

.legacy-title {
  flex: 1;
}

.selfcheck {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: flex-start;
  margin-top: 8px;
}

.dialog-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}

.dialog-label {
  width: 48px;
  font-size: 13px;
  color: var(--text-3);
}

.dialog-hint {
  font-size: 12px;
  color: var(--text-3);
}
</style>
