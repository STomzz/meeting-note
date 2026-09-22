<script setup lang="ts">
/**
 * 浮动录音面板（胶囊版）。
 *
 * - 收起时是一颗小胶囊：录音中显示红点 + 计时；
 * - 展开后选目标笔记、开始/停止；停止只保存文件，**不自动写引用**；
 * - 停止后可一键「插入引用」（插到当前笔记光标处或正文末尾）。
 */
import { computed } from 'vue'
import { useMeetingNoteStore } from '../stores/meetingNote'
import { formatDur } from '../core/meetingNote'

const store = useMeetingNoteStore()
const elapsed = computed(() => formatDur(store.recordElapsedMs))

async function insertLast() {
  if (!store.lastClip) return
  await store.insertClip(store.lastClip)
}
</script>

<template>
  <div class="recorder">
    <!-- 收起态：胶囊 -->
    <button
      v-if="!store.recorderOpen"
      class="capsule"
      :class="{ live: store.recording }"
      @click="store.toggleRecorder(true)"
    >
      <t-icon name="microphone-1" size="15px" />
      <template v-if="store.recording">
        <span class="dot" />
        <span class="time">{{ elapsed }}</span>
        <span class="seg">第 {{ store.recordSeq }} 段</span>
      </template>
      <template v-else>录音</template>
    </button>

    <!-- 展开态：面板 -->
    <div v-else class="panel">
      <div class="head">
        <t-icon name="microphone-1" size="15px" />
        <span class="title">录音</span>
        <span v-if="store.recording" class="live">
          <span class="dot" />
          {{ elapsed }} · 第 {{ store.recordSeq }} 段
        </span>
        <span class="spacer" />
        <button class="icon-btn" title="收起" @click="store.toggleRecorder(false)">
          <t-icon name="chevron-down" size="15px" />
        </button>
      </div>

      <div v-if="store.recording" class="level">
        <i :style="{ width: `${store.levelPercent}%` }" />
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

      <div v-if="store.lastClip && !store.recording" class="saved">
        <div class="saved-text">
          第 {{ store.lastClip.seq }} 段已保存
          <code>{{ store.lastClip.path }}</code>
        </div>
        <t-button size="small" theme="primary" variant="outline" @click="insertLast">
          插入引用
        </t-button>
      </div>

      <div v-if="store.error" class="err">{{ store.error }}</div>
      <div class="tip">
        停止后录音已保存、不会自动插引用；用笔记里的 <code>/v</code> 选择器，或上面「插入引用」。
        每 4 分钟自动分段。
      </div>
    </div>
  </div>
</template>

<style scoped>
.recorder {
  position: fixed;
  right: 20px;
  bottom: 20px;
  z-index: 900;
}

.capsule {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 38px;
  padding: 0 16px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--panel);
  color: var(--text-2);
  font-size: 13px;
  box-shadow: var(--shadow-2);
  cursor: pointer;
  transition: transform 0.12s ease, box-shadow 0.12s ease, color 0.12s ease;
}

.capsule:hover {
  transform: translateY(-1px);
  box-shadow: var(--shadow-3);
  color: var(--primary);
}

.capsule.live {
  color: #fff;
  background: var(--danger);
  border-color: transparent;
}

.capsule.live:hover {
  color: #fff;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #fff;
  animation: blink 1.2s ease-in-out infinite;
}

@keyframes blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.25;
  }
}

.time {
  font-variant-numeric: tabular-nums;
}

.seg {
  opacity: 0.85;
  font-size: 12px;
}

.panel {
  width: 330px;
  max-width: calc(100vw - 40px);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-l);
  box-shadow: var(--shadow-3);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.title {
  font-weight: 600;
  font-size: 13.5px;
}

.live {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--danger);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.live .dot {
  background: var(--danger);
}

.level {
  height: 4px;
  border-radius: 999px;
  background: var(--code-bg);
  overflow: hidden;
}

.level i {
  display: block;
  height: 100%;
  background: var(--danger);
  border-radius: 999px;
  transition: width 0.12s linear;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.label {
  font-size: 12px;
  color: var(--text-3);
  white-space: nowrap;
}

.target {
  flex: 1;
}

.saved {
  display: flex;
  align-items: center;
  gap: 8px;
}

.saved-text {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.5;
}

.saved-text code {
  font-size: 11px;
  color: var(--text-3);
  word-break: break-all;
}

.err {
  font-size: 12px;
  color: var(--danger);
  word-break: break-all;
}

.tip {
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-3);
}
</style>
