import { createRouter, createWebHashHistory } from 'vue-router'
import NotesView from '../views/NotesView.vue'
import ChatView from '../views/ChatView.vue'
import MeetingsView from '../views/MeetingsView.vue'
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
    { path: '/meetings', name: 'meetings', component: MeetingsView },
    { path: '/settings', name: 'settings', component: SettingsView },
  ],
})
