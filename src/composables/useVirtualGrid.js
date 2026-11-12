import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick, isRef } from 'vue'

/**
 * 网格虚拟滚动（参考 Eagle / Lightroom 的大库实现）
 *
 * 只渲染滚动视口内的行，其余行用 translateY 占位，
 * 从而把上千个卡片节点压缩到几十个，避免大量 DOM + IntersectionObserver 拖慢滚动。
 *
 * 行高统一（卡片高度固定），因此可以用 行号 * 行高 直接算出位置，
 * 不需要逐行测量，滚动时没有 layout thrashing。
 *
 * @param {Object} opts
 * @param {import('vue').Ref<HTMLElement|null>} opts.containerRef 滚动容器
 * @param {import('vue').Ref<HTMLElement|null>} opts.gridRef      网格容器（用它的 offsetTop/offsetLeft 换算坐标）
 * @param {import('vue').Ref<Array>}  opts.items        当前列表
 * @param {import('vue').Ref<number>} opts.minWidth     卡片最小宽度（决定列数）
 * @param {import('vue').Ref<number>} opts.infoHeight   卡片信息区高度（随密度变化）
 * @param {number} opts.gap        卡片间距
 * @param {number} opts.overscan   视口上下额外渲染的行数
 */
export function useVirtualGrid({
  containerRef,
  gridRef,
  items,
  minWidth,
  infoHeight,
  gap = 16,
  overscan = 2
}) {
  const unwrap = (v) => (isRef(v) ? v.value : v)
  const value = () => unwrap(items) || []

  const scrollTop = ref(0)
  const viewH = ref(900)
  const viewW = ref(1200)
  // 布局版本号：外部（预览尺寸 / 筛选结果）变化时递增，触发窗口重算
  const layoutVersion = ref(0)

  let rafId = null
  let ro = null

  function measure() {
    const el = containerRef.value
    const grid = gridRef.value
    if (el) viewH.value = el.clientHeight
    if (grid && grid.clientWidth) viewW.value = grid.clientWidth
    else if (el) viewW.value = el.clientWidth
  }

  function onScroll() {
    if (rafId != null) return
    rafId = requestAnimationFrame(() => {
      rafId = null
      const el = containerRef.value
      if (el) scrollTop.value = el.scrollTop
    })
  }

  const cols = computed(() => {
    layoutVersion.value
    const width = viewW.value || 1200
    const min = Math.max(40, unwrap(minWidth) || 100)
    return Math.max(1, Math.floor((width + gap) / (min + gap)))
  })

  const cardW = computed(() => {
    const width = viewW.value || 1200
    const c = cols.value
    return Math.floor((width - gap * (c - 1)) / c)
  })

  const coverH = computed(() => Math.round(cardW.value * 0.75))
  const cardH = computed(() => coverH.value + (unwrap(infoHeight) || 112))
  const rowH = computed(() => cardH.value + gap)

  const rows = computed(() => Math.ceil(value().length / cols.value) || 0)
  const totalHeight = computed(() => Math.max(0, rows.value * rowH.value - gap))

  /** 网格在滚动内容中的纵向偏移（文件夹导航高度变化后会变） */
  function gridTop() {
    layoutVersion.value
    return gridRef.value ? gridRef.value.offsetTop : 0
  }

  const startRow = computed(() => {
    const y = scrollTop.value - gridTop()
    return Math.max(0, Math.floor(y / rowH.value) - overscan)
  })

  const endRow = computed(() => {
    const y = scrollTop.value - gridTop() + viewH.value
    return Math.min(rows.value, Math.ceil(y / rowH.value) + overscan)
  })

  const visibleRows = computed(() => {
    const list = value()
    const c = cols.value
    const out = []
    for (let r = startRow.value; r < endRow.value; r++) {
      const from = r * c
      if (from >= list.length) break
      out.push({ key: r, top: r * rowH.value, items: list.slice(from, from + c) })
    }
    return out
  })

  /**
   * 第 index 个卡片在滚动容器坐标系里的矩形。
   * 供框选使用：即使卡片没渲染，也能按几何位置参与框选。
   */
  function rectAt(index) {
    const grid = gridRef.value
    const left0 = grid ? grid.offsetLeft : 0
    const top0 = grid ? grid.offsetTop : 0
    const c = cols.value
    const row = Math.floor(index / c)
    const col = index % c
    const left = left0 + col * (cardW.value + gap)
    const top = top0 + row * rowH.value
    return { left, top, right: left + cardW.value, bottom: top + cardH.value }
  }

  /** 布局变化后重算（列数/行高/导航高度可能都变了） */
  async function refresh() {
    measure()
    layoutVersion.value++
    await nextTick()
    measure()
    layoutVersion.value++
  }

  function scrollToIndex(index) {
    const el = containerRef.value
    if (!el) return
    const target = gridTop() + Math.floor(index / cols.value) * rowH.value
    el.scrollTop = Math.max(0, target - (el.clientHeight - cardH.value) / 2)
  }

  onMounted(() => {
    measure()
    const el = containerRef.value
    if (el) el.addEventListener('scroll', onScroll, { passive: true })
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver(() => {
        measure()
        layoutVersion.value++
      })
      if (gridRef.value) ro.observe(gridRef.value)
      if (el) ro.observe(el)
    }
    window.addEventListener('resize', onLayoutResize)
    layoutVersion.value++
  })

  function onLayoutResize() {
    measure()
    layoutVersion.value++
  }

  onBeforeUnmount(() => {
    if (rafId != null) cancelAnimationFrame(rafId)
    if (ro) ro.disconnect()
    ro = null
    containerRef.value?.removeEventListener('scroll', onScroll)
    window.removeEventListener('resize', onLayoutResize)
  })

  // 列表数量变化：可能从空变有（grid 重新挂载），刷新一次几何
  watch(
    () => value().length,
    async () => {
      await nextTick()
      measure()
      layoutVersion.value++
    }
  )

  return {
    cols,
    cardW,
    coverH,
    cardH,
    rowH,
    rows,
    totalHeight,
    visibleRows,
    rectAt,
    refresh,
    scrollToIndex
  }
}
