/**
 * Blockbench 动画解析与播放
 * 语义与 Blockbench 源码对齐（js/animations/timeline_animators.js / keyframe.js / animation_mode.js）：
 * - 关键帧 data_points：rotation 单位为度、position 为模型单位（16=1格）、scale 为倍率
 * - 应用方式：每帧先复位到静止姿态（showDefaultPose），再叠加插值结果
 *   （displayRotation += 度转弧度；displayPosition += 原值；displayScale 乘法）
 * - 插值：linear / step / catmullrom（THREE.SplineCurve）/ bezier（CubicBezierCurve 采样）
 * - molang：支持数学表达式子集（math.* / query.anim_time），未知变量回退 0
 * - loop='loop' 循环播放；'once' 停在末帧；'hold' 保持末帧
 */
import * as THREE from 'three'

const EPS = 1 / 1200
const D2R = THREE.MathUtils.degToRad

function num(v) {
  const n = typeof v === 'number' ? v : parseFloat(v)
  return Number.isFinite(n) ? n : 0
}

/* ---------------- molang 数值求值（数学表达式子集） ---------------- */

const DEG = Math.PI / 180
// molang math.* 三角函数以「度」为单位（与 Blockbench molang.js 一致）
const MOLANG_MATH = {
  pi: Math.PI,
  e: Math.E,
  abs: Math.abs,
  floor: Math.floor,
  ceil: Math.ceil,
  trunc: Math.trunc,
  sqrt: Math.sqrt,
  pow: Math.pow,
  sin: (d) => Math.sin(d * DEG),
  cos: (d) => Math.cos(d * DEG),
  asin: (x) => Math.asin(x) / DEG,
  acos: (x) => Math.acos(x) / DEG,
  atan: (x) => Math.atan(x) / DEG,
  atan2: (y, x) => Math.atan2(y, x) / DEG,
  mod: (a, b) => a - b * Math.floor(a / b),
  clamp: (v, a, b) => Math.min(Math.max(v, a), b),
  lerp: (a, b, t) => a + (b - a) * t,
  lerprotate: (a, b, t) => a + ((((b - a) % 360) + 540) % 360 - 180) * t,
  random: (a = 1, b) => (b === undefined ? Math.random() * a : a + Math.random() * (b - a)),
  die_roll: (a, b) => a + Math.random() * (b - a),
}
const molangCache = new Map()

/** 编译 molang 表达式为 JS 函数；不支持的结构返回 null（调用方回退 0） */
function compileMolang(src) {
  let s = String(src).trim()
  if (!s) return null
  // 多语句：分号分隔，取 return 子句或最后一段（与 molang 语义一致）
  if (s.includes(';')) {
    const parts = s.split(';').map((p) => p.trim()).filter(Boolean)
    if (parts.some((p) => /[{}]/.test(p))) return null
    const ri = parts.findIndex((p) => /^return\b/i.test(p))
    s = ri >= 0 ? parts[ri].replace(/^return\s*/i, '') : parts[parts.length - 1]
  }
  if (/[{}]/.test(s)) return null
  s = s
    .replace(/\b(?:query|q)\.anim_time\b/gi, '(q)')
    .replace(/\b(?:query|variable|context|temp|v|q|c|t)\.([a-z0-9_.]+)/gi, '0')
    .replace(
      /\bmath\.(sin|cos|asin|acos|atan|atan2|sqrt|pow|floor|ceil|trunc|abs|mod|clamp|lerp|lerprotate|random|die_roll)\s*\(/gi,
      (m, f) => 'M.' + f.toLowerCase() + '(',
    )
    .replace(/\bmath\.pi\b/gi, 'M.pi')
  // 残留的字母只允许是已映射的 M.xxx 函数和 (q) 时间参数；其余裸标识符直接放弃（回退 0）
  if (/[a-zA-Z_]/.test(s.replace(/M\.[a-z0-9_]+/gi, '').replace(/\bq\b/g, ''))) return null
  try {
    return new Function('q', 'M', '"use strict";return (' + s + ');')
  } catch {
    return null
  }
}

/** 数据点求值：数字直通 / 数字字符串解析 / molang 表达式（ctx.t = 动画时间） */
function evalValue(v, ctx) {
  if (typeof v === 'number') return Number.isFinite(v) ? v : 0
  if (typeof v !== 'string') return 0
  const s = v.trim()
  if (s === '' || s === '0') return 0
  const n = Number(s)
  if (Number.isFinite(n)) return n
  let fn = molangCache.get(s)
  if (fn === undefined) {
    fn = compileMolang(s)
    molangCache.set(s, fn)
  }
  if (!fn) return 0
  try {
    const r = fn(ctx?.t ?? 0, MOLANG_MATH)
    return Number.isFinite(r) ? r : 0
  } catch {
    return 0
  }
}

/** 解析 bbmodel 的 animations 数组 → 播放器可用动画列表（数据点保留原始值，求值推迟到播放） */
export function parseAnimations(list) {
  const out = []
  // 兼容两种导出结构：官方为数组，部分生成器写成以 uuid/名称为键的对象
  const arr = Array.isArray(list) ? list : list && typeof list === 'object' ? Object.values(list) : []
  for (const a of arr) {
    if (!a || typeof a !== 'object') continue
    const animators = {}
    let maxT = 0
    for (const [key, bp] of Object.entries(a.animators || {})) {
      // effects（粒子/声音）等没有 keyframes 数组，跳过
      if (!bp || !Array.isArray(bp.keyframes)) continue
      const channels = { rotation: [], position: [], scale: [] }
      for (const kf of bp.keyframes) {
        const ch = channels[kf.channel]
        if (!ch) continue
        const pts = kf.data_points && kf.data_points.length ? kf.data_points : [{ x: 0, y: 0, z: 0 }]
        // uniform 语义与 Blockbench keyframe.js:97 一致：仅当 scale 关键帧 x/y/z 完全相等时才视为统一值，
        // 否则按各轴独立读取（AI 生成模型常见 uniform=true 但带 z 拉伸值，如舌头伸出 scale z=7）
        const uniform = kf.uniform && !pts.some((p) => p?.x != p?.y || p?.x != p?.z)
        ch.push({
          time: num(kf.time),
          interpolation: kf.interpolation || 'linear',
          // 保留原始值（数字或 molang 字符串），播放时求值
          points: pts.map((p) => (uniform ? [p?.x, p?.x, p?.x] : [p?.x, p?.y, p?.z])),
          bezier: {
            lt: kf.bezier_left_time || [-0.1, -0.1, -0.1],
            lv: kf.bezier_left_value || [0, 0, 0],
            rt: kf.bezier_right_time || [0.1, 0.1, 0.1],
            rv: kf.bezier_right_value || [0, 0, 0],
          },
        })
        maxT = Math.max(maxT, num(kf.time))
      }
      if (channels.rotation.length || channels.position.length || channels.scale.length) {
        animators[key] = { type: bp.type || 'bone', channels }
      }
    }
    if (!Object.keys(animators).length) continue
    const length = typeof a.length === 'number' && a.length > 0 ? a.length : maxT
    if (!(length > 0)) continue
    out.push({
      name: a.name || `animation ${out.length + 1}`,
      loop: a.loop === 'loop' || a.loop === 'hold' ? a.loop : 'once',
      length,
      animators,
    })
  }
  return out
}

/* ---------------- 插值（ctx = { t: 动画时间, loop: 是否循环 }） ---------------- */

function calc(kf, dpIndex, axis, ctx) {
  const p = kf.points[Math.min(Math.max(dpIndex, 0), kf.points.length - 1)]
  return evalValue(p[axis], ctx)
}
function sample(kf, dp, ctx) {
  return [calc(kf, dp, 0, ctx), calc(kf, dp, 1, ctx), calc(kf, dp, 2, ctx)]
}
/** 与 Blockbench 一致：在关键帧时间点取第 0 个数据点，之后取最后一个 */
function dpRule(kf, t) {
  return kf.time > t || Math.abs(kf.time - t) < EPS ? 0 : kf.points.length - 1
}

function catmullromSample(kfs, before, after, alpha, ctx) {
  const sorted = kfs.slice().sort((a, b) => a.time - b.time)
  const i = sorted.indexOf(before)
  let bp = sorted[i - 1]
  let ap = sorted[i + 2]
  // 循环动画回绕取点（与 Blockbench interpolate() 一致）
  if (ctx?.loop && sorted.length >= 3) {
    if (!bp) bp = sorted[sorted.length - 2]
    if (!ap) ap = sorted[1]
  }
  const out = [0, 0, 0]
  for (let axis = 0; axis < 3; axis++) {
    const vectors = []
    if (bp && before.points.length === 1) vectors.push(new THREE.Vector2(bp.time, calc(bp, 1, axis, ctx)))
    vectors.push(new THREE.Vector2(before.time, calc(before, 1, axis, ctx)))
    vectors.push(new THREE.Vector2(after.time, calc(after, 0, axis, ctx)))
    if (ap && after.points.length === 1) vectors.push(new THREE.Vector2(ap.time, calc(ap, 0, axis, ctx)))
    const curve = new THREE.SplineCurve(vectors)
    const tt = (alpha + (bp ? 1 : 0)) / (vectors.length - 1)
    out[axis] = curve.getPoint(tt).y
  }
  return out
}

function bezierSample(before, after, alpha, ctx) {
  const out = [0, 0, 0]
  for (let axis = 0; axis < 3; axis++) {
    const vb = calc(before, before.points.length - 1, axis, ctx)
    const va = calc(after, 0, axis, ctx)
    const gap = after.time - before.time
    const tb = THREE.MathUtils.clamp(before.bezier.rt[axis] || 0, 0, gap)
    const ta = THREE.MathUtils.clamp(after.bezier.lt[axis] || 0, -gap, 0)
    const curve = new THREE.CubicBezierCurve(
      new THREE.Vector2(before.time, vb),
      new THREE.Vector2(before.time + tb, (vb + before.bezier.rv[axis]) || 0),
      new THREE.Vector2(after.time + ta, (va + after.bezier.lv[axis]) || 0),
      new THREE.Vector2(after.time, va),
    )
    const time = before.time + gap * alpha
    const pts = curve.getPoints(200)
    // 与 Blockbench getBezierLerp 一致：最近点 + 次近点按 x 距离线性过渡
    let closest = null
    let second = null
    let d1 = Infinity
    let d2 = Infinity
    for (const p of pts) {
      const d = Math.abs(p.x - time)
      if (d < d1) {
        second = closest
        d2 = d1
        closest = p
        d1 = d
      } else if (d < d2) {
        second = p
        d2 = d
      }
    }
    let v = closest ? closest.y : vb
    if (closest && second && second.x !== closest.x) {
      const f = THREE.MathUtils.clamp((time - closest.x) / (second.x - closest.x), 0, 1)
      v = closest.y + (second.y - closest.y) * f
    }
    out[axis] = v
  }
  return out
}

/** 返回 [x,y,z]，无关键帧覆盖时返回 null */
export function interpolateChannel(kfs, t, ctx) {
  let before = null
  let beforeT = 0
  let after = null
  let afterT = 0
  for (const kf of kfs) {
    if (kf.time < t) {
      if (!before || kf.time > beforeT) {
        before = kf
        beforeT = kf.time
      }
    } else if (!after || kf.time < afterT) {
      after = kf
      afterT = kf.time
    }
  }
  if (before && Math.abs(beforeT - t) < EPS) return sample(before, dpRule(before, t), ctx)
  if (after && Math.abs(afterT - t) < EPS) return sample(after, dpRule(after, t), ctx)
  if (before && before.interpolation === 'step') return sample(before, dpRule(before, t), ctx)
  if (before && !after) return sample(before, dpRule(before, t), ctx)
  if (after && !before) return sample(after, dpRule(after, t), ctx)
  if (!before && !after) return null

  const alpha = (t - beforeT) / (afterT - beforeT)
  if (before.interpolation === 'catmullrom' || after.interpolation === 'catmullrom') {
    return catmullromSample(kfs, before, after, alpha, ctx)
  }
  if (before.interpolation === 'bezier' || after.interpolation === 'bezier') {
    return bezierSample(before, after, alpha, ctx)
  }
  // linear（Blockbench：before 末点 → after 首点）
  return [0, 1, 2].map(
    (i) =>
      calc(before, before.points.length - 1, i, ctx) +
      (calc(after, 0, i, ctx) - calc(before, before.points.length - 1, i, ctx)) * alpha,
  )
}

/* ---------------- 播放器 ---------------- */

function restoreRest(node) {
  const r = node.userData.rest
  if (!r) return
  node.position.copy(r.pos)
  node.rotation.copy(r.rot)
  node.scale.copy(r.scale)
}

/**
 * 动画播放器：每帧复位静止姿态后叠加（与 Blockbench showDefaultPose + displayFrame 一致）
 * root 需带 userData.bbIndex = { byUuid: Map, byLowerName: Map }（bbmodel.js 构建）
 */
export class BbAnimator {
  constructor(root) {
    this.root = root
    this.anim = null
    this.time = 0
    this.playing = false
    this.nodes = []
  }

  setAnimation(anim) {
    for (const n of this.nodes) restoreRest(n)
    this.anim = anim || null
    this.time = 0
    this.nodes = []
    this.playing = !!this.anim
    this.apply(0)
  }

  /** 停止并回到静止姿态 */
  reset() {
    const had = this.anim
    this.anim = null
    this.playing = false
    for (const n of this.nodes) restoreRest(n)
    this.nodes = []
    return !!had
  }

  update(dt) {
    if (!this.anim || !this.playing) return
    this.time += dt
    if (this.anim.loop === 'loop') {
      this.time %= this.anim.length
    } else if (this.time >= this.anim.length) {
      this.time = this.anim.length
      this.playing = false // once/hold：停在末帧
    }
    this.apply(this.time)
  }

  /**
   * 骨骼解析（宽松匹配，处理 bedrock 动画 bone 命名习惯）：
   * 1. 组 uuid 精确匹配 → 2. 组名精确匹配 → 3. 剥离 uuid_ 前缀 →
   * 4. 最长后缀匹配（如 uuid_frog_cheeks → cheeks、uuid_crab_claw_left_tip → claw_left_tip）
   */
  resolveNode(key) {
    const idx = this.root?.userData?.bbIndex
    if (!idx) return null
    const k = String(key)
    let n = idx.byUuid.get(k)
    if (n) return n
    const lc = k.toLowerCase()
    n = idx.byLowerName.get(lc)
    if (n) return n
    const stripped = lc.replace(/^uuid[_:.]/, '')
    n = idx.byLowerName.get(stripped)
    if (n) return n
    let best = null
    let bestLen = 0
    for (const [name, node] of idx.byLowerName) {
      if (name.length >= 2 && stripped.endsWith(name) && name.length > bestLen) {
        best = node
        bestLen = name.length
      }
    }
    return best
  }

  apply(t) {
    for (const n of this.nodes) restoreRest(n)
    if (!this.anim) return
    const ctx = { t, loop: this.anim.loop === 'loop' }
    for (const [key, a] of Object.entries(this.anim.animators)) {
      const node = this.resolveNode(key)
      if (!node || !node.userData.rest) continue
      if (!this.nodes.includes(node)) this.nodes.push(node)
      const rest = node.userData.rest
      if (a.channels.rotation.length) {
        const v = interpolateChannel(a.channels.rotation, t, ctx)
        if (v) {
          node.rotation.set(
            rest.rot.x + D2R(v[0]),
            rest.rot.y + D2R(v[1]),
            rest.rot.z + D2R(v[2]),
          )
        }
      }
      if (a.channels.position.length) {
        const v = interpolateChannel(a.channels.position, t, ctx)
        if (v) node.position.set(rest.pos.x + v[0], rest.pos.y + v[1], rest.pos.z + v[2])
      }
      if (a.channels.scale.length) {
        const v = interpolateChannel(a.channels.scale, t, ctx)
        if (v) {
          // 与 displayScale 一致：基于 1 的乘法（骨骼静止 scale 恒为 1）
          node.scale.set(
            (rest.scale.x * v[0]) || 1e-5,
            (rest.scale.y * v[1]) || 1e-5,
            (rest.scale.z * v[2]) || 1e-5,
          )
        }
      }
    }
  }
}
