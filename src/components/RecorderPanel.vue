<script setup lang="ts">
/**
 * 浮动录音面板。
 *
 * - 收起态：一颗胶囊；录音中变成红色计时胶囊（带跳动的电平条）；
 * - 展开态：目标笔记 → 一个主按钮 → 已保存卡片 → 一行说明；
 * - 停止只保存文件，**不自动写引用**：引用由笔记里的 `/v` 选择器插。
 */
import { computed } from 'vue'
import { useMeetingNoteStore } from '../stores/meetingNote'
import { formatDur } from '../core/meetingNote'

const store = useMeetingNoteStore()
const elapsed = computed(() => formatDur(store.recordElapsedMs))
const targetLabel = computed(
  () => store.targetOptions.find((o) => o.value === store.targetNoteId)?.label ?? '',
)

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
      :title="store.recording ? '正在录音（点开看详情）' : '打开录音面板'"
      @click="store.toggleRecorder(true)"
    >
      <template v-if="store.recording">
        <span class="pulse" />
        <span class="time">{{ elapsed }}</span>
        <span class="eq" aria-hidden="true"><i /><i /><i /></span>
      </template>
      <template v-else>
        <t-icon name="microphone-1" size="15px" />
        <span>录音</span>
      </template>
    </button>

    <!-- 展开态：面板 -->
    <div v-else class="panel">
      <div class="head">
        <span class="badge" :class="{ live: store.recording }">
          <t-icon :name="store.recording ? 'sound' : 'microphone-1'" size="14px" />
        </span>
        <span class="title">{{ store.recording ? '录音中' : '录音' }}</span>
        <span v-if="store.recording" class="timer">{{ elapsed }}</span>
        <span class="spacer" />
        <button class="icon-btn" title="收起" @click="store.toggleRecorder(false)">
          <t-icon name="chevron-down" size="15px" />
        </button>
      </div>

      <template v-if="store.recording">
        <div class="level"><i :style="{ width: `${store.levelPercent}%` }" /></div>
        <div class="meta">
          <span class="chip">第 {{ store.recordSeq }} 段</span>
          <span class="chip quiet">已存 {{ store.recentClips.length }} 段</span>
          <span class="spacer" />
          <span v-if="targetLabel" class="where" :title="targetLabel">{{ targetLabel }}</span>
        </div>
      </template>

      <div v-else class="field">
        <span class="label">记到</span>
        <t-select
          :value="store.targetNoteId"
          class="target"
          size="small"
          placeholder="选择目标笔记"
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
        class="action"
        block
        size="large"
        theme="danger"
        :disabled="!store.targetNoteId"
        @click="store.startRecording()"
      >
        <span class="action-inner">
          <t-icon name="microphone-1" size="16px" />
          开始录音
        </span>
      </t-button>
      <t-button
        v-else
        class="action"
        block
        size="large"
        theme="default"
        @click="store.stopRecording()"
      >
        <span class="action-inner">
          <t-icon name="stop-circle-filled" size="16px" />
          停止录音
        </span>
      </t-button>

      <div v-if="store.lastClip && !store.recording" class="saved">
        <t-icon class="saved-icon" name="check-circle" size="15px" />
        <div class="saved-body">
          <div class="saved-title">第 {{ store.lastClip.seq }} 段已保存</div>
          <div class="saved-path" :title="store.lastClip.path">{{ store.lastClip.path }}</div>
        </div>
        <t-button size="small" theme="primary" variant="outline" @click="insertLast">
          插入引用
        </t-button>
      </div>

      <div v-if="store.error" class="err">{{ store.error }}</div>

      <div class="tip">每 4 分钟自动分段 · 引用用笔记里的 <code>/v</code> 插</div>
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

/* ---- 收起态 ---- */

.capsule {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 36px;
  padding: 0 15px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--panel);
  color: var(--text-2);
  font-family: var(--font-sans);
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
  padding: 0 14px;
  border-color: transparent;
  background: var(--danger);
  color: #fff;
  box-shadow: var(--shadow-2);
}

.capsule.live:hover {
  color: #fff;
}

.pulse {
  position: relative;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #fff;
}

.pulse::after {
  content: '';
  position: absolute;
  inset: -4px;
  border: 1px solid rgba(255, 255, 255, 0.75);
  border-radius: 50%;
  animation: pulse-ring 1.6s ease-out infinite;
}

@keyframes pulse-ring {
  0% {
    transform: scale(0.6);
    opacity: 0.9;
  }

  100% {
    transform: scale(1.45);
    opacity: 0;
  }
}

.time {
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}

/* 跳动的电平条（纯装饰，真实的电平在面板里） */
.eq {
  display: inline-flex;
  align-items: flex-end;
  gap: 2px;
  height: 12px;
}

.eq i {
  width: 3px;
  border-radius: 2px;
  background: currentColor;
  transform-origin: bottom;
  animation: eq-jump 1.1s ease-in-out infinite;
}

.eq i:nth-child(1) {
  height: 6px;
  animation-delay: -0.2s;
}

.eq i:nth-child(2) {
  height: 11px;
  animation-delay: -0.6s;
}

.eq i:nth-child(3) {
  height: 8px;
  animation-delay: -0.9s;
}

@keyframes eq-jump {
  0%,
  100% {
    transform: scaleY(0.45);
  }

  50% {
    transform: scaleY(1);
  }
}

/* ---- 展开态 ---- */

.panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 320px;
  max-width: calc(100vw - 32px);
  padding: 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-l);
  background: var(--panel);
  box-shadow: var(--shadow-3);
}

.head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.badge {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 50%;
  background: var(--brand-weak);
  color: var(--primary);
}

.badge.live {
  background: var(--danger);
  color: #fff;
}

.title {
  font-size: 13.5px;
  font-weight: 600;
}

.timer {
  color: var(--danger);
  font-size: 13.5px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.spacer {
  flex: 1;
}

.icon-btn {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-s);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}

.icon-btn:hover {
  background: var(--hover);
  color: var(--text);
}

.level {
  height: 5px;
  border-radius: 999px;
  background: var(--code-bg);
  overflow: hidden;
}

.level i {
  display: block;
  height: 100%;
  border-radius: 999px;
  background: var(--danger);
  transition: width 0.12s linear;
}

.meta {
  display: flex;
  align-items: center;
  gap: 6px;
}

.chip {
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--panel-2);
  color: var(--text-2);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.chip.quiet {
  background: transparent;
  border: 1px solid var(--border);
  color: var(--text-3);
}

.where {
  max-width: 46%;
  overflow: hidden;
  color: var(--text-3);
  font-size: 11.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field {
  display: flex;
  align-items: center;
  gap: 8px;
}

.label {
  flex: none;
  color: var(--text-3);
  font-size: 12px;
}

.target {
  flex: 1;
}

.action-inner {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.saved {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-m);
  background: var(--panel-2);
}

.saved-icon {
  flex: none;
  color: var(--success);
}

.saved-body {
  flex: 1;
  min-width: 0;
}

.saved-title {
  color: var(--text-2);
  font-size: 12px;
}

.saved-path {
  overflow: hidden;
  color: var(--text-3);
  font-family: var(--font-mono);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.err {
  color: var(--danger);
  font-size: 12px;
  word-break: break-all;
}

.tip {
  color: var(--text-3);
  font-size: 11px;
  line-height: 1.6;
}

.tip code {
  padding: 0 3px;
  border-radius: 3px;
  background: var(--panel-2);
}
</style>
