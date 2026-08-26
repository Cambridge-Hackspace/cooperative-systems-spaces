import { defineStore } from 'pinia'
import { ref } from 'vue'
import { apiClient } from '@/utils/api'

export enum LinkLocation {
  Navigation = 'Navigation',
  HomePage = 'HomePage',
  Both = 'Both',
}

export interface PublicPagesConfig {
  wiki_enabled: boolean
  wiki_link: string // Serialized from Rust enum
  site_enabled: boolean
  site_link: string // Serialized from Rust enum
}

export interface PublicSiteConfig {
  site_name: string
}

export interface PublicToolConfig {
  tool_categories: Array<{
    value: string
    label: string
  }>
}

export interface PublicRegistrationChallengeConfig {
  enabled: boolean
  hint: string
  throttle_enabled: boolean
  terms_of_service_checkbox: boolean
  terms_of_service_md: string
  recaptcha_enabled: boolean
  recaptcha_site_key: string
}

export interface PublicDoorsConfig {
  enabled: boolean
}

export interface PublicCalendarConfig {
  enabled: boolean
}

export interface PublicToolGuardConfig {
  enabled: boolean
}

export interface PublicConfig {
  registration_challenge: PublicRegistrationChallengeConfig
  tools: PublicToolConfig
  site: PublicSiteConfig
  pages: PublicPagesConfig
  // Feature flags surfaced from server config.toml. All default to false on
  // older servers that don't yet emit these blocks.
  doors?: PublicDoorsConfig
  calendar?: PublicCalendarConfig
  toolguard?: PublicToolGuardConfig
}

export const useConfigStore = defineStore('config', () => {
  // State
  const config = ref<PublicConfig | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // Actions
  async function fetchConfig() {
    loading.value = true
    error.value = null

    try {
      console.log('Fetching config from /config/public...')
      const response = await apiClient.get<PublicConfig>('/config/public')
      console.log('Raw API response:', response)
      console.log('Response data:', response.data)

      // Check if data is wrapped in ApiResponse structure
      if (response.success && response.data) {
        // Wrapped response: { success: true, data: { ... } }
        config.value = response.data
      } else {
        // Handle error case
        if (!response.success) {
          console.error('API returned unsuccessful response:', response)
          throw new Error(response.error || 'Failed to fetch config')
        }

        // This shouldn't happen, but handle it just in case
        console.error('Invalid response structure:', response)
        throw new Error('Failed to fetch config - no data in response')
      }

      console.log('Config loaded successfully:', config.value)
      console.log('Pages config:', config.value.pages)
      console.log('Wiki enabled:', config.value.pages?.wiki_enabled)
      console.log('Wiki link:', config.value.pages?.wiki_link)
      console.log('Site enabled:', config.value.pages?.site_enabled)
      console.log('Site link:', config.value.pages?.site_link)
    } catch (err: any) {
      error.value = err.message || 'Failed to fetch configuration'
      console.error('Config fetch error:', err)
      console.error('Error details:', err.response?.data)
    } finally {
      loading.value = false
    }
  }

  // Helper computed values
  // `?.` on `pages` as well as on `config`, and the difference is the whole
  // application.
  //
  // These four guards used to read `config.value?.pages.wiki_enabled`: the
  // optional chain covered `config` and stopped there. A config object whose
  // `pages` block is absent therefore threw
  // `Cannot read properties of undefined` -- inside a computed, during render.
  //
  // Vue stops patching a component whose render function throws. So App.vue
  // froze on whatever it had last rendered, which is the full-screen
  // `fixed inset-0 ... z-50` loading overlay: the application drew itself
  // correctly and then accepted no input at all, with no error visible anywhere
  // except the console.
  //
  // The type's own comment says feature blocks "default to false on older
  // servers that don't yet emit these blocks" -- so a server that omits one is
  // an expected case, and the guard was one level too shallow to survive it.
  //
  // Found by the browser tier, whose fake happened to serve a config without
  // `pages`. The fake was wrong; that is what made it a good test.
  function shouldShowWikiInNav(): boolean {
    if (!config.value?.pages?.wiki_enabled) {
      console.log('Wiki not enabled:', config.value?.pages)
      return false
    }
    const link = config.value?.pages?.wiki_link
    console.log('Wiki link location:', link)
    return link === 'Navigation' || link === 'Both'
  }

  function shouldShowSiteInNav(): boolean {
    if (!config.value?.pages?.site_enabled) {
      console.log('Site not enabled:', config.value?.pages)
      return false
    }
    const link = config.value?.pages?.site_link
    console.log('Site link location:', link)
    return link === 'Navigation' || link === 'Both'
  }

  function shouldShowWikiOnHomePage(): boolean {
    if (!config.value?.pages?.wiki_enabled) return false
    const link = config.value?.pages?.wiki_link
    return link === 'HomePage' || link === 'Both'
  }

  function shouldShowSiteOnHomePage(): boolean {
    if (!config.value?.pages?.site_enabled) return false
    const link = config.value?.pages?.site_link
    return link === 'HomePage' || link === 'Both'
  }

  function doorsEnabled(): boolean {
    return !!config.value?.doors?.enabled
  }

  function calendarEnabled(): boolean {
    return !!config.value?.calendar?.enabled
  }

  function toolguardEnabled(): boolean {
    return !!config.value?.toolguard?.enabled
  }

  return {
    config,
    loading,
    error,
    fetchConfig,
    shouldShowWikiInNav,
    shouldShowSiteInNav,
    shouldShowWikiOnHomePage,
    shouldShowSiteOnHomePage,
    doorsEnabled,
    calendarEnabled,
    toolguardEnabled,
  }
})
