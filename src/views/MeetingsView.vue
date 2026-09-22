<script setup lang="ts">
// P5：会议 —— 分段录音（可播放）→ 直连 ASR 转写 → 直连 LLM 生成纪要
import { computed, onMounted, ref } from 'vue'
import MarkdownIt from 'markdown-it'
import { MessagePlugin } from 'tdesign-vue-next'
import { useRouter } from 'vue-router'
import { useMeetingsStore } from '../stores/meetings'
import { useNotesStore } from '../stores/notes'
import {
  fmtDateTime,
  fmtDuration,
  MEETING_STATUS_LABEL,
  MEETING_STATUS_THEME,
  SEGMENT_STATUS_LABEL,
  type MeetingSegment,
} from '../core/meetings'

const meetings = useMeetingsStore()
const notes = useNotesStore()
const router = useRouter()
const md = new MarkdownIt({ html: false, linkify: true })

const audioRef = ref<HTMLAudioElement | null>(null)
const playingSeq = ref(0)
const renameVisible = ref(false)
const renameTitle = ref('')
const deleteVisible = ref(false)
const deleteFiles = ref(true)
const selfCheckVisible = ref(false)
const minutesVisible = ref(false)

const current = computed(() => meetings.current)
const hasSegments = computed(() => (current.value?.segments.length ?? 0) > 0)
const canTranscribe = computed(
  () => hasSegments.value && !meetings.recording && !meetings.transcribing,
)
const pendingSegments = computed(
  () => current.value?.segments.filter((s) => s.status !== 'done').length ?? 0,
)

onMounted(async () => {
  await meetings.loadList()
  if (!meetings.current && meetings.meetings.length) {
    await meetings.open(meetings.meetings[0].id)
  }
})

function statusLabel(s: string) {
  return MEETING_STATUS_LABEL[s] ?? s
}
function statusTheme(s: string) {
  return MEETING_STATUS_THEME[s] ?? 'default'
}
function segmentLabel(s: string) {
  return SEGMENT_STATUS_LABEL[s] ?? s
}

async function startNew() {
  const id = await meetings.startRecording()
  if (id) void MessagePlugin.success('开始录音（每 4 分钟自动分段）')
}

async function stop() {
  await meetings.stopRecording()
  void MessagePlugin.info('已停止录音')
}

async function openMeeting(id: string) {
  if (meetings.recording && id !== meetings.recordMeetingId) {
    void MessagePlugin.warning('正在录音，先停止录音再切换会议')
    return
  }
  stopPlayback()
  await meetings.open(id)
}

function play(seg: MeetingSegment) {
  const src = meetings.segmentSrc(seg)
  if (!src) {
    void MessagePlugin.warning('预览模式无法播放真实录音');
    return
  }
  if (playingSeq.value === seg.seq) {
    stopPlayback()
    return
  }
  playingSeq.value = seg.seq
  const el = audioRef.value
  if (el) {
    el.src = src
    el.currentTime = 0
    void el.play().catch((e) => {
      playingSeq.value = 0
      void MessagePlugin.error(`播放失败：${String(e)}`)
    })
  }
}

function stopPlayback() {
  playingSeq.value = 0
  const el = audioRef.value
  if (el) {
    el.pause()
    el.removeAttribute('src')
  }
}

async function transcribe() {
  await meetings.transcribe()
  if (meetings.error) void MessagePlugin.error(meetings.error)
  else void MessagePlugin.success(meetings.transcribeMessage)
}

async function generateMinutes() {
  await meetings.generateMinutes()
  if (meetings.error) void MessagePlugin.error(meetings.error)
  else {
    void MessagePlugin.success(meetings.minutesMessage)
    minutesVisible.value = true
  }
}

function openMinutesDialog() {
  minutesVisible.value = true
}

function openRename() {
  renameTitle.value = current.value?.meeting.title ?? ''
  renameVisible.value = true
}

async function confirmRename() {
  const id = current.value?.meeting.id
  if (!id) return
  await meetings.rename(id, renameTitle.value)
  renameVisible.value = false
}

function openDelete() {
  deleteFiles.value = true
  deleteVisible.value = true
}

async function confirmDelete() {
  const id = current.value?.meeting.id
  if (!id) return
  await meetings.remove(id, deleteFiles.value)
  deleteVisible.value = false
  void MessagePlugin.success(deleteFiles.value ? '已删除会议与录音文件' : '已删除会议记录（保留录音文件）')
}

async function showDir() {
  const id = current.value?.meeting.id
  if (!id) return
  const dir = await meetings.openDir(id)
  if (dir) void MessagePlugin.info(dir)
}

/** 打开自动保存的纪要笔记。 */
async function openNote() {
  const noteId = current.value?.meeting.noteId
  if (!noteId) {
    void MessagePlugin.info('还没有生成纪要笔记')
    return
  }
  try {
    if (!notes.notes.length) await notes.init()
    await notes.openNote(noteId)
    await router.push({ path: '/notes' })
  } catch (e) {
    void MessagePlugin.error(String(e))
  }
}

function copyTranscript() {
  const text = current.value?.transcript ?? ''
  if (!text) return
  void navigator.clipboard?.writeText(text)
  void MessagePlugin.success('已复制转写文本')
}

function render(text: string) {
  return md.render(text || '')
}

async function selfCheckRun() {
  await meetings.runSelfCheck()
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">会议</span>
      <span class="page-sub">录音 → 转写 → 纪要，音频与文本都保存在本机</span>
      <span class="spacer" />
      <t-button size="small" variant="outline" @click="selfCheckVisible = true">录音自检</t-button>
      <t-button v-if="!meetings.recording" theme="primary" size="small" @click="startNew">
        开始录音
      </t-button>
      <t-button v-else theme="danger" size="small" @click="stop">
        停止录音（{{ fmtDuration(meetings.displayDurationMs) }}）
      </t-button>
    </header>

    <div class="body">
      <aside class="list">
        <div v-if="meetings.listError" class="list-err">{{ meetings.listError }}</div>
        <div v-if="!meetings.meetings.length" class="list-empty">
          还没有会议。<br />点右上角「开始录音」新建一场。
        </div>
        <div
          v-for="m in meetings.meetings"
          :key="m.id"
          class="item"
          :class="{ active: m.id === current?.meeting.id }"
          @click="openMeeting(m.id)"
        >
          <div class="item-title">{{ m.title }}</div>
          <div class="item-meta">
            <t-tag size="small" :theme="statusTheme(m.status)" variant="light">
              {{ statusLabel(m.status) }}
            </t-tag>
            <span>{{ fmtDateTime(m.createdAt) }}</span>
          </div>
          <div class="item-meta">
            <span>{{ fmtDuration(m.durationMs) }}</span>
            <span v-if="m.segments">· {{ m.transcribedSegments }}/{{ m.segments }} 段已转写</span>
          </div>
        </div>
      </aside>

      <section class="detail">
        <div v-if="!current" class="detail-empty">
          <div class="empty-title">选择左侧会议，或开始一段新录音</div>
          <div class="empty-sub">
            录音按 4 分钟自动分段（WAV，原采样率保存）；转写会按静音切成 8~45 秒的片段送 ASR，
            单段失败只影响那一段，可重试。
          </div>
        </div>

        <template v-else>
          <div class="detail-head">
            <div class="title-row">
              <span class="meeting-title">{{ current.meeting.title }}</span>
              <t-tag size="small" :theme="statusTheme(current.meeting.status)" variant="light">
                {{ statusLabel(current.meeting.status) }}
              </t-tag>
              <span class="spacer" />
              <t-button size="small" variant="text" @click="openRename">重命名</t-button>
              <t-button size="small" variant="text" @click="showDir">音频目录</t-button>
              <t-button size="small" variant="text" theme="danger" @click="openDelete">删除</t-button>
            </div>
            <div class="stat-row">
              <span>{{ fmtDateTime(current.meeting.createdAt) }}</span>
              <span>时长 {{ fmtDuration(meetings.displayDurationMs) }}</span>
              <span>{{ current.segments.length }} 段</span>
              <span v-if="current.meeting.asrModel">ASR {{ current.meeting.asrModel }}</span>
              <span v-if="current.meeting.chatModel">对话 {{ current.meeting.chatModel }}</span>
            </div>
          </div>

          <!-- 录音中 -->
          <div v-if="meetings.recording" class="recording-panel">
            <div class="rec-head">
              <span class="dot" />
              <span>正在录音 · 第 {{ meetings.recordSeq }} 段</span>
              <span class="rec-time">{{ fmtDuration(meetings.recordElapsedMs) }}</span>
            </div>
            <div class="level">
              <div class="level-bar" :style="{ width: `${meetings.levelPercent}%` }" />
            </div>
            <div class="rec-hint">
              采样率 {{ meetings.recordInfo?.sampleRate ?? '—' }} Hz ·
              数据每 1 秒写入磁盘 ·
              {{ meetings.recordSegmentMs >= 180000 ? '即将分段' : '到达 4 分钟自动分段' }}
            </div>
          </div>

          <t-alert v-if="meetings.error" theme="error" :message="meetings.error" class="alert" />

          <!-- 分段列表 -->
          <div class="section">
            <div class="section-head">
              <span class="section-title">录音分段</span>
              <span class="spacer" />
              <t-button
                size="small"
                theme="primary"
                :disabled="!canTranscribe"
                :loading="meetings.transcribing"
                @click="transcribe"
              >
                {{ current.meeting.hasTranscript ? '重新转写未完成段' : '开始转写' }}
              </t-button>
              <t-button
                v-if="meetings.transcribing"
                size="small"
                variant="outline"
                @click="meetings.cancelTranscribe()"
              >
                取消
              </t-button>
            </div>

            <div v-if="meetings.transcribing || meetings.transcribeMessage" class="progress">
              <t-progress
                theme="line"
                :percentage="meetings.transcribeProgress"
                :label="false"
                size="small"
              />
              <span class="progress-text">{{ meetings.transcribeMessage }}</span>
            </div>

            <div v-if="!current.segments.length" class="hint">还没有录音分段</div>
            <div
              v-for="seg in current.segments"
              :key="seg.id"
              class="segment"
              :class="{ playing: playingSeq === seg.seq }"
            >
              <t-button
                size="small"
                variant="outline"
                :disabled="seg.bytes === 0"
                @click="play(seg)"
              >
                {{ playingSeq === seg.seq ? '停止' : '播放' }}
              </t-button>
              <span class="seg-title">第 {{ seg.seq }} 段</span>
              <span class="seg-meta">{{ fmtDuration(seg.durationMs) }}</span>
              <span class="seg-meta">{{ (seg.bytes / 1024 / 1024).toFixed(1) }} MB</span>
              <span class="seg-meta">{{ seg.srcRate }} Hz</span>
              <t-tag size="small" variant="light">{{ segmentLabel(seg.status) }}</t-tag>
              <span class="spacer" />
              <span v-if="seg.error" class="seg-err">{{ seg.error }}</span>
            </div>
            <audio ref="audioRef" class="hidden-audio" @ended="stopPlayback" />
          </div>

          <!-- 转写 -->
          <div class="section">
            <div class="section-head">
              <span class="section-title">转写文本</span>
              <span v-if="pendingSegments" class="section-sub">{{ pendingSegments }} 段待转写</span>
              <span class="spacer" />
              <t-button
                size="small"
                variant="outline"
                :disabled="!current.meeting.hasTranscript"
                @click="copyTranscript"
              >
                复制
              </t-button>
              <t-button
                size="small"
                theme="primary"
                :disabled="!current.meeting.hasTranscript || meetings.generating"
                :loading="meetings.generating"
                @click="generateMinutes"
              >
                生成纪要
              </t-button>
              <t-button
                size="small"
                variant="outline"
                :disabled="!current.meeting.hasMinutes"
                @click="openMinutesDialog"
              >
                查看纪要
              </t-button>
            </div>
            <div v-if="meetings.minutesMessage" class="hint">{{ meetings.minutesMessage }}</div>
            <pre v-if="current.transcript" class="transcript">{{ current.transcript }}</pre>
            <div v-else class="hint">
              还没有转写文本。点上方「开始转写」（需要先在设置里配置语音转写端点）。
            </div>
          </div>

          <div v-if="current.meeting.noteId" class="section">
            <div class="section-head">
              <span class="section-title">纪要笔记</span>
              <span class="spacer" />
              <t-button size="small" variant="outline" @click="openNote">
                打开笔记 {{ current.meeting.noteId }}
              </t-button>
            </div>
          </div>
        </template>
      </section>
    </div>

    <!-- 重命名 -->
    <t-dialog
      v-model:visible="renameVisible"
      header="重命名会议"
      :on-confirm="confirmRename"
      :confirm-btn="{ content: '保存' }"
    >
      <t-input v-model="renameTitle" placeholder="会议标题" />
    </t-dialog>

    <!-- 删除（明确询问是否删除音频文件） -->
    <t-dialog
      v-model:visible="deleteVisible"
      header="删除会议"
      theme="danger"
      :on-confirm="confirmDelete"
      :confirm-btn="{ content: '删除', theme: 'danger' }"
    >
      <p class="dlg-text">将删除会议「{{ current?.meeting.title }}」的记录（含转写与纪要记录）。</p>
      <t-checkbox v-model="deleteFiles">同时删除录音文件（不可恢复）</t-checkbox>
      <p class="dlg-path">音频目录：{{ current?.dir }}</p>
    </t-dialog>

    <!-- 纪要 -->
    <t-dialog
      v-model:visible="minutesVisible"
      header="会议纪要"
      width="820px"
      :footer="false"
      class="minutes-dialog"
    >
      <div v-if="current?.minutesMd" class="minutes" v-html="render(current.minutesMd)" />
      <div v-else class="hint">还没有纪要</div>
    </t-dialog>

    <!-- 录音自检（Desktop / Android spike 共用） -->
    <t-dialog
      v-model:visible="selfCheckVisible"
      header="录音自检"
      width="620px"
      :footer="false"
    >
      <div class="check-body">
        <div class="check-row">
          <t-button size="small" variant="outline" :loading="meetings.selfCheck.running" @click="selfCheckRun">
            运行环境自检
          </t-button>
          <t-button
            size="small"
            theme="primary"
            :loading="meetings.selfCheck.recording"
            @click="meetings.startSelfCheckRecording(5)"
          >
            录 5 秒并回放
          </t-button>
        </div>

        <div v-if="meetings.selfCheck.notes.length" class="check-notes">
          <div v-for="(n, i) in meetings.selfCheck.notes" :key="i" class="check-note">{{ n }}</div>
        </div>

        <div v-if="meetings.selfCheck.recording || meetings.selfCheck.playbackUrl" class="check-result">
          <div class="level">
            <div class="level-bar" :style="{ width: `${meetings.levelPercent}%` }" />
          </div>
          <div class="check-meta">
            <span v-if="meetings.selfCheck.recording">录制中… {{ meetings.selfCheck.seconds }}/5 s</span>
            <template v-else>
              <span>采样率 {{ meetings.selfCheck.info?.sampleRate }} Hz</span>
              <span>· 峰值 {{ meetings.selfCheck.peak }}</span>
              <span>· 时长约 5 s</span>
            </template>
          </div>
          <audio v-if="meetings.selfCheck.playbackUrl" :src="meetings.selfCheck.playbackUrl" controls />
        </div>

        <div class="check-tip">
          自检只在本机进行：验证 WebView 是否有麦克风权限、能否取到 PCM、以及采集到的音频能否回放。
          在 Android 上这一步过了，就说明「前台录音」链路可用。
        </div>
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
  width: 260px;
  flex: none;
  border-right: 1px solid var(--border);
  overflow: auto;
  padding: 8px;
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
  gap: 6px;
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
  max-width: 520px;
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
  line-height: 1.9;
}

.detail-head {
  border-bottom: 1px solid var(--border);
  padding-bottom: 10px;
  margin-bottom: 14px;
}

.title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.meeting-title {
  font-size: 16px;
  font-weight: 600;
}

.stat-row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--text-3);
  margin-top: 6px;
}

.recording-panel {
  border: 1px solid rgba(213, 73, 65, 0.3);
  background: rgba(213, 73, 65, 0.05);
  border-radius: 10px;
  padding: 10px 12px;
  margin-bottom: 14px;
}

.rec-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 500;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #d54941;
  animation: pulse 1.2s infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.25;
  }
}

.rec-time {
  margin-left: auto;
  font-variant-numeric: tabular-nums;
}

.level {
  height: 6px;
  border-radius: 3px;
  background: #ebedf0;
  overflow: hidden;
  margin: 8px 0 6px;
}

.level-bar {
  height: 100%;
  background: linear-gradient(90deg, #00a870, #e37318, #d54941);
  transition: width 0.12s linear;
}

.rec-hint {
  font-size: 11px;
  color: var(--text-3);
}

.alert {
  margin-bottom: 14px;
}

.section {
  margin-bottom: 20px;
}

.section-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
}

.section-sub {
  font-size: 11px;
  color: var(--text-3);
}

.progress {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}

.progress :deep(.t-progress) {
  flex: 1;
}

.progress-text {
  font-size: 11px;
  color: var(--text-3);
  white-space: nowrap;
}

.hint {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.9;
}

.segment {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  margin-bottom: 6px;
  font-size: 12px;
}

.segment.playing {
  border-color: #0052d9;
  background: rgba(0, 82, 217, 0.05);
}

.seg-title {
  font-weight: 500;
}

.seg-meta {
  color: var(--text-3);
}

.seg-err {
  color: #d54941;
  font-size: 11px;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hidden-audio {
  display: none;
}

.transcript {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
  font-size: 13px;
  line-height: 1.9;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  max-height: 420px;
  overflow: auto;
  margin: 0;
}

.minutes {
  font-size: 13px;
  line-height: 1.9;
  max-height: 60vh;
  overflow: auto;
}

.minutes :deep(h1) {
  font-size: 18px;
}

.minutes :deep(h2) {
  font-size: 15px;
  margin-top: 18px;
}

.minutes :deep(table) {
  border-collapse: collapse;
}

.minutes :deep(th),
.minutes :deep(td) {
  border: 1px solid var(--border);
  padding: 4px 8px;
}

.dlg-text {
  font-size: 13px;
  margin: 0 0 10px;
}

.dlg-path {
  font-size: 11px;
  color: var(--text-3);
  margin-top: 8px;
  word-break: break-all;
}

.check-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.check-row {
  display: flex;
  gap: 8px;
}

.check-notes {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px;
  font-size: 12px;
  line-height: 1.9;
  word-break: break-all;
}

.check-result {
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 10px;
}

.check-meta {
  display: flex;
  gap: 6px;
  font-size: 12px;
  color: var(--text-2);
  margin-bottom: 8px;
}

.check-tip {
  font-size: 11px;
  color: var(--text-3);
  line-height: 1.9;
}
</style>
