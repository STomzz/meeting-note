<script setup lang="ts">
/**
 * 浮动录音面板：任意页面可用。
 *
 * 用法：选定目标笔记 → 开始录音（默认 4 分钟一段）→ 停止时自动把
 * `/v 会议音频/...` 引用插进笔记（笔记正打开则插到光标处，否则追加到正文末尾）。
 */
import { computed, ref } from 'vue'
import { useMeetingNoteStore } from '../stores/meetingNote'
import { formatBytes, formatDur } from '../core/meetingNote'
import type { ClipStat } from '../core/meetingNote'

const store = useMeetingNoteStore()
const confirmClip = ref<ClipStat | null>(null)

const elapsed = computed(() => formatDur(store.recordElapsedMs))
const segmentElapsed = computed(() => formatDur(store.recordSegmentMs))
const recent = computed(() => store.recentClips.slice(0, 5))

async function discard() {
  const clip = confirmClip.value
  confirmClip.value = null
  if (clip) await store.discardClip(clip)
}
</script>

<template>
  <div class="recorder">
    <t-button
      v-if="!store.recorderOpen"
      class="recorder-fab"
      theme="primary"
      shape="round"
      @click="store.toggleRecorder(true)"
    >
      🎙 录音
    </t-button>

    <div v-else class="recorder-panel">
      <div class="head">
        <span class="title">🎙 会议录音</span>
        <span v-if="store.recording" class="live">● 录制中</span>
        <span class="spacer" />
        <t-button size="small" variant="text" @click="store.toggleRecorder(false)">收起</t-button>
      </div>

      <div class="row">
        <span class="label">目标笔记</span>
        <t-select
          :value="store.targetNoteId"
          class="target"
          size="small"
          placeholder="选择要写入哪篇笔记"
          :disabled="store.recording"
          @change="(v: unknown) => store.setTarget(String(v))"
        >
          <t-option
            v-for="opt in store.targetOptions"
            :key="opt.value"
            :value="opt.value"
            :label="opt.label"
          />
        </t-select>
      </div>

      <div v-if="store.recording" class="status">
        <span class="timer">{{ elapsed }}</span>
        <span class="seg">第 {{ store.recordSeq }} 段 · {{ segmentElapsed }} / 4:00</span>
        <div class="level">
          <div class="level-bar" :style="{ width: `${store.levelPercent}%` }" />
        </div>
      </div>

      <div class="actions">
        <t-button
          v-if="!store.recording"
          theme="danger"
          :disabled="!store.targetNoteId"
          @click="store.startRecording()"
        >
          开始录音
        </t-button>
        <template v-else>
          <t-button theme="primary" @click="store.stopRecording()">停止并插入引用</t-button>
        </template>
      </div>

      <div v-if="store.insertHint" class="hint">{{ store.insertHint }}</div>
      <div v-if="store.error" class="err">{{ store.error }}</div>

      <div v-if="recent.length" class="clips">
        <div class="clips-title">本次录音</div>
        <div v-for="clip in recent" :key="clip.seq" class="clip">
          <span class="clip-name">{{ clip.file }}</span>
          <span class="clip-meta">{{ formatDur(clip.durationMs) }} · {{ formatBytes(clip.bytes) }}</span>
          <t-button size="small" variant="text" @click="store.insertClip(clip)">插入引用</t-button>
          <t-button size="small" variant="text" theme="danger" @click="confirmClip = clip">
            丢弃
          </t-button>
        </div>
      </div>

      <div class="tip">
        录音存到 vault 的 <code>会议音频/</code> 下，跟着笔记一起备份；停止录音后会自动写入
        <code>/v 会议音频/…</code> 引用。
      </div>
    </div>

    <t-dialog
      :visible="!!confirmClip"
      header="丢弃这段录音？"
      :confirm-btn="{ content: '丢弃', theme: 'danger' }"
      @confirm="discard"
      @cancel="confirmClip = null"
      @close="confirmClip = null"
    >
      <p>
        将删除文件 <code>{{ confirmClip?.path }}</code>
        （{{ confirmClip ? formatDur(confirmClip.durationMs) : '' }}），已写入笔记的引用需要自己删。
      </p>
    </t-dialog>
  </div>
</template>

<style scoped>
.recorder {
  position: fixed;
  right: 24px;
  bottom: 24px;
  z-index: 900;
}
.recorder-panel {
  width: 380px;
  max-width: calc(100vw - 48px);
  background: var(--td-bg-color-container, #fff);
  border: 1px solid var(--td-component-border, #e7e7e7);
  border-radius: 12px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.16);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.title {
  font-weight: 600;
}
.live {
  color: #d54941;
  font-size: 12px;
}
.spacer {
  flex: 1;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.label {
  font-size: 12px;
  color: var(--td-text-color-secondary, #888);
  white-space: nowrap;
}
.target {
  flex: 1;
}
.status {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.timer {
  font-variant-numeric: tabular-nums;
  font-weight: 600;
  font-size: 14px;
}
.seg {
  color: var(--td-text-color-secondary, #888);
}
.level {
  flex: 1;
  height: 6px;
  border-radius: 3px;
  background: var(--td-bg-color-component, #eee);
  overflow: hidden;
}
.level-bar {
  height: 100%;
  background: #2ba471;
  transition: width 0.15s linear;
}
.actions {
  display: flex;
  gap: 8px;
}
.hint {
  font-size: 12px;
  color: #2ba471;
}
.err {
  font-size: 12px;
  color: #d54941;
  word-break: break-all;
}
.clips-title {
  font-size: 12px;
  color: var(--td-text-color-secondary, #888);
}
.clip {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}
.clip-name {
  flex: 1;
  font-family: ui-monospace, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.clip-meta {
  color: var(--td-text-color-secondary, #888);
}
.tip {
  font-size: 11px;
  line-height: 1.6;
  color: var(--td-text-color-secondary, #888);
}
</style>
