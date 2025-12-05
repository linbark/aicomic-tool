import { createApp } from 'vue'
import { createPinia } from 'pinia'
import router from './router'
import './style.css'
import App from './App.vue'

const app = createApp(App)

app.use(createPinia()) // 启用状态管理
app.use(router)        // 启用路由
app.mount('#app')