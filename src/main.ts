import { createApp } from 'vue'
import { createPinia } from 'pinia'
import TDesign from 'tdesign-vue-next'
import 'tdesign-vue-next/es/style/index.css'
import App from './App.vue'
import { router } from './router'
import { defineSessionPlayer } from './core/sessionPlayer'
import './styles.css'

// 场次播放器（一条长语音）：必须在渲染任何笔记之前注册，插进 HTML 的标签才会被升级
defineSessionPlayer()

createApp(App).use(createPinia()).use(router).use(TDesign).mount('#app')
