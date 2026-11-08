import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/',
      name: 'home',
      component: () => import('@/pages/index.vue'),
      meta: { title: 'Dashboard' }
    },
    {
      path: '/add-game',
      name: 'add-game',
      component: () => import('@/pages/add-game.vue'),
      meta: { title: 'Add Game' }
    },
    {
      path: '/game/:id',
      name: 'game-detail',
      component: () => import('@/pages/game-detail.vue'),
      meta: { title: 'Game & Save History' }
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/pages/settings.vue'),
      meta: { title: 'Settings & Cloud Storage' }
    },
    {
      path: '/about',
      name: 'about',
      component: () => import('@/pages/about.vue'),
      meta: { title: 'About' }
    }
  ]
})

export default router
