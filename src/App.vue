<script setup lang="ts">
import { computed } from 'vue'
import { isTauri } from './platform'

const nav = [
  { path: '/notes', label: '笔记', icon: '📝' },
  { path: '/chat', label: '问答', icon: '💬' },
  { path: '/graph', label: '图谱', icon: '🕸️' },
  { path: '/meetings', label: '会议', icon: '🎙️' },
  { path: '/settings', label: '设置', icon: '⚙️' },
]

const modeText = computed(() => (isTauri() ? '桌面客户端模式' : '浏览器预览模式'))
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
          <span class="nav-icon">{{ item.icon }}</span>
          <span>{{ item.label }}</span>
        </router-link>
      </nav>
      <div class="sidebar-foot">{{ modeText }}</div>
    </aside>
    <main class="main">
      <router-view />
    </main>
  </div>
</template>
