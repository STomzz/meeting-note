<script setup lang="ts">
/**
 * 浮动录音面板：简单版。
 *
 * - 选定「记到哪篇笔记」→ 开始 / 停止；
 * - 每 4 分钟自动分段，停止只保存文件，**不自动写引用**；
 * - 引用用笔记里的 `/v` 选择器或「本笔记录音 → 插入」。
 */
import { computed } from 'vue'
import { useMeetingNoteStore } from '../stores/meetingNote'
import { formatDur } from '../core/meetingNote'

const store = useMeetingNoteStore()
const elapsed = computed(() => formatDur(store.recordElapsedMs))
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
        <span class="title">🎙 录音</span>
        <span v-if="store.recording" class="live">● {{ elapsed }} · 第 {{ store.recordSeq }} 段</span>
        <span class="spacer" />
        <t-button size="small" variant="text" @click="store.toggleRecorder(false)">收起</t-button>
      </div>

      <div class="row">
        <span class="label">记到</span>
        <t-select
          :value="store.targetNoteId"
          class="target"
          size="small"
          placeholder="选择目标笔记"
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

      <t-button
        v-if="!store.recording"
        block
        theme="danger"
        :disabled="!store.targetNoteId"
        @click="store.startRecording()"
      >
        开始录音
      </t-button>
      <t-button v-else block theme="primary" @click="store.stopRecording()">停止录音</t-button>

      <div v-if="store.insertHint" class="hint">{{ store.insertHint }}</div>
      <div v-if="store.error" class="err">{{ store.error }}</div>
      <div class="tip">停止后录音已保存；到笔记里用 <code>/v</code> 插入引用。每 4 分钟自动分段。</div>
    </div>
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
  width: 340px;
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
  font-variant-numeric: tabular-nums;
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
.hint {
  font-size: 12px;
  color: #2ba471;
  word-break: break-all;
}
.err {
  font-size: 12px;
  color: #d54941;
  word-break: break-all;
}
.tip {
  font-size: 11px;
  line-height: 1.6;
  color: var(--td-text-color-secondary, #888);
}
</style>
