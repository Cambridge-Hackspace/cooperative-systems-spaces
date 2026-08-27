import { createRouter, createWebHistory } from 'vue-router'
import StatusView from '@/views/StatusView.vue'
import ToolGuardView from '@/views/ToolGuardView.vue'

const router = createRouter({
  history: createWebHistory('/'),
  routes: [
    {
      path: '/',
      name: 'status',
      component: StatusView,
    },
    {
      path: '/toolguard',
      name: 'toolguard',
      component: ToolGuardView,
    },
  ],
})

export default router
