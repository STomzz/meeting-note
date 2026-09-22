import { createRouter, createWebHashHistory } from 'vue-router'
import NotesView from '../views/NotesView.vue'
import ChatView from '../views/ChatView.vue'
import GraphView from '../views/GraphView.vue'
import MeetingsView from '../views/MeetingsView.vue'
import SettingsView from '../views/SettingsView.vue'

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
