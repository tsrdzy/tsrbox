import { ref } from 'vue'
import { useStore } from './useStore'

/**
 * 卡片上按住拖动：
 * 1. 拖到侧边栏文件夹 → 通过回调在应用内移动模型（不发起系统拖出）
 * 2. 拖到窗口外 → 调用主进程 startDrag，把资源源文件拖到桌面/其他应用
 *
 * 拖动中有跟随指针的悬浮提示（文件名 + 数量 + 目标文件夹名），
 * 鼠标移动事件经 rAF 节流，避免高频 elementFromPoint 造成卡顿。
 *
 * @param {Object} options
 * @param {string} options.type - models | images | audios | fonts
 * @param {number} options.threshold - 触发拖动的位移阈值（px）
 * @param {string} options.folderSelector - 文件夹放置区选择器（带 data-folder-drop 属性）
 * @param {Function} options.onFolderHover - 悬停文件夹回调 (key) => void
 * @param {Function} options.onFolderDrop - 释放到文件夹回调 (key, ids) => void
 * @param {Function} options.getFolderLabel - 取文件夹显示名 (key) => string
 * @param {string|Function} options.outsideHint - 指针靠近窗口边缘时的提示文案
 */
export function useNativeDrag({
  type,
  threshold = 6,
  folderSelector = '',
  onFolderHover = null,
  onFolderDrop = null,
  getFolderLabel = null,
  outsideHint = ''
} = {}) {
  const nativeDragActive = ref(false)
  let pressing = null
  let startX = 0
  let startY = 0
  let moveHandler = null
  let upHandler = null
  let leaveHandler = null
  let suppressClick = false
  // rAF 节流
  let rafId = 0
  let lastEvent = null
  // 悬浮提示
  let ghostEl = null

  function setDraggingCursor(on) {
    try {
      document.body.classList.toggle('tsrbox-dragging', !!on)
    } catch {
      // ignore
    }
  }

  function ensureGhost() {
    if (ghostEl) return
    ghostEl = document.createElement('div')
    ghostEl.className = 'tsrbox-drag-ghost'
    document.body.appendChild(ghostEl)
  }

  function destroyGhost() {
    if (ghostEl) {
      ghostEl.remove()
      ghostEl = null
    }
  }

  function resolveOutsideHint() {
    return typeof outsideHint === 'function' ? outsideHint() : outsideHint
  }

  /** 刷新悬浮提示的位置与内容 */
  function refreshGhost(ev) {
    if (!ghostEl || !pressing) return
    ghostEl.style.transform = `translate(${ev.clientX + 14}px, ${ev.clientY + 16}px)`

    const { item, selectedItems: list, folderKey, mode } = pressing
    const useSelection =
      list.length > 1 && list.some((x) => String(x.id) === String(item.id))
    const count = useSelection ? list.length : 1
    let label = item.fileName || ''
    if (count > 1) label += ` ×${count}`

    ghostEl.classList.remove('is-target', 'is-out')
    if (mode === 'folder' && folderKey) {
      const name = getFolderLabel?.(folderKey) || ''
      if (name) label += ` → ${name}`
      ghostEl.classList.add('is-target')
    } else if (isOutsideWindow(ev)) {
      const hint = resolveOutsideHint()
      if (hint) label += ` · ${hint}`
      ghostEl.classList.add('is-out')
    }
    if (ghostEl.textContent !== label) ghostEl.textContent = label
  }

  function cleanup() {
    document.removeEventListener('mousemove', moveHandler)
    document.removeEventListener('mouseup', upHandler)
    document.removeEventListener('mouseleave', leaveHandler)
    moveHandler = null
    upHandler = null
    leaveHandler = null
    if (rafId) {
      cancelAnimationFrame(rafId)
      rafId = 0
    }
    lastEvent = null
    pressing = null
    destroyGhost()
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

  /** rAF 节流的移动处理：每帧最多一次 elementFromPoint / DOM 更新 */
  function processMove() {
    rafId = 0
    const ev = lastEvent
    if (!pressing || !ev) return

    if (!pressing.mode) {
      const dx = ev.clientX - startX
      const dy = ev.clientY - startY
      if (Math.abs(dx) < threshold && Math.abs(dy) < threshold) return
      // 先进入「待定」状态：既能拖到侧边栏文件夹，也能拖出窗口
      pressing.mode = 'maybe'
      suppressClick = true
      setDraggingCursor(true)
      ensureGhost()
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
      } else if (pressing.mode === 'folder') {
        // 移出文件夹区域，回到待定状态
        pressing.mode = 'maybe'
        pressing.folderKey = ''
        onFolderHover?.('')
      }
    }

    refreshGhost(ev)

    // 指针移出窗口 → 转为系统拖出
    if (isOutsideWindow(ev)) {
      startNativeDrag(pressing.item, pressing.selectedItems)
    }
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
      lastEvent = ev
      if (!rafId) rafId = requestAnimationFrame(processMove)
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
