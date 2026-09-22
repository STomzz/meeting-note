/**
 * 主题模式：跟随系统 / 浅色 / 深色。
 *
 * TDesign 与我们的语义变量都认 `<html theme-mode="dark">`，
 * 所以这里只负责算一次「现在该不该深色」并写到根元素上。
 */

export type ThemeMode = 'auto' | 'light' | 'dark'

const STORAGE_KEY = 'bnu-notes-theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

export function getThemeMode(): ThemeMode {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw === 'light' || raw === 'dark' || raw === 'auto') return raw
  } catch {
    // localStorage 在部分 WebView 隐私模式下不可用：退回 auto
  }
  return 'auto'
}

export function resolveDark(mode: ThemeMode): boolean {
  if (mode === 'dark') return true
  if (mode === 'light') return false
  try {
    return window.matchMedia(DARK_QUERY).matches
  } catch {
    return false
  }
}

export function applyTheme(mode: ThemeMode = getThemeMode()): boolean {
  const dark = resolveDark(mode)
  const root = document.documentElement
  if (dark) root.setAttribute('theme-mode', 'dark')
  else root.removeAttribute('theme-mode')
  return dark
}

export function setThemeMode(mode: ThemeMode) {
  try {
    localStorage.setItem(STORAGE_KEY, mode)
  } catch {
    // 存不了也不影响本次生效
  }
  applyTheme(mode)
}

/** 页面启动时调用：应用当前模式，并在 auto 下跟随系统变化。返回解绑函数。 */
export function initTheme(): () => void {
  applyTheme()
  let media: MediaQueryList | null = null
  try {
    media = window.matchMedia(DARK_QUERY)
  } catch {
    media = null
  }
  if (!media) return () => {}
  const onChange = () => {
    if (getThemeMode() === 'auto') applyTheme('auto')
  }
  media.addEventListener('change', onChange)
  return () => media?.removeEventListener('change', onChange)
}

export const THEME_LABELS: Record<ThemeMode, string> = {
  auto: '跟随系统',
  light: '浅色',
  dark: '深色',
}
