import { reactive } from 'vue'

/**
 * 封面共享缓存
 * 列表接口不再内嵌封面 base64（上千模型时单次 IPC 响应过大），
 * 卡片挂载时按需读取并缓存，滚动复用不重复请求。
 *
 * 快速拖动滚动条时会瞬间掠过大量行，
 * 因此队列由列表按「当前视口」驱动（setWanted）：
 * 视口一变就丢弃旧视口遗留的排队请求，只按新视口从上到下的顺序读取，
 * 不会出现"从上面慢慢加载到最下面"的观感。
 */
const cache = reactive(new Map())
const pending = new Map()
const queue = []
const queued = new Set()
// 读取失败（封面文件缺失等）的 id 不再重试，避免每帧重复请求
const failed = new Set()

// 同时最多 8 个读取请求，其余排队
const MAX_CONCURRENT = 8
// 缓存上限：虚拟滚动下同时可见的卡片通常 < 100，
// 留出余量后按最久未使用淘汰，避免浏览整库时 base64 无限堆积占内存
const MAX_ENTRIES = 600
let active = 0

function evictIfNeeded() {
  while (cache.size > MAX_ENTRIES) {
    const oldest = cache.keys().next().value
    if (oldest === undefined) break
    cache.delete(oldest)
  }
}

function pump() {
  while (active < MAX_CONCURRENT && queue.length) {
    const id = queue.shift()
    queued.delete(id)
    if (cache.has(id) || pending.has(id) || failed.has(id)) continue
    active++
    const p = window.api.models
      .readImage(id, true, false)
      .then((url) => {
        const value = url || ''
        cache.set(id, value)
        evictIfNeeded()
        return value
      })
      .catch(() => {
        failed.add(id)
        return ''
      })
      .finally(() => {
        active--
        pending.delete(id)
        pump()
      })
    pending.set(id, p)
  }
}

/**
 * 列表告知「当前视口需要哪些封面」
 * 队列按传入顺序（视口从上到下）重建，旧视口遗留的排队请求立即丢弃；
 * 正在读取的少数请求不打断，结束后自然从新队列继续。
 */
function setWanted(models) {
  const next = []
  const seen = new Set()
  for (const m of models || []) {
    if (!m || !m.cover) continue
    const id = String(m.id)
    if (seen.has(id)) continue
    seen.add(id)
    next.push(id)
  }
  queue.length = 0
  queued.clear()
  for (const id of next) {
    if (cache.has(id) || pending.has(id) || failed.has(id)) continue
    queued.add(id)
    queue.push(id)
  }
  pump()
}

/** 请求封面：命中缓存直接返回，否则入队（不阻塞渲染，缓存更新后卡片自动刷新） */
function ensureCover(model) {
  if (!model || !model.cover) return Promise.resolve('')
  const id = String(model.id)
  if (cache.has(id)) return Promise.resolve(cache.get(id))
  if (pending.has(id)) return pending.get(id)
  if (queued.has(id) || failed.has(id)) return Promise.resolve('')
  queued.add(id)
  queue.push(id)
  pump()
  return Promise.resolve('')
}

/** 卡片离开视口 / 被回收时调用：还没开始的请求直接丢弃 */
function cancelCover(id) {
  const key = String(id)
  if (!queued.has(key)) return
  queued.delete(key)
  const i = queue.indexOf(key)
  if (i >= 0) queue.splice(i, 1)
}

function coverOf(id) {
  return cache.get(String(id)) || ''
}

function setCover(id, url) {
  const key = String(id)
  failed.delete(key)
  cache.set(key, url || '')
  evictIfNeeded()
}

function dropCover(id) {
  const key = String(id)
  cancelCover(key)
  failed.delete(key)
  cache.delete(key)
}

export function useCovers() {
  return { setWanted, ensureCover, cancelCover, coverOf, setCover, dropCover }
}
