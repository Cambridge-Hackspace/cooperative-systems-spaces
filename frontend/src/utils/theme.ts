/** Stored theme value meaning "follow the OS/browser light-dark setting". */
export const SYSTEM_THEME = 'system'

const prefersDark =
  typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null

/**
 * Resolve a stored theme preference to an actual daisyUI theme name.
 * Anonymous visitors and users who haven't picked a theme have no stored
 * value at all, which is treated the same as an explicit "system" choice.
 */
export function resolveTheme(theme: string | null | undefined): string {
  if (!theme || theme === SYSTEM_THEME) {
    return prefersDark?.matches ? 'css-dark' : 'css-light'
  }
  return theme
}

/** Subscribe to OS light/dark changes; returns an unsubscribe function. */
export function onSystemThemeChange(callback: () => void): () => void {
  prefersDark?.addEventListener('change', callback)
  return () => prefersDark?.removeEventListener('change', callback)
}
