<script setup>
import { ref, computed, shallowRef, onMounted, onBeforeUnmount, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { ElMessage, ElMessageBox } from 'element-plus'

import { useStore } from '../composables/useStore'
import { useDragUpload } from '../composables/useDragUpload'
import { askExportFormat } from '../composables/exportFormatDialog'
import { useUploadStore } from '../stores/upload'
import { useFolderStore } from '../stores/folders'
import TagInput from '../components/TagInput.vue'
import { base64ToBytes, base64ToText } from '../lib/geomCommon'
import { detectJsonKind } from '../lib/jsonModel'

// 异步加载 3D 预览组件，three.js 不进入首屏主包
const ModelViewer = defineAsyncComponent(() => import('../components/ModelViewer.vue'))

const { t } = useI18n()
const { state, goHome, deleteModel } = useStore()
const uploadStore = useUploadStore()
const folderStore = useFolderStore()

const MODEL_EXTS = ['bbmodel', 'json', 'vox']
const TEXTURE_EXTS = ['png', 'jpg', 'jpeg', 'webp']
const DROP_EXTS = [...MODEL_EXTS, ...TEXTURE_EXTS]

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

const isEdit = computed(() => state.view === 'edit' && !!state.editingModel)

// ===== 待上传模型行 =====
const rows = ref([])
let rowSeq = 0
const activeRowKey = ref(0)
const dropRowKey = ref(null)
const loadingModel = ref(false)
const saving = ref(false)
const exporting = ref(false)
const errorMsg = ref('')

function isJsonRow(row) {
  return String(row.fileType || '').toLowerCase() === 'json'
}

function visibleAux(row) {
  return row.auxFiles.filter((file) => !row.removedAux.includes(file.name))
}

function dimText(row) {
  const d = row.dimensions
  if (!d || (!d.x && !d.y && !d.z)) return ''
  return `${d.x} × ${d.y} × ${d.z}`
}

function toggleExpand(row) {
  row.expanded = !row.expanded
  activeRowKey.value = row.key
}

// ===== 离屏渲染缩略图（逐行串行执行）=====
const viewerVisible = ref(false)
const viewerKey = ref(0)
const viewerBuffer = shallowRef(null)
const viewerType = ref('')
const viewerAux = ref([])
const viewerParams = ref(null)
const viewerRef = ref(null)

let pendingResolve = null
let pendingReject = null
let renderTimeout = null
let renderQueue = Promise.resolve()

function finishRender(result, error) {
  if (renderTimeout) {
    clearTimeout(renderTimeout)
    renderTimeout = null
  }
  const resolve = pendingResolve
  const reject = pendingReject
  pendingResolve = null
  pendingReject = null
  if (error) reject?.(error)
  else resolve?.(result)
}

function onViewerLoaded(payload) {
  if (!pendingResolve) return
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      let cover = ''
      try {
        cover = viewerRef.value?.captureThumbnail() || ''
      } catch (e) {
        finishRender(null, e)
        return
      }
      finishRender({
        cover,
        dimensions: payload?.dimensions || { x: 0, y: 0, z: 0 },
        animationCount: payload?.animationCount || 0
      })
    })
  })
}

function onViewerError(error) {
  finishRender(null, error instanceof Error ? error : new Error(String(error)))
}

function renderWithViewer(buffer, fileType, auxFiles, viewParams) {
  return new Promise((resolve, reject) => {
    pendingResolve = resolve
    pendingReject = reject
    viewerKey.value++
    viewerBuffer.value = buffer
    viewerType.value = fileType
    viewerAux.value = auxFiles || []
    viewerParams.value = viewParams || null
    viewerVisible.value = true
    if (renderTimeout) clearTimeout(renderTimeout)
    renderTimeout = setTimeout(() => {
      finishRender(null, new Error(t('upload.renderFailed')))
    }, 30000)
  })
}

async function renderRow(row) {
  if (!rows.value.some((item) => item.key === row.key)) return
  row.rendering = true
  try {
    const aux = visibleAux(row).map((file) => ({
      name: file.name,
      role: file.role,
      dataBase64: file.dataBase64
    }))
    const result = await renderWithViewer(row.buffer, row.fileType, aux, row.viewParams)
    if (!rows.value.some((item) => item.key === row.key)) return
    if (result.cover) row.thumbnail = result.cover
    row.dimensions = result.dimensions
    row.animationCount = result.animationCount
    if (isEdit.value) row.coverDirty = true
  } catch (e) {
    console.warn('[Upload] 预览生成失败:', row.fileName, e)
    ElMessage.warning(`${row.fileName}: ${t('upload.renderFailed')}`)
  } finally {
    if (rows.value.some((item) => item.key === row.key)) row.rendering = false
    viewerVisible.value = false
    viewerBuffer.value = null
    viewerAux.value = []
    viewerParams.value = null
  }
}

function enqueueRender(row) {
  renderQueue = renderQueue.then(() => renderRow(row)).catch((e) => {
    console.warn('[Upload] 渲染队列异常:', e)
  })
  return renderQueue
}

async function waitRenders() {
  try {
    await renderQueue
  } catch {
    // ignore
  }
}

// ===== 添加模型 =====
function createRow(data, extra = {}) {
  return {
    key: ++rowSeq,
    path: extra.path || '',
    fileName: data.fileName || '',
    fileType: data.fileType || '',
    fileSize: Number(data.fileSize || 0),
    dataBase64: data.dataBase64 || '',
    buffer: extra.buffer || base64ToBytes(data.dataBase64 || ''),
    name: data.name || data.defaultName || '',
    description: data.description || '',
    tags: Array.isArray(data.tags) ? [...data.tags] : [],
    dimensions: data.dimensions || { x: 0, y: 0, z: 0 },
    animationCount: Number(data.animationCount || 0),
    auxFiles: Array.isArray(data.auxFiles) ? data.auxFiles : [],
    removedAux: [],
    thumbnail: data.thumbnail || '',
    viewParams: data.viewParams || null,
    uploadTime: data.uploadTime || new Date().toISOString(),
    expanded: !!extra.expanded,
    rendering: false,
    coverDirty: false
  }
}

async function selectFiles() {
  if (isEdit.value) return
  const paths = await window.api.models.pickFiles()
  if (paths && paths.length) await addModelPaths(paths)
}

async function addModelPaths(paths) {
  if (isEdit.value) return
  errorMsg.value = ''
  loadingModel.value = true
  const list = (paths || []).map(String).filter(Boolean)
  try {
    for (const path of list) {
      if (rows.value.some((row) => row.path === path)) continue
      try {
        const data = await window.api.models.stageModel(path)
        rows.value.push(createRow(data, { path }))
        const created = rows.value[rows.value.length - 1]
        if (!activeRowKey.value) activeRowKey.value = created.key
        enqueueRender(created)
      } catch (e) {
        ElMessage.error(`${path.split(/[\\/]/).pop()}: ${e.message || t('common.failed')}`)
      }
    }
  } finally {
    loadingModel.value = false
  }
}

function removeRow(row) {
  rows.value = rows.value.filter((item) => item.key !== row.key)
  if (activeRowKey.value === row.key) {
    activeRowKey.value = rows.value[0]?.key || 0
  }
}

function refreshRow(row) {
  enqueueRender(row)
}

// ===== 关联文件（贴图 / 动作）=====
function pushAuxToRow(row, staged) {
  if (row.removedAux.includes(staged.name)) {
    row.removedAux = row.removedAux.filter((name) => name !== staged.name)
  }
  const index = row.auxFiles.findIndex(
    (file) => file.name === staged.name && file.role === staged.role
  )
  if (index >= 0) row.auxFiles.splice(index, 1, staged)
  else row.auxFiles.push(staged)
}

async function addAuxPathsToRow(row, paths) {
  if (!row) return false
  if (!isJsonRow(row)) {
    ElMessage.warning(t('upload.auxJsonOnly'))
    return false
  }
  let changed = false
  for (const path of paths) {
    try {
      if (isImagePath(path)) {
        const staged = await window.api.models.stageAux(path, 'texture')
        pushAuxToRow(row, staged)
        changed = true
        continue
      }
      if (!/\.json$/i.test(path)) continue

      const staged = await window.api.models.stageAux(path, 'animation')
      if (isAnimationFile(path)) {
        pushAuxToRow(row, staged)
        changed = true
        continue
      }
      const json = JSON.parse(base64ToTextSafe(staged.dataBase64) || '{}')
      const kind = detectJsonKind(json)
      if (kind === 'animation') {
        pushAuxToRow(row, staged)
        changed = true
      } else if (kind === 'bedrock' || kind === 'java') {
        ElMessage.warning(t('upload.dropModelInstead'))
      } else {
        ElMessage.warning(t('upload.auxJsonOnly'))
      }
    } catch (e) {
      ElMessage.error(e.message || t('upload.addAnimation') + t('common.failed'))
    }
  }
  if (changed) enqueueRender(row)
  return changed
}

async function pickAuxForRow(row, role) {
  if (!isJsonRow(row)) return
  activeRowKey.value = row.key
  const paths = await window.api.models.pickAuxFiles(role)
  if (!paths || !paths.length) return
  await addAuxPathsToRow(row, role === 'texture' ? paths.filter(isImagePath) : paths)
}

function removeAux(row, file) {
  if (!file) return
  if (file.path) {
    row.auxFiles = row.auxFiles.filter((item) => item !== file)
  } else if (!row.removedAux.includes(file.name)) {
    row.removedAux = [...row.removedAux, file.name]
  }
  enqueueRender(row)
}

// ===== 拖拽（Tauri 原生拖放，可识别目标行）=====
function locateRow(position) {
  if (!position) return null
  const ratio = window.devicePixelRatio || 1
  for (const scale of [1 / ratio, 1]) {
    const el = document.elementFromPoint(position.x * scale, position.y * scale)
    const rowEl = el?.closest?.('[data-upload-row]')
    if (rowEl) return Number(rowEl.getAttribute('data-upload-row'))
  }
  return null
}

async function handleDroppedPaths(paths, payload) {
  const list = (paths || []).map(String).filter(Boolean)
  if (!list.length) return

  const targetKey = payload?.position ? locateRow(payload.position) : null
  const targetRow = targetKey ? rows.value.find((row) => row.key === targetKey) : null

  const modelPaths = []
  const auxPaths = []

  for (const path of list) {
    if (isImagePath(path) || isAnimationFile(path)) {
      auxPaths.push(path)
      continue
    }
    if (/\.(bbmodel|vox)$/i.test(path)) {
      modelPaths.push(path)
      continue
    }
    if (/\.json$/i.test(path)) {
      try {
        const staged = await window.api.models.stageAux(path, 'animation')
        const json = JSON.parse(base64ToTextSafe(staged.dataBase64) || '{}')
        if (detectJsonKind(json) === 'animation') auxPaths.push(path)
        else modelPaths.push(path)
      } catch {
        // 非法 JSON：交给模型/动画提示统一处理
        auxPaths.push(path)
      }
    }
  }

  if (targetRow) {
    if (auxPaths.length) {
      if (isJsonRow(targetRow)) await addAuxPathsToRow(targetRow, auxPaths)
      else ElMessage.warning(t('upload.auxJsonOnly'))
    }
    if (modelPaths.length) {
      if (isEdit.value) ElMessage.warning(t('upload.editNoReplace'))
      else await addModelPaths(modelPaths)
    }
    return
  }

  // 未指向具体行：模型新增行，贴图/动作优先挂到当前激活的 JSON 行
  if (modelPaths.length && !isEdit.value) await addModelPaths(modelPaths)

  if (auxPaths.length) {
    let auxTarget = null
    if (targetRow && isJsonRow(targetRow)) {
      auxTarget = targetRow
    } else {
      const active = rows.value.find((row) => row.key === activeRowKey.value)
      const jsonRows = rows.value.filter(isJsonRow)
      if (active && isJsonRow(active)) auxTarget = active
      else if (jsonRows.length === 1) auxTarget = jsonRows[0]
    }
    if (auxTarget) {
      await addAuxPathsToRow(auxTarget, auxPaths)
    } else {
      ElMessage.warning(t('upload.auxJsonOnly'))
    }
  }
}

const {
  isDragOver,
  onDragEnter,
  onDragOver,
  onDragLeave,
  onDrop
} = useDragUpload({
  extensions: DROP_EXTS,
  typeLabel: '模型文件',
  onUpload: handleDroppedPaths,
  onHover: (position) => {
    dropRowKey.value = position ? locateRow(position) : null
  }
})

const showDropOverlay = computed(() => isDragOver.value && !dropRowKey.value)

// ===== 编辑模式加载 =====
async function loadEditModel() {
  const m = state.editingModel
  if (!m) return
  loadingModel.value = true
  errorMsg.value = ''
  try {
    const res = await window.api.models.loadFile({
      id: String(m.id),
      fileName: String(m.fileName),
      fileType: String(m.fileType),
      files: JSON.parse(JSON.stringify(m.files || []))
    })
    const row = createRow(
      {
        fileName: m.fileName,
        fileType: res.fileType || m.fileType,
        fileSize: m.fileSize,
        dataBase64: res.dataBase64,
        name: m.name || m.fileName,
        description: m.description || '',
        tags: Array.isArray(m.tags) ? m.tags : [],
        dimensions: m.dimensions || { x: 0, y: 0, z: 0 },
        animationCount: m.animationCount || 0,
        auxFiles: (res.auxFiles || []).map((file) => ({
          name: file.name,
          role: file.role,
          size: file.size || 0,
          dataBase64: file.dataBase64
        })),
        thumbnail: m.coverBase64 || '',
        viewParams: m.viewParams || null,
        uploadTime: m.uploadTime
      },
      { expanded: true }
    )
    rows.value = [row]
    activeRowKey.value = row.key
  } catch (e) {
    errorMsg.value = e.message || t('upload.loadModel') + t('common.failed')
    ElMessage.error(errorMsg.value)
  } finally {
    loadingModel.value = false
  }
}

// ===== 提交 =====
function buildMeta(row, id, files, coverName, existing, folderId) {
  const meta = {
    id: String(id),
    name: String((row.name || '').trim()),
    description: String((row.description || '').trim()),
    fileName: String(row.fileName),
    fileType: String(row.fileType),
    fileSize: Number(row.fileSize || 0),
    files: JSON.parse(JSON.stringify(files || [])),
    dimensions: {
      x: Number(row.dimensions?.x) || 0,
      y: Number(row.dimensions?.y) || 0,
      z: Number(row.dimensions?.z) || 0
    },
    animationCount: Number(row.animationCount || 0),
    uploadTime: String(row.uploadTime || new Date().toISOString()),
    cover: coverName || null,
    viewParams: row.viewParams || null,
    tags: (row.tags || []).map((tag) => String(tag))
  }
  if (!existing) {
    meta.folderId = folderId !== undefined ? folderId : folderStore.activeFolderId || ''
  }
  return meta
}

async function handleSubmit() {
  if (!rows.value.length) return
  const invalid = rows.value.find((row) => !String(row.name || '').trim())
  if (invalid) {
    invalid.expanded = true
    activeRowKey.value = invalid.key
    ElMessage.warning(t('upload.modelName'))
    return
  }

  saving.value = true
  errorMsg.value = ''
  try {
    await waitRenders()
    if (isEdit.value) {
      await saveEdit()
    } else {
      await uploadAllRows()
    }
  } catch (e) {
    errorMsg.value = e.message || t('common.upload') + t('common.failed')
    ElMessage.error(errorMsg.value)
  } finally {
    saving.value = false
  }
}

async function uploadAllRows() {
  // 同一批次内：同名但不同格式（如 bbmodel + json）通常是同一模型，先提示
  const nameGroups = new Map()
  for (const row of rows.value) {
    const key = String(row.name || '').trim().toLowerCase()
    if (!key) continue
    if (!nameGroups.has(key)) nameGroups.set(key, [])
    nameGroups.get(key).push(row)
  }
  let suspectedGroups = 0
  for (const list of nameGroups.values()) {
    const types = new Set(list.map((row) => String(row.fileType || '').toLowerCase()))
    if (list.length > 1 && types.size > 1) suspectedGroups++
  }
  if (suspectedGroups > 0) {
    try {
      await ElMessageBox.confirm(
        t('upload.sameNameBatch', { count: suspectedGroups }),
        t('home.sameFilesTitle'),
        {
          confirmButtonText: t('upload.continueAll'),
          cancelButtonText: t('common.cancel'),
          type: 'warning'
        }
      )
    } catch {
      return
    }
  }

  const total = rows.value.length
  const taskId = uploadStore.begin({
    title: t('upload.uploadAll'),
    type: 'models',
    total
  })
  const update = (patch) => uploadStore.update(taskId, patch)

  const failures = []
  const ready = []
  const duplicates = []

  for (let i = 0; i < total; i++) {
    const row = rows.value[i]
    update({
      current: i + 1,
      total,
      fileName: row.fileName,
      progress: Math.round((i / total) * 50)
    })
    try {
      const aux = row.auxFiles
        .filter((file) => file.path)
        .map((file) => ({ path: file.path, role: file.role }))
      const data = await window.api.models.uploadWithAux(row.path, aux)
      if (data?.duplicate) duplicates.push({ row, data })
      else if (data) ready.push({ row, data })
    } catch (e) {
      failures.push({ name: row.name || row.fileName, message: e.message || t('common.failed') })
    }
  }

  if (duplicates.length) {
    let overwrite = false
    try {
      await ElMessageBox.confirm(
        t('upload.sameFilesMulti', { count: duplicates.length }),
        t('home.sameFilesTitle'),
        {
          confirmButtonText: t('home.overwriteAll'),
          cancelButtonText: t('home.skipAll'),
          type: 'warning'
        }
      )
      overwrite = true
    } catch {
      overwrite = false
    }

    if (overwrite) {
      for (const dup of duplicates) {
        try {
          const oldFolderId = dup.data.existingModel?.folderId ?? ''
          const data = await window.api.models.overwrite(
            dup.data.existingModel,
            dup.data.pendingFile
          )
          if (oldFolderId) {
            await window.api.folders.moveModels([String(data.id)], oldFolderId)
          }
          ready.push({ row: dup.row, data, overwrote: true, folderId: oldFolderId })
        } catch (e) {
          failures.push({
            name: dup.row.name || dup.row.fileName,
            message: e.message || t('common.failed')
          })
        }
      }
    } else {
      for (const dup of duplicates) {
        failures.push({
          name: dup.row.name || dup.row.fileName,
          message: t('home.skippedDuplicate')
        })
      }
    }
  }

  const saveTotal = ready.length
  let success = 0
  for (let i = 0; i < saveTotal; i++) {
    const { row, data, overwrote, folderId } = ready[i]
    update({
      current: i + 1,
      total: saveTotal,
      fileName: row.name || row.fileName,
      progress: 50 + Math.round(((i + 1) / Math.max(saveTotal, 1)) * 50)
    })
    try {
      if (row.thumbnail) {
        await window.api.models.saveImage(String(data.id), 'cover', row.thumbnail)
      }
      await window.api.models.save(
        buildMeta(row, data.id, data.files, row.thumbnail ? 'cover.png' : null, false, folderId)
      )
      success++
    } catch (e) {
      // 全新上传失败时清理半成品；覆盖已有模型时不能删除
      if (!overwrote) {
        try {
          await window.api.models.delete(String(data.id))
        } catch {
          // ignore
        }
      }
      failures.push({ name: row.name || row.fileName, message: e.message || t('common.failed') })
    }
  }

  uploadStore.finish(taskId, {
    success,
    failed: failures.length,
    details: failures.map((item) => ({ name: item.name, success: false, error: item.message }))
  })

  await goHome()

  if (!failures.length && success > 0) {
    ElMessage.success(t('upload.uploadRowsDone', { count: success }))
  } else if (success > 0) {
    ElMessage.warning(t('upload.uploadPartial', { success, failed: failures.length }))
  } else {
    ElMessage.error(t('upload.uploadFailed'))
  }
}

async function saveEdit() {
  const row = rows.value[0]
  const m = state.editingModel
  const id = String(m.id)

  const files = JSON.parse(JSON.stringify(m.files || [])).filter(
    (file) => !row.removedAux.includes(file.name)
  )

  for (const fileName of row.removedAux) {
    await window.api.models.removeAuxFile(id, fileName)
  }
  for (const file of row.auxFiles.filter((item) => item.path)) {
    const info = await window.api.models.addAuxFile(id, file.path, file.role)
    if (info) files.push(info)
  }
  if (row.coverDirty && row.thumbnail) {
    await window.api.models.saveImage(id, 'cover', row.thumbnail)
  }

  const meta = buildMeta(row, id, files, row.thumbnail ? 'cover.png' : null, true)
  meta.viewParams = m.viewParams || null
  await window.api.models.update(id, meta)
  ElMessage.success(t('common.save') + t('common.ok'))
  setTimeout(goHome, 400)
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
      confirmButtonClass: 'el-button--danger'
    })
  } catch {
    return
  }
  try {
    await deleteModel(String(state.editingModel.id))
    ElMessage.success(t('common.delete') + t('common.ok'))
    setTimeout(goHome, 400)
  } catch (e) {
    errorMsg.value = e.message || t('common.delete') + t('common.failed')
    ElMessage.error(errorMsg.value)
  }
}

async function handleExport() {
  if (!isEdit.value) return
  const format = await askExportFormat(state.library?.settings?.exportFormat || 'original')
  if (!format) return
  exporting.value = true
  try {
    const m = state.editingModel
    const row = rows.value[0]
    const res = await window.api.models.export(
      {
        id: String(m.id),
        name: String(row?.name || m.name || m.fileName || 'model'),
        fileName: String(m.fileName)
      },
      format
    )
    if (res) {
      ElMessage.success(t('upload.exportCount', { count: res.fileCount, dir: res.targetDir }))
    }
    if (res?.warnings?.length) ElMessage.warning(res.warnings[0])
  } catch (e) {
    errorMsg.value = e.message || t('common.export') + t('common.failed')
    ElMessage.error(errorMsg.value)
  } finally {
    exporting.value = false
  }
}

onMounted(() => {
  if (isEdit.value) loadEditModel()
})

onBeforeUnmount(() => {
  // 页面关闭时终止未完成的离屏渲染
  if (renderTimeout) {
    clearTimeout(renderTimeout)
    renderTimeout = null
  }
  pendingResolve = null
  pendingReject = null
  viewerVisible.value = false
})
</script>

<template>
  <div
    class="upload-page"
    @dragenter="onDragEnter"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <header class="topbar">
      <el-button text @click="handleBack">
        <i class="iconfont icon-chevron-left"></i> {{ t('common.back') }}
      </el-button>
      <span class="title">{{ isEdit ? t('upload.editModel') : t('upload.uploadModel') }}</span>
      <div class="topbar-actions">
        <el-button v-if="isEdit" :disabled="exporting" @click="handleExport">
          <i class="iconfont icon-cloud-download"></i>
          {{ exporting ? t('common.exporting') : t('common.export') }}
        </el-button>
        <el-button v-if="isEdit" type="danger" plain @click="handleDelete">
          <i class="iconfont icon-trash-alt"></i> {{ t('common.delete') }}
        </el-button>
      </div>
    </header>

    <div class="upload-body">
      <!-- 添加入口 -->
      <div
        v-if="!isEdit"
        class="dropzone"
        :class="{ 'drag-active': isDragOver, compact: rows.length > 0 }"
        @click="selectFiles"
      >
        <i class="iconfont icon-cloud-upload dropzone-icon"></i>
        <div class="dropzone-text">
          <p class="dropzone-title">
            {{ loadingModel ? t('common.loading') + '...' : t('upload.dropOrSelect') }}
          </p>
          <span class="dropzone-sub">{{ t('upload.formats') }} · {{ t('upload.dropAuxRow') }}</span>
        </div>
        <el-button type="primary" :loading="loadingModel" @click.stop="selectFiles">
          <i class="iconfont icon-folder-plus"></i>
          {{ rows.length ? t('upload.addMore') : t('upload.addModels') }}
        </el-button>
      </div>

      <!-- 待上传列表 -->
      <div v-if="rows.length" class="staged-list">
        <div
          v-for="row in rows"
          :key="row.key"
          class="staged-row"
          :class="{
            active: activeRowKey === row.key,
            'drag-over': dropRowKey === row.key
          }"
          :data-upload-row="row.key"
          @click="activeRowKey = row.key"
        >
          <div class="staged-thumb">
            <img v-if="row.thumbnail" :src="row.thumbnail" alt="preview" draggable="false" />
            <div v-else class="staged-thumb-empty">
              <i class="iconfont icon-movie" :class="{ spinning: row.rendering }"></i>
              <span>{{ row.rendering ? t('common.loading') : t('upload.noPreview') }}</span>
            </div>
            <span class="staged-format">{{ (row.fileType || '').toUpperCase() }}</span>
          </div>

          <div class="staged-main">
            <div class="staged-title">
              <span class="staged-name" :title="row.fileName">{{ row.name || row.fileName }}</span>
              <el-tag size="small" type="info">{{ formatSize(row.fileSize) }}</el-tag>
              <span v-if="row.animationCount" class="staged-meta">
                <i class="iconfont icon-movie"></i> {{ row.animationCount }}
              </span>
              <span v-if="dimText(row)" class="staged-meta">{{ dimText(row) }}</span>
            </div>
            <div class="staged-sub" :title="row.fileName">{{ row.fileName }}</div>

            <div class="staged-aux">
              <span
                v-for="file in visibleAux(row)"
                :key="`${file.role}-${file.name}`"
                class="aux-chip"
                :class="file.role"
              >
                <i
                  class="iconfont"
                  :class="file.role === 'texture' ? 'icon-image' : 'icon-movie'"
                ></i>
                <span class="aux-name" :title="file.name">{{ file.name }}</span>
                <span class="aux-remove" @click.stop="removeAux(row, file)">×</span>
              </span>
              <template v-if="isJsonRow(row)">
                <el-button text size="small" class="aux-add" @click.stop="pickAuxForRow(row, 'texture')">
                  <i class="iconfont icon-plus"></i>{{ t('upload.addTexture') }}
                </el-button>
                <el-button text size="small" class="aux-add" @click.stop="pickAuxForRow(row, 'animation')">
                  <i class="iconfont icon-plus"></i>{{ t('upload.addAnimation') }}
                </el-button>
                <span v-if="!visibleAux(row).length" class="aux-hint">
                  {{ t('upload.dropAuxRow') }}
                </span>
              </template>
            </div>
          </div>

          <div class="staged-actions">
            <el-button text size="small" @click.stop="toggleExpand(row)">
              <i class="iconfont icon-chevron-down chevron" :class="{ open: row.expanded }"></i>
              {{ row.expanded ? t('upload.collapseInfo') : t('upload.editInfo') }}
            </el-button>
            <el-button
              text
              size="small"
              :disabled="row.rendering"
              :title="t('upload.refreshThumb')"
              @click.stop="refreshRow(row)"
            >
              <i class="iconfont icon-repeat"></i>
            </el-button>
            <el-button
              v-if="!isEdit"
              text
              size="small"
              class="remove-btn"
              :title="t('common.delete')"
              @click.stop="removeRow(row)"
            >
              <i class="iconfont icon-trash-alt"></i>
            </el-button>
          </div>

          <div v-if="row.expanded" class="staged-detail" @click.stop>
            <el-form label-position="top" class="detail-form">
              <div class="detail-grid">
                <el-form-item :label="t('upload.modelName')" required>
                  <el-input v-model="row.name" :placeholder="t('upload.modelName')" />
                </el-form-item>
                <el-form-item :label="t('common.tags')">
                  <TagInput v-model="row.tags" :pool="state.tags" />
                </el-form-item>
              </div>
              <el-form-item :label="t('upload.description')">
                <el-input
                  v-model="row.description"
                  type="textarea"
                  :rows="2"
                  :placeholder="t('upload.descriptionPlaceholder')"
                />
              </el-form-item>
            </el-form>
          </div>
        </div>
      </div>

      <el-alert v-if="errorMsg" :title="errorMsg" type="error" show-icon :closable="false" />
    </div>

    <footer class="upload-footer">
      <span class="footer-tip">{{ t('upload.onlyOnSubmit') }}</span>
      <span class="footer-count">
        {{ t('folders.uploadTo', { name: folderStore.activeFolder?.name || t('folders.allFiles') }) }}
      </span>
      <span class="footer-count">{{ t('upload.stagedCount', { count: rows.length }) }}</span>
      <el-button @click="handleBack">{{ t('common.cancel') }}</el-button>
      <el-button
        type="primary"
        size="large"
        :loading="saving"
        :disabled="!rows.length || loadingModel"
        @click="handleSubmit"
      >
        {{ isEdit ? t('common.save') : t('upload.uploadAll') }}
      </el-button>
    </footer>

    <div v-if="showDropOverlay" class="drop-overlay">
      <div class="drop-overlay-inner">
        <i class="iconfont icon-cloud-upload"></i>
        <p>{{ t('common.dropHere') }}</p>
        <span>{{ t('upload.dropFormats') }}</span>
      </div>
    </div>

    <!-- 离屏渲染缩略图 -->
    <div class="hidden-viewer" v-if="viewerVisible">
      <ModelViewer
        :key="viewerKey"
        ref="viewerRef"
        :buffer="viewerBuffer"
        :file-type="viewerType"
        :aux-files="viewerAux"
        :view-params="viewerParams"
        @loaded="onViewerLoaded"
        @error="onViewerError"
      />
    </div>
  </div>
</template>

<style scoped>
.upload-page {
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
.upload-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px 24px;
  overflow: hidden;
}
.dropzone {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 18px 20px;
  border: 1px dashed var(--border);
  border-radius: var(--radius-lg);
  background: var(--bg-soft);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.dropzone.compact {
  padding: 12px 18px;
}
.dropzone:hover,
.dropzone.drag-active {
  border-color: var(--primary);
  background: var(--primary-soft);
}
.dropzone-icon {
  font-size: 38px;
  color: var(--text-3);
  flex-shrink: 0;
}
.dropzone.compact .dropzone-icon {
  font-size: 26px;
}
.dropzone-text {
  flex: 1;
  min-width: 0;
}
.dropzone-title {
  margin: 0 0 4px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.dropzone-sub {
  font-size: 12px;
  color: var(--text-3);
}
.staged-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-right: 4px;
}
.staged-row {
  position: relative;
  flex-shrink: 0;
  display: grid;
  grid-template-columns: 104px minmax(0, 1fr) auto;
  gap: 14px;
  align-items: start;
  padding: 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-soft);
  transition: border-color 0.15s, background 0.15s;
}
.staged-row.active {
  border-color: var(--primary);
}
.staged-row.drag-over {
  border-color: var(--primary);
  background: var(--primary-soft);
  box-shadow: inset 0 0 0 1px var(--primary);
}
.staged-thumb {
  position: relative;
  width: 104px;
  height: 78px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--bg-mute);
  display: flex;
  align-items: center;
  justify-content: center;
}
.staged-thumb img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.staged-thumb-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  font-size: 10px;
  color: var(--text-3);
  text-align: center;
  padding: 0 4px;
}
.staged-thumb-empty .iconfont {
  font-size: 18px;
}
.staged-thumb-empty .spinning {
  animation: spin 1s linear infinite;
  color: var(--primary);
}
.staged-format {
  position: absolute;
  left: 4px;
  bottom: 4px;
  padding: 0 5px;
  font-size: 9px;
  line-height: 14px;
  color: #fff;
  background: rgba(0, 0, 0, 0.55);
  border-radius: 3px;
  letter-spacing: 0.4px;
}
.staged-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.staged-title {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  min-width: 0;
}
.staged-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 360px;
}
.staged-meta {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  color: var(--text-3);
}
.staged-meta .iconfont {
  font-size: 12px;
}
.staged-sub {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.staged-aux {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 2px;
}
.aux-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 240px;
  padding: 1px 6px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--bg-panel);
  font-size: 11px;
  color: var(--text-2);
}
.aux-chip.texture .iconfont {
  color: var(--primary);
}
.aux-chip.animation .iconfont {
  color: var(--success);
}
.aux-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.aux-remove {
  cursor: pointer;
  color: var(--text-3);
  font-size: 12px;
  line-height: 1;
}
.aux-remove:hover {
  color: var(--danger);
}
.aux-add {
  height: auto !important;
  padding: 0 4px !important;
  font-size: 11px;
}
.aux-add .iconfont {
  font-size: 11px;
  margin-right: 2px;
}
.aux-hint {
  font-size: 11px;
  color: var(--text-3);
}
.staged-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.staged-actions :deep(.el-button) {
  padding: 4px 6px;
}
.chevron {
  font-size: 12px;
  transition: transform 0.15s;
  margin-right: 2px;
}
.chevron.open {
  transform: rotate(180deg);
}
.remove-btn {
  color: var(--danger) !important;
}
.staged-detail {
  grid-column: 1 / -1;
  margin-top: 4px;
  padding-top: 10px;
  border-top: 1px dashed var(--border-soft);
}
.detail-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}
.detail-form :deep(.el-form-item) {
  margin-bottom: 10px;
}
.detail-form :deep(.el-form-item__label) {
  line-height: 1.4;
  padding-bottom: 4px;
}
.detail-form :deep(.el-input__wrapper),
.detail-form :deep(.el-textarea__inner) {
  width: 100%;
  box-sizing: border-box;
}
.upload-footer {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 24px;
  border-top: 1px solid var(--border-soft);
  background: var(--bg);
  position: sticky;
  bottom: 0;
  z-index: 2;
}
.footer-tip {
  margin-right: auto;
  font-size: 12px;
  color: var(--text-3);
}
.footer-count {
  font-size: 12px;
  color: var(--text-2);
}
.drop-overlay {
  position: fixed;
  inset: 0;
  z-index: 9998;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(3px);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drop-overlay-inner {
  text-align: center;
  color: #fff;
}
.drop-overlay-inner .iconfont {
  display: block;
  font-size: 56px;
  color: var(--primary);
  margin-bottom: 14px;
}
.drop-overlay-inner p {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}
.drop-overlay-inner span {
  display: block;
  margin-top: 6px;
  font-size: 12px;
  opacity: 0.75;
}
.hidden-viewer {
  position: fixed;
  left: -9999px;
  top: 0;
  width: 480px;
  height: 360px;
  pointer-events: none;
  z-index: -1;
  opacity: 0;
}
@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
