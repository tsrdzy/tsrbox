import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

let seq = 0

/**
 * 全局后台上传任务
 * 上传不再使用阻塞式弹窗，所有进度汇总到底部进度条。
 */
export const useUploadStore = defineStore('upload', () => {
  const tasks = ref([])
  const barVisible = ref(false)
  const panelOpen = ref(false)

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

  function begin(payload = {}) {
    const id = ++seq
    tasks.value.push({
      id,
      title: payload.title || '上传',
      type: payload.type || '',
      progress: 0,
      current: 0,
      total: payload.total || 0,
      fileName: '',
      status: 'running',
      success: 0,
      failed: 0,
      message: '',
      details: []
    })
    barVisible.value = true
    return id
  }

  function update(id, patch = {}) {
    const task = tasks.value.find((t) => t.id === id)
    if (!task) return
    Object.assign(task, patch)
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
    barVisible.value = true
    if (hasActive.value) return
    // 全部完成后延时收起
    setTimeout(() => {
      if (!hasActive.value) {
        panelOpen.value = false
        barVisible.value = false
      }
    }, 4000)
  }

  function hideBar() {
    barVisible.value = false
    panelOpen.value = false
  }

  function showBar() {
    barVisible.value = true
  }

  function togglePanel() {
    panelOpen.value = !panelOpen.value
    barVisible.value = true
  }

  function clear() {
    tasks.value = []
    panelOpen.value = false
    barVisible.value = false
  }

  return {
    tasks,
    barVisible,
    panelOpen,
    runningTasks,
    hasActive,
    overallPercent,
    summaryText,
    begin,
    update,
    finish,
    hideBar,
    showBar,
    togglePanel,
    clear
  }
})
