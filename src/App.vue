<script setup>
import { ref, reactive, onMounted, onBeforeUnmount, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useStore } from './composables/useStore'
import { useFolderStore, FOLDER_COLORS } from './stores/folders'
import { useTagFilterStore } from './stores/tagFilter'
import { useSettingsStore } from './stores/settings'
import { pluginSlots } from './pluginManager'
import TagFilterBar from './components/TagFilterBar.vue'
import { ElMessageBox, ElMessage, ElConfigProvider } from 'element-plus'
import UploadProgress from './components/UploadProgress.vue'
import {
  EXPORT_FORMATS,
  exportFormatDialog,
  closeExportFormatDialog
} from './composables/exportFormatDialog'
import pkg from '@root/package.json'

import zhCn from 'element-plus/dist/locale/zh-cn.mjs'
import en from 'element-plus/dist/locale/en.mjs'
import es from 'element-plus/dist/locale/es.mjs'
import fr from 'element-plus/dist/locale/fr.mjs'
import de from 'element-plus/dist/locale/de.mjs'
import ja from 'element-plus/dist/locale/ja.mjs'
import ru from 'element-plus/dist/locale/ru.mjs'

const elementPlusLocales = {
  'zh-CN': zhCn,
  'en-US': en,
  'es-ES': es,
  'fr-FR': fr,
  'de-DE': de,
  'ja-JP': ja,
  'ru-RU': ru
}

const { state, checkStatus, loadAll, saveSettings } = useStore()
const foldersStore = useFolderStore()
const tagFilter = useTagFilterStore()
const settingsStore = useSettingsStore()
const { locale, t } = useI18n()
const route = useRoute()
const router = useRouter()

// 切换资源库时清空标签筛选，避免旧库标签残留
watch(
  () => state.libraryPath,
  () => tagFilter.clearTags()
)

// ===== 导出格式选择弹窗 =====
const exportFormatOptions = computed(() =>
  EXPORT_FORMATS.map((value) => ({
    value,
    label: t(`exportDialog.formats.${value}`),
    desc: t(`exportDialog.desc.${value}`)
  }))
)

function confirmExportFormat() {
  const format = exportFormatDialog.format
  if (exportFormatDialog.remember) {
    saveSettings({ exportFormat: format }).catch((e) =>
      console.warn('[Export] 保存默认导出格式失败:', e)
    )
  }
  closeExportFormatDialog(format)
}

const elLocale = computed(() => {
  return elementPlusLocales[locale.value] || zhCn
})

const isDark = computed(() => settingsStore.isDark)

// ===== 语言切换 =====
const languages = [
  { code: 'zh-CN', label: '中文' },
  { code: 'en-US', label: 'English' },
  { code: 'es-ES', label: 'Español' },
  { code: 'fr-FR', label: 'Français' },
  { code: 'de-DE', label: 'Deutsch' },
  { code: 'ja-JP', label: '日本語' },
  { code: 'ru-RU', label: 'Русский' }
]

function changeLanguage(lang) {
  locale.value = lang
  localStorage.setItem('imodel-language', lang)
}

function initLanguage() {
  const saved = localStorage.getItem('imodel-language')
  const validLanguages = languages.map(l => l.code)
  if (saved && validLanguages.includes(saved)) changeLanguage(saved)
  else changeLanguage('zh-CN')
}

// ===== 自定义标题栏 =====
const isMax = ref(false)
let removeMaxListener = null

function minimize() {
  window.api.windowControl.minimize()
}

function toggleMaximize() {
  window.api.windowControl.toggleMaximize()
}

function closeApp() {
  window.api.windowControl.close()
}

// ===== 更多菜单功能 =====
const GITHUB_REPO = 'https://github.com/tsrdzy/ipixel'

function openGitHubUrl(path) {
  window.api.openExternal(GITHUB_REPO + path)
}

// ===== 侧边栏文件夹 =====
const folderMenu = ref(null)
const colorDialogVisible = ref(false)
const colorTarget = ref(null)
const folderColor = ref(FOLDER_COLORS[0])
const folderDragging = ref('')
const folderDropInvalid = ref('')
/** 拖拽时的浮层提示：跟随鼠标显示正在拖动的文件夹与目标位置 */
const folderDragHint = reactive({
  visible: false,
  x: 0,
  y: 0,
  name: '',
  label: '',
  invalid: false
})

const folderCounts = computed(() => {
  const direct = {}
  for (const model of state.models) {
    const id = model.folderId || ''
    if (id) direct[id] = (direct[id] || 0) + 1
  }
  // 沿父链累计：文件夹显示数量 = 本层级 + 所有子层级的模型总数
  const totals = {}
  const byId = new Map(foldersStore.folders.map((folder) => [folder.id, folder]))
  for (const [id, count] of Object.entries(direct)) {
    if (!byId.has(id)) {
      totals[id] = (totals[id] || 0) + count
      continue
    }
    let current = byId.get(id)
    const guard = new Set()
    while (current && !guard.has(current.id)) {
      guard.add(current.id)
      totals[current.id] = (totals[current.id] || 0) + count
      current = current.parentId ? byId.get(current.parentId) : null
    }
  }
  return totals
})

function folderCount(id) {
  return folderCounts.value[id] || 0
}

function selectFolder(id) {
  // 刚完成的文件夹拖拽不应触发选中
  if (suppressFolderClick) {
    suppressFolderClick = false
    return
  }
  foldersStore.setActiveFolder(id || '')
  if (route.path !== '/model') router.push('/model')
}

/** 侧边栏选择标签后回到模型列表 */
function onSidebarTagsChanged() {
  if (route.path !== '/model') router.push('/model')
}

function openFolderMenu(event, folder) {
  event.preventDefault()
  event.stopPropagation()
  const x = Math.min(event.clientX, window.innerWidth - 180)
  const y = Math.min(event.clientY, window.innerHeight - 200)
  folderMenu.value = { x, y, folder }
}

async function handleCreateFolder(parentId = '') {
  // 侧边栏按钮直接作为 click 回调时会传入事件对象，这里只接受字符串父级 id
  const parent = typeof parentId === 'string' ? parentId : ''
  folderMenu.value = null
  let name = ''
  try {
    const result = await ElMessageBox.prompt(t('folders.namePlaceholder'), t('folders.create'), {
      confirmButtonText: t('common.ok'),
      cancelButtonText: t('common.cancel'),
      inputValidator: (v) => (v && v.trim() ? true : t('folders.nameRequired'))
    })
    name = result.value.trim()
  } catch {
    return
  }
  try {
    const folder = await foldersStore.createFolder(name, undefined, parent)
    ElMessage.success(t('folders.created'))
    selectFolder(folder.id)
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

async function handleRenameFolder(folder) {
  folderMenu.value = null
  if (!folder) return
  let name = ''
  try {
    const result = await ElMessageBox.prompt(t('folders.namePlaceholder'), t('folders.rename'), {
      inputValue: folder.name,
      confirmButtonText: t('common.ok'),
      cancelButtonText: t('common.cancel'),
      inputValidator: (v) => (v && v.trim() ? true : t('folders.nameRequired'))
    })
    name = result.value.trim()
  } catch {
    return
  }
  try {
    await foldersStore.updateFolder(folder.id, { name })
    ElMessage.success(t('folders.renamed'))
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

function openFolderColor(folder) {
  folderMenu.value = null
  if (!folder) return
  colorTarget.value = folder
  folderColor.value = folder.color || FOLDER_COLORS[0]
  colorDialogVisible.value = true
}

async function saveFolderColor() {
  if (!colorTarget.value) return
  try {
    await foldersStore.updateFolder(colorTarget.value.id, { color: folderColor.value })
    colorDialogVisible.value = false
    ElMessage.success(t('folders.colorSaved'))
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

async function handleDeleteFolder(folder) {
  folderMenu.value = null
  if (!folder) return
  try {
    await ElMessageBox.confirm(
      t('folders.deleteConfirm', { name: folder.name }),
      t('folders.delete'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        type: 'warning',
        confirmButtonClass: 'el-button--danger'
      }
    )
  } catch {
    return
  }
  try {
    await foldersStore.deleteFolder(folder.id)
    await loadAll()
    ElMessage.success(t('folders.deleted'))
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

function handleFolderCommand(command, folder) {
  if (command === 'newChild') handleCreateFolder(folder.id)
  else if (command === 'rename') handleRenameFolder(folder)
  else if (command === 'color') openFolderColor(folder)
  else if (command === 'delete') handleDeleteFolder(folder)
}

// ===== 侧边栏高度调整（文件夹 / 标签）=====
const sidebarRef = ref(null)
const folderPaneHeight = ref(Number(localStorage.getItem('tsrbox-sidebar-folder-height')) || 0)
let sidebarResize = null

const folderPaneStyle = computed(() =>
  folderPaneHeight.value > 0
    ? { flex: '0 0 auto', height: `${folderPaneHeight.value}px` }
    : {}
)

function startSidebarResize(event) {
  if (event.button !== 0) return
  event.preventDefault()
  const listEl = document.querySelector('.sidebar .folder-list')
  sidebarResize = {
    startY: event.clientY,
    startHeight: folderPaneHeight.value || listEl?.clientHeight || 200,
    total: sidebarRef.value?.clientHeight || 0
  }
  document.addEventListener('mousemove', onSidebarResizeMove)
  document.addEventListener('mouseup', stopSidebarResize)
  document.body.classList.add('sidebar-resizing')
}

function onSidebarResizeMove(event) {
  if (!sidebarResize) return
  const next = sidebarResize.startHeight + (event.clientY - sidebarResize.startY)
  const min = 90
  const max = Math.max(min + 60, sidebarResize.total - 160)
  folderPaneHeight.value = Math.round(Math.min(Math.max(next, min), max))
}

function stopSidebarResize() {
  document.removeEventListener('mousemove', onSidebarResizeMove)
  document.removeEventListener('mouseup', stopSidebarResize)
  sidebarResize = null
  document.body.classList.remove('sidebar-resizing')
  try {
    localStorage.setItem('tsrbox-sidebar-folder-height', String(folderPaneHeight.value))
  } catch {
    // ignore
  }
}

function resetSidebarResize() {
  folderPaneHeight.value = 0
  try {
    localStorage.removeItem('tsrbox-sidebar-folder-height')
  } catch {
    // ignore
  }
}

// ===== 文件夹层级拖动 =====
// 说明：Windows 上 Tauri 开启 dragDropEnabled 后会拦截 HTML5 拖拽事件，
// 因此这里和模型卡片一样用鼠标事件实现文件夹拖拽，避免影响外部文件拖入上传。
// 落点判定：行的上下边缘 = 同级插入，行中部 = 放入该文件夹，侧边栏空白 = 顶层末尾。
let folderPress = null
let folderDragActive = false
let suppressFolderClick = false

/** 当前拖拽落点 { type: 'before'|'after'|'inside'|'root', targetId?, parentId, index, depth } */
const folderDropIndicator = ref(null)

function isDropSlot(type, folderId) {
  const indicator = folderDropIndicator.value
  return !!indicator && indicator.type === type && indicator.targetId === folderId
}

function isDropInside(folderId) {
  const indicator = folderDropIndicator.value
  return !!indicator && indicator.type === 'inside' && indicator.targetId === folderId
}

function setDropInvalid(id) {
  folderDropIndicator.value = null
  folderDropInvalid.value = id
  folderDragHint.label = t('folders.dropInvalid')
  folderDragHint.invalid = true
}

function clearDrop() {
  folderDropIndicator.value = null
  folderDropInvalid.value = ''
  folderDragHint.label = t('folders.dragHint')
  folderDragHint.invalid = false
}

function onFolderMouseDown(event, folder) {
  if (event.button !== 0) return
  if (event.target?.closest?.('.folder-caret, .folder-more')) return
  folderPress = { folder, x: event.clientX, y: event.clientY }
  folderDragActive = false
  document.addEventListener('mousemove', onFolderMouseMove)
  document.addEventListener('mouseup', onFolderMouseUp)
  document.addEventListener('mouseout', onFolderMouseOut)
}

function onFolderMouseMove(event) {
  if (!folderPress) return
  if (!folderDragActive) {
    if (Math.hypot(event.clientX - folderPress.x, event.clientY - folderPress.y) < 6) return
    folderDragActive = true
    suppressFolderClick = true
    folderDragging.value = folderPress.folder.id
    folderDragHint.visible = true
    folderDragHint.name = folderPress.folder.name
    document.body.classList.add('folder-dragging')
  }

  // 浮层跟随鼠标，靠近窗口边缘时向内侧翻转
  folderDragHint.x = Math.min(event.clientX + 16, window.innerWidth - 230)
  folderDragHint.y = Math.min(event.clientY + 16, window.innerHeight - 90)

  const pointEl = document.elementFromPoint(event.clientX, event.clientY)
  // 不在侧边栏内 → 无落点
  if (!pointEl?.closest?.('.sidebar')) {
    clearDrop()
    return
  }
  // 悬停在虚线占位块上时保持当前落点，避免占位块自身导致判定抖动
  if (pointEl.closest('.folder-drop-slot')) return

  const draggedId = folderPress.folder.id
  const rowEl = pointEl.closest('[data-folder-drop]')

  // 侧边栏空白区域 / 「全部文件」行 → 顶层末尾
  if (!rowEl || rowEl.getAttribute('data-folder-drop') === '__all__') {
    const siblings = foldersStore.childrenOf('').filter((folder) => folder.id !== draggedId)
    folderDropIndicator.value = {
      type: 'root',
      targetId: '',
      parentId: '',
      index: siblings.length,
      depth: 0
    }
    folderDropInvalid.value = ''
    folderDragHint.label = t('folders.dropToTop')
    folderDragHint.invalid = false
    return
  }

  const key = rowEl.getAttribute('data-folder-drop') || ''
  const target = foldersStore.visibleFolders.find((folder) => folder.id === key)
  if (!target) {
    clearDrop()
    return
  }
  if (key === draggedId) {
    setDropInvalid(key)
    return
  }

  const rect = rowEl.getBoundingClientRect()
  const edge = Math.min(10, rect.height / 4)
  const offset = event.clientY - rect.top
  const zone = offset < edge ? 'before' : offset > rect.height - edge ? 'after' : 'inside'

  if (zone === 'inside') {
    if (foldersStore.isDescendant(key, draggedId)) {
      setDropInvalid(key)
      return
    }
    const children = foldersStore.childrenOf(key).filter((folder) => folder.id !== draggedId)
    folderDropIndicator.value = {
      type: 'inside',
      targetId: key,
      parentId: key,
      index: children.length,
      depth: target.depth || 0
    }
    folderDropInvalid.value = ''
    folderDragHint.label = t('folders.dropInto', { name: target.name })
    folderDragHint.invalid = false
    return
  }

  // 同级插入：目标父级不能是被拖动文件夹自身或其子孙
  const parentId = target.parentId || ''
  if (parentId === draggedId || foldersStore.isDescendant(parentId, draggedId)) {
    setDropInvalid(key)
    return
  }
  const siblings = foldersStore.childrenOf(parentId).filter((folder) => folder.id !== draggedId)
  const targetIndex = siblings.findIndex((folder) => folder.id === key)
  const index = targetIndex < 0 ? siblings.length : targetIndex + (zone === 'after' ? 1 : 0)
  folderDropIndicator.value = {
    type: zone,
    targetId: key,
    parentId,
    index,
    depth: target.depth || 0
  }
  folderDropInvalid.value = ''
  folderDragHint.label = t('folders.dropAsSibling', { name: target.name })
  folderDragHint.invalid = false
}

/** 移动后展开父级，保证能立刻看到结果 */
function revealFolder(parentId) {
  if (!parentId) return
  foldersStore.expandTo(parentId)
  if (!foldersStore.expandedIds.includes(parentId)) {
    foldersStore.toggleExpanded(parentId)
  }
}

async function onFolderMouseUp() {
  const press = folderPress
  const active = folderDragActive
  const indicator = folderDropIndicator.value
  cleanupFolderDrag()
  // 拖拽结束后的 click 不应触发选中
  if (active) setTimeout(() => { suppressFolderClick = false }, 0)
  if (!press || !active || !indicator) return
  try {
    await foldersStore.moveFolder(press.folder.id, indicator.parentId || '', indicator.index)
    revealFolder(indicator.type === 'inside' ? indicator.targetId : indicator.parentId)
    ElMessage.success(t('folders.moved'))
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

function onFolderMouseOut(event) {
  // 指针移出窗口时取消本次拖拽
  if (event.relatedTarget === null) cleanupFolderDrag()
}

function cleanupFolderDrag() {
  document.removeEventListener('mousemove', onFolderMouseMove)
  document.removeEventListener('mouseup', onFolderMouseUp)
  document.removeEventListener('mouseout', onFolderMouseOut)
  folderPress = null
  folderDragActive = false
  folderDragging.value = ''
  folderDropIndicator.value = null
  folderDropInvalid.value = ''
  folderDragHint.visible = false
  folderDragHint.label = ''
  folderDragHint.invalid = false
  document.body.classList.remove('folder-dragging')
}

async function showTutorial() {
  try {
    await ElMessageBox.alert(
      `<div style="text-align: left; padding: 10px;">
        <h4 style="margin: 0 0 12px 0;">${t('menu.tutorialTitle')}</h4>
        <p style="margin: 8px 0;">${t('menu.tutorial1')}</p>
        <p style="margin: 8px 0;">${t('menu.tutorial2')}</p>
        <p style="margin: 8px 0;">${t('menu.tutorial3')}</p>
        <p style="margin: 8px 0;">${t('menu.tutorial4')}</p>
        <p style="margin: 8px 0;">${t('menu.tutorial5')}</p>
      </div>`,
      t('menu.tutorial'),
      {
        dangerouslyUseHTMLString: true,
        confirmButtonText: t('common.close'),
        customClass: 'about-dialog'
      }
    )
  } catch {
    // 用户取消
  }
}

async function showAbout() {
  try {
    await ElMessageBox.alert(
      `<div style="text-align: center; padding: 10px;">
        <div style="font-size: 24px; font-weight: 700; margin-bottom: 8px;">tsrbox</div>
        <div style="font-size: 14px; color: var(--text-2); margin-bottom: 16px;">${t('menu.version')} ${pkg.version}</div>
        <div style="font-size: 13px; color: var(--text-3); line-height: 1.6;">
          <p>${t('menu.aboutDesc')}</p>
          <p style="margin-top: 12px;">${t('menu.license')}</p>
        </div>
        <div style="margin-top: 20px; display: flex; justify-content: center; gap: 24px;">
          <a href="https://qm.qq.com/q/WqXZDSTHoW" target="_blank" style="color: var(--text-2); text-decoration: none; font-size: 24px;" title="QQ群">
            <span style="font-family: 'iconfont';">&#xe882;</span>
          </a>
          <a href="https://space.bilibili.com/364755642?spm_id_from=333.1007.0.0" target="_blank" style="color: var(--text-2); text-decoration: none; font-size: 24px;" title="B站">
            <span style="font-family: 'iconfont';">&#xea95;</span>
          </a>
          <a href="${GITHUB_REPO}" target="_blank" style="color: var(--text-2); text-decoration: none; font-size: 24px;" title="GitHub">
            <span style="font-family: 'iconfont';">&#xe691;</span>
          </a>
          <a href="https://ifdian.net/a/ipixel" target="_blank" style="color: var(--text-2); text-decoration: none; font-size: 24px;" title="爱发电">
            <span style="font-family: 'iconfont';">&#xe601;</span>
          </a>
          <a href="mailto:420792287@qq.com" target="_blank" style="color: var(--text-2); text-decoration: none; font-size: 24px;" title="邮件反馈">
            <span style="font-family: 'iconfont';">&#xe62e;</span>
          </a>
        </div>
      </div>`,
      t('menu.about'),
      {
        dangerouslyUseHTMLString: true,
        confirmButtonText: t('common.close'),
        customClass: 'about-dialog'
      }
    )
  } catch {
    // 用户取消
  }
}

function showShortcuts() {
  window.location.hash = '#/shortcuts'
}

function handleMoreCommand(command) {
  bottomMenuVisible.value = false
  switch (command) {
    case 'tutorial':
      showTutorial()
      break
    case 'feedback':
      openGitHubUrl('/issues')
      break
    case 'contact':
      window.api.openExternal('https://qm.qq.com/q/jCqfx7t58k')
      break
    case 'settings':
      window.location.hash = '#/settings'
      break
    case 'shortcuts':
      showShortcuts()
      break
    case 'about':
      showAbout()
      break
    case 'logs':
      window.location.hash = '#/logs'
      break
    case 'switch-library':
      window.location.hash = '#/select-library'
      break
    case 'new-library':
      window.location.hash = '#/select-library'
      break
  }
}



async function handleUpdateCheck() {
  // 自动检查更新：断网或仓库无 Release 时静默忽略
  const updateResult = await settingsStore.checkUpdate()
  if (updateResult?.hasUpdate) {
    try {
      await ElMessageBox.confirm(
        t('menu.newVersion', { version: updateResult.latestVersion }),
        t('menu.updateAvailable'),
        {
          confirmButtonText: t('menu.download'),
          cancelButtonText: t('common.cancel'),
          type: 'success',
          checkbox: true,
          checkboxLabel: t('menu.dontRemind'),
          checkboxValue: false
        }
      ).then(async ({ checkboxChecked }) => {
        if (checkboxChecked) {
          settingsStore.setCheckForUpdates(false)
        }
        if (updateResult.releaseUrl) {
          window.api.openExternal(updateResult.releaseUrl)
        }
      })
    } catch {
      // 用户取消
    }
  }
}

// ===== 外部链接统一交给系统浏览器 =====
function handleExternalLink(e) {
  const anchor = e.target?.closest?.('a[href^="http"], a[href^="mailto:"]')
  if (!anchor) return
  e.preventDefault()
  window.api.openExternal(anchor.href)
}

// ===== 左下角弹层（插件可通过 pluginSlots 注入按钮内容 / 面板 / 菜单项）=====
const bottomMenuVisible = ref(false)

function closeBottomMenu() {
  bottomMenuVisible.value = false
}

onMounted(async () => {
  settingsStore.init()
  initLanguage()
  checkStatus()
  document.addEventListener('click', handleExternalLink)

  // 初始化最大化状态
  try {
    isMax.value = await window.api.windowControl.isMaximized()
  } catch {
    // ignore
  }

  // 监听最大化状态变化
  removeMaxListener = window.api.windowControl.onMaximizeChange((max) => {
    isMax.value = max
  })

  // 仅在用户开启“启动时检查更新”时才联网检查，避免默认产生网络请求
  if (settingsStore.checkForUpdates) {
    setTimeout(handleUpdateCheck, 3000)
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('click', handleExternalLink)
  if (removeMaxListener) removeMaxListener()
})
</script>

<template>

  <ElConfigProvider :locale="elLocale">
    <div class="app-shell">
    <!-- 自定义标题栏 -->
    <header class="titlebar">
      <div class="titlebar-left" data-tauri-drag-region>
        <div class="logo">
          <img src="./assets/tsr-logo.svg" alt="tsrbox" />
        </div>
        <div class="title" data-tauri-drag-region>{{ t('common.appTitle') }}</div>
      </div>

      <div class="titlebar-drag" data-tauri-drag-region />

      <div class="titlebar-right">
        <div
          v-if="settingsStore.showThemeInTitlebar"
          class="title-btn theme-btn"
          @click="settingsStore.toggleTheme"
          :title="isDark ? t('common.theme.toggleLight') : t('common.theme.toggleDark')"
        >
          <i :class="['iconfont', isDark ? 'icon-sun' : 'icon-moon']"></i>
        </div>
        <el-dropdown v-if="settingsStore.showLanguageInTitlebar" @command="changeLanguage">
          <div class="title-btn lang-btn">
            <span style="font-family: 'iconfont'; font-size: 14px;">&#xeaae;</span>
          </div>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item v-for="lang in languages" :key="lang.code" :command="lang.code">
                <span style="font-family: 'iconfont'; margin-right: 6px;">&#xeaae;</span>
                {{ lang.label }}
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <div class="title-btn" @click="minimize" title="最小化">
          <i class="iconfont icon-minus"></i>
        </div>
        <div class="title-btn" @click="toggleMaximize" :title="isMax ? '还原' : '最大化'">
          <i class="iconfont icon-checkbox-on"></i>
        </div>
        <div class="title-btn close" @click="closeApp" title="关闭">
          <i class="iconfont icon-close"></i>
        </div>
      </div>
    </header>

    <!-- 内容区 -->
    <main class="app-content">
      <!-- 左侧边栏：仅在资源库已设置时显示 -->
      <transition name="sidebar">
        <nav class="sidebar" v-if="state.initialized" ref="sidebarRef" data-folder-area>
          <div class="sidebar-head">
            <span class="sidebar-title">{{ t('folders.title') }}</span>
            <div class="sidebar-head-actions">
              <button
                class="sidebar-add"
                :title="t('recycle.title')"
                @click="router.push('/recycle')"
              >
                <i class="iconfont icon-trash-alt"></i>
              </button>
              <button class="sidebar-add" :title="t('folders.create')" @click="handleCreateFolder()">
                <i class="iconfont icon-folder-plus"></i>
              </button>
            </div>
          </div>
          <div class="folder-list" :style="folderPaneStyle">
            <div
              class="folder-item"
              :class="{
                active: !foldersStore.activeFolderId,
                'drop-over': foldersStore.dragOverKey === '__all__',
                'drop-target': folderDropIndicator?.type === 'root'
              }"
              data-folder-drop="__all__"
              :title="t('folders.allFiles')"
              @click="selectFolder('')"
            >
              <i class="iconfont icon-folder folder-icon"></i>
              <span class="folder-name">{{ t('folders.allFiles') }}</span>
              <span class="folder-count">{{ state.models.length }}</span>
            </div>
            <template v-for="folder in foldersStore.visibleFolders" :key="folder.id">
              <div
                v-if="isDropSlot('before', folder.id)"
                class="folder-drop-slot"
                :style="{ marginLeft: 8 + folder.depth * 14 + 'px' }"
              >
                <i class="iconfont icon-folder"></i>
                <span>{{ t('folders.dropHere') }}</span>
              </div>
              <div
                class="folder-item"
                :class="{
                  active: foldersStore.activeFolderId === folder.id,
                  'drop-over': foldersStore.dragOverKey === folder.id,
                  'drop-inside': isDropInside(folder.id),
                  'drop-invalid': folderDropInvalid === folder.id,
                  dragging: folderDragging === folder.id
                }"
                :data-folder-drop="folder.id"
                :title="folder.name"
                :style="{ paddingLeft: 8 + folder.depth * 14 + 'px' }"
                @click="selectFolder(folder.id)"
                @contextmenu="openFolderMenu($event, folder)"
                @mousedown="onFolderMouseDown($event, folder)"
              >
                <button
                  v-if="folder.children.length"
                  class="folder-caret"
                  @click.stop="foldersStore.toggleExpanded(folder.id)"
                >
                  <i
                    class="iconfont icon-chevron-right"
                    :class="{ open: foldersStore.expandedIds.includes(folder.id) }"
                  ></i>
                </button>
                <span v-else class="folder-caret placeholder"></span>
                <i
                  class="iconfont icon-folder folder-icon-colored"
                  :style="{ color: folder.color || FOLDER_COLORS[0] }"
                ></i>
                <span class="folder-name">{{ folder.name }}</span>
                <span class="folder-count">{{ folderCount(folder.id) }}</span>
                <button class="folder-more" @click.stop="openFolderMenu($event, folder)">
                  <i class="iconfont icon-more-vertical"></i>
                </button>
              </div>
              <div
                v-if="isDropSlot('after', folder.id)"
                class="folder-drop-slot"
                :style="{ marginLeft: 8 + folder.depth * 14 + 'px' }"
              >
                <i class="iconfont icon-folder"></i>
                <span>{{ t('folders.dropHere') }}</span>
              </div>
            </template>
            <div
              v-if="folderDropIndicator?.type === 'root'"
              class="folder-drop-slot root-slot"
              :style="{ marginLeft: '8px' }"
            >
              <i class="iconfont icon-folder"></i>
              <span>{{ t('folders.dropHere') }}</span>
            </div>
            <div v-if="!foldersStore.folders.length" class="folder-empty">
              {{ t('folders.empty') }}
            </div>
          </div>

          <!-- 拖动分隔条调整文件夹 / 标签各自高度 -->
          <div
            class="sidebar-resizer"
            :title="t('folders.resizeHint')"
            @mousedown="startSidebarResize"
            @dblclick="resetSidebarResize"
          >
            <span class="sidebar-resizer-grip"></span>
          </div>

          <!-- 标签筛选：位于文件夹下方，两列布局（每个占一半） -->
          <div class="sidebar-tags" :class="{ resized: folderPaneHeight > 0 }">
            <div class="sidebar-tags-head">
              <span class="sidebar-title">{{ t('common.tags') }}</span>
              <span class="sidebar-tags-count">{{ tagFilter.allTagsInUse.length }}</span>
            </div>
            <div class="sidebar-tags-body">
              <TagFilterBar
                v-model="tagFilter.selectedTags"
                :tags="tagFilter.allTagsInUse"
                variant="sidebar"
                @update:modelValue="onSidebarTagsChanged"
              />
            </div>
          </div>
          <div class="sidebar-bottom">
            <el-popover
              v-model:visible="bottomMenuVisible"
              placement="top-start"
              trigger="click"
              :width="240"
            >
              <template #reference>
                <button
                  class="sidebar-item sidebar-more-btn"
                  :class="{ loading: pluginSlots.bottomButton?.loading }"
                  :title="pluginSlots.bottomButton?.text || t('menu.more')"
                >
                  <template v-if="pluginSlots.bottomButton?.type === 'avatar'">
                    <img
                      v-if="pluginSlots.bottomButton.avatarUrl"
                      class="sidebar-avatar"
                      :src="pluginSlots.bottomButton.avatarUrl"
                      alt=""
                    />
                    <span v-else class="sidebar-avatar fallback">{{ pluginSlots.bottomButton.initial }}</span>
                    <span class="sidebar-user-name">{{ pluginSlots.bottomButton.text }}</span>
                  </template>
                  <template v-else>
                    <span
                      class="iconfont sidebar-icon"
                      :class="pluginSlots.bottomButton?.icon || 'icon-shenglvehao'"
                    ></span>
                    <span class="sidebar-user-name">
                      {{ pluginSlots.bottomButton?.text || t('menu.more') }}
                    </span>
                  </template>
                </button>
              </template>
              <div class="bottom-pop">
                <!-- 插件面板（如登录插件的用户卡片） -->
                <component
                  v-for="(panel, index) in pluginSlots.bottomPanels"
                  :key="index"
                  :is="panel"
                  :close-popover="closeBottomMenu"
                />
                <div class="user-pop-actions">
                  <button class="user-pop-item" @click="handleMoreCommand('tutorial')">
                    <i class="iconfont icon-clipboard"></i>
                    <span>{{ t('menu.tutorial') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('feedback')">
                    <i class="iconfont icon-github"></i>
                    <span>{{ t('menu.feedback') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('contact')">
                    <i class="iconfont icon-QQ"></i>
                    <span>{{ t('menu.contactUs') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('settings')">
                    <i class="iconfont icon-shezhi"></i>
                    <span>{{ t('menu.settings') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('shortcuts')">
                    <i class="iconfont icon-jianpankuaijiejian"></i>
                    <span>{{ t('menu.shortcuts') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('logs')">
                    <i class="iconfont icon-lvzhou_caozuorizhi"></i>
                    <span>{{ t('menu.operationLogs') }}</span>
                  </button>
                  <button class="user-pop-item" @click="handleMoreCommand('about')">
                    <i class="iconfont icon-guanyu"></i>
                    <span>{{ t('menu.about') }}</span>
                  </button>
                  <!-- 插件追加的菜单项（如退出登录） -->
                  <template v-for="item in pluginSlots.bottomMenuItems" :key="item.id">
                    <button
                      v-if="!item.hidden"
                      class="user-pop-item"
                      :class="{ danger: item.danger }"
                      @click="item.onClick && item.onClick(closeBottomMenu)"
                    >
                      <i class="iconfont" :class="item.icon"></i>
                      <span>{{ item.text }}</span>
                    </button>
                  </template>
                </div>
              </div>
            </el-popover>
          </div>
        </nav>
      </transition>
      <!-- 路由内容 -->
      <div class="main-content">
        <router-view />
      </div>
    </main>

    <!-- 文件夹右键菜单 -->
    <div
      v-if="folderMenu"
      class="folder-ctx-overlay"
      @click="folderMenu = null"
      @contextmenu.prevent="folderMenu = null"
    ></div>
    <div
      v-if="folderMenu"
      class="folder-ctx-menu"
      :style="{ left: folderMenu.x + 'px', top: folderMenu.y + 'px' }"
    >
      <button class="folder-ctx-item" @click="handleFolderCommand('newChild', folderMenu.folder)">
        <i class="iconfont icon-folder-plus"></i> {{ t('folders.newSubfolder') }}
      </button>
      <div class="folder-ctx-divider"></div>
      <button class="folder-ctx-item" @click="handleFolderCommand('rename', folderMenu.folder)">
        <i class="iconfont icon-script"></i> {{ t('folders.rename') }}
      </button>
      <button class="folder-ctx-item" @click="handleFolderCommand('color', folderMenu.folder)">
        <i class="iconfont icon-image"></i> {{ t('folders.color') }}
      </button>
      <div class="folder-ctx-divider"></div>
      <button class="folder-ctx-item danger" @click="handleFolderCommand('delete', folderMenu.folder)">
        <i class="iconfont icon-trash-alt"></i> {{ t('folders.delete') }}
      </button>
    </div>

    <!-- 文件夹颜色 -->
    <el-dialog v-model="colorDialogVisible" :title="t('folders.color')" width="360px" append-to-body>
      <div class="folder-color-body">
        <div class="folder-color-swatches">
          <span
            v-for="color in FOLDER_COLORS"
            :key="color"
            class="folder-color-swatch"
            :class="{ active: folderColor === color }"
            :style="{ background: color }"
            @click="folderColor = color"
          ></span>
        </div>
        <div class="folder-color-custom">
          <span>{{ t('folders.customColor') }}</span>
          <el-color-picker v-model="folderColor" />
        </div>
      </div>
      <template #footer>
        <el-button @click="colorDialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="saveFolderColor">{{ t('common.ok') }}</el-button>
      </template>
    </el-dialog>

    <!-- 导出格式选择 -->
    <el-dialog
      v-model="exportFormatDialog.visible"
      :title="t('exportDialog.title')"
      width="420px"
      append-to-body
    >
      <div class="export-format-list">
        <div
          v-for="option in exportFormatOptions"
          :key="option.value"
          class="export-format-item"
          :class="{ active: exportFormatDialog.format === option.value }"
          @click="exportFormatDialog.format = option.value"
        >
          <span class="export-format-dot"></span>
          <div class="export-format-text">
            <div class="export-format-name">{{ option.label }}</div>
            <div class="export-format-desc">{{ option.desc }}</div>
          </div>
        </div>
      </div>
      <template #footer>
        <div class="export-format-footer">
          <el-checkbox v-model="exportFormatDialog.remember">
            {{ t('exportDialog.remember') }}
          </el-checkbox>
          <div class="export-format-actions">
            <el-button @click="closeExportFormatDialog(null)">{{ t('common.cancel') }}</el-button>
            <el-button type="primary" @click="confirmExportFormat">
              {{ t('common.confirm') }}
            </el-button>
          </div>
        </div>
      </template>
    </el-dialog>

    <!-- 文件夹拖动浮层提示 -->
    <div
      v-if="folderDragHint.visible"
      class="folder-drag-hint"
      :class="{ invalid: folderDragHint.invalid }"
      :style="{ left: folderDragHint.x + 'px', top: folderDragHint.y + 'px' }"
    >
      <div class="folder-drag-hint-name">
        <i class="iconfont icon-folder"></i>
        <span>{{ folderDragHint.name }}</span>
      </div>
      <div class="folder-drag-hint-label">{{ folderDragHint.label }}</div>
    </div>

    <UploadProgress />
  </div>
  </ElConfigProvider>
</template>

<style scoped>
.app-shell {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}

.titlebar {
  height: 36px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  background: var(--bg-soft);
  border-bottom: 1px solid var(--border-soft);
  user-select: none;
  padding-left: 12px;
}

.titlebar-left {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.logo {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.logo img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
  image-rendering: pixelated;
}
/* 透明图标在亮色主题下反转为深色，保证可见 */
html:not(.dark) .logo img {
  filter: invert(1);
}
.win-btn .iconfont {
  font-size: 14px;
}

.title {
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 1.2px;
  color: var(--text-1);
}

.title-dropdown {
  display: flex;
  align-items: center;
  cursor: pointer;
  padding: 0 8px;
  border-radius: 4px;
  transition: background 0.15s;
}

.title-dropdown:hover {
  background: var(--bg-hover);
}

/* 中间可拖拽区域（让窗口能拖动） */
.titlebar-drag {
  flex: 1;
  height: 100%;
  -webkit-app-region: drag;
}

.titlebar-right {
  display: flex;
  align-items: center;
  height: 100%;
  -webkit-app-region: no-drag;
  gap: 0;
}

.title-btn {
  width: 46px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--text-2);
  border: none;
  outline: none;
  transition: all 0.15s;
}

/* 下拉触发器（语言 / 更多）不显示焦点描边，避免悬停时出现黑色边框 */
.titlebar-right :deep(.el-tooltip__trigger),
.titlebar-right :deep(.el-tooltip__trigger:focus),
.titlebar-right :deep(.el-tooltip__trigger:focus-visible),
.titlebar-right :deep(.el-dropdown) {
  outline: none !important;
  box-shadow: none;
}

.title-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.title-btn.close:hover {
  background: #e81123;
  color: #fff;
}

.title-btn .iconfont {
  font-size: 14px;
}

.app-content {
  flex: 1;
  overflow: hidden;
  display: flex;
}

.sidebar {
  width: 196px;
  flex-shrink: 0;
  background: var(--bg-soft);
  border-right: 1px solid var(--border-soft);
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  user-select: none;
}
.sidebar-head {
  height: 38px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--border-soft);
}
.sidebar-head-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
.sidebar-title {
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.5px;
  color: var(--text-2);
}
.sidebar-add {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-3);
  border-radius: 4px;
  cursor: pointer;
  outline: none;
  transition: all 0.15s;
}
.sidebar-add:hover {
  background: var(--bg-hover);
  color: var(--primary);
}
.sidebar-add .iconfont {
  font-size: 15px;
}
.folder-list {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.folder-item {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 8px;
  border-radius: 6px;
  border: 1px solid transparent;
  color: var(--text-2);
  cursor: pointer;
  user-select: none;
  transition: background 0.15s, color 0.15s;
  position: relative;
}
.folder-item:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}
.folder-item.active {
  background: var(--primary-soft);
  border-color: var(--primary);
  color: var(--primary);
}
.folder-item.drop-over {
  background: var(--primary-soft);
  border-color: var(--primary);
  box-shadow: inset 0 0 0 1px var(--primary);
}
.folder-item.drop-target {
  background: var(--primary-soft);
  border-color: var(--primary);
  box-shadow: inset 0 0 0 1px var(--primary), 0 0 0 2px var(--primary-soft);
  color: var(--primary);
  transform: scale(1.015);
}
.folder-item.drop-invalid {
  background: rgba(239, 68, 68, 0.08);
  border-color: var(--danger);
  box-shadow: inset 0 0 0 1px var(--danger);
  color: var(--danger);
}
.folder-item.dragging {
  opacity: 0.55;
}
.folder-item.drop-inside {
  border: 1px dashed var(--primary);
  background: var(--primary-soft);
  color: var(--primary);
}
/* 拖拽落位虚线占位：提示会移动到哪个位置 */
.folder-drop-slot {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 8px;
  margin: 2px 0;
  border: 1px dashed var(--primary);
  border-radius: 6px;
  background: var(--primary-soft);
  color: var(--primary);
  font-size: 12px;
  flex-shrink: 0;
}
.folder-drop-slot .iconfont {
  font-size: 14px;
}
.folder-drop-slot.root-slot {
  margin-top: 4px;
}
.folder-drag-hint {
  position: fixed;
  z-index: 4000;
  min-width: 150px;
  max-width: 214px;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--primary);
  background: var(--bg-card);
  box-shadow: 0 10px 24px rgba(0, 0, 0, 0.28);
  pointer-events: none;
  font-size: 12px;
  color: var(--text-1);
}
.folder-drag-hint.invalid {
  border-color: var(--danger);
}
.folder-drag-hint-name {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.folder-drag-hint-name .iconfont {
  color: var(--primary);
  flex-shrink: 0;
}
.folder-drag-hint-label {
  margin-top: 5px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-3);
  white-space: normal;
  word-break: break-all;
}
.folder-drag-hint:not(.invalid) .folder-drag-hint-label {
  color: var(--primary);
}
.folder-drag-hint.invalid .folder-drag-hint-label {
  color: var(--danger);
}
body.folder-dragging,
body.folder-dragging * {
  cursor: grabbing !important;
  user-select: none !important;
}
.folder-caret {
  width: 14px;
  height: 20px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.folder-caret.placeholder {
  visibility: hidden;
}
.folder-caret .iconfont {
  font-size: 11px;
  transition: transform 0.15s;
}
.folder-caret .iconfont.open {
  transform: rotate(90deg);
}
.folder-icon {
  font-size: 15px;
  flex-shrink: 0;
}
.folder-icon-colored {
  font-size: 15px;
  flex-shrink: 0;
}
.folder-name {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.folder-count {
  font-size: 10px;
  color: var(--text-3);
  flex-shrink: 0;
}
.folder-more {
  display: none;
  width: 18px;
  height: 18px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-3);
  border-radius: 3px;
  cursor: pointer;
  flex-shrink: 0;
}
.folder-more .iconfont {
  font-size: 13px;
}
.folder-item:hover .folder-more {
  display: flex;
  align-items: center;
  justify-content: center;
}
.folder-item:hover .folder-count {
  display: none;
}
.folder-more:hover {
  color: var(--primary);
}
.folder-empty {
  font-size: 11px;
  color: var(--text-3);
  padding: 8px;
  line-height: 1.6;
}
.sidebar-bottom {
  display: flex;
  flex-direction: row;
  padding-bottom: 4px;
  border-top: 1px solid var(--border-soft);
}
/* 左下角入口：点击弹出更多选项，插件可注入按钮内容与面板 */
.sidebar-more-btn {
  justify-content: flex-start;
  gap: 8px;
  padding: 0 10px;
}
.sidebar-more-btn.loading {
  color: var(--primary);
}
.sidebar-more-btn.loading .sidebar-icon {
  animation: icon-spin 1.2s linear infinite;
}
@keyframes icon-spin {
  to {
    transform: rotate(360deg);
  }
}
.sidebar-user-name {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100px;
}
.sidebar-avatar {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  object-fit: cover;
  flex-shrink: 0;
  image-rendering: pixelated;
}
.sidebar-avatar.fallback {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--primary);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
}
.sidebar-bottom :deep(.el-tooltip__trigger) {
  flex: 1;
  min-width: 0;
  display: flex;
}
.user-pop-actions {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px dashed var(--border-soft);
}
.user-pop-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 8px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-1);
  font-size: 12px;
  cursor: pointer;
  text-align: left;
  transition: all 0.15s;
}
.user-pop-item:hover {
  background: var(--bg-hover);
  color: var(--primary);
}
.user-pop-item.danger {
  color: #f43f5e;
}

/* 标签筛选（文件夹下方） */
.sidebar-resizer {
  height: 8px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: row-resize;
  border-top: 1px solid var(--border-soft);
  background: transparent;
}
.sidebar-resizer:hover {
  background: var(--primary-soft);
}
.sidebar-resizer-grip {
  width: 26px;
  height: 2px;
  border-radius: 1px;
  background: var(--border);
}
.sidebar-resizer:hover .sidebar-resizer-grip {
  background: var(--primary);
}
body.sidebar-resizing,
body.sidebar-resizing * {
  cursor: row-resize !important;
  user-select: none !important;
}
.sidebar-tags {
  flex: 0 0 auto;
  max-height: 44%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 8px 8px 6px;
}
.sidebar-tags.resized {
  flex: 1;
  max-height: none;
}
.sidebar-tags-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px 6px;
  flex-shrink: 0;
}
.sidebar-tags-count {
  font-size: 11px;
  color: var(--text-3);
}
.sidebar-tags-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}
.sidebar-item {
  flex: 1;
  width: auto;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--text-3);
  text-decoration: none;
  border: none;
  background: transparent;
  transition: all 0.15s;
  user-select: none;
  -webkit-user-drag: none;
}
.sidebar-item:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}
.sidebar-icon {
  font-size: 17px;
}

/* 文件夹右键菜单 */
.folder-ctx-overlay {
  position: fixed;
  inset: 0;
  z-index: 3000;
}
.folder-ctx-menu {
  position: fixed;
  z-index: 3001;
  min-width: 140px;
  padding: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.18);
}
.folder-ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-1);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.folder-ctx-item:hover {
  background: var(--primary-soft);
  color: var(--primary);
}
.folder-ctx-item.danger {
  color: var(--danger);
}
.folder-ctx-item.danger:hover {
  background: rgba(245, 108, 108, 0.12);
  color: var(--danger);
}
.folder-ctx-divider {
  height: 1px;
  margin: 4px 6px;
  background: var(--border-soft);
}
.folder-color-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.folder-color-swatches {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}
.folder-color-swatch {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  cursor: pointer;
  border: 2px solid transparent;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.08);
}
.folder-color-swatch.active {
  border-color: var(--primary);
  transform: scale(1.08);
}
.folder-color-custom {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  color: var(--text-2);
}

/* ===== 导出格式弹窗 ===== */
.export-format-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.export-format-item {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}
.export-format-item:hover {
  background: var(--surface-hover, rgba(128, 128, 128, 0.08));
}
.export-format-item.active {
  border-color: var(--primary);
  background: var(--primary-soft, rgba(64, 158, 255, 0.08));
}
.export-format-dot {
  width: 12px;
  height: 12px;
  margin-top: 3px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 2px solid var(--border-strong, #b0b3ba);
}
.export-format-item.active .export-format-dot {
  border-color: var(--primary);
  background: var(--primary);
  box-shadow: inset 0 0 0 2px var(--bg-panel, #fff);
}
.export-format-text {
  min-width: 0;
}
.export-format-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.export-format-desc {
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.4;
  color: var(--text-3, var(--text-2));
}
.export-format-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.export-format-actions {
  display: flex;
  gap: 8px;
}

.main-content {
  flex: 1;
  height: 100%;
  overflow: hidden;
  display: flex;
}

.sidebar-enter-active,
.sidebar-leave-active {
  transition: all 0.25s ease;
}

.sidebar-enter-from,
.sidebar-leave-to {
  width: 0;
  opacity: 0;
  overflow: hidden;
}
</style>
