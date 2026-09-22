<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { isTauri } from './platform'
import RecorderPanel from './components/RecorderPanel.vue'
import {
  THEME_EVENT,
  THEME_LABELS,
  getThemeMode,
  initTheme,
  setThemeMode,
  type ThemeMode,
} from './core/theme'

const nav = [
  { path: '/notes', label: '会议笔记', icon: 'edit-1' },
  { path: '/chat', label: '问答', icon: 'chat-bubble' },
  { path: '/graph', label: '图谱', icon: 'relation' },
  { path: '/settings', label: '设置', icon: 'setting' },
]

const modeText = computed(() => (isTauri() ? '桌面客户端模式' : '浏览器预览模式'))

const theme = ref<ThemeMode>(getThemeMode())
let disposeTheme: (() => void) | null = null

function onThemeEvent(event: Event) {
  const mode = (event as CustomEvent<ThemeMode>).detail
  if (mode === 'auto' || mode === 'light' || mode === 'dark') theme.value = mode
}

onMounted(() => {
  disposeTheme = initTheme()
  window.addEventListener(THEME_EVENT, onThemeEvent)
})

onBeforeUnmount(() => {
  disposeTheme?.()
  window.removeEventListener(THEME_EVENT, onThemeEvent)
})

const themeIcon = computed(() =>
  theme.value === 'dark' ? 'moon' : theme.value === 'light' ? 'sunny' : 'desktop',
)

/** 循环切换：跟随系统 → 浅色 → 深色。 */
function cycleTheme() {
  const order: ThemeMode[] = ['auto', 'light', 'dark']
  const next = order[(order.indexOf(theme.value) + 1) % order.length]
  theme.value = next
  setThemeMode(next)
}
</script>

<template>
  <div class="app">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-logo">BNU</span>
        <span class="brand-name">Notes</span>
      </div>
      <nav class="nav">
        <router-link
          v-for="item in nav"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          active-class="active"
        >
          <t-icon :name="item.icon" size="17px" />
          <span>{{ item.label }}</span>
        </router-link>
      </nav>
      <div class="sidebar-foot">
        <t-icon name="desktop" size="14px" />
        <span class="foot-text">{{ modeText }}</span>
        <span class="spacer" />
        <button
          class="icon-btn"
          :title="`主题：${THEME_LABELS[theme]}（点击切换）`"
          @click="cycleTheme"
        >
          <t-icon :name="themeIcon" size="15px" />
        </button>
      </div>
    </aside>
    <main class="main">
      <router-view />
    </main>
    <!-- 浮动录音面板：任意页面都能录音并写入会议笔记 -->
    <RecorderPanel />
  </div>
</template>

<style scoped>
.foot-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
