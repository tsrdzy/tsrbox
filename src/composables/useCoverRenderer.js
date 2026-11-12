import { reactive, ref, shallowRef } from 'vue'
import { useCovers } from './useCovers'

/**
 * 全局封面渲染器
 * 隐藏 ModelViewer 常驻 App.vue（首次使用时挂载，之后复用同一 WebGL 上下文），
 * 因此「批量上传封面」「一键重建封面」都不会因路由切换而中断，
 * 且逐模型渲染不再重建组件/GL 上下文，速度显著提升。
 */
const mounted = ref(false)
const buffer = shallowRef(null)
const fileType = ref('')
const auxFiles = ref([])
const viewParams = ref(null)
const viewerRef = ref(null)

let pendingResolve = null
let pendingReject = null
let renderTimeout = null

/** 一键重建封面进度（标题栏展示用） */
const regen = reactive({
  running: false,
  current: 0,
  total: 0,
  success: 0,
  failed: 0,
  cancelled: false
})

/** 取消标记：在模型之间生效，当前正在渲染的模型会正常完成并保存 */
let cancelRequested = false

function cancelRegen() {
  if (regen.running) cancelRequested = true
}

function base64ToUint8Array(base64) {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

function settle() {
  pendingResolve = null
  pendingReject = null
  if (renderTimeout) {
    clearTimeout(renderTimeout)
    renderTimeout = null
  }
}

/** 渲染单个模型并截图（有 idle 动画时按 idle 姿态） */
function renderModelForCover(base64, type, aux = [], params = null) {
  return new Promise((resolve, reject) => {
    // 串行使用；若上一个仍在挂起则直接失败它，避免超时泄漏
    if (pendingReject) {
      pendingReject(new Error('渲染被新任务取代'))
      settle()
    }
    pendingResolve = resolve
    pendingReject = reject
    mounted.value = true
    // buffer 引用每次必新 → 触发 ModelViewer 内部 watch 重新加载（不重建组件）
    auxFiles.value = aux || []
    viewParams.value = params || null
    fileType.value = type
    buffer.value = base64ToUint8Array(base64)
    renderTimeout = setTimeout(() => {
      if (pendingReject) {
        pendingReject(new Error('模型渲染超时'))
        settle()
      }
    }, 30000)
  })
}

function onViewerLoaded(payload) {
  if (!pendingResolve) return
  // 等 2 帧确保渲染完成
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      let cover = null
      try {
        // 有 idle 动画时按 idle 姿态截图
        cover = viewerRef.value?.captureCover?.() || viewerRef.value?.captureThumbnail() || null
      } catch (e) {
        console.warn('封面截图失败:', e)
      }
      const resolve = pendingResolve
      settle()
      resolve({
        dimensions: payload?.dimensions || { x: 0, y: 0, z: 0 },
        animationCount: payload?.animationCount || 0,
        cover
      })
    })
  })
}

function onViewerError(err) {
  if (!pendingReject) return
  const reject = pendingReject
  settle()
  reject(err instanceof Error ? err : new Error(String(err)))
}

/** 一键重建全部模型封面（后台运行，切换页面不中断） */
async function regenerateAll(models) {
  if (regen.running || !models?.length) return null
  cancelRequested = false
  Object.assign(regen, { running: true, current: 0, total: models.length, success: 0, failed: 0, cancelled: false })
  const covers = useCovers()
  for (const m of models) {
    if (cancelRequested) {
      regen.cancelled = true
      break
    }
    regen.current++
    try {
      const res = await window.api.models.loadFile({
        id: String(m.id),
        fileName: String(m.fileName),
        fileType: String(m.fileType),
        files: JSON.parse(JSON.stringify(m.files || []))
      })
      const rendered = await renderModelForCover(
        res.dataBase64,
        m.fileType,
        JSON.parse(JSON.stringify(res.auxFiles || []))
      )
      if (rendered.cover) {
        await window.api.models.saveImage(String(m.id), 'cover', rendered.cover)
        covers.setCover(m.id, rendered.cover)
        m.cover = true
      }
      regen.success++
    } catch (e) {
      regen.failed++
      console.warn('[RegenCovers] 失败:', m.fileName, e)
    }
  }
  regen.running = false
  return { success: regen.success, failed: regen.failed, cancelled: regen.cancelled }
}

export function useCoverRenderer() {
  return {
    mounted,
    buffer,
    fileType,
    auxFiles,
    viewParams,
    viewerRef,
    regen,
    cancelRegen,
    renderModelForCover,
    onViewerLoaded,
    onViewerError,
    regenerateAll
  }
}
