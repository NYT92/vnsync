import { createApp } from 'vue'
import ui from "@nuxt/ui/vue-plugin";

import App from './App.vue'
import router from './router'

import './assets/style.css'

const app = createApp(App)

// Block devtools / reload / view-source shortcuts in production builds only.
if (import.meta.env.PROD) {
  window.addEventListener('keydown', (e) => {
    const key = e.key.toUpperCase()
    const ctrl = e.ctrlKey || e.metaKey
    if (
      key === 'F12' ||
      key === 'F5' ||
      (ctrl && e.shiftKey && ['I', 'J', 'C'].includes(key)) ||
      (ctrl && ['R', 'U', 'P'].includes(key))
    ) {
      e.preventDefault()
      e.stopPropagation()
    }
  }, true)
}

app.use(router)
app.use(ui)

app.mount('#app')
