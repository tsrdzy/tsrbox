import { defineStore } from 'pinia'
import { ref } from 'vue'

import pkg from '@root/package.json'

function isNewerVersion(latest, current) {
  const latestParts = String(latest).split('.').map((v) => Number(v) || 0)
  const currentParts = String(current).split('.').map((v) => Number(v) || 0)
  const length = Math.max(latestParts.length, currentParts.length)
  for (let i = 0; i < length; i++) {
    const a = latestParts[i] || 0
    const b = currentParts[i] || 0
    if (a > b) return true
    if (a < b) return false
  }
  return false
}

// ===== 更新检查 =====
// 网络不可达（国内网络常见）或仓库尚无 Release 时静默处理，不输出错误日志
const UPDATE_API = 'https://api.github.com/repos/tsrdzy/tsrbox/releases/latest'
const UPDATE_CACHE_MS = 10 * 60 * 1000
let updateCache = null

async function requestLatestRelease() {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), 8000)
  try {
    const response = await fetch(UPDATE_API, {
      signal: controller.signal,
      cache: 'no-store',
      headers: { Accept: 'application/vnd.github+json' }
    })
    if (response.status === 404) return { status: 'no-release' }
    if (!response.ok) return { status: 'failed' }
    return { status: 'ok', data: await response.json() }
  } catch {
    // 断网 / 超时 / 被拦截
    return { status: 'offline' }
  } finally {
    clearTimeout(timer)
  }
}

export const useSettingsStore = defineStore('settings', () => {
  const isDark = ref(true)
  const showThemeInTitlebar = ref(true)
  const showLanguageInTitlebar = ref(true)
  const deviceInfo = ref(null)
  // 默认启动时检查更新，用户可在设置中关闭
  const checkForUpdates = ref(true)
  const skin = ref('#409EFF')
  const shortcuts = ref({
    selectAll: { key: 'a', ctrl: true },
    delete: { key: 'Delete', ctrl: false },
    export: { key: 's', ctrl: true },
    arrowUp: { key: 'ArrowUp', ctrl: false },
    arrowDown: { key: 'ArrowDown', ctrl: false },
    arrowLeft: { key: 'ArrowLeft', ctrl: false },
    arrowRight: { key: 'ArrowRight', ctrl: false }
  })

  async function loadDeviceInfo() {
    try {
      const info = await window.api.logs.getDeviceInfo()
      deviceInfo.value = info
      localStorage.setItem('imodel-deviceInfo', JSON.stringify(info))
    } catch (e) {
      console.error('加载设备信息失败:', e)
    }
  }

  function init() {
    const savedTheme = localStorage.getItem('imodel-theme')
    if (savedTheme === 'light') isDark.value = false
    else isDark.value = true

    const savedThemeTitlebar = localStorage.getItem('imodel-showThemeInTitlebar')
    if (savedThemeTitlebar !== null) showThemeInTitlebar.value = savedThemeTitlebar === 'true'

    const savedLangTitlebar = localStorage.getItem('imodel-showLanguageInTitlebar')
    if (savedLangTitlebar !== null) showLanguageInTitlebar.value = savedLangTitlebar === 'true'

    const savedDeviceInfo = localStorage.getItem('imodel-deviceInfo')
    if (savedDeviceInfo) {
      try {
        deviceInfo.value = JSON.parse(savedDeviceInfo)
      } catch {
        deviceInfo.value = null
      }
    }

    const savedCheckUpdates = localStorage.getItem('imodel-checkForUpdates')
    if (savedCheckUpdates !== null) checkForUpdates.value = savedCheckUpdates === 'true'

    const savedSkin = localStorage.getItem('imodel-skin')
    if (savedSkin) skin.value = savedSkin

    const savedShortcuts = localStorage.getItem('imodel-shortcuts')
    if (savedShortcuts) {
      try {
        shortcuts.value = { ...shortcuts.value, ...JSON.parse(savedShortcuts) }
      } catch {
        // ignore
      }
    }

    applyTheme()
  }

  function applyTheme() {
    const root = document.documentElement
    if (isDark.value) root.classList.add('dark')
    else root.classList.remove('dark')
    applySkin()
  }

  function applySkin() {
    const root = document.documentElement
    root.style.setProperty('--el-color-primary', skin.value)
    root.style.setProperty('--primary', skin.value)
    root.style.setProperty('--primary-hover', skin.value)
    root.style.setProperty('--primary-soft', `${skin.value}20`)
  }

  function setSkin(color) {
    skin.value = color
    localStorage.setItem('imodel-skin', color)
    applySkin()
  }

  function toggleTheme() {
    isDark.value = !isDark.value
    localStorage.setItem('imodel-theme', isDark.value ? 'dark' : 'light')
    applyTheme()
  }

  function setShowThemeInTitlebar(val) {
    showThemeInTitlebar.value = val
    localStorage.setItem('imodel-showThemeInTitlebar', val ? 'true' : 'false')
  }

  function setShowLanguageInTitlebar(val) {
    showLanguageInTitlebar.value = val
    localStorage.setItem('imodel-showLanguageInTitlebar', val ? 'true' : 'false')
  }

  function setCheckForUpdates(val) {
    checkForUpdates.value = val
    localStorage.setItem('imodel-checkForUpdates', val ? 'true' : 'false')
  }

  function setShortcuts(key, value) {
    shortcuts.value[key] = value
    localStorage.setItem('imodel-shortcuts', JSON.stringify(shortcuts.value))
  }

  function resetShortcuts() {
    shortcuts.value = {
      selectAll: { key: 'a', ctrl: true },
      delete: { key: 'Delete', ctrl: false },
      export: { key: 's', ctrl: true },
      arrowUp: { key: 'ArrowUp', ctrl: false },
      arrowDown: { key: 'ArrowDown', ctrl: false },
      arrowLeft: { key: 'ArrowLeft', ctrl: false },
      arrowRight: { key: 'ArrowRight', ctrl: false }
    }
    localStorage.setItem('imodel-shortcuts', JSON.stringify(shortcuts.value))
  }

  /**
   * 检查更新
   * @param {{ force?: boolean, manual?: boolean }} [options]
   * @returns {Promise<{ status: 'disabled'|'offline'|'failed'|'no-release'|'latest'|'update', hasUpdate: boolean, latestVersion?: string, currentVersion?: string, releaseUrl?: string }>}
   */
  async function checkUpdate(options = {}) {
    // 自动检查受设置开关约束；手动检查（设置页按钮）不受限制
    if (!checkForUpdates.value && !options.manual) {
      return { status: 'disabled', hasUpdate: false }
    }

    // 自动检查较频繁（启动 + 设置页），短时间内复用结果，避免重复请求
    if (!options.force && updateCache && Date.now() - updateCache.at < UPDATE_CACHE_MS) {
      return updateCache.result
    }

    const response = await requestLatestRelease()
    let result
    if (response.status !== 'ok') {
      result = { status: response.status, hasUpdate: false }
    } else {
      const data = response.data || {}
      const latestVersion = String(data.tag_name || '').replace(/^v/i, '')
      const currentVersion = pkg.version
      if (!latestVersion) {
        result = { status: 'no-release', hasUpdate: false }
      } else if (isNewerVersion(latestVersion, currentVersion)) {
        result = {
          status: 'update',
          hasUpdate: true,
          latestVersion,
          currentVersion,
          releaseUrl: data.html_url || ''
        }
      } else {
        result = {
          status: 'latest',
          hasUpdate: false,
          latestVersion,
          currentVersion,
          releaseUrl: data.html_url || ''
        }
      }
    }

    updateCache = { at: Date.now(), result }
    return result
  }

  return {
    isDark,
    showThemeInTitlebar,
    showLanguageInTitlebar,
    deviceInfo,
    checkForUpdates,
    skin,
    shortcuts,
    init,
    toggleTheme,
    setShowThemeInTitlebar,
    setShowLanguageInTitlebar,
    setCheckForUpdates,
    loadDeviceInfo,
    checkUpdate,
    applyTheme,
    applySkin,
    setSkin,
    setShortcuts,
    resetShortcuts
  }
})
