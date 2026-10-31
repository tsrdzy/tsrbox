import './assets/main.css'
import './iconfont/iconfont.css'
import { createApp } from 'vue'
import { createPinia } from 'pinia'

import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'

import App from './App.vue'
import i18n from './i18n'
import router from './router'
import { installApi } from './api'
import { initPlugins } from './pluginManager'

installApi()

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(ElementPlus)
app.use(i18n).use(router)

// 加载 src/plugins/ 下的插件（目录被 gitignore，可为空）
initPlugins({ router, i18n })

app.mount('#app')
