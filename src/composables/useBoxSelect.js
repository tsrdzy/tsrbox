import { ref } from 'vue'

/**
 * 统一的框选 composable（模型/图片/音频/字体列表共用）
 *
 * 性能要点：
 * 1. mousedown 时一次性缓存所有卡片的矩形坐标（offset 系列），拖动过程中不再读 DOM，
 *    避免每帧 querySelectorAll + 强制同步布局（layout thrashing）导致的掉帧。
 * 2. mousemove 用 requestAnimationFrame 合帧，每帧最多一次更新。
 * 3. 选择框样式通过 selectionBoxRef 直接写 DOM，不走响应式，避免每帧触发整组件重渲染。
 * 4. 仅当选中集合真正发生变化时才写入 selectedIds，避免每帧触发卡片重渲染。
 * 5. 拖动期间对 content 加 user-select:none，防止文本选中干扰。
 *
 * @param {Object} opts
 * @param {Ref<HTMLElement|null>} opts.contentRef
 * @param {Ref<HTMLElement|null>} opts.selectionBoxRef - 框选框 DOM 引用
 * @param {string} opts.cardSelector   - 卡片选择器，如 '.card'
 * @param {string} opts.idAttr         - 卡片 id 属性，如 'data-model-id'
 * @param {Ref<Array>}  opts.selectedIds
 * @param {Ref<boolean>} opts.isMultiSelect
 * @param {Ref<string|null>} opts.anchorId
 * @param {Object} [opts.virtual] 虚拟滚动几何信息：{ count: () => number, idAt: (i) => string, rectAt: (i) => rect }
 *        提供后框选按几何位置计算，未渲染的卡片同样可被框选。
 */
export function useBoxSelect({ contentRef, selectionBoxRef, cardSelector, idAttr, selectedIds, isMultiSelect, anchorId, virtual }) {
  const isDragging = ref(false)
  const showSelectionBox = ref(false)
  const dragStartGlobal = ref({ x: 0, y: 0 })
  const dragEndGlobal = ref({ x: 0, y: 0 })

  let lastMouseX = 0
  let lastMouseY = 0
  let rafId = null
  let justBoxSelected = false

  // 拖动终点坐标（局部变量，不走响应式，避免触发重渲染）
  let dragEnd = { x: 0, y: 0 }

  // mousedown 时缓存的卡片矩形列表：{ id, left, top, right, bottom }
  let cachedRects = []

  function cacheCardRects() {
    // 虚拟滚动模式：直接按几何位置生成全部卡片的矩形，不依赖 DOM
    if (virtual) {
      const total = virtual.count()
      const rects = new Array(total)
      for (let i = 0; i < total; i++) {
        const r = virtual.rectAt(i)
        rects[i] = { id: virtual.idAt(i), left: r.left, top: r.top, right: r.right, bottom: r.bottom }
      }
      cachedRects = rects
      return
    }
    if (!contentRef.value) {
      cachedRects = []
      return
    }
    const cards = contentRef.value.querySelectorAll(cardSelector)
    const rects = new Array(cards.length)
    for (let i = 0; i < cards.length; i++) {
      const card = cards[i]
      rects[i] = {
        id: card.getAttribute(idAttr),
        left: card.offsetLeft,
        top: card.offsetTop,
        right: card.offsetLeft + card.offsetWidth,
        bottom: card.offsetTop + card.offsetHeight
      }
    }
    cachedRects = rects
  }

  function selectionFromRects(startX, startY, endX, endY) {
    const result = []
    for (let i = 0; i < cachedRects.length; i++) {
      const r = cachedRects[i]
      if (r.id && r.left < endX && r.right > startX && r.top < endY && r.bottom > startY) {
        result.push(r.id)
      }
    }
    return result
  }

  /** 直接写选择框 DOM 样式，不走响应式，避免整组件重渲染 */
  function applySelectionBoxStyle(sx, sy, ex, ey) {
    const el = selectionBoxRef?.value
    if (!el) return
    const s = el.style
    s.left = sx + 'px'
    s.top = sy + 'px'
    s.width = (ex - sx) + 'px'
    s.height = (ey - sy) + 'px'
  }

  function flushDragEnd() {
    rafId = null
    if (!isDragging.value || !contentRef.value) return
    const rect = contentRef.value.getBoundingClientRect()
    const vx = lastMouseX - rect.left
    const vy = lastMouseY - rect.top
    // 仅更新局部变量，不写响应式 ref，避免触发组件重渲染
    dragEnd = {
      x: vx + contentRef.value.scrollLeft,
      y: vy + contentRef.value.scrollTop
    }
    const sx = Math.min(dragStartGlobal.value.x, dragEnd.x)
    const sy = Math.min(dragStartGlobal.value.y, dragEnd.y)
    const ex = Math.max(dragStartGlobal.value.x, dragEnd.x)
    const ey = Math.max(dragStartGlobal.value.y, dragEnd.y)

    // 拖动过程中只更新选择框样式（直接写 DOM，不触发组件重渲染），
    // 选中集合留到 mouseup 再计算——这是原始流畅实现的关键。
    applySelectionBoxStyle(sx, sy, ex, ey)
  }

  /** 计算最终选中集合（mouseup 时调用一次） */
  function computeFinalSelection() {
    const sx = Math.min(dragStartGlobal.value.x, dragEnd.x)
    const sy = Math.min(dragStartGlobal.value.y, dragEnd.y)
    const ex = Math.max(dragStartGlobal.value.x, dragEnd.x)
    const ey = Math.max(dragStartGlobal.value.y, dragEnd.y)
    return selectionFromRects(sx, sy, ex, ey)
  }

  function scheduleUpdate() {
    if (rafId != null) return
    rafId = requestAnimationFrame(flushDragEnd)
  }

  function handleMouseMove(e) {
    if (!isDragging.value) return
    lastMouseX = e.clientX
    lastMouseY = e.clientY
    scheduleUpdate()
  }

  function handleMouseUp(e) {
    if (!isDragging.value) return
    e.stopPropagation()
    if (rafId != null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    flushDragEnd()
    isDragging.value = false
    showSelectionBox.value = false
    document.removeEventListener('mousemove', handleMouseMove)
    document.removeEventListener('mouseup', handleMouseUp)
    if (contentRef.value) {
      contentRef.value.removeEventListener('scroll', handleScroll)
      contentRef.value.classList.remove('box-selecting')
    }

    // mouseup 时一次性计算最终选中集合（拖动过程中不更新，避免卡顿）
    const newSelected = computeFinalSelection()
    if (newSelected.length > 0) {
      selectedIds.value = newSelected
      isMultiSelect.value = true
    } else if (!(e.ctrlKey || e.metaKey)) {
      // 空框选且无修饰键 → 清空选中
      selectedIds.value = []
      isMultiSelect.value = false
      anchorId.value = null
    }

    cachedRects = []
    justBoxSelected = true
  }

  function handleScroll() {
    if (!isDragging.value) return
    scheduleUpdate()
  }

  function handleMouseDown(e) {
    if (!contentRef.value) return
    if (e.button !== 0) return
    if (e.target.closest('.card, .image-card, .audio-card, .font-card')) return
    if (e.ctrlKey || e.metaKey || e.shiftKey) return

    e.preventDefault()

    // 一次性缓存所有卡片矩形，拖动期间不再读 DOM
    cacheCardRects()

    isDragging.value = true
    lastMouseX = e.clientX
    lastMouseY = e.clientY
    const rect = contentRef.value.getBoundingClientRect()
    const vx = e.clientX - rect.left
    const vy = e.clientY - rect.top
    dragStartGlobal.value = {
      x: vx + contentRef.value.scrollLeft,
      y: vy + contentRef.value.scrollTop
    }
    dragEndGlobal.value = { ...dragStartGlobal.value }
    showSelectionBox.value = true
    contentRef.value.classList.add('box-selecting')

    // 初始化选择框样式（0 尺寸）
    applySelectionBoxStyle(dragStartGlobal.value.x, dragStartGlobal.value.y, dragStartGlobal.value.x, dragStartGlobal.value.y)

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
    contentRef.value.addEventListener('scroll', handleScroll)
  }

  /** 容器 click 事件中调用：若刚完成框选则跳过清除操作 */
  function consumeBoxSelect() {
    if (justBoxSelected) {
      justBoxSelected = false
      return true
    }
    return false
  }

  return {
    isDragging,
    showSelectionBox,
    dragStartGlobal,
    dragEndGlobal,
    handleMouseDown,
    consumeBoxSelect
  }
}
