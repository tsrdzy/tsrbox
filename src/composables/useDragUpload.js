import { ref, onMounted, onBeforeUnmount } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { ElMessage } from 'element-plus'

/**
 * 拖拽上传 composable（Tauri 原生 drag-drop 事件版）
 * Tauri 会拦截 HTML5 拖拽事件并通过 onDragDropEvent 提供真实文件路径。
 *
 * @param {Object} options
 * @param {string[]} options.extensions - 支持的文件扩展名
 * @param {string} options.typeLabel - 资源类型标签（用于提示）
 * @param {Function} options.onUpload - 上传回调 (filePaths, payload) => Promise
 * @param {Function} [options.onHover] - 拖拽位置回调 (position|null) => void
 * @param {Function} [options.onInternalHover] - 应用内拖出回到窗口时的位置回调
 * @param {Function} [options.onInternalDrop] - 应用内拖出的文件重新放回窗口时回调 (items, position)
 */
export function useDragUpload({
  extensions,
  typeLabel,
  onUpload,
  onHover,
  onInternalHover,
  onInternalDrop
}) {
  const isDragOver = ref(false)
  const isUploading = ref(false)

  const INTERNAL_DRAG_FLAG = '__tsrboxInternalDragOut'

  function isInternalDragOut() {
    try {
      return !!window[INTERNAL_DRAG_FLAG]
    } catch {
      return false
    }
  }

  function clearInternalDragFlag() {
    try {
      window[INTERNAL_DRAG_FLAG] = false
    } catch {
      // ignore
    }
  }

  /** 取出本次应用内拖出的资源（用于拖回窗口时移动到文件夹等） */
  function popInternalDrag() {
    try {
      const data = window.__tsrboxInternalDrag || null
      window.__tsrboxInternalDrag = null
      return data
    } catch {
      return null
    }
  }

  function hasValidPath(paths) {
    if (!Array.isArray(paths) || paths.length === 0) return false
    return paths.some((p) => {
      const lower = String(p).toLowerCase()
      // 文件夹可能没有扩展名，交给后端递归扫描
      return extensions.some((ext) => lower.endsWith('.' + ext)) || !lower.includes('.')
    })
  }

  async function handleDrop(paths, payload) {
    if (isUploading.value) return
    if (!Array.isArray(paths) || paths.length === 0) return

    isUploading.value = true
    try {
      await onUpload(paths, payload)
    } catch (err) {
      console.error('[DragUpload] 上传失败:', err)
      ElMessage.error(err.message || '上传失败')
    } finally {
      isUploading.value = false
    }
  }

  let unlisten = null

  onMounted(async () => {
    try {
      unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload

        if (payload.type === 'enter') {
          if (isInternalDragOut()) {
            onInternalHover?.(payload.position || null)
            return
          }
          isDragOver.value = hasValidPath(payload.paths)
          if (isDragOver.value) onHover?.(payload.position || null)
        } else if (payload.type === 'over') {
          if (isInternalDragOut()) {
            onInternalHover?.(payload.position || null)
            return
          }
          if (isDragOver.value) onHover?.(payload.position || null)
        } else if (payload.type === 'leave') {
          isDragOver.value = false
          onHover?.(null)
          onInternalHover?.(null)
        } else if (payload.type === 'drop') {
          isDragOver.value = false
          onHover?.(null)
          onInternalHover?.(null)
          if (isInternalDragOut()) {
            clearInternalDragFlag()
            const internal = popInternalDrag()
            if (onInternalDrop) {
              onInternalDrop(internal?.items || [], payload.position || null)
            }
            return
          }
          handleDrop(payload.paths, payload)
        }
      })
    } catch (e) {
      console.error('[DragUpload] 无法监听拖拽事件:', e)
    }
  })

  onBeforeUnmount(() => {
    if (unlisten) {
      unlisten()
      unlisten = null
    }
  })

  // 保留原有 DOM 事件接口：Tauri 已接管原生拖拽，这里只做阻止默认行为
  function prevent(e) {
    if (e && e.preventDefault) e.preventDefault()
    if (e && e.stopPropagation) e.stopPropagation()
  }

  return {
    isDragOver,
    isUploading,
    typeLabel,
    onDragEnter: prevent,
    onDragOver: prevent,
    onDragLeave: prevent,
    onDrop: prevent
  }
}
