import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { UserRole } from '@/types'

const router = createRouter({
  history: createWebHistory('/'),
  routes: [
    {
      path: '/',
      name: 'home',
      component: () => import('@/views/HomeView.vue'),
    },
    {
      path: '/about',
      name: 'about',
      component: () => import('@/views/AboutView.vue'),
    },
    {
      path: '/contact',
      name: 'contact',
      component: () => import('@/views/ContactView.vue'),
    },
    {
      path: '/platform',
      name: 'platform',
      component: () => import('@/views/PlatformView.vue'),
    },
    {
      path: '/join',
      name: 'join',
      component: () => import('@/views/JoinView.vue'),
    },
    {
      path: '/events',
      name: 'events',
      component: () => import('@/views/EventsView.vue'),
    },
    {
      path: '/directions',
      name: 'directions',
      component: () => import('@/views/DirectionsView.vue'),
    },
    {
      path: '/terms',
      name: 'terms',
      component: () => import('@/views/TermsView.vue'),
    },
    {
      path: '/privacy',
      name: 'privacy',
      component: () => import('@/views/PrivacyView.vue'),
    },
    {
      path: '/501c3',
      name: 'nonprofit',
      component: () => import('@/views/NonProfitView.vue'),
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: {
        requiresGuest: true,
      },
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterView.vue'),
      meta: {
        requiresGuest: true,
      },
    },
    {
      path: '/forgot-password',
      name: 'forgot-password',
      component: () => import('@/views/ForgotPasswordView.vue'),
      meta: {
        requiresGuest: true,
      },
    },
    {
      // No meta at all, deliberately -- not even requiresGuest.
      //
      // These two are reached by opening a link from an email, and the person
      // doing that may well still have a live session in that browser. Under
      // requiresGuest the navigation guard would bounce them to the home page,
      // and the emailed link would appear to do nothing whatsoever.
      path: '/reset-password',
      name: 'reset-password',
      component: () => import('@/views/ResetPasswordView.vue'),
    },
    {
      path: '/verify-email',
      name: 'verify-email',
      component: () => import('@/views/VerifyEmailView.vue'),
    },
    {
      path: '/profile/mfa',
      name: 'profile-mfa',
      component: () => import('@/components/MfaSettings.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/profile/card',
      name: 'profile-card',
      component: () => import('@/views/CardSetupView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/profile/password',
      name: 'profile-password',
      component: () => import('@/views/PasswordChangeView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/profile/:userId?',
      name: 'profile',
      component: () => import('@/views/ProfileView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/roster',
      name: 'admin-roster',
      component: () => import('@/views/RosterView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff,
      },
    },
    {
      path: '/admin/audit',
      name: 'admin-audit',
      component: () => import('@/views/AuditView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/devices',
      name: 'admin-devices',
      component: () => import('@/components/DeviceManagement.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/webhooks',
      name: 'admin-webhooks',
      component: () => import('@/components/WebhookManagement.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/profile-config',
      name: 'admin-profile-config',
      component: () => import('@/components/ProfileConfigAdmin.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/doors',
      redirect: { name: 'admin-facility', query: { tab: 'doors' } },
    },
    {
      path: '/admin/facility',
      name: 'admin-facility',
      component: () => import('@/components/FacilityManagement.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/nfc-provisioning',
      name: 'admin-nfc-provisioning',
      component: () => import('@/components/NfcDeviceProvisioning.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    {
      path: '/admin/home-links',
      name: 'admin-home-links',
      component: () => import('@/components/HomeLinkManagement.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Admin,
      },
    },
    // Backward-compatible deep links — redirect into the combined Facility page.
    {
      path: '/admin/places',
      redirect: { name: 'admin-facility', query: { tab: 'places' } },
    },
    {
      path: '/door/:id/checkin',
      name: 'door-checkin',
      component: () => import('@/views/DoorCheckinView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/tools',
      name: 'tools',
      component: () => import('@/views/ToolsView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/training',
      name: 'training',
      component: () => import('@/views/TrainingView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff,
      },
    },
    {
      path: '/users',
      name: 'users',
      component: () => import('@/views/UsersView.vue'),
      meta: {
        requiresAuth: true,
        requiredRole: UserRole.Staff,
      },
    },
    {
      path: '/users/:userId',
      name: 'user-detail',
      component: () => import('@/views/ProfileView.vue'),
      meta: {
        requiresAuth: true,
      },
    },
    {
      path: '/wiki/:slug(.*)*',
      name: 'wiki',
      component: () => import('@/views/WikiView.vue'),
    },
    {
      path: '/page/:slug(.*)*',
      name: 'site',
      component: () => import('@/views/SiteView.vue'),
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('@/views/NotFoundView.vue'),
    },
  ],
})

// Navigation guard for authentication and authorization
router.beforeEach(async (to) => {
  const authStore = useAuthStore()

  // Initialize auth store if needed
  if (!authStore.initialized) {
    await authStore.initialize()
  }

  const requiresAuth = to.matched.some((record) => record.meta.requiresAuth)
  const requiresGuest = to.matched.some((record) => record.meta.requiresGuest)
  const requiredRole = to.matched.find((record) => record.meta.requiredRole)?.meta.requiredRole as
    UserRole | undefined

  // Handle guest-only routes (login, register)
  if (requiresGuest && authStore.isAuthenticated) {
    return { name: 'home' }
  }

  // Handle authentication requirement
  if (requiresAuth && !authStore.isAuthenticated) {
    return {
      name: 'login',
      query: { redirect: to.fullPath },
    }
  }

  // Handle role-based access control
  if (requiredRole && authStore.user) {
    const userRole = authStore.user.role

    // Check if user has required role or higher
    const roleHierarchy: Record<string, number> = {
      unknown: 0,
      newbie: 1,
      member: 2,
      staff: 3,
      admin: 4,
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
