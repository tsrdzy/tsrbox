import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

/** 文件夹默认配色（新建时按顺序循环使用） */
export const FOLDER_COLORS = [
  '#409EFF',
  '#67C23A',
  '#E6A23C',
  '#F56C6C',
  '#9B59B6',
  '#00B8A9',
  '#8E9AAF',
  '#E06C9F'
]

const EXPANDED_KEY = 'tsrbox-folder-expanded'

/** 虚拟分类「未入文件夹」的 activeFolderId 取值 */
export const UNFILED_ID = '__unfiled__'

function loadExpanded() {
  try {
    const raw = localStorage.getItem(EXPANDED_KEY)
    const list = raw ? JSON.parse(raw) : []
    return Array.isArray(list) ? list : []
  } catch {
    return []
  }
}

/** 同级排序：sortOrder 优先，其次创建时间 */
function compareFolders(a, b) {
  const orderA = Number(a.sortOrder || 0)
  const orderB = Number(b.sortOrder || 0)
  if (orderA !== orderB) return orderA - orderB
  return String(a.createdAt || '').localeCompare(String(b.createdAt || ''))
}

export const useFolderStore = defineStore('folders', () => {
  const folders = ref([])
  /** 当前筛选的文件夹；空字符串表示「全部文件」 */
  const activeFolderId = ref('')
  /** 拖动资源时悬停的文件夹：'' 未悬停 | '__all__' 全部文件 | 文件夹 id */
  const dragOverKey = ref('')
  /** 折叠树中展开的文件夹 id */
  const expandedIds = ref(loadExpanded())

  const activeFolder = computed(
    () => folders.value.find((folder) => folder.id === activeFolderId.value) || null
  )

  /** 文件夹树（同级按创建时间排序） */
  const tree = computed(() => {
    const map = new Map(folders.value.map((folder) => [folder.id, { ...folder, children: [] }]))
    const roots = []
    for (const node of map.values()) {
      const parent = node.parentId ? map.get(node.parentId) : null
      if (parent && parent.id !== node.id) parent.children.push(node)
      else roots.push(node)
    }
    const sortNodes = (nodes) => {
      nodes.sort(compareFolders)
      nodes.forEach((node) => sortNodes(node.children))
      return nodes
    }
    return sortNodes(roots)
  })

  /** 扁平化后的可见文件夹（带层级深度） */
  const visibleFolders = computed(() => {
    const expandSet = new Set(expandedIds.value)
    const out = []
    const walk = (nodes, depth) => {
      for (const node of nodes) {
        out.push({ ...node, depth })
        if (node.children.length && expandSet.has(node.id)) walk(node.children, depth + 1)
      }
    }
    walk(tree.value, 0)
    return out
  })

  function persistExpanded() {
    try {
      localStorage.setItem(EXPANDED_KEY, JSON.stringify(expandedIds.value))
    } catch {
      // ignore
    }
  }

  function toggleExpanded(id) {
    if (expandedIds.value.includes(id)) {
      expandedIds.value = expandedIds.value.filter((item) => item !== id)
    } else {
      expandedIds.value = [...expandedIds.value, id]
    }
    persistExpanded()
  }

  /** 展开某个文件夹的所有祖先，保证它可见 */
  function expandTo(id) {
    if (!id) return
    const byId = new Map(folders.value.map((folder) => [folder.id, folder]))
    const add = new Set(expandedIds.value)
    let current = byId.get(id)
    const guard = new Set()
    while (current && !guard.has(current.id)) {
      guard.add(current.id)
      if (current.parentId) add.add(current.parentId)
      current = current.parentId ? byId.get(current.parentId) : null
    }
    expandedIds.value = [...add]
    persistExpanded()
  }

  /** 某个文件夹的子文件夹 */
  function childrenOf(parentId) {
    return folders.value
      .filter((folder) => (folder.parentId || '') === (parentId || ''))
      .sort(compareFolders)
  }

  /** 从顶层到指定文件夹的路径 */
  function folderPath(id) {
    if (!id) return []
    const byId = new Map(folders.value.map((folder) => [folder.id, folder]))
    const result = []
    const guard = new Set()
    let current = byId.get(id)
    while (current && !guard.has(current.id)) {
      guard.add(current.id)
      result.unshift(current)
      current = current.parentId ? byId.get(current.parentId) : null
    }
    return result
  }

  /** candidate 是否为 ancestorId 的子孙（用于拖放的即时校验） */
  function isDescendant(candidateId, ancestorId) {
    if (!candidateId || !ancestorId) return false
    const byId = new Map(folders.value.map((folder) => [folder.id, folder]))
    let current = byId.get(candidateId)
    const guard = new Set()
    while (current && current.parentId && !guard.has(current.id)) {
      guard.add(current.id)
      if (current.parentId === ancestorId) return true
      current = byId.get(current.parentId)
    }
    return false
  }

  function reset() {
    folders.value = []
    activeFolderId.value = ''
    dragOverKey.value = ''
  }

  async function loadFolders() {
    try {
      const list = await window.api.folders.list()
      folders.value = Array.isArray(list) ? list : []
      if (
        activeFolderId.value &&
        activeFolderId.value !== UNFILED_ID &&
        !folders.value.some((folder) => folder.id === activeFolderId.value)
      ) {
        activeFolderId.value = ''
      }
    } catch (e) {
      console.error('[folders] 加载失败:', e)
      folders.value = []
    }
  }

  async function createFolder(name, color, parentId) {
    const parent = parentId || ''
    const nextColor = color || FOLDER_COLORS[folders.value.length % FOLDER_COLORS.length]
    const folder = await window.api.folders.create(name, nextColor, parent)
    folders.value = [...folders.value, folder]
    if (parent) expandTo(parent)
    return folder
  }

  async function updateFolder(id, patch) {
    const folder = await window.api.folders.update(id, patch)
    folders.value = folders.value.map((item) => (item.id === id ? folder : item))
    return folder
  }

  /** 移动文件夹到指定父级并插入到 index（同父级即调整顺序） */
  async function moveFolder(id, parentId, index = 0) {
    await window.api.folders.move(id, parentId || '', index)
    await loadFolders()
    return folders.value.find((folder) => folder.id === id) || null
  }

  async function deleteFolder(id) {
    const target = folders.value.find((folder) => folder.id === id)
    await window.api.folders.delete(id)
    folders.value = folders.value.filter((folder) => folder.id !== id)
    if (activeFolderId.value === id) {
      activeFolderId.value = target?.parentId || ''
    }
  }

  /** 批量移动模型；folderId 为空表示移回「全部文件」 */
  async function moveModels(ids, folderId) {
    if (!ids.length) return 0
    const res = await window.api.folders.moveModels(ids, folderId)
    return Number(res?.moved || 0)
  }

  function setActiveFolder(id) {
    activeFolderId.value = id || ''
    if (id && id !== UNFILED_ID) expandTo(id)
  }

  return {
    folders,
    activeFolderId,
    activeFolder,
    dragOverKey,
    expandedIds,
    tree,
    visibleFolders,
    reset,
    loadFolders,
    createFolder,
    updateFolder,
    moveFolder,
    deleteFolder,
    moveModels,
    setActiveFolder,
    toggleExpanded,
    expandTo,
    childrenOf,
    folderPath,
    isDescendant
  }
})
