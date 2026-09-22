import { createRouter, createWebHashHistory } from 'vue-router'
import NotesView from '../views/NotesView.vue'
import ChatView from '../views/ChatView.vue'
import SettingsView from '../views/SettingsView.vue'

// 图谱视图依赖 G6（gzip 后 ~800KB），按需加载：不进首屏包
const GraphView = () => import('../views/GraphView.vue')

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/notes' },
    { path: '/notes', name: 'notes', component: NotesView },
    { path: '/chat', name: 'chat', component: ChatView },
    { path: '/graph', name: 'graph', component: GraphView },
    // 旧版会议页已合并进「会议笔记」；老书签/深链兜底
    { path: '/meetings', redirect: '/notes' },
    { path: '/settings', name: 'settings', component: SettingsView },
  ],
})
