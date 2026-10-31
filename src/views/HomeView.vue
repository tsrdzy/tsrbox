<script setup>
import { ref, computed, shallowRef, reactive, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { ElMessage, ElMessageBox } from 'element-plus'

import { useStore } from '../composables/useStore'
import { useDragUpload } from '../composables/useDragUpload'
import { useResourceShortcuts } from '../composables/useResourceShortcuts'
import { useNativeDrag } from '../composables/useNativeDrag'
import { askExportFormat } from '../composables/exportFormatDialog'
import { useBoxSelect } from '../composables/useBoxSelect'
import { useUploadStore } from '../stores/upload'
import { useFolderStore } from '../stores/folders'
import { useTagFilterStore } from '../stores/tagFilter'
import ModelCard from '../components/ModelCard.vue'

// 异步加载 3D 预览组件，three.js 不进入首屏主包
const ModelViewer = defineAsyncComponent(() => import('../components/ModelViewer.vue'))

const { state, goUpload, goEdit, closeLibrary, renameLibrary, batchUpload, loadAll, deleteModel, saveSettings } = useStore()
const { t } = useI18n()
const uploadStore = useUploadStore()
const folderStore = useFolderStore()
const tagFilter = useTagFilterStore()

const keyword = ref('')
const sortBy = ref('uploadTime') // uploadTime | fileSize | name | dimension
const sortDir = ref('desc') // asc | desc

// ===== 筛选条件 =====
const filterFormats = ref([]) // ['bbmodel', 'json', 'vox']
const filterAnimation = ref('all') // all | with | without
const filterSizeMin = ref(undefined) // MB
const filterSizeMax = ref(undefined) // MB
const filterDateRange = ref(null) // [Date, Date]

const activeFilterCount = computed(() => {
  let count = 0
  if (filterFormats.value.length) count++
  if (filterAnimation.value !== 'all') count++
  if (filterSizeMin.value != null || filterSizeMax.value != null) count++
  if (filterDateRange.value && filterDateRange.value.length === 2) count++
  return count
})

function resetFilters() {
  filterFormats.value = []
  filterAnimation.value = 'all'
  filterSizeMin.value = undefined
  filterSizeMax.value = undefined
  filterDateRange.value = null
}

const selectedIds = ref([]) // 选中的模型ID列表
const isMultiSelect = ref(false) // 是否处于多选模式
const anchorId = ref(null) // shift连续选择的基准ID
const previewSize = ref(state.library?.settings?.modelPreviewSize || 5)
const gridMinWidth = computed(() => 100 + (previewSize.value - 1) * 30)
const displaySettings = reactive(state.library?.settings?.modelDisplay || { name: true, tags: true, dimensions: true, fileSize: true, uploadTime: true, fileType: true })

function onPreviewSizeChange() {
  saveSettings({ modelPreviewSize: previewSize.value })
}
function onDisplayChange() {
  saveSettings({ modelDisplay: { ...displaySettings } })
}
const allFieldsSelected = computed({
  get() {
    return Object.values(displaySettings).every(v => v)
  },
  set(val) {
    Object.keys(displaySettings).forEach(key => {
      displaySettings[key] = val
    })
    onDisplayChange()
  }
})
const isIndeterminate = computed(() => {
  const values = Object.values(displaySettings)
  return values.some(v => v) && !values.every(v => v)
})
function onSelectAll(val) {
  allFieldsSelected.value = val
}

// 框选相关
const contentRef = ref(null)
const selectionBoxRef = ref(null)
const { isDragging, showSelectionBox, handleMouseDown, consumeBoxSelect } = useBoxSelect({
  contentRef,
  selectionBoxRef,
  cardSelector: '.card',
  idAttr: 'data-model-id',
  selectedIds,
  isMultiSelect,
  anchorId
})

/** 资源库名字下拉菜单命令处理 */
async function handleLibCommand(cmd) {
  if (cmd === 'create') {
    window.location.hash = '#/select-library?tab=create'
  } else if (cmd === 'switch') {
    window.location.hash = '#/select-library?tab=open'
  } else if (cmd === 'close') {
    closeLibrary()
    window.location.hash = '#/select-library'
  } else if (cmd === 'rename') {
    try {
      const { value } = await ElMessageBox.prompt(t('init.libraryName'), t('init.rename'), {
        inputValue: state.library?.name || '',
        confirmButtonText: t('common.ok'),
        cancelButtonText: t('common.cancel'),
        inputValidator: (v) => (v && v.trim() ? true : t('common.name') + t('common.empty'))
      })
      await renameLibrary(value.trim())
      ElMessage.success(t('common.ok'))
    } catch (e) {
      // 用户取消
    }
  }
}

const filteredModels = computed(() => {
  let list = state.models.slice()

  const q = keyword.value.trim().toLowerCase()
  if (q) {
    list = list.filter((m) => {
      return (
        (m.name && m.name.toLowerCase().includes(q)) ||
        (m.description && m.description.toLowerCase().includes(q)) ||
        (m.tags && m.tags.some((t) => t.toLowerCase().includes(q)))
      )
    })
  }

  // 标签筛选（标签选择在侧边栏）
  const activeTags = tagFilter.selectedTags
  if (activeTags.length) {
    list = list.filter((m) => activeTags.some((t) => m.tags && m.tags.includes(t)))
  }

  // 当前文件夹（空字符串 = 全部文件）
  const folderId = folderStore.activeFolderId
  if (folderId) {
    list = list.filter((m) => String(m.folderId || '') === folderId)
  }

  // 文件格式
  if (filterFormats.value.length) {
    list = list.filter((m) =>
      filterFormats.value.includes(String(m.fileType || '').toLowerCase())
    )
  }

  // 是否含动画
  if (filterAnimation.value === 'with') {
    list = list.filter((m) => Number(m.animationCount || 0) > 0)
  } else if (filterAnimation.value === 'without') {
    list = list.filter((m) => !Number(m.animationCount || 0))
  }

  // 文件大小区间（MB）
  if (filterSizeMin.value != null) {
    list = list.filter((m) => Number(m.fileSize || 0) >= filterSizeMin.value * 1024 * 1024)
  }
  if (filterSizeMax.value != null) {
    list = list.filter((m) => Number(m.fileSize || 0) <= filterSizeMax.value * 1024 * 1024)
  }

  // 上传时间区间
  if (filterDateRange.value && filterDateRange.value.length === 2) {
    const toTime = (value, isEnd) => {
      const [year, month, day] = String(value).split('-').map(Number)
      if (!year || !month || !day) return isEnd ? Infinity : -Infinity
      return new Date(
        year,
        month - 1,
        day,
        isEnd ? 23 : 0,
        isEnd ? 59 : 0,
        isEnd ? 59 : 0,
        isEnd ? 999 : 0
      ).getTime()
    }
    const start = toTime(filterDateRange.value[0], false)
    const end = toTime(filterDateRange.value[1], true)
    list = list.filter((m) => {
      const time = new Date(m.uploadTime || 0).getTime()
      return time >= start && time <= end
    })
  }

  const dir = sortDir.value === 'asc' ? 1 : -1
  list.sort((a, b) => {
    let av, bv
    switch (sortBy.value) {
      case 'fileSize':
        av = a.fileSize || 0
        bv = b.fileSize || 0
        break
      case 'name':
        av = (a.name || '').toLowerCase()
        bv = (b.name || '').toLowerCase()
        return av.localeCompare(bv) * dir
      case 'dimension':
        av = a.dimensions ? (a.dimensions.x || 0) * (a.dimensions.y || 0) * (a.dimensions.z || 0) : 0
        bv = b.dimensions ? (b.dimensions.x || 0) * (b.dimensions.y || 0) * (b.dimensions.z || 0) : 0
        break
      default:
        av = new Date(a.uploadTime || 0).getTime()
        bv = new Date(b.uploadTime || 0).getTime()
    }
    return (av - bv) * dir
  })

  return list
})

/** 排序下拉菜单命令处理 */
function handleSortCommand(cmd) {
  if (cmd === 'asc' || cmd === 'desc') {
    sortDir.value = cmd
  } else {
    sortBy.value = cmd
  }
}

function isSelected(id) {
  const idStr = String(id)
  return selectedIds.value.some((sid) => String(sid) === idStr)
}

const selectedModels = computed(() => filteredModels.value.filter((m) => isSelected(m.id)))

const emptyDescription = computed(() => {
  if (!state.models.length) return t('home.noModel')
  if (folderStore.activeFolderId) {
    const hasInFolder = state.models.some(
      (m) => String(m.folderId || '') === folderStore.activeFolderId
    )
    if (!hasInFolder) return t('folders.emptyFolder')
  }
  return t('home.noMatch')
})

// ===== 多级文件夹导航 =====
const folderPath = computed(() => folderStore.folderPath(folderStore.activeFolderId))
const childFolders = computed(() => {
  if (!folderStore.activeFolderId) return []
  const counts = {}
  for (const model of state.models) {
    const id = String(model.folderId || '')
    if (id) counts[id] = (counts[id] || 0) + 1
  }
  return folderStore.childrenOf(folderStore.activeFolderId).map((folder) => ({
    ...folder,
    modelCount: counts[folder.id] || 0
  }))
})
const showChildFolders = ref(localStorage.getItem('tsrbox-show-child-folders') !== '0')

function toggleChildFolders() {
  showChildFolders.value = !showChildFolders.value
  localStorage.setItem('tsrbox-show-child-folders', showChildFolders.value ? '1' : '0')
}

function enterFolder(id) {
  folderStore.setActiveFolder(id || '')
  clearSelection()
}

const { onCardMouseDown, shouldSuppressClick } = useNativeDrag({
  type: 'models',
  folderSelector: '[data-folder-drop]',
  onFolderHover: (key) => {
    folderStore.dragOverKey = key
  },
  onFolderDrop: (key, ids) => {
    folderStore.dragOverKey = ''
    moveModelsToFolder(ids, key === '__all__' ? '' : key)
  }
})

/** 根据窗口内指针坐标判断是否落在侧边栏文件夹上 */
function folderKeyAt(position) {
  if (!position) return ''
  const ratio = window.devicePixelRatio || 1
  for (const scale of [1 / ratio, 1]) {
    const el = document.elementFromPoint(position.x * scale, position.y * scale)
    const folderEl = el?.closest?.('[data-folder-drop]')
    if (folderEl) return folderEl.getAttribute('data-folder-drop') || ''
  }
  return ''
}

/** 批量移动模型到文件夹；folderId 为空表示移回「全部文件」 */
async function moveModelsToFolder(ids, folderId) {
  const idList = (ids || []).map((id) => String(id))
  if (!idList.length) return
  try {
    const moved = await folderStore.moveModels(idList, folderId)
    await loadAll()
    clearSelection()
    if (moved > 0) {
      const name = folderStore.folders.find((f) => f.id === folderId)?.name || ''
      ElMessage.success(
        folderId
          ? t('folders.movedTo', { count: moved, name })
          : t('folders.movedOut', { count: moved })
      )
    }
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

function toggleSelect(model) {
  const modelId = String(model.id)
  const idx = selectedIds.value.findIndex((sid) => String(sid) === modelId)
  if (idx === -1) {
    selectedIds.value.push(model.id)
  } else {
    selectedIds.value.splice(idx, 1)
  }
  isMultiSelect.value = selectedIds.value.length > 0
}

function handleSelect(model, e) {
  if (shouldSuppressClick()) return
  const event = e || window.event
  if (!event) {
    selectedIds.value = [model.id]
    isMultiSelect.value = true
    anchorId.value = String(model.id)
    return
  }

  const modelId = String(model.id)
  if (event.ctrlKey || event.metaKey) {
    toggleSelect(model)
    if (!anchorId.value) {
      anchorId.value = modelId
    }
  } else if (event.shiftKey) {
    const models = filteredModels.value
    const currentIdx = models.findIndex((m) => String(m.id) === modelId)
    if (currentIdx === -1) return
    if (!anchorId.value || selectedIds.value.length === 0) {
      anchorId.value = modelId
      selectedIds.value = [model.id]
      isMultiSelect.value = true
    } else {
      const anchorIdx = models.findIndex((m) => String(m.id) === anchorId.value)
      if (anchorIdx === -1) {
        anchorId.value = modelId
        selectedIds.value = [model.id]
        isMultiSelect.value = true
        return
      }
      const start = Math.min(currentIdx, anchorIdx)
      const end = Math.max(currentIdx, anchorIdx)
      const newSelected = []
      for (let i = start; i <= end; i++) {
        newSelected.push(models[i].id)
      }
      selectedIds.value = newSelected
      isMultiSelect.value = true
    }
  } else {
    selectedIds.value = [model.id]
    isMultiSelect.value = true
    anchorId.value = modelId
  }
}

function handleDblClick(model) {
  if (selectedIds.value.length === 0 || selectedIds.value.length === 1 && selectedIds.value[0] === model.id) {
    goEdit(model)
  }
}

function clearSelection() {
  selectedIds.value = []
  isMultiSelect.value = false
  anchorId.value = null
}

function selectAll() {
  const models = filteredModels.value
  selectedIds.value = models.map((m) => m.id)
  isMultiSelect.value = selectedIds.value.length > 0
  if (selectedIds.value.length > 0) {
    anchorId.value = String(models[0].id)
  }
}

function selectInverse() {
  const models = filteredModels.value
  const allIds = new Set(models.map((m) => String(m.id)))
  const selectedSet = new Set(selectedIds.value.map((id) => String(id)))
  const inverseIds = []
  allIds.forEach((id) => {
    if (!selectedSet.has(id)) {
      const model = models.find((m) => String(m.id) === id)
      if (model) inverseIds.push(model.id)
    }
  })
  selectedIds.value = inverseIds
  isMultiSelect.value = selectedIds.value.length > 0
  if (selectedIds.value.length > 0) {
    anchorId.value = String(selectedIds.value[0])
  } else {
    anchorId.value = null
  }
}

async function handleBatchDelete() {
  if (selectedIds.value.length === 0) return
  try {
    await ElMessageBox.confirm(
      t('home.confirmBatchDelete', { count: selectedIds.value.length }),
      t('home.batchDelete'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        type: 'warning',
        confirmButtonClass: 'el-button--danger'
      }
    )
  } catch {
    return
  }
  for (const id of selectedIds.value) {
    try {
      await deleteModel(String(id))
    } catch (e) {
      console.error(t('home.deletionFailed') + ':', id, e)
    }
  }
  await loadAll()
  clearSelection()
  ElMessage.success(t('home.deletionSuccess'))
}

async function handleBatchAddTag() {
  if (selectedIds.value.length === 0) return
  try {
    const { value } = await ElMessageBox.prompt(t('home.enterTag'), t('home.batchAddTag'), {
      confirmButtonText: t('common.ok'),
      cancelButtonText: t('common.cancel'),
      inputValidator: (v) => (v && v.trim() ? true : t('home.enterTag'))
    })
    const tag = value.trim()
    for (const id of selectedIds.value) {
      const model = state.models.find((m) => m.id === id)
      if (model) {
        if (!model.tags) model.tags = []
        if (!model.tags.includes(tag)) {
          model.tags.push(tag)
          try {
            const patch = { tags: [...model.tags] }
            await window.api.models.update(String(id), patch)
          } catch (e) {
            console.error(t('home.batchAddTag') + t('common.failed') + ':', id, e)
          }
        }
      }
    }
    await loadAll()
    ElMessage.success(t('home.tagsAdded', { count: selectedIds.value.length }))
  } catch (e) {
    // 用户取消
  }
}

function handleContentClick(e) {
  if (consumeBoxSelect()) return
  // 多选操作条浮在内容区上方，点击它不应清空选择
  if (e.target.closest('.batch-actions')) return
  if (e.target === contentRef.value || (e.target.closest('.content') === contentRef.value && !e.target.closest('.card'))) {
    clearSelection()
  }
}

/** 内容区 mousedown：点在多选操作条上不启动框选 */
function onContentMouseDown(e) {
  if (e.target.closest('.batch-actions')) return
  handleMouseDown(e)
}

// ===== 批量上传（后台上传任务，不阻塞界面） =====
// 隐藏 ModelViewer 用于生成封面
const batchViewerVisible = ref(false)
const batchRenderKey = ref(0)
const currentBatchBuffer = shallowRef(null)
const currentBatchType = ref('')
const currentBatchAux = ref([])
const currentBatchViewParams = ref(null)
const batchViewerRef = ref(null)

let pendingResolve = null
let pendingReject = null
let renderTimeout = null

function base64ToUint8Array(base64) {
  const binary = atob(base64)
  const len = binary.length
  const bytes = new Uint8Array(len)
  for (let i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

function onBatchModelLoaded(payload) {
  if (!pendingResolve) return
  // 等 2 帧确保渲染完成
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      let cover = null
      try {
        cover = batchViewerRef.value?.captureThumbnail() || null
      } catch (e) {
        console.warn('封面截图失败:', e)
      }
      const result = {
        dimensions: payload.dimensions || { x: 0, y: 0, z: 0 },
        animationCount: payload.animationCount || 0,
        cover
      }
      const resolve = pendingResolve
      pendingResolve = null
      pendingReject = null
      if (renderTimeout) {
        clearTimeout(renderTimeout)
        renderTimeout = null
      }
      resolve(result)
    })
  })
}

function onBatchModelError(err) {
  console.warn('[BatchUpload] 模型加载失败:', err)
  if (!pendingReject) return
  const reject = pendingReject
  pendingResolve = null
  pendingReject = null
  if (renderTimeout) {
    clearTimeout(renderTimeout)
    renderTimeout = null
  }
  reject(err)
}

/** 用隐藏的 ModelViewer 加载模型生成封面 */
function renderModelForCover(base64, fileType, auxFiles = [], viewParams = null) {
  return new Promise((resolve, reject) => {
    pendingResolve = resolve
    pendingReject = reject
    const buffer = base64ToUint8Array(base64)
    // 通过 key 变化强制重新挂载 ModelViewer
    batchRenderKey.value++
    currentBatchBuffer.value = buffer
    currentBatchType.value = fileType
    currentBatchAux.value = auxFiles || []
    currentBatchViewParams.value = viewParams || null
    batchViewerVisible.value = true
    // 超时保护（30s）
    renderTimeout = setTimeout(() => {
      if (pendingReject) {
        pendingReject(new Error('模型渲染超时'))
        pendingResolve = null
        pendingReject = null
      }
    }, 30000)
  })
}

// ===== 右键菜单 =====
const cardMenu = ref(null)
const refreshingCoverId = ref(null)

function handleCardContextMenu(event, model) {
  event.preventDefault()
  // 右键未选中项时，先把它设为当前选择
  if (!isSelected(model.id)) {
    selectedIds.value = [model.id]
    isMultiSelect.value = true
    anchorId.value = String(model.id)
  }
  const x = Math.min(event.clientX, window.innerWidth - 230)
  const y = Math.min(event.clientY, window.innerHeight - 300)
  cardMenu.value = { x, y, model, foldersOpen: false }
}

function closeCardMenu() {
  cardMenu.value = null
}

/** 右键菜单当前作用对象（多选时作用于选中集合） */
const menuTargets = computed(() => {
  if (!cardMenu.value) return []
  const model = cardMenu.value.model
  if (selectedIds.value.length > 1 && isSelected(model.id)) {
    return filteredModels.value.filter((m) => isSelected(m.id))
  }
  return [model]
})

function handleMenuOpenDetail() {
  const model = cardMenu.value?.model
  closeCardMenu()
  if (model) goEdit(model)
}

async function handleMenuRename() {
  const model = cardMenu.value?.model
  closeCardMenu()
  if (!model) return
  try {
    const { value } = await ElMessageBox.prompt(t('home.enterName'), t('home.renameModel'), {
      inputValue: model.name || '',
      confirmButtonText: t('common.ok'),
      cancelButtonText: t('common.cancel'),
      inputValidator: (v) => (v && v.trim() ? true : t('home.enterName'))
    })
    await window.api.models.update(String(model.id), { name: value.trim() })
    await loadAll()
    ElMessage.success(t('home.renamed'))
  } catch {
    // 用户取消
  }
}

/** 为单个模型重新渲染封面 */
async function refreshCoverForModel(target) {
  refreshingCoverId.value = String(target.id)
  try {
    const res = await window.api.models.loadFile({
      id: String(target.id),
      fileName: String(target.fileName),
      fileType: String(target.fileType),
      files: JSON.parse(JSON.stringify(target.files || []))
    })
    const rendered = await renderModelForCover(
      res.dataBase64,
      res.fileType,
      res.auxFiles || [],
      null
    )
    if (!rendered.cover) throw new Error(t('home.refreshCoverFailed'))
    await window.api.models.saveImage(String(target.id), 'cover', rendered.cover)
    target.coverBase64 = rendered.cover
    target.cover = true
  } finally {
    batchViewerVisible.value = false
    currentBatchBuffer.value = null
    currentBatchAux.value = []
    currentBatchViewParams.value = null
    refreshingCoverId.value = null
  }
}

async function handleMenuRefreshCovers() {
  const list = menuTargets.value.slice()
  closeCardMenu()
  if (!list.length || pendingResolve || refreshingCoverId.value) return

  const taskId = uploadStore.begin({
    title: t('home.refreshCover'),
    type: 'models',
    total: list.length
  })
  let success = 0
  let failed = 0
  for (let i = 0; i < list.length; i++) {
    uploadStore.update(taskId, {
      current: i + 1,
      total: list.length,
      fileName: list[i].name || list[i].fileName || '',
      progress: Math.round(((i + 1) / list.length) * 100)
    })
    try {
      await refreshCoverForModel(list[i])
      success++
    } catch (e) {
      failed++
      console.warn('[Home] 预览图刷新失败:', list[i].fileName, e)
    }
  }
  uploadStore.finish(taskId, { success, failed })
  if (failed > 0) {
    ElMessage.warning(t('home.refreshCoverPartial', { success, failed }))
  } else {
    ElMessage.success(t('home.refreshCoverDone'))
  }
}

function handleMenuAddTag() {
  closeCardMenu()
  handleBatchAddTag()
}

async function handleMenuMove(folderId) {
  const ids = menuTargets.value.map((m) => String(m.id))
  closeCardMenu()
  await moveModelsToFolder(ids, folderId)
}

async function handleMenuExport() {
  const list = menuTargets.value.slice()
  closeCardMenu()
  await exportModels(list)
}

async function exportModels(list) {
  if (!list?.length) return
  const format = await askExportFormat(state.library?.settings?.exportFormat || 'original')
  if (!format) return
  try {
    if (list.length === 1) {
      const model = list[0]
      const res = await window.api.models.export(
        {
          id: String(model.id),
          name: String(model.name || model.fileName || 'model'),
          fileName: String(model.fileName)
        },
        format
      )
      if (!res) return
      ElMessage.success(t('upload.exportCount', { count: res.fileCount, dir: res.targetDir }))
      if (res.warnings?.length) {
        ElMessage.warning(res.warnings[0])
      }
      return
    }
    const res = await window.api.models.exportBatch(
      list.map((m) => ({ id: String(m.id), name: String(m.name || ''), fileName: String(m.fileName) })),
      format
    )
    if (!res) return
    if (res.failed?.length) {
      ElMessage.warning(t('folders.exportBatchPartial', { success: res.count, failed: res.failed.length }))
    } else {
      ElMessage.success(t('folders.exportBatchDone', { count: res.count, dir: res.targetDir }))
    }
    if (res.warnings?.length) {
      ElMessage.warning(res.warnings[0].message)
    }
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

function handleMenuDelete() {
  closeCardMenu()
  handleBatchDelete()
}

async function handleBatchUpload() {
  const taskId = uploadStore.begin({ title: '批量上传模型', type: 'models' })
  const update = (patch) => uploadStore.update(taskId, patch)

  try {
    // 阶段 1：复制文件（占 0-40%）
    const res = await batchUpload((data) => {
      update({
        current: data.current,
        total: data.total,
        fileName: data.fileName,
        progress: data.total > 0 ? Math.round((data.current / data.total) * 40) : 0
      })
    })

    if (!res) {
      // 用户取消选择
      uploadStore.finish(taskId, { success: 0, failed: 0, message: '已取消' })
      return
    }

    // 分类：正常项 / 重复项 / 跳过项 / 错误项
    const normalItems = res.items.filter((it) => !it.error && !it.duplicate && !it.skipped)
    const duplicateItems = res.items.filter((it) => it.duplicate)
    const skippedItems = res.items.filter((it) => it.skipped)
    const errorItems = res.items.filter((it) => it.error)

    // 询问用户是否覆盖重复项
    let overwriteDuplicates = false
    if (duplicateItems.length > 0) {
      try {
        await ElMessageBox.confirm(
          `发现 ${duplicateItems.length} 个重复模型，是否覆盖已有模型？\n覆盖将删除旧模型及其封面、标签等信息。`,
          '发现重复模型',
          {
            confirmButtonText: '全部覆盖',
            cancelButtonText: '全部跳过',
            type: 'warning'
          }
        )
        overwriteDuplicates = true
      } catch (action) {
        overwriteDuplicates = false
      }
    }

    // 阶段 2：渲染封面 + 提交（占 40-100%）
    // 待处理列表：正常项 + （若覆盖则重复项经 overwrite 后变为正常项）
    const processItems = [...normalItems]
    const results = []

    // 处理需要覆盖的重复项：调用 overwrite 得到新模型数据
    if (overwriteDuplicates) {
      for (const dup of duplicateItems) {
        try {
          const overwritten = await window.api.models.overwrite(
            dup.existingModel,
            dup.pendingFile
          )
          overwritten.folderId =
            dup.existingModel?.folderId || folderStore.activeFolderId || ''
          processItems.push(overwritten)
        } catch (e) {
          console.error('[BatchUpload] 覆盖失败:', dup.defaultName, e)
          results.push({ success: false, name: dup.defaultName, error: '覆盖失败：' + (e.message || '') })
        }
      }
    } else {
      // 不覆盖：将重复项记为跳过
      for (const dup of duplicateItems) {
        results.push({ success: false, name: dup.defaultName, error: '已跳过（重复）' })
      }
    }

    // 批内重复与错误项直接记入结果
    for (const sk of skippedItems) {
      results.push({ success: false, name: sk.defaultName, error: '已跳过（批次内重复）' })
    }
    for (const err of errorItems) {
      results.push({ success: false, name: err.defaultName, error: err.message })
    }

    const total = processItems.length
    for (let i = 0; i < total; i++) {
      const item = processItems[i]
      update({
        current: i + 1,
        total,
        fileName: item.defaultName,
        progress: 40 + Math.round((i / total) * 60)
      })

      let dimensions = { x: 0, y: 0, z: 0 }
      let animCount = 0
      let cover = null

      try {
        const rendered = await renderModelForCover(item.dataBase64, item.fileType)
        dimensions = rendered.dimensions
        animCount = rendered.animationCount || 0
        cover = rendered.cover
      } catch (e) {
        console.warn('[BatchUpload] 渲染失败，跳过封面:', item.defaultName, e)
      }

      // 关闭隐藏 viewer 节省资源（下一个模型会重新挂载）
      batchViewerVisible.value = false
      currentBatchBuffer.value = null

      try {
        // 保存封面
        let coverField = null
        if (cover) {
          await window.api.models.saveImage(String(item.id), 'cover', cover)
          coverField = 'cover.png'
        }

        // 提交模型到对应分片
        const meta = {
          id: item.id,
          name: item.defaultName,
          description: '',
          fileName: String(item.fileName),
          fileType: String(item.fileType),
          fileSize: Number(item.fileSize),
          files: JSON.parse(JSON.stringify(item.files)),
          dimensions: {
            x: Number(dimensions.x) || 0,
            y: Number(dimensions.y) || 0,
            z: Number(dimensions.z) || 0
          },
          animationCount: Number(animCount) || 0,
          uploadTime: String(item.uploadTime),
          cover: coverField,
          viewParams: null,
          tags: [],
          folderId: item.folderId || folderStore.activeFolderId || ''
        }
        await window.api.models.save(meta)
        results.push({ success: true, name: item.defaultName, id: item.id, hasCover: !!cover })
      } catch (e) {
        console.error('[BatchUpload] 提交失败:', item.defaultName, e)
        results.push({ success: false, name: item.defaultName, error: e.message || '提交失败' })
      }
    }

    batchViewerVisible.value = false

    const succ = results.filter((r) => r.success).length
    const failed = results.length - succ
    uploadStore.finish(taskId, {
      success: succ,
      failed,
      details: results.map((r) => ({ name: r.name, success: r.success, error: r.error }))
    })

    // 刷新模型列表
    await loadAll()

    if (res.total > 0 && succ === res.total) {
      ElMessage.success(`成功上传 ${succ} 个模型`)
    } else if (succ > 0) {
      ElMessage.warning(`完成：成功 ${succ} / 失败 ${res.total - succ}`)
    } else if (results.length > 0) {
      ElMessage.error('全部上传失败')
    }
  } catch (e) {
    uploadStore.finish(taskId, { success: 0, failed: 0, error: true, message: e.message || '批量上传失败' })
    ElMessage.error(e.message || '批量上传失败')
    batchViewerVisible.value = false
  }
}

// ===== 拖拽上传（仅 BBMODEL / VOX；JSON 请走专属上传页）=====
const MODEL_EXTS = ['bbmodel', 'vox']

async function handleDragUpload(filePaths) {
  if (filePaths.some((path) => /\.json$/i.test(String(path)))) {
    ElMessage.warning(t('upload.jsonDragNotAllowed'))
    return
  }

  const taskId = uploadStore.begin({ title: '拖拽上传模型', type: 'models' })
  const update = (patch) => uploadStore.update(taskId, patch)

  let unsubscribe = null
  unsubscribe = window.api.models.onBatchUploadProgress((data) => {
    update({
      current: data.current,
      total: data.total,
      fileName: data.fileName,
      progress: data.total > 0 ? Math.round((data.current / data.total) * 40) : 0
    })
  })

  try {
    const res = await window.api.models.batchUploadByPaths(filePaths)
    if (unsubscribe) unsubscribe()

    if (res.invalidFormat) {
      ElMessage.warning(t('common.invalidFormat', { formats: MODEL_EXTS.join(', ').toUpperCase() }))
      uploadStore.finish(taskId, { success: 0, failed: 0, message: '格式不支持' })
      return
    }

    if (!res || res.total === 0) {
      uploadStore.finish(taskId, { success: 0, failed: 0, message: '没有可上传的文件' })
      return
    }

    const normalItems = res.items.filter((it) => !it.error && !it.duplicate && !it.skipped)
    const duplicateItems = res.items.filter((it) => it.duplicate)

    let overwriteDuplicates = false
    if (duplicateItems.length > 0) {
      try {
        await ElMessageBox.confirm(
          t('home.sameFilesBatch', { count: duplicateItems.length }),
          t('home.sameFilesTitle'),
          { confirmButtonText: t('home.overwriteAll'), cancelButtonText: t('home.skipAll'), type: 'warning' }
        )
        overwriteDuplicates = true
      } catch {
        overwriteDuplicates = false
      }
    }

    const processItems = [...normalItems]
    const results = []

    if (overwriteDuplicates) {
      for (const dup of duplicateItems) {
        try {
          const overwritten = await window.api.models.overwrite(dup.existingModel, dup.pendingFile)
          overwritten.folderId =
            dup.existingModel?.folderId || folderStore.activeFolderId || ''
          processItems.push(overwritten)
        } catch (e) {
          results.push({ name: dup.fileName, success: false, message: e.message })
        }
      }
    }

    const total = processItems.length
    for (let i = 0; i < processItems.length; i++) {
      const item = processItems[i]
      update({
        current: i + 1,
        total,
        fileName: item.fileName || item.defaultName || '',
        progress: 40 + Math.round((i / Math.max(total, 1)) * 60)
      })

      let cover = null
      let dims = { x: 0, y: 0, z: 0 }
      let animCount = 0

      try {
        const rendered = await renderModelForCover(item.dataBase64, item.fileType)
        cover = rendered.cover
        dims = rendered.dimensions
        animCount = rendered.animationCount
      } catch (e) {
        console.warn('[DragUpload] 封面生成失败:', item.fileName, e)
      }

      // 关闭隐藏 viewer 节省资源
      batchViewerVisible.value = false
      currentBatchBuffer.value = null

      try {
        // 保存封面
        let coverField = null
        if (cover) {
          await window.api.models.saveImage(String(item.id), 'cover', cover)
          coverField = 'cover.png'
        }

        // 提交模型
        const meta = {
          id: item.id,
          name: item.defaultName || '',
          description: '',
          fileName: String(item.fileName),
          fileType: String(item.fileType),
          fileSize: Number(item.fileSize),
          files: JSON.parse(JSON.stringify(item.files)),
          dimensions: {
            x: Number(dims.x) || 0,
            y: Number(dims.y) || 0,
            z: Number(dims.z) || 0
          },
          animationCount: Number(animCount) || 0,
          uploadTime: String(item.uploadTime),
          cover: coverField,
          viewParams: null,
          tags: [],
          folderId: item.folderId || folderStore.activeFolderId || ''
        }
        await window.api.models.save(meta)
        results.push({ name: item.defaultName || item.fileName, success: true, hasCover: !!cover })
      } catch (e) {
        results.push({ name: item.defaultName || item.fileName, success: false, message: e.message })
      }
    }

    const succ = results.filter((r) => r.success).length
    uploadStore.finish(taskId, {
      success: succ,
      failed: results.length - succ,
      details: results.map((r) => ({ name: r.name, success: r.success, error: r.message }))
    })

    await loadAll()
  } catch (e) {
    if (unsubscribe) unsubscribe()
    uploadStore.finish(taskId, { success: 0, failed: 0, error: true, message: e.message || '拖拽上传失败' })
    ElMessage.error(e.message || '拖拽上传失败')
    batchViewerVisible.value = false
  }
}

const { isDragOver: isDragOverModel, onDragEnter: onModelDragEnter, onDragOver: onModelDragOver, onDragLeave: onModelDragLeave, onDrop: onModelDrop } = useDragUpload({
  extensions: MODEL_EXTS,
  typeLabel: '模型文件',
  onUpload: handleDragUpload,
  onInternalHover: (position) => {
    folderStore.dragOverKey = position ? folderKeyAt(position) : ''
  },
  onInternalDrop: (items, position) => {
    folderStore.dragOverKey = ''
    const key = position ? folderKeyAt(position) : ''
    if (!key) return
    const ids = (items || []).map((item) => String(item.id))
    moveModelsToFolder(ids, key === '__all__' ? '' : key)
  }
})

useResourceShortcuts({
  filteredList: filteredModels,
  selectedIds,
  isMultiSelect,
  anchorId,
  contentRef,
  onSelectAll: selectAll,
  onClearSelection: clearSelection,
  onBatchDelete: handleBatchDelete,
  onBatchAddTag: handleBatchAddTag,
  onExport: () => exportModels(selectedModels.value),
  onRefresh: loadAll,
  onOpenDetail: (model) => goEdit(model)
})
</script>

<template>
  <div class="home">
    <!-- 顶栏：标题、搜索、排序、上传 -->
    <header class="topbar">
      <div class="topbar-left">
        <el-dropdown trigger="click" @command="handleLibCommand">
          <span class="lib-name iconfont">
            &#xeb1a; {{ state.library?.name || '模型库' }}
          </span>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="create">{{ t('init.create') }}</el-dropdown-item>
              <el-dropdown-item command="switch">{{ t('init.switchLibrary') }}</el-dropdown-item>
              <el-dropdown-item command="close">{{ t('init.closeLibrary') }}</el-dropdown-item>
              <el-dropdown-item command="rename" divided>{{ t('init.rename') }}</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
      <div class="topbar-center">
        <el-input
          v-model="keyword"
          :placeholder="t('home.searchPlaceholder')"
          clearable
          size="large"
          class="search-input"
        >
          <template #prefix>
            <i class="iconfont icon-search"></i>
          </template>
        </el-input>
      </div>
      <div class="topbar-right">
        <el-popover trigger="click" placement="bottom" :width="240">
          <template #reference>
            <el-button type="primary" :title="t('common.previewSize')">
              <i class="iconfont">&#xeb56;</i>
            </el-button>
          </template>
          <div style="padding: 8px 4px;">
            <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px;">
              <span style="font-size: 13px; color: var(--text-2);">{{ t('common.previewSize') }}</span>
              <span style="font-size: 13px; font-weight: 600; color: var(--el-color-primary);">{{ previewSize }}</span>
            </div>
            <el-slider v-model="previewSize" :min="1" :max="10" :step="1" @change="onPreviewSizeChange" />
          </div>
        </el-popover>
        <el-popover trigger="click" placement="bottom" :width="200">
          <template #reference>
            <el-button type="primary" :title="t('common.displayFields')">
              <i class="iconfont">&#xeb14;</i>
            </el-button>
          </template>
          <div style="display: flex; flex-direction: column; gap: 8px; padding: 4px;">
            <el-checkbox v-model="allFieldsSelected" :indeterminate="isIndeterminate" @change="onSelectAll">{{ t('common.selectAll') }}</el-checkbox>
            <div style="border-bottom: 1px solid var(--border-soft); margin: 4px 0;"></div>
            <el-checkbox v-model="displaySettings.name" @change="onDisplayChange">{{ t('common.name') }}</el-checkbox>
            <el-checkbox v-model="displaySettings.tags" @change="onDisplayChange">{{ t('common.tags') }}</el-checkbox>
            <el-checkbox v-model="displaySettings.dimensions" @change="onDisplayChange">{{ t('common.dimensions') }}</el-checkbox>
            <el-checkbox v-model="displaySettings.fileSize" @change="onDisplayChange">{{ t('common.fileSize') }}</el-checkbox>
            <el-checkbox v-model="displaySettings.uploadTime" @change="onDisplayChange">{{ t('common.uploadTime') }}</el-checkbox>
            <el-checkbox v-model="displaySettings.fileType" @change="onDisplayChange">{{ t('common.format') }}</el-checkbox>
          </div>
        </el-popover>
        <el-dropdown trigger="click" popper-class="sort-popper" @command="handleSortCommand">
          <el-button type="primary">
            <i class="iconfont icon-sort"></i>
          </el-button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="asc" :class="{ 'is-active': sortDir === 'asc' }">{{ t('common.sort') }} A-Z</el-dropdown-item>
              <el-dropdown-item command="desc" :class="{ 'is-active': sortDir === 'desc' }">{{ t('common.sort') }} Z-A</el-dropdown-item>
              <el-dropdown-item divided command="uploadTime" :class="{ 'is-active': sortBy === 'uploadTime' }">{{ t('common.uploadTime') }}</el-dropdown-item>
              <el-dropdown-item command="fileSize" :class="{ 'is-active': sortBy === 'fileSize' }">{{ t('common.fileSize') }}</el-dropdown-item>
              <el-dropdown-item command="name" :class="{ 'is-active': sortBy === 'name' }">{{ t('common.name') }}</el-dropdown-item>
              <el-dropdown-item command="dimension" :class="{ 'is-active': sortBy === 'dimension' }">{{ t('common.dimensions') }}</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>

        <el-button type="primary" :title="t('home.singleUpload')" @click="goUpload()">
          <i class="iconfont icon-cloud-upload"></i>
        </el-button>
      </div>
    </header>

    <!-- 筛选栏：每类筛选一个下拉，整体一行 -->
    <div class="filter-bar">
      <div class="filter-tools">
        <el-select
          v-model="filterFormats"
          multiple
          collapse-tags
          clearable
          size="small"
          class="filter-select"
          :placeholder="t('home.filterFormat')"
        >
          <el-option label="BBMODEL" value="bbmodel" />
          <el-option label="JSON" value="json" />
          <el-option label="VOX" value="vox" />
        </el-select>
        <el-select v-model="filterAnimation" size="small" class="filter-select narrow">
          <el-option :label="t('home.filterAnimationAll')" value="all" />
          <el-option :label="t('home.filterAnimationWith')" value="with" />
          <el-option :label="t('home.filterAnimationWithout')" value="without" />
        </el-select>
        <div class="filter-size">
          <el-input-number
            v-model="filterSizeMin"
            :min="0"
            :step="0.1"
            :controls="false"
            size="small"
            :placeholder="t('home.sizeMin')"
            class="size-input"
          />
          <span class="filter-sep">-</span>
          <el-input-number
            v-model="filterSizeMax"
            :min="0"
            :step="0.1"
            :controls="false"
            size="small"
            :placeholder="t('home.sizeMax')"
            class="size-input"
          />
          <span class="filter-unit">MB</span>
        </div>
        <el-date-picker
          v-model="filterDateRange"
          type="daterange"
          size="small"
          class="filter-date"
          :start-placeholder="t('home.dateStart')"
          :end-placeholder="t('home.dateEnd')"
          value-format="YYYY-MM-DD"
        />
        <el-button v-if="activeFilterCount" text size="small" @click="resetFilters">
          {{ t('common.clearFilters') }}
        </el-button>
      </div>
      <span class="filter-total">{{ t('home.filteredCount', { count: filteredModels.length }) }}</span>
    </div>

    <!-- 模型网格 -->
    <main class="content"
          ref="contentRef"
          @click="handleContentClick"
          @mousedown="onContentMouseDown"
          @dragenter="onModelDragEnter"
          @dragover="onModelDragOver"
          @dragleave="onModelDragLeave"
          @drop="onModelDrop">
      <!-- 多选操作：浮空在文件区右上角 -->
      <transition name="fade">
        <div
          v-if="selectedIds.length > 0"
          class="batch-actions floating"
          :class="{ 'below-nav': !!folderStore.activeFolderId }"
        >
          <span class="selected-count">{{ t('common.selected') }} {{ selectedIds.length }}</span>
          <el-button text size="small" @click="selectAll">{{ t('home.selectAll') }}</el-button>
          <el-button text size="small" @click="selectInverse">{{ t('home.selectInverse') }}</el-button>
          <el-button text size="small" @click="clearSelection">{{ t('common.cancel') }}</el-button>
          <el-dropdown trigger="click" @command="(key) => moveModelsToFolder(selectedIds, key === '__all__' ? '' : key)">
            <el-button text size="small">
              <i class="iconfont icon-folder"></i> {{ t('folders.moveTo') }}
            </el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="__all__">{{ t('folders.allFiles') }}</el-dropdown-item>
                <el-dropdown-item v-for="folder in folderStore.folders" :key="folder.id" :command="folder.id">
                  <i class="iconfont icon-folder menu-folder-icon" :style="{ color: folder.color || '#409EFF' }"></i>
                  {{ folder.name }}
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
          <el-button text size="small" @click="handleBatchAddTag">
            <i class="iconfont icon-label"></i> {{ t('home.addTag') }}
          </el-button>
          <el-button text size="small" type="danger" @click="handleBatchDelete">
            <i class="iconfont icon-trash-alt"></i> {{ t('common.delete') }}
          </el-button>
        </div>
      </transition>

      <!-- 文件夹导航与子文件夹 -->
      <div v-if="folderStore.activeFolderId" class="folder-nav">
        <div class="folder-breadcrumb">
          <el-button text size="small" @click="enterFolder('')">{{ t('folders.allFiles') }}</el-button>
          <template v-for="(crumb, index) in folderPath" :key="crumb.id">
            <i class="iconfont icon-chevron-right crumb-sep"></i>
            <el-button
              text
              size="small"
              :class="{ 'crumb-current': index === folderPath.length - 1 }"
              @click="enterFolder(crumb.id)"
            >
              {{ crumb.name }}
            </el-button>
          </template>
        </div>
        <el-button v-if="childFolders.length" text size="small" @click="toggleChildFolders">
          <i class="iconfont icon-chevron-down child-toggle" :class="{ open: showChildFolders }"></i>
          {{ showChildFolders ? t('folders.hideChildren') : t('folders.showChildren') }}
        </el-button>
      </div>

      <div v-if="folderStore.activeFolderId && childFolders.length && showChildFolders" class="child-folders">
        <div
          v-for="folder in childFolders"
          :key="folder.id"
          class="child-folder-card"
          @click="enterFolder(folder.id)"
        >
          <i
            class="iconfont icon-folder child-folder-icon"
            :style="{ color: folder.color || '#409EFF' }"
          ></i>
          <span class="child-folder-name" :title="folder.name">{{ folder.name }}</span>
          <span class="child-folder-count">{{ folder.modelCount }}</span>
        </div>
      </div>

      <div v-if="filteredModels.length" class="grid">
        <ModelCard
          v-for="m in filteredModels"
          :key="m.id"
          :model="m"
          :selected="isSelected(m.id)"
          :is-dragging="isDragging"
          :display="displaySettings"
          :density="previewSize <= 2 ? 'tiny' : previewSize <= 3 ? 'compact' : 'full'"
          :data-model-id="m.id"
          @mousedown="onCardMouseDown($event, m, selectedModels)"
          @contextmenu="handleCardContextMenu"
          @select="handleSelect"
          @dblclick="handleDblClick"
        />
      </div>

      <el-empty v-else :description="emptyDescription" />

      <!-- 框选框 -->
      <div
        v-if="showSelectionBox"
        class="selection-box"
        ref="selectionBoxRef"
      />
    </main>

    <!-- 拖拽上传蒙版 -->
    <!-- 卡片右键菜单 -->
    <div
      v-if="cardMenu"
      class="ctx-overlay"
      @click="closeCardMenu"
      @contextmenu.prevent="closeCardMenu"
    ></div>
    <div
      v-if="cardMenu"
      class="ctx-menu"
      :style="{ left: cardMenu.x + 'px', top: cardMenu.y + 'px' }"
    >
      <button v-if="menuTargets.length === 1" class="ctx-item" @click="handleMenuOpenDetail">
        <i class="iconfont icon-open"></i> {{ t('common.openDetail') }}
      </button>
      <button v-if="menuTargets.length === 1" class="ctx-item" @click="handleMenuRename">
        <i class="iconfont icon-script"></i> {{ t('home.renameModel') }}
      </button>
      <button class="ctx-item" @click="handleMenuRefreshCovers">
        <i class="iconfont icon-repeat"></i> {{ t('home.refreshCover') }}
        <span v-if="menuTargets.length > 1" class="ctx-badge">{{ menuTargets.length }}</span>
      </button>
      <button class="ctx-item" @click="cardMenu.foldersOpen = !cardMenu.foldersOpen">
        <i class="iconfont icon-folder"></i> {{ t('folders.moveTo') }}
        <i
          class="iconfont icon-chevron-right ctx-arrow"
          :class="{ open: cardMenu.foldersOpen }"
        ></i>
      </button>
      <div v-if="cardMenu.foldersOpen" class="ctx-sub">
        <button class="ctx-sub-item" @click="handleMenuMove('')">
          <i class="iconfont icon-folder"></i> {{ t('folders.allFiles') }}
        </button>
        <button
          v-for="folder in folderStore.folders"
          :key="folder.id"
          class="ctx-sub-item"
          @click="handleMenuMove(folder.id)"
        >
          <i
            class="iconfont icon-folder menu-folder-icon"
            :style="{ color: folder.color || '#409EFF' }"
          ></i>
          {{ folder.name }}
        </button>
        <div v-if="!folderStore.folders.length" class="ctx-sub-empty">{{ t('folders.empty') }}</div>
      </div>
      <button class="ctx-item" @click="handleMenuAddTag">
        <i class="iconfont icon-label"></i> {{ t('home.addTag') }}
      </button>
      <button class="ctx-item" @click="handleMenuExport">
        <i class="iconfont icon-cloud-download"></i> {{ t('common.export') }}
        <span v-if="menuTargets.length > 1" class="ctx-badge">{{ menuTargets.length }}</span>
      </button>
      <div class="ctx-divider"></div>
      <button class="ctx-item danger" @click="handleMenuDelete">
        <i class="iconfont icon-trash-alt"></i> {{ t('common.delete') }}
        <span v-if="menuTargets.length > 1" class="ctx-badge">{{ menuTargets.length }}</span>
      </button>
    </div>

    <div v-if="isDragOverModel" class="drag-overlay">
      <div class="drag-overlay-content">
        <i class="iconfont icon-cloud-upload"></i>
        <p>{{ t('common.dropHere') }}</p>
        <p class="drag-overlay-formats">BBMODEL / VOX</p>
      </div>
    </div>
    <!-- 隐藏的 ModelViewer 用于批量上传时生成封面 -->
    <div class="hidden-viewer" v-if="batchViewerVisible">
      <ModelViewer
        :key="batchRenderKey"
        ref="batchViewerRef"
        :buffer="currentBatchBuffer"
        :file-type="currentBatchType"
        :aux-files="currentBatchAux"
        :view-params="currentBatchViewParams"
        @loaded="onBatchModelLoaded"
        @error="onBatchModelError"
      />
    </div>
  </div>
</template>

<style scoped>
.home {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 24px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.topbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
}
.topbar-center {
  flex: 1;
  display: flex;
  justify-content: center;
  padding: 0 20px;
}
.lib-name {
  display: inline-flex;
  align-items: center;
  padding: 4px 10px;
  font-size: 16px;
  font-weight: 600;
  cursor: pointer;
  outline: none;
  border-radius: 4px;
  transition: background 0.2s;
}
.lib-name:hover {
  background: var(--el-fill-color-light);
}
.topbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}
.search-input {
  width: 400px;
}
.search-input :deep(.el-input__prefix .iconfont) {
  font-size: 14px;
}
.filter-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 24px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
  min-height: 44px;
  flex-wrap: nowrap;
}
.filter-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
}
.filter-select {
  width: 148px;
  flex-shrink: 0;
}
.filter-select.narrow {
  width: 126px;
}
.filter-size {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.size-input {
  width: 74px;
}
.filter-sep {
  color: var(--text-3);
}
.filter-unit {
  font-size: 11px;
  color: var(--text-3);
}
.filter-date {
  width: 150px;
  flex-shrink: 0;
}
.filter-total {
  font-size: 12px;
  color: var(--text-3);
  flex-shrink: 0;
}
.menu-folder-icon {
  font-size: 13px;
  margin-right: 6px;
  vertical-align: middle;
}
/* 多选操作条：浮空显示，不占用额外一行 */
.batch-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  background: var(--bg-card);
  border-radius: 10px;
  border: 1px solid var(--primary);
  box-shadow: 0 8px 22px rgba(0, 0, 0, 0.18);
  white-space: nowrap;
  flex-shrink: 0;
}
.batch-actions.floating {
  position: sticky;
  top: 8px;
  z-index: 30;
  width: max-content;
  margin-left: auto;
  margin-bottom: -40px;
}
.batch-actions.floating.below-nav {
  top: 44px;
}
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
.selected-count {
  font-size: 13px;
  font-weight: 600;
  padding: 2px 8px;
  background: var(--primary-soft);
  color: var(--primary);
  border: 1px solid var(--primary);
  border-radius: 10px;
  white-space: nowrap;
  flex-shrink: 0;
}
.content {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 20px 24px 32px;
  position: relative;
}
.folder-nav {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
.folder-breadcrumb {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-wrap: wrap;
  min-width: 0;
}
.crumb-sep {
  font-size: 11px;
  color: var(--text-3);
}
.crumb-current {
  color: var(--primary) !important;
  font-weight: 600;
}
.child-toggle {
  font-size: 12px;
  margin-right: 2px;
  transition: transform 0.15s;
}
.child-toggle.open {
  transform: rotate(180deg);
}
.child-folders {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 14px;
}
.child-folder-card {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 130px;
  max-width: 230px;
  padding: 8px 12px;
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  background: var(--bg-soft);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.child-folder-card:hover {
  border-color: var(--primary);
  background: var(--primary-soft);
}
.child-folder-icon {
  font-size: 15px;
  flex-shrink: 0;
}
.child-folder-name {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.child-folder-count {
  font-size: 11px;
  color: var(--text-3);
}
.content.box-selecting {
  user-select: none;
  cursor: crosshair;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(v-bind(gridMinWidth + 'px'), 1fr));
  gap: 16px;
}

/* 批量上传弹窗 */
.batch-body {
  padding: 4px 0;
}
.batch-info {
  margin-top: 16px;
}
.batch-status {
  font-size: 13px;
  margin-bottom: 8px;
}
.batch-file {
  font-size: 12px;
  color: var(--text-3);
  word-break: break-all;
}
.batch-result {
  max-height: 240px;
  overflow-y: auto;
  margin-top: 8px;
}
.batch-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 0;
  font-size: 12px;
  border-bottom: 1px solid var(--border-soft);
}
.batch-item:last-child {
  border-bottom: none;
}
.batch-item.ok .batch-item-status {
  color: var(--success);
}
.batch-item.fail .batch-item-status {
  color: var(--danger);
}
.batch-item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-right: 8px;
}
.batch-item-status {
  flex-shrink: 0;
}

/* 框选框 */
.selection-box {
  position: absolute;
  border: 2px dashed var(--primary);
  background: var(--primary-soft);
  pointer-events: none;
  z-index: 100;
}

/* 右键菜单 */
.ctx-overlay {
  position: fixed;
  inset: 0;
  z-index: 3000;
}
.ctx-menu {
  position: fixed;
  z-index: 3001;
  min-width: 190px;
  padding: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.18);
}
.ctx-item {
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
.ctx-item:hover {
  background: var(--primary-soft);
  color: var(--primary);
}
.ctx-item.danger {
  color: var(--danger);
}
.ctx-item.danger:hover {
  background: rgba(245, 108, 108, 0.12);
  color: var(--danger);
}
.ctx-divider {
  height: 1px;
  margin: 4px 6px;
  background: var(--border-soft);
}
.ctx-badge {
  margin-left: auto;
  min-width: 18px;
  padding: 0 5px;
  font-size: 11px;
  line-height: 16px;
  text-align: center;
  color: var(--text-2);
  background: var(--bg-mute);
  border-radius: 8px;
}
.ctx-arrow {
  margin-left: auto;
  font-size: 12px;
  transition: transform 0.15s;
}
.ctx-arrow.open {
  transform: rotate(90deg);
}
.ctx-sub {
  display: flex;
  flex-direction: column;
  max-height: 190px;
  overflow-y: auto;
  margin: 2px 4px 4px 16px;
  padding-left: 6px;
  border-left: 1px solid var(--border-soft);
}
.ctx-sub-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.ctx-sub-item:hover {
  background: var(--primary-soft);
  color: var(--primary);
}
.ctx-sub-empty {
  padding: 4px 8px;
  font-size: 11px;
  color: var(--text-3);
}

/* 隐藏的 ModelViewer 容器：必须有真实尺寸供 WebGL 渲染，但移出可视区域 */
.hidden-viewer {
  position: fixed;
  left: -9999px;
  top: 0;
  width: 400px;
  height: 300px;
  pointer-events: none;
  z-index: -1;
  opacity: 0;
}

/* 拖拽上传蒙版 */
.drag-overlay {
  position: fixed;
  inset: 0;
  z-index: 9998;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drag-overlay-content {
  text-align: center;
  color: #fff;
}
.drag-overlay-content .iconfont {
  font-size: 64px;
  color: var(--primary);
  display: block;
  margin-bottom: 16px;
}
.drag-overlay-content p {
  font-size: 20px;
  font-weight: 600;
  margin: 0;
}
.drag-overlay-formats {
  font-size: 13px !important;
  font-weight: 400 !important;
  margin-top: 8px !important;
  opacity: 0.7;
}
</style>

<style>
/* 排序下拉菜单选中项高亮（popper 渲染到 body，需用全局样式） */
.sort-popper .is-active {
  color: var(--el-color-primary);
  font-weight: 600;
}

/* 卡片拖动中（拖到文件夹 / 拖出窗口） */
body.tsrbox-dragging,
body.tsrbox-dragging * {
  cursor: grabbing !important;
}
</style>
