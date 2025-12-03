import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { UserRole } from '@/types'

const router = createRouter({
  history: createWebHistory('/'),
  routes: [
    {
      path: '/',
      name: 'home',
      component: () => import('@/views/HomeView.vue')
    },
    {
      path: '/about',
      name: 'about',
      component: () => import('@/views/AboutView.vue')
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: {
        requiresGuest: true
      }
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterView.vue'),
      meta: {
        requiresGuest: true
      }
    },
    {
      path: '/profile/:userId?',
      name: 'profile',
      component: () => import('@/views/ProfileView.vue'),
      meta: {
        requiresAuth: true
      }
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin
      }
    },
    {
      path: '/admin/roster',
      name: 'admin-roster',
      component: () => import('@/views/RosterView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff
      }
    },
    {
      path: '/admin/audit',
      name: 'admin-audit',
      component: () => import('@/views/AuditView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin
      }
    },
    {
      path: '/tools',
      name: 'tools',
      component: () => import('@/views/ToolsView.vue'),
      meta: {
        requiresAuth: true
      }
    },
    {
      path: '/training',
      name: 'training',
      component: () => import('@/views/TrainingView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff
      }
    },
    {
      path: '/users',
      name: 'users',
      component: () => import('@/views/UsersView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff
      }
    },
    {
      path: '/users/:userId',
      name: 'user-detail',
      component: () => import('@/views/ProfileView.vue'),
      meta: {
        requiresAuth: true
      }
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('@/views/NotFoundView.vue')
    }
  ]
})

// Navigation guard for authentication and authorization
router.beforeEach(async (to) => {
  const authStore = useAuthStore()
  
  // Initialize auth store if needed
  if (!authStore.initialized) {
    await authStore.initialize()
  }

  const requiresAuth = to.matched.some(record => record.meta.requiresAuth)
  const requiresGuest = to.matched.some(record => record.meta.requiresGuest)
  const requiredRole = to.matched.find(record => record.meta.requiredRole)?.meta.requiredRole as UserRole | undefined

  // Handle guest-only routes (login, register)
  if (requiresGuest && authStore.isAuthenticated) {
    return { name: 'home' }
  }

  // Handle authentication requirement
  if (requiresAuth && !authStore.isAuthenticated) {
    return { 
      name: 'login', 
      query: { redirect: to.fullPath }
    }
  }

  // Handle role-based access control
  if (requiredRole && authStore.user) {
    const userRole = authStore.user.role
    
    // Check if user has required role or higher
    const roleHierarchy: Record<string, number> = {
      'unknown': 0,
      'newbie': 1,
      'member': 2,
      'staff': 3,
      'admin': 4
    }
    
    const userRoleString = String(userRole).toLowerCase()
    const requiredRoleString = String(requiredRole).toLowerCase()
    
    const userLevel = roleHierarchy[userRoleString] || 0
    const requiredLevel = roleHierarchy[requiredRoleString] || 0
    
    if (userLevel < requiredLevel) {
      return { name: 'home' }
    }
  }

  // Handle profile routes
  if (to.name === 'profile' && !to.params.userId) {
    // Redirect /profile to /profile/me for current user
    return { name: 'profile', params: { userId: 'me' } }
  }

  return true
})

export default router
