import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

let seq = 0
let hideTimer = null

/**
 * 全局后台上传任务
 * 进度显示在标题栏最右侧（最顶部）：一条汇总进度条，点击后在下方展开每个任务的明细。
 * 上传中始终可见；用户点击「收起」后只保留标题栏小胶囊，不再常驻展开；
 * 全部完成后自动收起并清空，不在界面上遗留。
 */
export const useUploadStore = defineStore('upload', () => {
  const tasks = ref([])
  const dockVisible = ref(false) // 顶部进度条（含下拉明细）是否显示
  const panelOpen = ref(false) // 下拉明细是否展开
  const minimized = ref(false) // 用户收起后转为标题栏小胶囊

  const runningTasks = computed(() => tasks.value.filter((t) => t.status === 'running'))
  const hasActive = computed(() => runningTasks.value.length > 0)
  const overallPercent = computed(() => {
    if (tasks.value.length === 0) return 0
    const sum = tasks.value.reduce((acc, t) => acc + (t.status === 'done' ? 100 : t.progress || 0), 0)
    return Math.round(sum / tasks.value.length)
  })
  const summaryText = computed(() => {
    const running = runningTasks.value
    if (running.length === 0) {
      const done = tasks.value.filter((t) => t.status === 'done')
      return done.length ? '上传完成' : ''
    }
    const current = running.reduce((acc, t) => acc + (t.current || 0), 0)
    const total = running.reduce((acc, t) => acc + (t.total || 0), 0)
    return total > 0 ? `正在上传 ${current}/${total}` : '正在上传'
  })

  /** 标题栏进度条文案 */
  const barTitle = computed(() => summaryText.value || '上传')
  /** 收起后的小胶囊：展示汇总状态 + 百分比 */
  const restoreLabel = computed(() => {
    const text = summaryText.value || '上传'
    return overallPercent.value < 100 ? `${text} ${overallPercent.value}%` : text
  })
  const restoreVisible = computed(() => tasks.value.length > 0 && !dockVisible.value && minimized.value)

  /** 为任务生成一行简明描述（进度明细里显示在文件名下方） */
  function decorate(task) {
    if (task.status === 'error') {
      task.detail = task.message || `失败 ${task.failed || 0}`
      return
    }
    if (task.status === 'done') {
      if (task.message) {
        task.detail = task.message
        return
      }
      const parts = []
      if (task.success) parts.push(`成功 ${task.success}`)
      if (task.failed) parts.push(`失败 ${task.failed}`)
      task.detail = parts.join(' · ') || '已完成'
      return
    }
    task.detail = task.total > 0 ? `${task.current || 0}/${task.total}` : '准备中'
  }

  function cancelHideTimer() {
    if (hideTimer) {
      clearTimeout(hideTimer)
      hideTimer = null
    }
  }

  function reset() {
    tasks.value = []
    dockVisible.value = false
    panelOpen.value = false
    minimized.value = false
  }

  function begin(payload = {}) {
    cancelHideTimer()
    const id = ++seq
    const task = {
      id,
      title: payload.title || '上传',
      type: payload.type || '',
      progress: 0,
      current: 0,
      total: payload.total || 0,
      fileName: payload.fileName || '',
      status: 'running',
      success: 0,
      failed: 0,
      message: '',
      detail: '',
      details: []
    }
    decorate(task)
    tasks.value.push(task)
    // 开始上传：立刻在顶部展示进度并展开明细，避免"没有提示"
    dockVisible.value = true
    panelOpen.value = true
    minimized.value = false
    return id
  }

  function update(id, patch = {}) {
    const task = tasks.value.find((t) => t.id === id)
    if (!task) return
    Object.assign(task, patch)
    decorate(task)
  }

  function finish(id, payload = {}) {
    const task = tasks.value.find((t) => t.id === id)
    if (!task) return
    task.status = payload.error ? 'error' : 'done'
    task.progress = 100
    task.success = payload.success || 0
    task.failed = payload.failed || 0
    task.message = payload.message || ''
    if (Array.isArray(payload.details)) task.details = payload.details
    decorate(task)
    if (hasActive.value) return
    // 全部完成后延时收起，不在界面上常驻
    cancelHideTimer()
    hideTimer = setTimeout(() => {
      hideTimer = null
      if (!hasActive.value) reset()
    }, 4000)
  }

  /** 完全隐藏 */
  function hideBar() {
    dockVisible.value = false
    panelOpen.value = false
    minimized.value = false
  }

  function showBar() {
    cancelHideTimer()
    dockVisible.value = true
    minimized.value = false
  }

  function togglePanel() {
    panelOpen.value = !panelOpen.value
  }

  /** 收起为标题栏小胶囊（关闭后不常驻展开） */
  function minimizeToRestore() {
    dockVisible.value = false
    panelOpen.value = false
    minimized.value = tasks.value.length > 0
  }

  /** 点击小胶囊重新展开进度明细 */
  function restorePanel() {
    dockVisible.value = true
    panelOpen.value = true
    minimized.value = false
  }

  function clear() {
    cancelHideTimer()
    reset()
  }

  return {
    tasks,
    dockVisible,
    panelOpen,
    minimized,
    runningTasks,
    hasActive,
    overallPercent,
    summaryText,
    barTitle,
    restoreLabel,
    restoreVisible,
    begin,
    update,
    finish,
    hideBar,
    showBar,
    togglePanel,
    minimizeToRestore,
    restorePanel,
    clear
  }
})
