import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'

/** 调用 Rust 命令，并把字符串错误统一转成 Error（保持 e.message 可用） */
async function call(command, args) {
  try {
    return await invoke(command, args)
  } catch (e) {
    const message = typeof e === 'string' ? e : (e && e.message) || JSON.stringify(e)
    throw new Error(message)
  }
}

async function pickModelFiles(multiple = false) {
  const res = await openDialog({
    multiple,
    directory: false,
    filters: [{ name: '模型文件', extensions: ['bbmodel', 'json', 'vox'] }]
  })
  if (!res) return null
  return res
}

async function pickModelFile() {
  const res = await openDialog({
    multiple: false,
    directory: false,
    filters: [{ name: '模型文件', extensions: ['bbmodel', 'json', 'vox'] }]
  })
  return res || null
}

async function pickAuxFile(role) {
  const filters =
    role === 'texture'
      ? [{ name: '贴图文件', extensions: ['png', 'jpg', 'jpeg', 'webp'] }]
      : [{ name: '动作文件', extensions: ['json'] }]
  const res = await openDialog({ multiple: false, directory: false, filters })
  return res || null
}

async function pickAuxFiles(role) {
  const filters =
    role === 'texture'
      ? [{ name: '贴图文件', extensions: ['png', 'jpg', 'jpeg', 'webp'] }]
      : [{ name: '动作文件', extensions: ['json'] }]
  const res = await openDialog({ multiple: true, directory: false, filters })
  if (!res) return []
  return Array.isArray(res) ? res : [res]
}

async function pickDirectory() {
  const res = await openDialog({ directory: true, multiple: false })
  return res || null
}

// ===== 批量上传进度（Tauri 事件监听需异步注册，这里做同步订阅封装）=====
const progressListeners = new Set()
let progressReady = null

function ensureProgressListener() {
  if (!progressReady) {
    progressReady = listen('batch-upload-progress', (event) => {
      for (const cb of progressListeners) {
        try {
          cb(event.payload)
        } catch (e) {
          console.error('[progress] callback error:', e)
        }
      }
    })
  }
  return progressReady
}

function onBatchUploadProgress(callback) {
  ensureProgressListener()
  progressListeners.add(callback)
  return () => progressListeners.delete(callback)
}

// ===== 窗口控制 =====
const appWindow = getCurrentWindow()

function onMaximizeChange(callback) {
  let active = true
  let unlisten = null
  appWindow
    .onResized(async () => {
      if (!active) return
      try {
        callback(await appWindow.isMaximized())
      } catch {
        // ignore
      }
    })
    .then((fn) => {
      if (active) unlisten = fn
      else fn()
    })
  return () => {
    active = false
    if (unlisten) unlisten()
  }
}

export const api = {
  // ====== 资源库 ======
  library: {
    status: () => call('library_status'),
    defaultPath: () => call('library_default_path'),
    list: () => call('library_list'),
    selectFolder: () => pickDirectory(),
    create: (folderPath, name) => call('library_create', { folderPath, name }),
    open: (folderPath) => call('library_open', { folderPath }),
    close: () => call('library_close'),
    remove: (folderPath) => call('library_remove', { folderPath }),
    rename: (folderPath, newName) => call('library_rename', { folderPath, newName }),
    selectAndOpen: async () => {
      const folderPath = await pickDirectory()
      if (!folderPath) return null
      return call('library_open', { folderPath })
    },
    updateSettings: (settings) => call('library_update_settings', { settings })
  },

  // ====== 标签 ======
  tags: {
    list: () => call('tags_list'),
    add: (tag) => call('tags_add', { tag }),
    remove: (tag) => call('tags_remove', { tag })
  },

  // ====== 文件夹 ======
  folders: {
    list: () => call('folders_list'),
    create: (name, color, parentId) =>
      call('folders_create', { name, color: color || '', parentId: parentId || '' }),
    update: (id, patch = {}) =>
      call('folders_update', {
        id,
        name: patch.name ?? null,
        color: patch.color ?? null,
        parentId: patch.parentId ?? null
      }),
    move: (id, parentId, index = 0) =>
      call('folders_move', { id, parentId: parentId || '', index }),
    delete: (id) => call('folders_delete', { id }),
    moveModels: (ids, folderId) =>
      call('models_move_folder', { ids, folderId: folderId || null })
  },

  // ====== 模型 ======
  models: {
    list: () => call('models_list'),
    listDeleted: () => call('models_list_deleted'),
    restore: (id) => call('models_restore', { id }),
    deleteForever: (id) => call('models_delete_forever', { id }),
    emptyRecycle: () => call('models_empty_recycle'),
    pickFile: () => pickModelFile(),
    pickFiles: () => pickModelFiles(true),
    pickAux: (role) => pickAuxFile(role),
    pickAuxFiles: (role) => pickAuxFiles(role),
    stageModel: (path) => call('models_stage_model', { path }),
    stageAux: (path, role) => call('models_stage_aux', { path, role }),
    uploadByPath: (path) => call('models_upload_by_path', { path }),
    uploadWithAux: (path, aux) => call('models_upload_with_aux', { path, aux }),
    overwrite: (existingModel, pendingFile) =>
      call('models_overwrite', { existingModel, pendingFile }),
    batchUpload: async () => {
      const paths = await pickModelFiles(true)
      if (!paths) return null
      await ensureProgressListener()
      return call('models_batch_upload_by_paths', { paths })
    },
    batchUploadByPaths: async (paths) => {
      await ensureProgressListener()
      return call('models_batch_upload_by_paths', { paths })
    },
    onBatchUploadProgress,
    loadFile: (model) => call('models_load_file', { model }),
    readFileData: (id, fileName) => call('models_read_file_data', { id, fileName }),
    addAuxFile: (id, path, role) => call('models_add_aux_file', { id, path, role }),
    removeAuxFile: (id, name) => call('models_remove_aux_file', { id, name }),
    saveImage: (id, type, base64) => call('models_save_image', { id, kind: type, base64 }),
    readImage: (id, hasCover, hasThumb) =>
      call('models_read_image', { id, hasCover: !!hasCover, hasThumb: !!hasThumb }),
    save: (meta) => call('models_save', { meta }),
    update: (id, patch) => call('models_update', { id, patch }),
    delete: (id) => call('models_delete', { id }),
    deleteMany: (ids) => call('models_delete_batch', { ids: (ids || []).map(String) }),
    export: async (model, format = 'original') => {
      const targetRoot = await pickDirectory()
      if (!targetRoot) return null
      return call('models_export', { model, targetRoot, format })
    },
    exportBatch: async (models, format = 'original') => {
      const targetRoot = await pickDirectory()
      if (!targetRoot) return null
      return call('models_export_batch', { models, targetRoot, format })
    }
  },

  // ====== 插件通用 KV 存储（核心不含具体业务，由插件自行使用）======
  pluginStorage: {
    get: (key) => call('plugin_storage_get', { key }),
    set: (key, value) => call('plugin_storage_set', { key, value }),
    remove: (key) => call('plugin_storage_remove', { key })
  },

  // ====== 窗口控制（自定义标题栏）======
  windowControl: {
    minimize: () => appWindow.minimize(),
    maximize: () => appWindow.maximize(),
    unmaximize: () => appWindow.unmaximize(),
    toggleMaximize: () => appWindow.toggleMaximize(),
    close: () => appWindow.close(),
    isMaximized: () => appWindow.isMaximized(),
    onMaximizeChange
  },

  // ====== 操作日志 ======
  logs: {
    query: (options) => call('logs_query', { options }),
    export: (options) => call('logs_export', { options }),
    exportFile: async (options, defaultName = 'operation_logs.json') => {
      const path = await saveDialog({
        defaultPath: defaultName,
        filters: [{ name: 'JSON', extensions: ['json'] }]
      })
      if (!path) return null
      const logs = await call('logs_export', { options })
      await call('logs_write_file', {
        path,
        content: JSON.stringify(logs, null, 2)
      })
      return path
    },
    clear: (beforeDate) => call('logs_clear', { beforeDate: beforeDate || null }),
    getDeviceInfo: () => call('logs_get_device_info')
  },

  // ====== 系统外链 ======
  openExternal: (url) => openUrl(url).catch((e) => console.error('[openExternal]', e)),

  // ====== 兼容旧接口（Tauri 使用原生拖拽事件，无需文件路径转换）======
  getPathForFile: () => '',

  // ====== 资源原生拖出（拖到桌面）======
  resources: {
    startDrag: (payload) => {
      const items = payload?.items || []
      if (!items.length) return
      call('resources_start_drag', { items, format: payload?.format || null }).catch((e) =>
        console.error('[NativeDrag] 拖出失败:', e)
      )
    }
  }
}

export function installApi() {
  window.api = api
}
