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

/** 左侧导航收起（只留图标）：给编辑区让宽度，状态记在 localStorage。 */
const NAV_KEY = 'bnu-notes-nav-collapsed'

function readNavCollapsed(): boolean {
  try {
    return localStorage.getItem(NAV_KEY) === '1'
  } catch {
    return false
  }
}

const navCollapsed = ref(readNavCollapsed())

function toggleNav() {
  navCollapsed.value = !navCollapsed.value
  try {
    localStorage.setItem(NAV_KEY, navCollapsed.value ? '1' : '0')
  } catch {
    // 存不了不影响本次切换
  }
}

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
    <aside class="sidebar" :class="{ collapsed: navCollapsed }">
      <div class="brand">
        <span class="brand-logo">BNU</span>
        <span v-if="!navCollapsed" class="brand-name">Notes</span>
        <span class="spacer" />
        <button
          class="icon-btn nav-toggle"
          :title="navCollapsed ? '展开侧栏' : '收起侧栏'"
          @click="toggleNav"
        >
          <t-icon :name="navCollapsed ? 'chevron-right' : 'chevron-left'" size="15px" />
        </button>
      </div>
      <nav class="nav">
        <router-link
          v-for="item in nav"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          active-class="active"
          :title="item.label"
        >
          <t-icon :name="item.icon" size="17px" />
          <span v-if="!navCollapsed">{{ item.label }}</span>
        </router-link>
      </nav>
      <div class="sidebar-foot">
        <t-icon v-if="!navCollapsed" name="desktop" size="14px" />
        <span v-if="!navCollapsed" class="foot-text">{{ modeText }}</span>
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
