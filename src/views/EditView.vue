<script setup>
import { ref, shallowRef, onMounted, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { ElMessage, ElMessageBox } from 'element-plus'

import { useStore } from '../composables/useStore'
import { useDragUpload } from '../composables/useDragUpload'
import { askExportFormat } from '../composables/exportFormatDialog'
import TagInput from '../components/TagInput.vue'
import { base64ToBytes, base64ToText } from '../lib/geomCommon'
import { detectJsonKind } from '../lib/jsonModel'

const ModelViewer = defineAsyncComponent(() => import('../components/ModelViewer.vue'))

const { t } = useI18n()
const { state, goHome, deleteModel } = useStore()

const TEXTURE_EXTS = ['png', 'jpg', 'jpeg', 'webp']
const DROP_EXTS = ['json', ...TEXTURE_EXTS]

function extOf(path) {
  return String(path || '').toLowerCase().split('.').pop()
}
function isAnimationFile(path) {
  return /\.animation\.json$/i.test(String(path || ''))
}
function isImagePath(path) {
  return TEXTURE_EXTS.includes(extOf(path))
}
function formatSize(bytes) {
  if (!bytes) return '0 B'
  if (bytes < 1024) return bytes + ' B'
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB'
  return (bytes / 1024 / 1024).toFixed(2) + ' MB'
}
function base64ToTextSafe(base64) {
  try {
    return base64ToText(base64)
  } catch {
    return ''
  }
}

const mainFile = ref(null)
const buffer = shallowRef(null)
const fileType = ref('')
const auxFiles = ref([])
const removedAux = ref([])
const visibleAux = ref([])

const name = ref('')
const description = ref('')
const tags = ref([])
const dimensions = ref({ x: 0, y: 0, z: 0 })
const animationCount = ref(0)
const viewParams = ref(null)
const coverData = ref('')

const viewerRef = ref(null)
const loadingModel = ref(false)
const saving = ref(false)
const exporting = ref(false)
const errorMsg = ref('')

function syncVisibleAux() {
  visibleAux.value = auxFiles.value.filter((file) => !removedAux.value.includes(file.name))
}

function showToast(msg, type = 'success') {
  ElMessage({ message: msg, type, duration: 2000 })
}

function isJsonRow() {
  return String(fileType.value || '').toLowerCase() === 'json'
}

// ===== 关联文件（贴图 / 动作）=====
function pushAux(staged) {
  if (removedAux.value.includes(staged.name)) {
    removedAux.value = removedAux.value.filter((item) => item !== staged.name)
  }
  const exists = auxFiles.value.some(
    (file) => file.role === staged.role && file.name === staged.name,
  )
  if (!exists) auxFiles.value = [...auxFiles.value, staged]
  syncVisibleAux()
}

async function stageAuxPath(path, role) {
  const staged = await window.api.models.stageAux(path, role)
  pushAux(staged)
  return staged
}

async function addAuxPaths(paths) {
  if (!isJsonRow()) {
    ElMessage.warning(t('upload.auxJsonOnly'))
    return
  }
  for (const path of paths) {
    try {
      if (isImagePath(path)) {
        await stageAuxPath(path, 'texture')
        continue
      }
      if (!/\.json$/i.test(path)) continue
      const staged = await window.api.models.stageAux(path, 'animation')
      if (isAnimationFile(path)) {
        pushAux(staged)
        continue
      }
      const json = JSON.parse(base64ToTextSafe(staged.dataBase64) || '{}')
      if (detectJsonKind(json) === 'animation') {
        pushAux(staged)
      } else {
        ElMessage.warning(t('upload.auxJsonOnly'))
      }
    } catch (e) {
      showToast(e.message || t('upload.addAnimation') + t('common.failed'), 'error')
    }
  }
  syncVisibleAux()
}

async function addAuxPick(role) {
  if (!isJsonRow()) return
  const paths = await window.api.models.pickAuxFiles(role)
  if (!paths.length) return
  await addAuxPaths(paths)
}

function removeAux(file) {
  if (!file) return
  if (file.path) {
    auxFiles.value = auxFiles.value.filter((item) => item !== file)
  } else if (!removedAux.value.includes(file.name)) {
    removedAux.value = [...removedAux.value, file.name]
  }
  syncVisibleAux()
}

const { isDragOver, onDragEnter, onDragOver, onDragLeave, onDrop } = useDragUpload({
  extensions: DROP_EXTS,
  typeLabel: '关联文件',
  onUpload: async (paths) => addAuxPaths(paths)
})

// ===== 预览回调 =====
function onModelLoaded(payload) {
  dimensions.value = payload.dimensions || { x: 0, y: 0, z: 0 }
  animationCount.value = payload.animationCount || 0
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      try {
        viewParams.value = viewerRef.value?.getViewParams() || null
        coverData.value = viewerRef.value?.captureThumbnail() || ''
      } catch (e) {
        console.warn('[Edit] 封面截图失败:', e)
      }
    })
  })
}

// ===== 加载编辑模型 =====
async function loadEditModel() {
  const m = state.editingModel
  if (!m) return
  name.value = m.name || ''
  description.value = m.description || ''
  tags.value = Array.isArray(m.tags) ? [...m.tags] : []
  dimensions.value = m.dimensions || { x: 0, y: 0, z: 0 }
  animationCount.value = Number(m.animationCount || 0)

  loadingModel.value = true
  try {
    const res = await window.api.models.loadFile({
      id: String(m.id),
      fileName: String(m.fileName),
      fileType: String(m.fileType),
      files: JSON.parse(JSON.stringify(m.files || []))
    })
    mainFile.value = {
      fileName: m.fileName,
      fileType: m.fileType,
      fileSize: m.fileSize,
      defaultName: m.name || m.fileName
    }
    buffer.value = base64ToBytes(res.dataBase64)
    fileType.value = res.fileType
    auxFiles.value = (res.auxFiles || []).map((file) => ({
      name: file.name,
      role: file.role,
      size: file.size || 0,
      dataBase64: file.dataBase64
    }))
    viewParams.value = m.viewParams || null
    syncVisibleAux()
  } catch (e) {
    errorMsg.value = e.message || t('upload.loadModel') + t('common.failed')
    showToast(errorMsg.value, 'error')
  } finally {
    loadingModel.value = false
  }
}

// ===== 提交 =====
function buildMeta(id, files) {
  return {
    id: String(id),
    name: String(name.value.trim()),
    description: String(description.value.trim()),
    fileName: String(mainFile.value.fileName),
    fileType: String(fileType.value),
    fileSize: Number(mainFile.value.fileSize || 0),
    files: JSON.parse(JSON.stringify(files)),
    dimensions: {
      x: Number(dimensions.value.x) || 0,
      y: Number(dimensions.value.y) || 0,
      z: Number(dimensions.value.z) || 0
    },
    animationCount: Number(animationCount.value || 0),
    uploadTime: String(state.editingModel.uploadTime || new Date().toISOString()),
    cover: coverData.value ? 'cover.png' : state.editingModel.cover || null,
    viewParams: viewParams.value,
    tags: tags.value.map((tag) => String(tag))
  }
}

async function handleSubmit() {
  if (!buffer.value) return
  if (!name.value.trim()) {
    ElMessage.warning(t('upload.modelName'))
    return
  }
  try {
    if (viewerRef.value) {
      viewParams.value = viewerRef.value.getViewParams() || viewParams.value
      coverData.value = viewerRef.value.captureThumbnail() || coverData.value
    }
  } catch (e) {
    console.warn('[Edit] 封面截图失败:', e)
  }

  saving.value = true
  errorMsg.value = ''
  try {
    await saveEdit()
  } catch (e) {
    errorMsg.value = e.message || t('common.save') + t('common.failed')
    showToast(errorMsg.value, 'error')
  } finally {
    saving.value = false
  }
}

async function saveEdit() {
  const id = String(state.editingModel.id)
  const files = JSON.parse(JSON.stringify(state.editingModel.files || [])).filter(
    (file) => !removedAux.value.includes(file.name),
  )

  for (const fileName of removedAux.value) {
    await window.api.models.removeAuxFile(id, fileName)
  }
  for (const file of auxFiles.value.filter((item) => item.path)) {
    const info = await window.api.models.addAuxFile(id, file.path, file.role)
    if (info) files.push(info)
  }
  if (coverData.value) {
    await window.api.models.saveImage(id, 'cover', coverData.value)
  }
  await window.api.models.update(id, buildMeta(id, files))
  showToast(t('common.save') + t('common.ok'))
  setTimeout(goHome, 600)
}

async function handleBack() {
  goHome()
}

async function handleDelete() {
  try {
    await ElMessageBox.confirm(t('upload.deleteConfirm'), t('upload.deleteConfirm'), {
      confirmButtonText: t('upload.delete'),
      cancelButtonText: t('common.cancel'),
      type: 'warning',
      confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  try {
    await deleteModel(String(state.editingModel.id))
    showToast(t('common.delete') + t('common.ok'))
    setTimeout(goHome, 400)
  } catch (e) {
    errorMsg.value = e.message || t('common.delete') + t('common.failed')
    showToast(errorMsg.value, 'error')
  }
}

async function handleExport() {
  const format = await askExportFormat(state.library?.settings?.exportFormat || 'original')
  if (!format) return
  exporting.value = true
  try {
    const m = state.editingModel
    const res = await window.api.models.export(
      {
        id: String(m.id),
        name: String(name.value || m.name || m.fileName || 'model'),
        fileName: String(m.fileName),
      },
      format
    )
    if (res) showToast(t('upload.exportCount', { count: res.fileCount, dir: res.targetDir }))
    if (res?.warnings?.length) showToast(res.warnings[0], 'warning')
  } catch (e) {
    errorMsg.value = e.message || t('common.export') + t('common.failed')
    showToast(errorMsg.value, 'error')
  } finally {
    exporting.value = false
  }
}

onMounted(() => {
  loadEditModel()
})
</script>

<template>
  <div class="edit-page">
    <header class="topbar">
      <el-button text @click="handleBack">
        <i class="iconfont icon-chevron-left"></i> {{ t('common.back') }}
      </el-button>
      <span class="title">{{ t('upload.editModel') }}</span>
      <div class="topbar-actions">
        <el-button :disabled="exporting" @click="handleExport">
          <i class="iconfont icon-cloud-download"></i>
          {{ exporting ? t('common.exporting') : t('common.export') }}
        </el-button>
        <el-button type="danger" plain @click="handleDelete">
          <i class="iconfont icon-trash-alt"></i> {{ t('common.delete') }}
        </el-button>
      </div>
    </header>

    <div class="edit-main">
      <div
        class="preview-box"
        :class="{ 'drag-active': isDragOver }"
        @dragenter="onDragEnter"
        @dragover="onDragOver"
        @dragleave="onDragLeave"
        @drop="onDrop"
      >
        <ModelViewer
          v-if="buffer"
          ref="viewerRef"
          :buffer="buffer"
          :file-type="fileType"
          :aux-files="visibleAux"
          :view-params="viewParams"
          @loaded="onModelLoaded"
        />
        <div v-else class="preview-loading">
          <i class="iconfont icon-movie spinning"></i>
          <span>{{ loadingModel ? t('common.loading') + '...' : t('upload.noPreview') }}</span>
        </div>
        <div v-if="isDragOver" class="preview-drop">
          <i class="iconfont icon-cloud-upload"></i>
          <p>{{ t('upload.dropAux') }}</p>
        </div>
      </div>
    </div>

    <div class="edit-footer">
      <div class="file-column">
        <div v-if="mainFile" class="file-row file-main">
          <i class="iconfont icon-file file-icon"></i>
          <div class="file-meta">
            <span class="file-name" :title="mainFile.fileName">{{ mainFile.fileName }}</span>
            <span class="file-sub">
              {{ (mainFile.fileType || '').toUpperCase() }} · {{ formatSize(mainFile.fileSize) }}
            </span>
          </div>
          <el-tag size="small">{{ t('common.main') }}</el-tag>
        </div>

        <div v-for="file in visibleAux" :key="`${file.role}-${file.name}`" class="file-row">
          <i
            class="iconfont file-icon"
            :class="file.role === 'texture' ? 'icon-image' : 'icon-movie'"
          ></i>
          <div class="file-meta">
            <span class="file-name" :title="file.name">{{ file.name }}</span>
            <span class="file-sub">{{ formatSize(file.size) }}</span>
          </div>
          <el-tag size="small" type="info">
            {{ file.role === 'texture' ? t('upload.texture') : t('upload.animationFile') }}
          </el-tag>
          <el-button text size="small" class="file-remove" @click="removeAux(file)">
            <i class="iconfont icon-trash-alt"></i>
          </el-button>
        </div>

        <div v-if="isJsonRow()" class="file-row file-drop">
          <i class="iconfont icon-plus file-icon"></i>
          <span class="file-drop-text">{{ t('upload.dropAux') }}</span>
          <el-button size="small" @click.stop="addAuxPick('texture')">
            {{ t('upload.addTexture') }}
          </el-button>
          <el-button size="small" @click.stop="addAuxPick('animation')">
            {{ t('upload.addAnimation') }}
          </el-button>
        </div>
        <div v-else class="file-row file-hint-row">
          <i class="iconfont icon-info file-icon"></i>
          <span class="file-drop-text">{{ t('upload.auxJsonOnly') }}</span>
        </div>
      </div>

      <div class="meta-column">
        <el-form label-position="top" class="meta-form">
          <div class="meta-grid">
            <el-form-item :label="t('upload.modelName')" required>
              <el-input v-model="name" :placeholder="t('upload.modelName')" />
            </el-form-item>
            <el-form-item :label="t('common.tags')">
              <TagInput v-model="tags" :pool="state.tags" />
            </el-form-item>
          </div>
          <el-form-item :label="t('upload.description')">
            <el-input
              v-model="description"
              type="textarea"
              :rows="2"
              :placeholder="t('upload.descriptionPlaceholder')"
            />
          </el-form-item>
        </el-form>
        <el-alert v-if="errorMsg" :title="errorMsg" type="error" show-icon :closable="false" />
      </div>

      <div class="action-column">
        <span class="submit-tip">{{ t('upload.tip') }}</span>
        <el-button @click="handleBack">{{ t('common.cancel') }}</el-button>
        <el-button
          type="primary"
          size="large"
          :loading="saving"
          :disabled="!buffer || loadingModel"
          @click="handleSubmit"
        >
          {{ t('common.save') }}
        </el-button>
      </div>
    </div>

    <div v-if="saving" class="saving-mask">
      <div class="saving-content">
        <i class="iconfont icon-movie saving-spinner"></i>
        <p class="saving-text">{{ t('common.saving') }}...</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.edit-page {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 24px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.title {
  flex: 1;
  font-size: 15px;
  font-weight: 600;
}
.topbar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.edit-main {
  flex: 1;
  min-height: 0;
  padding: 16px 24px;
}
.preview-box {
  position: relative;
  width: 100%;
  height: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  overflow: hidden;
  background: var(--bg-soft);
  transition: border-color 0.15s, background 0.15s;
}
.preview-box.drag-active {
  border-color: var(--primary);
  background: var(--primary-soft);
}
.preview-loading {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-3);
  font-size: 13px;
}
.preview-loading .iconfont {
  font-size: 40px;
}
.spinning {
  animation: spin 1s linear infinite;
}
.preview-drop {
  position: absolute;
  inset: 0;
  z-index: 3;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  background: var(--bg-panel);
  backdrop-filter: blur(2px);
  pointer-events: none;
}
.preview-drop .iconfont {
  font-size: 46px;
  color: var(--primary);
}
.preview-drop p {
  margin: 0;
  font-size: 14px;
  color: var(--text-1);
}
.edit-footer {
  flex-shrink: 0;
  max-height: 46%;
  overflow: auto;
  border-top: 1px solid var(--border-soft);
  padding: 12px 24px 16px;
  display: grid;
  grid-template-columns: minmax(280px, 1fr) minmax(340px, 1.15fr);
  gap: 16px 20px;
  align-items: start;
  background: var(--bg);
}
.file-column {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.file-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-soft);
  min-width: 0;
}
.file-main {
  background: var(--primary-soft);
  border-color: transparent;
}
.file-icon {
  font-size: 15px;
  color: var(--text-2);
  flex-shrink: 0;
}
.file-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.file-name {
  font-size: 13px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-sub {
  font-size: 11px;
  color: var(--text-3);
}
.file-remove {
  padding: 0 4px !important;
  height: auto !important;
}
.file-drop {
  justify-content: center;
  border-style: dashed;
  background: transparent;
  cursor: pointer;
  color: var(--text-3);
  transition: all 0.15s;
}
.file-drop:hover {
  border-color: var(--primary);
  color: var(--primary);
}
.file-drop-text {
  font-size: 12px;
}
.file-hint-row {
  background: transparent;
  border-style: dashed;
  color: var(--text-3);
}
.meta-column {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.meta-form :deep(.el-form-item) {
  margin-bottom: 10px;
}
.meta-form :deep(.el-form-item:last-child) {
  margin-bottom: 0;
}
.meta-form :deep(.el-form-item__label) {
  line-height: 1.4;
  padding-bottom: 4px;
}
.meta-form :deep(.el-input__wrapper),
.meta-form :deep(.el-textarea__inner) {
  width: 100%;
  box-sizing: border-box;
}
.action-column {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--border-soft);
  position: sticky;
  bottom: 0;
  z-index: 1;
  background: var(--bg);
}
.submit-tip {
  margin-right: auto;
  font-size: 12px;
  color: var(--text-3);
  text-align: left;
  line-height: 1.5;
}
.saving-mask {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
  backdrop-filter: blur(4px);
}
.saving-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: #fff;
}
.saving-spinner {
  font-size: 48px;
  animation: spin 1s linear infinite;
}
.saving-text {
  font-size: 16px;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
</style>
