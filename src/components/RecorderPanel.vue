<script setup lang="ts">
/**
 * 浮动录音面板。
 *
 * - 收起态：一颗胶囊；录音中变成红色计时胶囊（带跳动的电平条）；
 * - 展开态：**尽量小的一行小条**（v0.3.6 起）——待录时「记到 ▾ + 开始」，
 *   录音中「计时 + 电平 + 停止」，其余信息压到 11px 的副行里；
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

/** 本场（当前录音目录）已经存下的分段数。 */
const sessionCount = computed(() =>
  store.recordDir
    ? store.recentClips.filter((c) => c.dir === store.recordDir).length
    : store.recentClips.length,
)

/** 插入本场的长语音引用（一条 = 一整场，播放器自己连播分段）。 */
async function insertLast() {
  const dir = store.lastClip?.dir || store.recordDir
  if (!dir) return
  await store.insertSessionRef(dir)
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

    <!-- 展开态：一行小条（占位尽量小，别挡内容） -->
    <div v-else class="panel">
      <template v-if="store.recording">
        <div class="row">
          <span class="dot" aria-hidden="true" />
          <span class="timer">{{ elapsed }}</span>
          <span class="level" aria-hidden="true"><i :style="{ width: `${store.levelPercent}%` }" /></span>
          <button class="mini danger" title="停止录音" @click="store.stopRecording()">
            <t-icon name="stop-circle-filled" size="14px" />
            <span>停止</span>
          </button>
          <button class="icon-btn" title="收起（后台继续录）" @click="store.toggleRecorder(false)">
            <t-icon name="chevron-down" size="14px" />
          </button>
        </div>
        <div class="sub">
          <span class="chip">第 {{ store.recordSeq }} 段</span>
          <span class="chip">已存 {{ sessionCount }} 段</span>
          <span v-if="store.transcribeDone || store.transcribeFailed" class="chip">
            转写 {{ store.transcribeDone }}<template v-if="store.transcribeFailed">
              · 失败 {{ store.transcribeFailed }}</template
            >
          </span>
          <span class="where" :title="targetLabel">· {{ targetLabel }}</span>
        </div>
      </template>

      <template v-else>
        <div class="row">
          <t-select
            :value="store.targetNoteId"
            class="target"
            size="small"
            placeholder="记到哪篇笔记"
            :title="targetLabel"
            @change="(v: unknown) => store.setTarget(String(v))"
          >
            <t-option
              v-for="opt in store.targetOptions"
              :key="opt.value"
              :value="opt.value"
              :label="opt.label"
            />
          </t-select>
          <button
            class="mini danger"
            :disabled="!store.targetNoteId"
            title="开始录音"
            @click="store.startRecording()"
          >
            <t-icon name="microphone-1" size="14px" />
            <span>开始</span>
          </button>
          <button class="icon-btn" title="收起" @click="store.toggleRecorder(false)">
            <t-icon name="chevron-down" size="14px" />
          </button>
        </div>

        <div v-if="store.lastClip" class="saved">
          <t-icon class="saved-icon" name="check-circle" size="13px" />
          <span class="saved-text" :title="store.lastClip.dir">
            本场已存 {{ sessionCount }} 段
          </span>
          <button class="link" title="插入一行整场引用（一条长语音）" @click="insertLast">
            插入整场
          </button>
        </div>

        <div v-if="store.transcribeHint" class="sub">
          <span class="chip">{{ store.transcribeHint }}</span>
        </div>
      </template>

      <div v-if="store.error" class="err">{{ store.error }}</div>
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

/* ---- 展开态：一行小条（236px 宽，待录态仅一行 ≈40px 高） ---- */

.panel {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 236px;
  max-width: calc(100vw - 28px);
  padding: 8px 9px;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  background: var(--panel);
  box-shadow: var(--shadow-2);
}

.row {
  display: flex;
  align-items: center;
  gap: 7px;
}

.target {
  flex: 1;
  min-width: 0;
}

.dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--danger);
  animation: dot-blink 1.3s ease-in-out infinite;
}

@keyframes dot-blink {
  0%,
  100% {
    opacity: 1;
  }

  50% {
    opacity: 0.3;
  }
}

.timer {
  flex: none;
  color: var(--danger);
  font-size: 12.5px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.level {
  flex: 1;
  min-width: 24px;
  height: 4px;
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

/* 小按钮：开始 / 停止 */
.mini {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 3px;
  height: 26px;
  padding: 0 9px;
  border: 1px solid transparent;
  border-radius: 999px;
  font-family: var(--font-sans);
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.mini.danger {
  background: var(--danger);
  color: #fff;
}

.mini.danger:hover:not(:disabled) {
  filter: brightness(1.08);
}

.mini:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.icon-btn {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
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

/* 副行：11px 轻文字，超长省略 */
.sub {
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  color: var(--text-3);
  font-size: 11px;
  line-height: 1.45;
  white-space: nowrap;
}

.chip {
  flex: none;
  font-variant-numeric: tabular-nums;
}

.where {
  overflow: hidden;
  text-overflow: ellipsis;
}

.saved {
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  color: var(--text-2);
  font-size: 11.5px;
  line-height: 1.45;
}

.saved-icon {
  flex: none;
  color: var(--success);
}

.saved-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.link {
  flex: none;
  margin-left: auto;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--primary);
  font-family: var(--font-sans);
  font-size: 11.5px;
  cursor: pointer;
}

.link:hover {
  text-decoration: underline;
}

.err {
  color: var(--danger);
  font-size: 11.5px;
  word-break: break-all;
}

/* 触屏没有 hover，按钮放大一档（仍是一行小条，只是好点） */
@media (pointer: coarse) {
  .mini {
    height: 30px;
    padding: 0 11px;
    font-size: 12.5px;
  }

  .icon-btn {
    width: 26px;
    height: 26px;
  }

  .capsule {
    height: 38px;
  }
}
</style>
