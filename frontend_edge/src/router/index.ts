import { createRouter, createWebHistory } from 'vue-router'
import StatusView from '@/views/StatusView.vue'

const router = createRouter({
  history: createWebHistory('/'),
  routes: [
    {
      path: '/',
      name: 'status',
      component: StatusView
    }
  ]
})

export default router
