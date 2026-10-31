<script setup>
import { computed, ref, shallowRef, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSettingsStore } from '../stores/settings'
import { useStore } from '../composables/useStore'
import { ElMessage, ElMessageBox } from 'element-plus'

const { t, locale } = useI18n()
const settingsStore = useSettingsStore()
const { state, loadAll, saveSettings } = useStore()

const ModelViewer = defineAsyncComponent(() => import('../components/ModelViewer.vue'))

import pkg from '@root/package.json'
const version = computed(() => pkg.version)
const latestVersion = ref('')
const hasUpdate = ref(false)
const releaseUrl = ref('')
const checkingUpdate = ref(false)

async function checkUpdate() {
  // 复用全局更新检查：断网 / 无 Release 时静默处理，不输出错误日志
  const result = await settingsStore.checkUpdate()
  hasUpdate.value = !!result?.hasUpdate
  latestVersion.value = result?.latestVersion || ''
  releaseUrl.value = result?.releaseUrl || ''
}

/** 手动检查更新（不受“启动时检查”开关限制） */
async function runUpdateCheck() {
  if (checkingUpdate.value) return
  checkingUpdate.value = true
  try {
    const result = await settingsStore.checkUpdate({ manual: true, force: true })
    if (result?.hasUpdate) {
      hasUpdate.value = true
      latestVersion.value = result.latestVersion || ''
      releaseUrl.value = result.releaseUrl || ''
      try {
        await ElMessageBox.confirm(
          t('menu.newVersion', { version: result.latestVersion }),
          t('menu.updateAvailable'),
          {
            confirmButtonText: t('menu.download'),
            cancelButtonText: t('common.cancel'),
            type: 'success'
          }
        )
        if (result.releaseUrl) window.api.openExternal(result.releaseUrl)
      } catch {
        // 用户取消
      }
    } else if (result?.status === 'latest') {
      hasUpdate.value = false
      latestVersion.value = result.latestVersion || ''
      ElMessage.success(t('menu.latestVersion'))
    } else {
      hasUpdate.value = false
      latestVersion.value = ''
      ElMessage.warning(t('menu.checkUpdateFailed'))
    }
  } finally {
    checkingUpdate.value = false
  }
}

function openReleaseUrl() {
  if (releaseUrl.value) {
    window.api.openExternal(releaseUrl.value)
  }
}

checkUpdate()

const themes = [
  { value: 'dark', label: t('settings.dark') },
  { value: 'light', label: t('settings.light') }
]

const skins = [
  { value: '#F56C6C', label: '红', name: 'red' },
  { value: '#E6A23C', label: '橙', name: 'orange' },
  { value: '#E6E600', label: '黄', name: 'yellow' },
  { value: '#67C23A', label: '绿', name: 'green' },
  { value: '#409EFF', label: '蓝', name: 'blue' },
  { value: '#9B59B6', label: '紫', name: 'purple' },
  { value: '#909399', label: '灰', name: 'gray' }
]

const languages = [
  { code: 'zh-CN', label: '中文' },
  { code: 'en-US', label: 'English' },
  { code: 'es-ES', label: 'Español' },
  { code: 'fr-FR', label: 'Français' },
  { code: 'de-DE', label: 'Deutsch' },
  { code: 'ja-JP', label: '日本語' },
  { code: 'ru-RU', label: 'Русский' }
]

// ===== 模型格式（资源库设置）=====
const FORMAT_OPTIONS = ['original', 'bbmodel', 'bedrock', 'java', 'generic']
const formatOptions = computed(() =>
  FORMAT_OPTIONS.map((value) => ({ value, label: t(`exportDialog.formats.${value}`) }))
)

function saveLibrarySetting(patch) {
  saveSettings(patch).catch((e) => console.warn('[Settings] 保存资源库设置失败:', e))
}

const dragFormat = computed({
  get: () => state.library?.settings?.dragFormat || 'original',
  set: (value) => saveLibrarySetting({ dragFormat: value })
})

const exportFormat = computed({
  get: () => state.library?.settings?.exportFormat || 'original',
  set: (value) => saveLibrarySetting({ exportFormat: value })
})

const currentTheme = computed({
  get() { return settingsStore.isDark ? 'dark' : 'light' },
  set(val) {
    settingsStore.isDark = val === 'dark'
    localStorage.setItem('imodel-theme', val)
    settingsStore.applyTheme()
  }
})

function changeLanguage(lang) {
  locale.value = lang
  localStorage.setItem('imodel-language', lang)
}

// ===== 一键重新生成全部预览图 =====
const regenerating = ref(false)
const regenCurrent = ref(0)
const regenTotal = ref(0)

const viewerVisible = ref(false)
const viewerKey = ref(0)
const viewerBuffer = shallowRef(null)
const viewerFileType = ref('')
const viewerAux = ref([])
const viewerViewParams = ref(null)
const viewerRef = ref(null)

let pendingResolve = null
let pendingReject = null
let renderTimeout = null

function base64ToBytes(base64) {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

function finishRender(cover, error) {
  if (renderTimeout) {
    clearTimeout(renderTimeout)
    renderTimeout = null
  }
  const resolve = pendingResolve
  const reject = pendingReject
  pendingResolve = null
  pendingReject = null
  if (error) reject?.(error)
  else resolve?.(cover)
}

function onViewerLoaded() {
  if (!pendingResolve) return
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      let cover = ''
      try {
        cover = viewerRef.value?.captureThumbnail() || ''
      } catch (e) {
        finishRender('', e)
        return
      }
      finishRender(cover)
    })
  })
}

function onViewerError(error) {
  finishRender('', error instanceof Error ? error : new Error(String(error)))
}

function renderCoverForModel(model) {
  return new Promise(async (resolve, reject) => {
    try {
      const res = await window.api.models.loadFile({
        id: String(model.id),
        fileName: String(model.fileName),
        fileType: String(model.fileType),
        files: JSON.parse(JSON.stringify(model.files || []))
      })
      pendingResolve = resolve
      pendingReject = reject
      viewerKey.value++
      viewerBuffer.value = base64ToBytes(res.dataBase64)
      viewerFileType.value = res.fileType
      viewerAux.value = res.auxFiles || []
      // 重新生成时统一自动取景，避免沿用旧封面/旧包围盒造成的错误视角
      viewerViewParams.value = null
      viewerVisible.value = true
      renderTimeout = setTimeout(() => {
        finishRender('', new Error('模型渲染超时'))
      }, 30000)
    } catch (e) {
      reject(e)
    }
  })
}

async function regenerateAllCovers() {
  if (regenerating.value) return
  try {
    await ElMessageBox.confirm(t('settings.regenerateDesc'), t('settings.regenerateAll'), {
      confirmButtonText: t('settings.regenerateAll'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    })
  } catch {
    return
  }

  regenerating.value = true
  regenCurrent.value = 0
  try {
    const models = await window.api.models.list()
    regenTotal.value = models.length
    let success = 0
    let failed = 0
    for (const model of models) {
      regenCurrent.value++
      try {
        const cover = await renderCoverForModel(model)
        if (!cover) throw new Error('封面为空')
        await window.api.models.saveImage(String(model.id), 'cover', cover)
        success++
      } catch (e) {
        failed++
        console.warn('[Settings] 预览图生成失败:', model.fileName, e)
      }
    }
    await loadAll()
    if (failed > 0) ElMessage.warning(t('settings.regenerateFailed', { count: failed }))
    else ElMessage.success(t('settings.regenerateDone', { count: success }))
  } catch (e) {
    ElMessage.error(e.message || t('settings.regenerateFailed', { count: 0 }))
  } finally {
    regenerating.value = false
    viewerVisible.value = false
    viewerBuffer.value = null
    viewerAux.value = []
    viewerViewParams.value = null
  }
}
</script>

<template>
  <div class="settings-container h-100">
    <div class="settings-header">
      <h2>{{ t('menu.settings') }}</h2>
    </div>
    <div class="settings-content">
      <el-card>
        <template #header>
          <span>{{ t('settings.general') }}</span>
        </template>
        <div class="settings-section">
          <div class="setting-item">
            <span class="setting-label">{{ t('settings.version') }}</span>
            <div class="version-info">
              <span class="setting-value">v{{ version }}</span>
              <span v-if="hasUpdate" class="update-badge update-badge-warning" @click="openReleaseUrl">{{ t('menu.newVersionAvailable', { version: latestVersion }) }}</span>
              <span v-else-if="latestVersion" class="update-badge update-badge-success">{{ t('menu.latestVersion') }}</span>
              <el-button size="small" :loading="checkingUpdate" @click="runUpdateCheck">
                {{ t('menu.checkUpdate') }}
              </el-button>
            </div>
          </div>
          <div class="setting-item">
            <span class="setting-label setting-label-wide">{{ t('settings.autoCheckUpdate') }}</span>
            <el-switch v-model="settingsStore.checkForUpdates" @change="(val) => settingsStore.setCheckForUpdates(val)" />
          </div>
        </div>
      </el-card>

      <el-card>
        <template #header>
          <div class="card-header">
            <span>{{ t('settings.appearance') }}</span>
            <el-checkbox
              v-model="settingsStore.showThemeInTitlebar"
              @change="(val) => settingsStore.setShowThemeInTitlebar(val)"
              size="small"
            >
              {{ t('settings.showInTitlebar') }}
            </el-checkbox>
          </div>
        </template>
        <div class="settings-section">
          <div class="setting-item">
            <span class="setting-label">{{ t('settings.theme') }}</span>
            <el-select v-model="currentTheme" style="width: 100%">
              <el-option v-for="theme in themes" :key="theme.value" :label="theme.label" :value="theme.value" />
            </el-select>
          </div>
          <div class="setting-item setting-item-left">
            <span class="setting-label">{{ t('settings.skin') }}</span>
            <div class="skin-picker">
              <div
                v-for="skinItem in skins"
                :key="skinItem.value"
                class="skin-item"
                :class="{ active: settingsStore.skin === skinItem.value }"
                :style="{ '--skin-color': skinItem.value }"
                @click="settingsStore.setSkin(skinItem.value)"
                :title="skinItem.label"
              >
                <div class="skin-color"></div>
                <span class="skin-label">{{ skinItem.label }}</span>
              </div>
            </div>
          </div>
        </div>
      </el-card>

      <el-card>
        <template #header>
          <div class="card-header">
            <span>{{ t('settings.language') }}</span>
            <el-checkbox
              v-model="settingsStore.showLanguageInTitlebar"
              @change="(val) => settingsStore.setShowLanguageInTitlebar(val)"
              size="small"
            >
              {{ t('settings.showInTitlebar') }}
            </el-checkbox>
          </div>
        </template>
        <div class="settings-section">
          <div class="setting-item">
            <span class="setting-label">{{ t('settings.language') }}</span>
            <el-select v-model="locale" style="width: 100%" @change="changeLanguage">
              <el-option v-for="lang in languages" :key="lang.code" :label="lang.label" :value="lang.code" />
            </el-select>
          </div>
        </div>
      </el-card>

      <el-card v-if="state.initialized">
        <template #header>
          <span>{{ t('settings.modelFormat') }}</span>
        </template>
        <div class="settings-section">
          <div class="setting-item">
            <span class="setting-label setting-label-wide">{{ t('settings.dragFormat') }}</span>
            <el-select v-model="dragFormat" style="width: 220px">
              <el-option
                v-for="item in formatOptions"
                :key="item.value"
                :label="item.label"
                :value="item.value"
              />
            </el-select>
          </div>
          <div class="setting-item">
            <span class="setting-label setting-label-wide">{{ t('settings.exportFormat') }}</span>
            <el-select v-model="exportFormat" style="width: 220px">
              <el-option
                v-for="item in formatOptions"
                :key="item.value"
                :label="item.label"
                :value="item.value"
              />
            </el-select>
          </div>
          <div class="setting-hint">{{ t('settings.formatHint') }}</div>
        </div>
      </el-card>

      <el-card>
        <template #header>
          <span>{{ t('settings.maintenance') }}</span>
        </template>
        <div class="maintenance-card">
          <div class="maintenance-icon">
            <i class="iconfont icon-repeat"></i>
          </div>
          <div class="maintenance-text">
            <div class="maintenance-title">{{ t('settings.regenerateAll') }}</div>
            <div class="maintenance-desc">{{ t('settings.regenerateDesc') }}</div>
          </div>
          <el-button
            type="primary"
            class="maintenance-btn"
            :loading="regenerating"
            :disabled="regenerating"
            @click="regenerateAllCovers"
          >
            {{ regenerating
              ? t('settings.regenerating', { current: regenCurrent, total: regenTotal })
              : t('settings.regenerateAll') }}
          </el-button>
        </div>
      </el-card>
    </div>

    <div v-if="viewerVisible" class="cover-renderer">
      <ModelViewer
        :key="viewerKey"
        ref="viewerRef"
        :buffer="viewerBuffer"
        :file-type="viewerFileType"
        :aux-files="viewerAux"
        :view-params="viewerViewParams"
        @loaded="onViewerLoaded"
        @error="onViewerError"
      />
    </div>
  </div>
</template>

<style scoped>
.settings-container {
  padding: 24px;
  width: 100%;
  height: 100%;
  overflow: auto;
}

.settings-header {
  margin-bottom: 24px;
}

.settings-header h2 {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-1);
  margin: 0;
}

.settings-content {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.settings-section {
  padding: 16px 0;
}

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-soft);
}

.setting-item:last-child {
  border-bottom: none;
}

.setting-label {
  font-size: 14px;
  color: var(--text-1);
  flex-shrink: 0;
  width: 80px;
}

.setting-label-wide {
  width: 150px;
}

.setting-hint {
  padding-top: 10px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3, var(--text-2));
}

.setting-value {
  font-size: 14px;
  color: var(--text-2);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.support-item {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 0;
}

.support-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 215, 0, 0.1);
  border-radius: 12px;
}

.support-info {
  flex: 1;
}

.support-title {
  display: block;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}

.support-desc {
  display: block;
  font-size: 12px;
  color: var(--text-3);
  margin-top: 4px;
}

.version-info {
  display: flex;
  align-items: center;
  gap: 8px;
}
.update-badge {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 4px;
}

.update-badge-success {
  color: #67c23a;
  background: rgba(103, 194, 58, 0.1);
}

.update-badge-warning {
  color: #f56c6c;
  background: rgba(245, 108, 108, 0.1);
  cursor: pointer;
  text-decoration: underline;
}

.update-badge-warning:hover {
  text-decoration: underline;
}

.skin-picker {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  justify-content: flex-start;
}

.setting-item-left {
  justify-content: flex-start;
  gap: 16px;
}

.skin-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 8px 12px;
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
  border: 2px solid transparent;
}

.skin-item:hover {
  background: var(--bg-hover);
  border-color: rgba(0, 0, 0, 0.1);
}

.skin-item.active {
  border-color: var(--primary);
  background: var(--primary-soft);
}

.skin-color {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background-color: var(--skin-color);
  border: 2px solid rgba(0, 0, 0, 0.1);
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

.skin-label {
  font-size: 12px;
  color: var(--text-3);
}

.regenerate-item {
  align-items: flex-start;
  gap: 16px;
}
.regenerate-item .setting-label {
  width: auto;
  max-width: 220px;
  padding-top: 6px;
  line-height: 1.4;
}
.regenerate-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  min-width: 0;
}
.regenerate-desc {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.6;
}

.maintenance-card {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 6px 4px 2px;
}
.maintenance-icon {
  width: 40px;
  height: 40px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  background: var(--primary-soft);
  color: var(--primary);
}
.maintenance-icon .iconfont {
  font-size: 19px;
}
.maintenance-text {
  flex: 1;
  min-width: 0;
}
.maintenance-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
  margin-bottom: 5px;
}
.maintenance-desc {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.6;
}
.maintenance-btn {
  flex-shrink: 0;
}

/* 离屏封面渲染器：需要真实尺寸才能正常出图 */
.cover-renderer {
  position: fixed;
  left: -9999px;
  top: 0;
  width: 400px;
  height: 300px;
  pointer-events: none;
}

</style>
