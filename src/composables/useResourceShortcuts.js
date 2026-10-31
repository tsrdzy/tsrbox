import { ref, onMounted, onUnmounted, nextTick } from 'vue'
import { useSettingsStore } from '../stores/settings'

const CARD_SELECTOR = '.card'
const ID_ATTRS = ['data-model-id']

const DEFAULT_SHORTCUTS = {
  selectAll: { key: 'a', ctrl: true },
  delete: { key: 'Delete', ctrl: false },
  export: { key: 's', ctrl: true },
  arrowUp: { key: 'ArrowUp', ctrl: false },
  arrowDown: { key: 'ArrowDown', ctrl: false },
  arrowLeft: { key: 'ArrowLeft', ctrl: false },
  arrowRight: { key: 'ArrowRight', ctrl: false }
}

export function useResourceShortcuts({
  filteredList,
  selectedIds,
  isMultiSelect,
  anchorId,
  contentRef,
  onSelectAll,
  onClearSelection,
  onDelete,
  onBatchDelete,
  onBatchAddTag,
  onExport,
  onRefresh,
  onOpenDetail,
  onAddTag
}) {
  const settingsStore = useSettingsStore()

  const contextMenuVisible = ref(false)
  const contextMenuPosition = ref({ x: 0, y: 0 })
  const contextMenuType = ref('')
  const contextMenuItem = ref(null)

  const gridCols = ref(5)
  let resizeObserver = null

  function activeShortcuts() {
    const stored = settingsStore.shortcuts || {}
    const merged = {}
    for (const key of Object.keys(DEFAULT_SHORTCUTS)) {
      merged[key] = stored[key] || DEFAULT_SHORTCUTS[key]
    }
    return merged
  }

  function calculateGridCols() {
    if (!contentRef.value) return
    const cards = contentRef.value.querySelectorAll(CARD_SELECTOR)
    if (cards.length < 1) {
      gridCols.value = 1
      return
    }
    const firstTop = cards[0].offsetTop
    let cols = 0
    for (let i = 0; i < cards.length; i++) {
      if (cards[i].offsetTop === firstTop) cols++
      else break
    }
    gridCols.value = Math.max(1, cols)
  }

  /** 判断事件是否命中某个快捷键配置 */
  function matches(e, sc) {
    if (!sc || !sc.key) return false
    const wantCtrl = !!sc.ctrl
    const hasCtrl = e.ctrlKey || e.metaKey
    if (wantCtrl !== hasCtrl) return false
    // 方向键 / Delete 等特殊键区分大小写；字母不区分
    if (sc.key.length === 1) {
      return e.key.toLowerCase() === sc.key.toLowerCase()
    }
    if (sc.key.toLowerCase() === 'delete') {
      return e.key === 'Delete' || e.key === 'Backspace'
    }
    return e.key === sc.key
  }

  function isTypingTarget(target) {
    if (!target) return false
    const tag = target.tagName
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
    if (target.isContentEditable) return true
    return false
  }

  function hasOpenOverlay() {
    return Array.from(document.querySelectorAll('.el-overlay')).some((el) => {
      const style = window.getComputedStyle(el)
      return style.display !== 'none' && style.visibility !== 'hidden'
    })
  }

  function handleKeydown(e) {
    // 弹窗打开时不响应列表快捷键
    if (hasOpenOverlay()) return
    if (isTypingTarget(e.target)) return

    const sc = activeShortcuts()

    if (matches(e, sc.selectAll)) {
      e.preventDefault()
      onSelectAll && onSelectAll()
      return
    }
    if (matches(e, sc.delete)) {
      e.preventDefault()
      if (selectedIds.value.length > 0 && onBatchDelete) onBatchDelete()
      return
    }
    if (matches(e, sc.export)) {
      e.preventDefault()
      if (selectedIds.value.length > 0 && onExport) onExport()
      return
    }
    if (matches(e, sc.arrowUp)) {
      e.preventDefault()
      handleArrowKey('ArrowUp')
      return
    }
    if (matches(e, sc.arrowDown)) {
      e.preventDefault()
      handleArrowKey('ArrowDown')
      return
    }
    if (matches(e, sc.arrowLeft)) {
      e.preventDefault()
      handleArrowKey('ArrowLeft')
      return
    }
    if (matches(e, sc.arrowRight)) {
      e.preventDefault()
      handleArrowKey('ArrowRight')
    }
  }

  function handleArrowKey(key) {
    const list = filteredList.value
    if (!list || list.length === 0) return

    if (selectedIds.value.length === 0) {
      selectedIds.value = [list[0].id]
      isMultiSelect.value = true
      anchorId.value = String(list[0].id)
      scrollToItem(0)
      return
    }

    const currentId = selectedIds.value[selectedIds.value.length - 1]
    const currentIdx = list.findIndex((item) => String(item.id) === String(currentId))
    if (currentIdx === -1) return

    calculateGridCols()
    const cols = gridCols.value

    let newIdx = currentIdx
    switch (key) {
      case 'ArrowUp':
        newIdx = currentIdx - cols
        break
      case 'ArrowDown':
        newIdx = currentIdx + cols
        break
      case 'ArrowLeft':
        newIdx = currentIdx - 1
        break
      case 'ArrowRight':
        newIdx = currentIdx + 1
        break
    }

    if (newIdx < 0) newIdx = 0
    if (newIdx >= list.length) newIdx = list.length - 1

    selectedIds.value = [list[newIdx].id]
    isMultiSelect.value = true
    anchorId.value = String(list[newIdx].id)
    scrollToItem(newIdx)
  }

  function scrollToItem(idx) {
    if (!contentRef.value) return
    const cards = contentRef.value.querySelectorAll(CARD_SELECTOR)
    const card = cards[idx]
    if (card) card.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
  }

  function findItemId(card) {
    for (const attr of ID_ATTRS) {
      const val = card.getAttribute(attr)
      if (val) return val
    }
    return null
  }

  function handleContextMenu(e) {
    e.preventDefault()

    const card = e.target.closest(CARD_SELECTOR)
    if (card) {
      const itemId = findItemId(card)
      const item = filteredList.value.find((i) => String(i.id) === itemId)
      if (item) {
        contextMenuType.value = selectedIds.value.length > 1 ? 'batch' : 'item'
        contextMenuItem.value = item
      } else {
        contextMenuType.value = 'blank'
        contextMenuItem.value = null
      }
    } else {
      contextMenuType.value = 'blank'
      contextMenuItem.value = null
    }

    contextMenuPosition.value = { x: e.clientX, y: e.clientY }
    contextMenuVisible.value = true
  }

  function closeContextMenu() {
    contextMenuVisible.value = false
  }

  function handleContextMenuCommand(cmd) {
    closeContextMenu()
    switch (cmd) {
      case 'select-all':
        onSelectAll && onSelectAll()
        break
      case 'refresh':
        onRefresh && onRefresh()
        break
      case 'open-detail':
        if (contextMenuItem.value && onOpenDetail) onOpenDetail(contextMenuItem.value)
        break
      case 'add-tag':
        if (contextMenuItem.value && onAddTag) onAddTag(contextMenuItem.value)
        break
      case 'delete':
        if (contextMenuItem.value && onDelete) onDelete(contextMenuItem.value)
        break
      case 'batch-add-tag':
        onBatchAddTag && onBatchAddTag()
        break
      case 'batch-delete':
        onBatchDelete && onBatchDelete()
        break
    }
  }

  onMounted(() => {
    document.addEventListener('keydown', handleKeydown)
    document.addEventListener('click', closeContextMenu)
    nextTick(() => {
      calculateGridCols()
      if (contentRef.value && window.ResizeObserver) {
        resizeObserver = new ResizeObserver(() => calculateGridCols())
        resizeObserver.observe(contentRef.value)
      }
      window.addEventListener('resize', calculateGridCols)
    })
  })

  onUnmounted(() => {
    document.removeEventListener('keydown', handleKeydown)
    document.removeEventListener('click', closeContextMenu)
    window.removeEventListener('resize', calculateGridCols)
    if (resizeObserver) {
      resizeObserver.disconnect()
      resizeObserver = null
    }
  })

  return {
    contextMenuVisible,
    contextMenuPosition,
    contextMenuType,
    contextMenuItem,
    handleContextMenu,
    handleContextMenuCommand
  }
}
