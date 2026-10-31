import { ref } from 'vue'
import { useStore } from './useStore'

/**
 * 卡片上按住拖动：
 * 1. 拖到侧边栏文件夹 → 通过回调在应用内移动模型（不发起系统拖出）
 * 2. 拖到窗口外 → 调用主进程 startDrag，把资源源文件拖到桌面/其他应用
 *
 * @param {Object} options
 * @param {string} options.type - models | images | audios | fonts
 * @param {number} options.threshold - 触发拖动的位移阈值（px）
 * @param {string} options.folderSelector - 文件夹放置区选择器（带 data-folder-drop 属性）
 * @param {Function} options.onFolderHover - 悬停文件夹回调 (key) => void
 * @param {Function} options.onFolderDrop - 释放到文件夹回调 (key, ids) => void
 */
export function useNativeDrag({
  type,
  threshold = 6,
  folderSelector = '',
  onFolderHover = null,
  onFolderDrop = null
} = {}) {
  const nativeDragActive = ref(false)
  let pressing = null
  let startX = 0
  let startY = 0
  let moveHandler = null
  let upHandler = null
  let leaveHandler = null
  let suppressClick = false

  function setDraggingCursor(on) {
    try {
      document.body.classList.toggle('tsrbox-dragging', !!on)
    } catch {
      // ignore
    }
  }

  function cleanup() {
    document.removeEventListener('mousemove', moveHandler)
    document.removeEventListener('mouseup', upHandler)
    document.removeEventListener('mouseleave', leaveHandler)
    moveHandler = null
    upHandler = null
    leaveHandler = null
    pressing = null
    setDraggingCursor(false)
  }

  /** 全局标记：当前是否存在由本应用发起的原生拖出（用于在 drop 时拦截同窗口拖回，避免重复上传） */
  const INTERNAL_DRAG_FLAG = '__tsrboxInternalDragOut'

  function setInternalDragFlag(value) {
    try {
      window[INTERNAL_DRAG_FLAG] = value
    } catch (_) { /* ignore */ }
  }

  function isOutsideWindow(ev) {
    return (
      ev.clientX <= 1 ||
      ev.clientY <= 1 ||
      ev.clientX >= window.innerWidth - 1 ||
      ev.clientY >= window.innerHeight - 1
    )
  }

  /** 拖出到系统：真正发起 OLE 拖拽（必须在指针移出窗口后调用） */
  function startNativeDrag(pressedItem, list) {
    cleanup()
    const useSelection =
      list.length > 1 && list.some((x) => String(x.id) === String(pressedItem.id))
    const items = (useSelection ? list : [pressedItem]).map((x) => ({
      id: String(x.id),
      fileName: x.fileName
    }))
    suppressClick = true
    nativeDragActive.value = true
    // 标记：本次原生拖出由本应用发起；拖回到同窗口时不上传
    setInternalDragFlag(true)
    try {
      window.__tsrboxInternalDrag = { type, items }
    } catch {
      // ignore
    }
    // 拖出格式取自资源库设置（默认保留原始格式）
    const dragFormat = useStore().state.library?.settings?.dragFormat || 'original'
    try {
      window.api.resources.startDrag({ type, items, format: dragFormat })
    } catch (err) {
      console.error('[NativeDrag] 拖出失败:', err)
      setInternalDragFlag(false)
    }
    setTimeout(() => {
      nativeDragActive.value = false
      // 兜底：超过一定时间后清除标记，防止外部拖入被误拦截
      setInternalDragFlag(false)
      try {
        window.__tsrboxInternalDrag = null
      } catch {
        // ignore
      }
    }, 15000)
  }

  /**
   * @param {MouseEvent} e
   * @param {Object} item - 当前按压的资源
   * @param {Array} selectedItems - 当前选中的资源集合（多选时拖出全部）
   */
  function onCardMouseDown(e, item, selectedItems = []) {
    if (e.button !== 0) return
    if (e.ctrlKey || e.metaKey || e.shiftKey) return
    if (!item || !item.fileName) return

    // 关键：阻止浏览器默认的文本选中/图片拖拽行为
    e.preventDefault()

    suppressClick = false
    startX = e.clientX
    startY = e.clientY
    pressing = { item, selectedItems, mode: '', folderKey: '' }

    moveHandler = (ev) => {
      if (!pressing) return

      if (!pressing.mode) {
        const dx = ev.clientX - startX
        const dy = ev.clientY - startY
        if (Math.abs(dx) < threshold && Math.abs(dy) < threshold) return
        // 先进入「待定」状态：既能拖到侧边栏文件夹，也能拖出窗口
        pressing.mode = 'maybe'
        suppressClick = true
        setDraggingCursor(true)
      }

      // 悬停在侧边栏文件夹 → 应用内移动
      if (folderSelector) {
        const dropEl = document
          .elementFromPoint(ev.clientX, ev.clientY)
          ?.closest(folderSelector)
        const key = dropEl ? dropEl.getAttribute('data-folder-drop') || '' : ''

        if (key) {
          pressing.mode = 'folder'
          if (key !== pressing.folderKey) {
            pressing.folderKey = key
            onFolderHover?.(key)
          }
          return
        }
        if (pressing.mode === 'folder') {
          // 移出文件夹区域，回到待定状态
          pressing.mode = 'maybe'
          pressing.folderKey = ''
          onFolderHover?.('')
        }
      }

      // 指针移出窗口 → 转为系统拖出
      if (isOutsideWindow(ev)) {
        startNativeDrag(pressing.item, pressing.selectedItems)
      }
    }

    leaveHandler = () => {
      if (!pressing) return
      if (pressing.mode === 'maybe' || pressing.mode === 'folder') {
        startNativeDrag(pressing.item, pressing.selectedItems)
      }
    }

    upHandler = () => {
      const current = pressing
      if (!current) return
      const mode = current.mode
      const key = current.folderKey
      const { item: pressedItem, selectedItems: list } = current
      cleanup()
      if (mode === 'folder') {
        suppressClick = true
        onFolderHover?.('')
        if (key) {
          const useSelection =
            list.length > 1 && list.some((x) => String(x.id) === String(pressedItem.id))
          const ids = (useSelection ? list : [pressedItem]).map((x) => String(x.id))
          onFolderDrop?.(key, ids)
        }
      }
    }

    document.addEventListener('mousemove', moveHandler)
    document.addEventListener('mouseup', upHandler)
    document.addEventListener('mouseleave', leaveHandler)
  }

  /** 卡片 click 事件中调用，判断是否刚完成一次原生拖拽 */
  function shouldSuppressClick() {
    if (!suppressClick) return false
    suppressClick = false
    return true
  }

  return { onCardMouseDown, shouldSuppressClick, nativeDragActive, INTERNAL_DRAG_FLAG }
}
