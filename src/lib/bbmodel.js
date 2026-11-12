/**
 * Blockbench .bbmodel 解析器 → THREE.Group
 * 面顺序、BoxUV 公式、顶点 UV 分配均与 Blockbench 官方渲染逻辑一致：
 *   face_order = ['east','west','up','down','south','north']（对应 BoxGeometry +x,-x,+y,-y,+z,-z）
 *   每面 4 顶点依次写入 UV 矩形的 左上/右上/左下/右下
 */
import * as THREE from 'three'
import { parseAnimations } from './bbanim.js'

const FACE_ORDER = ['east', 'west', 'up', 'down', 'south', 'north']

/**
 * 版本比较：a < b 返回 -1（只支持 "x.y.z" 数字段）
 * format_version 缺失或非版本号（极老文件 meta.format 是格式名）按 0 处理 → 一律视为 < 5.0
 */
function compareVersion(a, b) {
  const parse = (v) => String(v || '').split('.').map((n) => parseInt(n, 10) || 0)
  const pa = parse(a)
  const pb = parse(b)
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) < (pb[i] || 0) ? -1 : 1
  }
  return 0
}

/**
 * 关键帧数据点取反（bbmodel format_version < 5.0 迁移，与 Blockbench bbmodel.js parsePipeline 一致）：
 * position/rotation 通道 x 取反；rotation 通道 y 取反；bezier 控制值同步取反
 */
function negateValue(v) {
  if (typeof v === 'number') return -v
  if (typeof v === 'string') {
    const s = v.trim()
    if (s === '' || s === '0') return v
    const n = Number(s)
    if (Number.isFinite(n)) return String(-n)
    return '-(' + s + ')' // molang 表达式
  }
  return v
}
function migrateAnimationsV5(animations) {
  // 兼容官方数组与部分生成器的对象结构，避免非可迭代值抛错
  const list = Array.isArray(animations) ? animations : animations && typeof animations === 'object' ? Object.values(animations) : []
  for (const anim of list) {
    for (const bp of Object.values(anim?.animators || {})) {
      if (!bp || !Array.isArray(bp.keyframes)) continue
      for (const kf of bp.keyframes) {
        if (!Array.isArray(kf.data_points)) continue
        const negX = kf.channel === 'position' || kf.channel === 'rotation'
        const negY = kf.channel === 'rotation'
        for (const dp of kf.data_points) {
          if (negX && dp.x) dp.x = negateValue(dp.x)
          if (negY && dp.y) dp.y = negateValue(dp.y)
        }
        if (kf.interpolation === 'bezier') {
          if (negX && kf.bezier_left_value) kf.bezier_left_value[0] *= -1
          if (negX && kf.bezier_right_value) kf.bezier_right_value[0] *= -1
          if (negY && kf.bezier_left_value) kf.bezier_left_value[1] *= -1
          if (negY && kf.bezier_right_value) kf.bezier_right_value[1] *= -1
        }
      }
    }
  }
}

// Blockbench 16 色标记色板（用于无贴图回退）
const MARKER_COLORS = [
  '#8cf5ba', '#7ee3a6', '#a2e08c', '#d4e37a', '#f5d979', '#f5b875', '#f59e7e', '#f57e8f',
  '#f07ec8', '#c98cf5', '#9e8cf5', '#7ea6f5', '#7ed4f5', '#8cf5e3', '#c4cccc', '#e23c78',
]

/**
 * euler order：Blockbench 所有格式统一 Format.euler_order 默认 'ZYX'（io/format.ts:704，无格式覆盖）
 * 与 Blockbench 中 mesh.rotation.order / bone.rotation.order 一致
 */
function eulerOrderFor() {
  return 'ZYX'
}

/** 解析贴图：data URI → THREE.Texture */
function loadTexture(tex) {
  return new Promise((resolve) => {
    if (!tex || typeof tex.source !== 'string' || !tex.source.startsWith('data:image')) {
      resolve(null)
      return
    }
    const img = new Image()
    img.onload = () => {
      const t = new THREE.Texture(img)
      t.magFilter = THREE.NearestFilter
      t.minFilter = THREE.NearestFilter
      t.generateMipmaps = false
      t.colorSpace = THREE.SRGBColorSpace
      // Blockbench 对超出 UV 空间的坐标按重复平铺处理（模型可能存在 uv_width 与实际
      // UV 坐标范围不一致的文件，如 Drowned_Shrine_Urn uv=16 但坐标到 42），默认的
      // ClampToEdge 会把边缘像素拉成条纹，必须 Repeat
      t.wrapS = THREE.RepeatWrapping
      t.wrapT = THREE.RepeatWrapping
      t.needsUpdate = true
      resolve(t)
    }
    img.onerror = () => resolve(null)
    img.src = tex.source
  })
}

/**
 * 检测贴图是否含部分透明像素（0 < alpha < 255）
 * 半透明贴图必须走混合路径（transparent + alphaTest≈0），走 alphaTest:0.5 硬裁剪
 * 会把半透明像素渲染成实心、把 alpha<0.5 的像素挖洞（与 Blockbench 官方材质参数不符）
 * 顺带输出非透明像素包围盒极值（用于 UV 空间推断）；scanH 可限制只扫顶部若干行
 * （帧动画贴图只扫第一帧，堆叠帧的包围盒对 UV 推断无意义）
 */
function analyzeImage(img, scanH) {
  const empty = { partial: false, artMaxX: 0, artMaxY: 0 }
  try {
    const w = img.naturalWidth
    const h = Math.min(img.naturalHeight, scanH || img.naturalHeight)
    const c = document.createElement('canvas')
    c.width = w
    c.height = h
    const ctx = c.getContext('2d', { willReadFrequently: true })
    if (!ctx || !w || !h) return empty
    ctx.drawImage(img, 0, 0)
    const d = ctx.getImageData(0, 0, w, h).data
    let partial = false
    let maxX = -1
    let maxY = -1
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const a = d[(y * w + x) * 4 + 3]
        if (a > 0) {
          if (x > maxX) maxX = x
          if (y > maxY) maxY = y
          if (a < 255) partial = true
        }
      }
    }
    return { partial, artMaxX: maxX + 1, artMaxY: maxY + 1 }
  } catch {
    return empty
  }
}

/** 计算单个 cube 的 BoxUV 矩形（与 Blockbench updateUV 一致） */
function boxUVRects(el) {
  // 官方 updateUV 用 element.size 取 Math.floor(to-from+1e-7) 的取整尺寸：
  // 浮点尺寸（如 4.98）直接进公式会让 UV 矩形偏移 → 纹理整体错位
  const size = [0, 1, 2].map((i) => Math.floor(el.to[i] - el.from[i] + 1e-7))
  const [w, h, d] = size
  const u = el.uv_offset?.[0] ?? 0
  const v = el.uv_offset?.[1] ?? 0
  const list = [
    { face: 'east', from: [0, d], size: [d, h] },
    { face: 'west', from: [d + w, d], size: [d, h] },
    { face: 'up', from: [d + w, d], size: [-w, -d] },
    { face: 'down', from: [d + w * 2, 0], size: [-w, d] },
    { face: 'south', from: [d * 2 + w, d], size: [w, h] },
    { face: 'north', from: [d, d], size: [w, h] },
  ]
  if (el.mirror_uv) {
    list.forEach((f) => {
      f.from[0] += f.size[0]
      f.size[0] *= -1
    })
    const p = list[0]
    list[0] = list[1]
    list[1] = p
  }
  const rects = {}
  for (const f of list) {
    rects[f.face] = [
      f.from[0] + u, f.from[1] + v,
      f.from[0] + f.size[0] + u, f.from[1] + f.size[1] + v,
    ]
  }
  return rects
}

/** 为一个 cube 构建 Mesh（几何体原点=element.origin，UV 按 Blockbench 规则写入） */
function buildCube(el, texList, resolution) {
  // 检测：所有面的 uv 是否全为 [0,0,0,0]。
  // 部分 workshop 预览数据（bedrock 格式）会丢失逐面 UV，所有面 uv 写成 [0,0,0,0]，
  // 直接采样会命中贴图 (0,0) 透明像素 → 整模不可见。
  // Blockbench bedrock 格式默认 box_uv=true，此时回退到 box 公式（uv_offset 缺省 [0,0]）可恢复可见。
  let allUvZero = true
  if (el.faces) {
    for (const k of FACE_ORDER) {
      const uv = el.faces[k]?.uv
      if (uv && (uv[0] || uv[1] || uv[2] || uv[3])) { allUvZero = false; break }
    }
  } else {
    allUvZero = false
  }
  // box_uv 模型（无逐面 faces 字段）：与 Blockbench 一致，默认绑定第一张有效贴图
  const defaultTex = el.box_uv && !el.faces ? (texList.find((t) => t) || null) : null
  // box_uv 元素的 UV 有两个来源：
  //   1) 正规 Blockbench 导出的 box_uv 模型：faces[*].uv 由 updateUV 按 box 公式推导，与公式相符；
  //   2) 部分生成器（如 AI 生成模型）会写 box_uv:true，却按「逐面手工排版」写 faces[*].uv。
  // 官方 updateUV 对 box_uv 一律重算，第 2 种会被算错（整栋建筑都错用同一块 box 展开区，纹理整体错位）。
  // 判据：某元素的逐面 uv 与 box 公式「一面都不相符」时，它一定是手排布局 → 改用逐面 uv；
  // 只要有一面相符就按 box 公式（mirror_uv 元素仅 east/west 两面因镜像交换而不同，仍归此类，行为不变）。
  // allUvZero 时强制走 box 公式（不参与 preferFaceUV 判定，否则零 uv 会被误判为「手排」）。
  const useBoxFormula = el.box_uv || allUvZero
  const rectMap = useBoxFormula ? boxUVRects(el) : null
  let preferFaceUV = false
  if (rectMap && el.faces && !allUvZero) {
    let compared = 0
    let matched = 0
    for (const k of FACE_ORDER) {
      const uv = el.faces[k]?.uv
      const r = rectMap[k]
      if (!uv || !r) continue
      compared++
      if (uv.every((v, i) => Math.abs(v - r[i]) < 1e-3)) matched++
    }
    preferFaceUV = compared > 0 && matched === 0
  }
  const texOf = (fkey) => {
    const face = el.faces?.[fkey]
    if (face) return resolveTexture(face.texture, texList)
    return defaultTex
  }
  // inflate / stretch
  let from = el.from.slice()
  let to = el.to.slice()
  const inflate = el.inflate || 0
  if (inflate !== 0) {
    for (let i = 0; i < 3; i++) {
      from[i] -= inflate
      to[i] += inflate
    }
  }
  // 负尺寸立方体（from > to，常见于格式转换/镜像）归一化：确保 from <= to，
  // 否则 BoxGeometry 的面法线与实际朝向不一致，光照/可见性异常。
  for (let i = 0; i < 3; i++) {
    if (from[i] > to[i]) { const t = from[i]; from[i] = to[i]; to[i] = t }
  }
  const size = [to[0] - from[0], to[1] - from[1], to[2] - from[2]]
  const geo = new THREE.BoxGeometry(Math.abs(size[0]) || 0.001, Math.abs(size[1]) || 0.001, Math.abs(size[2]) || 0.001)

  const uvAttr = geo.attributes.uv

  FACE_ORDER.forEach((fkey, fIndex) => {
    const face = el.faces?.[fkey]
    let rect = (preferFaceUV && face?.uv) || (rectMap ? rectMap[fkey] : face?.uv)
    if (!rect) return
    rect = rect.slice()

    // box_uv 抗贴图渗色：向内缩 1/64 像素（allUvZero 回退也走 box 公式，同样需要）
    if (useBoxFormula) {
      for (let si = 0; si < 2; si++) {
        let margin = 1 / 64
        if (rect[si] > rect[si + 2]) margin = -margin
        rect[si] += margin
        rect[si + 2] -= margin
      }
    }

    // 归一化（v 翻转；帧动画贴图取第 0 帧 = 图片顶部切片）
    // UV 归一化基准：优先该面贴图自身的 uv_width/uv_height（Blockbench 支持单贴图独立 UV 尺寸），
    // 否则回落项目 resolution —— 多贴图且各自尺寸不同的模型（如 Warthog）此前整体错位即源于此
    const faceTex = texOf(fkey)
    const pw = faceTex?.uvWidth || resolution.width
    const ph = faceTex?.uvHeight || resolution.height
    let arr = [
      [rect[0] / pw, 1 - rect[1] / ph],
      [rect[2] / pw, 1 - rect[1] / ph],
      [rect[0] / pw, 1 - rect[3] / ph],
      [rect[2] / pw, 1 - rect[3] / ph],
    ]

    // 面旋转（每 90° 循环，与 Blockbench 一致）
    let rot = Math.round((face?.rotation || 0) / 90)
    while (rot > 0) {
      const a = arr[0]
      arr[0] = arr[2]; arr[2] = arr[3]; arr[3] = arr[1]; arr[1] = a
      rot--
    }

    const base = fIndex * 8
    uvAttr.array[base + 0] = arr[0][0]; uvAttr.array[base + 1] = arr[0][1] // 0,1 左上
    uvAttr.array[base + 2] = arr[1][0]; uvAttr.array[base + 3] = arr[1][1] // 1,1 右上
    uvAttr.array[base + 4] = arr[2][0]; uvAttr.array[base + 5] = arr[2][1] // 0,0 左下
    uvAttr.array[base + 6] = arr[3][0]; uvAttr.array[base + 7] = arr[3][1] // 1,0 右下
  })
  uvAttr.needsUpdate = true

  // 材质：6 面各一个，按有无贴图选择；无贴图面（texture:null/false/缺失）不渲染
  let hiddenMaterial = null
  const hiddenFaceMaterial = () => {
    if (!hiddenMaterial) hiddenMaterial = new THREE.MeshBasicMaterial({ visible: false })
    return hiddenMaterial
  }
  const materials = FACE_ORDER.map((fkey) => {
    const face = el.faces?.[fkey]
    if (el.faces && (!face || face.texture === null || face.texture === false)) {
      return hiddenFaceMaterial()
    }
    const tex = texOf(fkey)
    if (tex) return tex.material
    const ci = (((el.color ?? 0) % MARKER_COLORS.length) + MARKER_COLORS.length) % MARKER_COLORS.length
    return colorMaterial(ci)
  })

  const mesh = new THREE.Mesh(geo, materials)
  mesh.rotation.order = eulerOrderFor()

  // 几何体平移至以 element.origin 为枢轴
  const hasRot = el.rotation && el.rotation.some((v) => v !== 0)
  const pivot = el.origin ?? [0, 0, 0]
  geo.translate(
    (from[0] + to[0]) / 2 - pivot[0],
    (from[1] + to[1]) / 2 - pivot[1],
    (from[2] + to[2]) / 2 - pivot[2],
  )
  mesh.position.set(pivot[0], pivot[1], pivot[2])
  if (hasRot) {
    mesh.rotation.set(
      THREE.MathUtils.degToRad(el.rotation[0]),
      THREE.MathUtils.degToRad(el.rotation[1]),
      THREE.MathUtils.degToRad(el.rotation[2]),
    )
  }
  mesh.name = el.name || 'cube'
  return { mesh, pivot }
}

let colorMaterials = null
function colorMaterial(index) {
  if (!colorMaterials) {
    colorMaterials = MARKER_COLORS.map(
      (c) => new THREE.MeshStandardMaterial({ color: new THREE.Color(c), roughness: 0.85, metalness: 0.05, side: THREE.DoubleSide }),
    )
  }
  return colorMaterials[index]
}

/**
 * 构建 mesh 元素（Blockbench 自由网格，element.type === 'mesh'）
 * 结构：vertices 为 { 顶点uuid: [x,y,z] }，faces 为 { 面uuid: { texture, uv: {顶点uuid:[u,v]}, vertices: [uuid,...] } }
 * 同一顶点在不同面可有不同 UV，故按「面-角点」展开为非索引几何，三角扇切分多边形；
 * 按贴图分组生成多个 Mesh，挂在以 element.origin 为枢轴的 Group 下。
 */
function buildMeshElement(el, texList, resolution) {
  const vertices = el.vertices || {}
  const faces = el.faces || {}
  const origin = Array.isArray(el.origin) ? el.origin : [0, 0, 0]
  const groups = new Map() // key → { tex, positions, uvs }

  for (const face of Object.values(faces)) {
    if (!face) continue
    // texture:null/false 的面不渲染（与 cube 的 hitbox 语义一致）
    if (face.texture === null || face.texture === false) continue
    const vids = (Array.isArray(face.vertices) ? face.vertices : []).filter((v) => Array.isArray(vertices[v]))
    if (vids.length < 3) continue

    const tex = resolveTexture(face.texture, texList)
    const key = tex ? (tex.uuid || tex.id || tex.name) : `color-${el.color ?? 0}`
    if (!groups.has(key)) groups.set(key, { tex, positions: [], uvs: [] })
    const g = groups.get(key)
    const pw = tex?.uvWidth || resolution.width
    const ph = tex?.uvHeight || resolution.height

    // 三角扇：v0,vi,vi+1
    // 顶点坐标是相对 element.origin 的局部坐标（与 cube 的 from/to 不同，后两者是绝对坐标）：
    // 不能再减 origin，否则非零 origin 的网格会被整体平移，带旋转的部件会绕错误枢轴转动而散开
    // （Ravager Unit 等模型的封面呈「炸开」状即此因）
    for (let i = 1; i < vids.length - 1; i++) {
      for (const vid of [vids[0], vids[i], vids[i + 1]]) {
        const p = vertices[vid]
        g.positions.push(p[0], p[1], p[2])
        const uv = face.uv?.[vid] || [0, 0]
        g.uvs.push(uv[0] / pw, 1 - uv[1] / ph)
      }
    }
  }

  if (!groups.size) return null

  const node = new THREE.Group()
  node.rotation.order = eulerOrderFor()
  for (const { tex, positions, uvs } of groups.values()) {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3))
    geo.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2))
    geo.computeVertexNormals() // 非索引几何 → 自动得到平直着色法线
    const material = tex
      ? tex.material
      : colorMaterial((((el.color ?? 0) % MARKER_COLORS.length) + MARKER_COLORS.length) % MARKER_COLORS.length)
    node.add(new THREE.Mesh(geo, material))
  }
  node.name = el.name || 'mesh'
  const rot = el.rotation && el.rotation.some((v) => v !== 0) ? el.rotation : null
  if (rot) {
    node.rotation.set(
      THREE.MathUtils.degToRad(rot[0]),
      THREE.MathUtils.degToRad(rot[1]),
      THREE.MathUtils.degToRad(rot[2]),
    )
  }
  return { node, pivot: origin }
}

/** face.texture 可能是索引/uuid/id 字符串 */
function resolveTexture(ref, texList) {
  if (ref === null || ref === undefined || ref === false) return null
  let tex = null
  if (typeof ref === 'number') tex = texList[ref] ?? null
  else if (typeof ref === 'string') {
    tex =
      texList.find((t) => t && t.uuid === ref) ||
      texList.find((t) => t && t.id === ref) ||
      texList.find((t) => t && t.name === ref) ||
      null
    // 部分生成器把数组下标写成字符串（"0"、"1"）
    if (!tex && /^\d+$/.test(ref)) tex = texList[Number(ref)] ?? null
  }
  return tex
}

/** 构建 texture 条目列表（含材质） */
async function prepareTextures(json) {
  const list = []
  const textures = json.textures || []
  for (const tex of textures) {
    const t = await loadTexture(tex)
    if (t) {
      // 帧动画贴图：MC/Blockbench 动画帧按「图片高度 ÷ 图片宽度」的方形帧垂直堆叠，
      // 只取第 0 帧（顶部切片）。注意不能按 uv_height 判断 —— 高分辨率贴图（如 64x64 配 32x32 UV 空间）
      // 会被误判为动画导致只显示上半截（Wrench 案例）
      const imgW = t.image?.naturalWidth || 0
      const imgH = t.image?.naturalHeight || 0
      const frames = imgW > 0 && imgH > imgW && imgH % imgW === 0 ? imgH / imgW : 1
      const sliced = frames > 1
      if (sliced) {
        t.repeat.set(1, 1 / frames)
        t.offset.set(0, 1 - 1 / frames)
      }
      // 帧动画贴图只分析顶部一帧（堆叠帧的包围盒对 UV 空间推断无意义）
      const info = analyzeImage(t.image, sliced ? imgW : imgH)
      const renderMode = tex.render_mode || 'default'
      // 半透明贴图（或声明 blend/additive）走混合路径，参数对齐 Blockbench：
      // transparent:true + alphaTest≈0 + depthWrite:true（默认）；
      // 纯二值贴图保留 alphaTest:0.5 硬裁剪，避免既有不透明模型回归
      const blend = renderMode === 'blend' || renderMode === 'additive' || info.partial
      const mat = new THREE.MeshStandardMaterial({
        map: t,
        roughness: 0.85,
        metalness: 0.05,
        alphaTest: blend ? 0.001 : 0.5,
        transparent: blend,
        blending: renderMode === 'additive' ? THREE.AdditiveBlending : THREE.NormalBlending,
        side: THREE.DoubleSide, // 双面渲染：翅膀/披风等单面结构背面不再透明
      })
      if (renderMode === 'emissive') {
        mat.emissiveMap = t
        mat.emissive = new THREE.Color(0xffffff)
        mat.emissiveIntensity = 1
      }
      list.push({
        uuid: tex.uuid,
        id: tex.id,
        name: tex.name,
        // 单贴图独立 UV 尺寸（Blockbench Texture.uv_width/uv_height），缺省回落项目 resolution
        uvWidth: Number(tex.uv_width) || 0,
        uvHeight: Number(tex.uv_height) || 0,
        // 有效像素尺寸（帧动画取单帧）与非透明像素边界，供 computeEffectiveUvDims 推断 UV 空间
        pxW: imgW,
        pxH: sliced ? imgW : imgH,
        sliced,
        artMaxX: info.artMaxX,
        artMaxY: info.artMaxY,
        material: mat,
      })
    } else {
      list.push(null)
    }
  }
  return list
}

/**
 * 计算每个贴图的「有效 UV 空间」。
 * 部分导出文件的 texture.uv_width/uv_height 与面 UV 坐标实际所在空间不一致：
 * 声明值可能偏大（Claymore 声明 128 但坐标按项目 64 空间书写）或与项目值同样能覆盖坐标
 * （untitled 声明 32 与项目 16 都覆盖坐标 16，但真实空间是 32）。
 * 判据（逐轴）：候选 {贴图声明值, 项目 resolution, 图片像素} 中，使
 * 「坐标极值 × 图片像素 ÷ 候选值」最接近非透明像素边界的胜出 —— 美术图是按真实 UV 空间绘制的。
 * 无美术像素参照（全透明贴图）时退化为「能覆盖坐标的最小候选」，都覆盖不了回落声明值。
 */
function computeEffectiveUvDims(json, texList, resolution) {
  const maxU = texList.map(() => -Infinity)
  const maxV = texList.map(() => -Infinity)
  const minU = texList.map(() => Infinity)
  const minV = texList.map(() => Infinity)
  const idxOf = (ref) => {
    if (ref === null || ref === undefined || ref === false) return -1
    if (typeof ref === 'number') return ref
    const i = texList.findIndex((t) => t && t.uuid === ref)
    if (i >= 0) return i
    const n = Number(ref)
    return Number.isFinite(n) ? n : -1
  }
  const scan = (ti, u, v) => {
    if (ti < 0 || ti >= texList.length || !texList[ti]) return
    if (u > maxU[ti]) maxU[ti] = u
    if (v > maxV[ti]) maxV[ti] = v
    if (u < minU[ti]) minU[ti] = u
    if (v < minV[ti]) minV[ti] = v
  }
  for (const el of json.elements || []) {
    const faces = el.faces || {}
    if (el.type === 'mesh') {
      // mesh 面：uv 为 { 顶点uuid: [u,v] }
      for (const fk of Object.keys(faces)) {
        const face = faces[fk]
        const ti = idxOf(face?.texture)
        for (const vk of Object.keys(face?.uv || {})) {
          const c = face.uv[vk]
          if (Array.isArray(c) && c.length >= 2) scan(ti, c[0], c[1])
        }
      }
    } else {
      // cube 面：uv 为 [u1,v1,u2,v2]
      for (const fk of Object.keys(faces)) {
        const face = faces[fk]
        if (!face || !Array.isArray(face.uv) || face.uv.length < 4) continue
        scan(idxOf(face.texture), Math.max(face.uv[0], face.uv[2]), Math.max(face.uv[1], face.uv[3]))
        scan(idxOf(face.texture), Math.min(face.uv[0], face.uv[2]), Math.min(face.uv[1], face.uv[3]))
      }
    }
  }
  /**
   * 美术像素对齐：取使 坐标极值×(imgPx/cand) 最贴近非透明像素边界的候选。
   * 约束：候选 c 对应的最大采样像素 (coordAbs*imgPx/c) 必须 ≥ artMax，
   * 否则美术内容延伸到了 UV 坐标无法采样的区域 → c 偏大，剔除。
   * 例：Zweihander 声明 uv_width=64、图 128px、坐标极值 44、artMaxX=50；
   *   c=64 → 采样 88 ≥ 50（美术可完整采样）；c=128 → 采样 44 < 50（美术被裁）→ 剔除 c=128。
   * 若无候选满足约束，回退到误差最小者（兼容美术未填满整图、或 artMax 检测不准的情况）。
   */
  const pickAxis = (declared, project, imgPx, coordAbs, artMax) => {
    const cands = []
    for (const c of [declared, project, imgPx]) {
      if (c > 0 && !cands.includes(c)) cands.push(c)
    }
    if (!cands.length) return declared || project || 16
    if (!(coordAbs > 0)) return declared > 0 ? declared : cands[0]
    if (artMax > 0 && imgPx > 0) {
      // 只保留「美术可被 UV 完整采样」的候选
      const fitted = cands.filter((c) => (coordAbs * imgPx) / c >= artMax - 1e-3)
      const pool = fitted.length ? fitted : cands
      let best = pool[0]
      let bestErr = Infinity
      for (const c of pool) {
        const err = Math.abs((coordAbs * imgPx) / c - artMax)
        if (err < bestErr - 1e-6) {
          best = c
          bestErr = err
        }
      }
      return best
    }
    return pickByCovering(declared, project, coordAbs)
  }
  texList.forEach((t, i) => {
    if (!t) return
    const absU = Math.max(Math.abs(minU[i] === Infinity ? 0 : minU[i]), Math.abs(maxU[i] === -Infinity ? 0 : maxU[i]))
    const absV = Math.max(Math.abs(minV[i] === Infinity ? 0 : minV[i]), Math.abs(maxV[i] === -Infinity ? 0 : maxV[i]))
    t.uvWidth = pickAxis(t.uvWidth || 0, resolution.width, t.pxW, absU, t.artMaxX)
    // 帧动画贴图按方形帧切片渲染，纵轴与整图美术像素无对应关系，保持覆盖规则
    t.uvHeight = t.sliced
      ? pickByCovering(t.uvHeight || 0, resolution.height, absV)
      : pickAxis(t.uvHeight || 0, resolution.height, t.pxH, absV, t.artMaxY)
  })
}

/** 覆盖规则：候选 {贴图声明值, 项目 resolution} 中能覆盖实际坐标的最小者，都覆盖不了回落声明值 */
function pickByCovering(declared, project, coordAbs) {
  const cands = []
  for (const c of [declared, project]) {
    if (c > 0 && !cands.includes(c)) cands.push(c)
  }
  if (!cands.length) return declared || project || 16
  if (!(coordAbs > 0)) return declared > 0 ? declared : cands[0]
  const containing = cands.filter((c) => coordAbs <= c + 1e-4)
  if (containing.length) return Math.min(...containing)
  return declared > 0 ? declared : cands[0]
}

/**
 * 主入口：解析 bbmodel JSON → THREE.Group
 * @returns {Promise<{group: THREE.Group, info: {name, format, elementCount, textureCount, animations, size}}>}
 */
export async function buildBBModel(json) {
  if (!json || json.meta === undefined || !Array.isArray(json.elements)) {
    throw new Error('不是有效的 .bbmodel 文件（缺少 meta/elements 字段）')
  }
  const format = json.meta?.model_format || json.meta?.format || 'free'
  const defaultBoxUV = json.meta?.box_uv === true
  // < 5.0 的文件关键帧 x/y 存的是反向值，先迁移（Blockbench bbmodel.js parsePipeline 同款逻辑）
  if (compareVersion(json.meta?.format_version, '5.0') < 0) {
    migrateAnimationsV5(json.animations)
  }
  const resolution = json.resolution || { width: 16, height: 16 }
  const texList = await prepareTextures(json)
  // 修正与坐标实际空间不符的 uv_width/uv_height（Claymore/Urn 等文件）
  computeEffectiveUvDims(json, texList, resolution)

  const root = new THREE.Group()
  root.rotation.order = eulerOrderFor()
  // 动画节点索引：uuid / 小写名称 → THREE 节点
  root.userData.bbIndex = { byUuid: new Map(), byLowerName: new Map() }
  const indexNode = (key, node) => {
    if (!key) return
    root.userData.bbIndex.byUuid.set(key, node)
    const lc = String(node.name || key).toLowerCase()
    if (!root.userData.bbIndex.byLowerName.has(lc)) root.userData.bbIndex.byLowerName.set(lc, node)
  }
  const elementMap = new Map()
  for (const el of json.elements) elementMap.set(el.uuid, el)

  /**
   * 组数据解析（兼容 bbmodel 4.x 与 5.0+，与 Blockbench Outliner.loadJSON 语义一致）：
   * - 5.0+：完整组数据（name/origin/rotation/visibility）在独立的 json.groups 数组，
   *         outliner 树里只有 {uuid, isOpen, children} 引用节点 → 按 uuid 从 groups 数组取数据
   * - 4.x ：组数据内联在 outliner 树节点上（节点有 name 字段）→ 直接用节点数据
   * - 字符串节点：元素 uuid 优先，其次组 uuid
   */
  const groupMap = new Map()
  for (const g of json.groups || []) {
    if (g && g.uuid) groupMap.set(g.uuid, g)
  }
  function resolveGroupData(treeNode) {
    if (!treeNode || typeof treeNode !== 'object') return treeNode
    if (treeNode.name !== undefined) return treeNode // 4.x 内联组数据
    const data = groupMap.get(treeNode.uuid)
    if (!data) return treeNode
    // 5.0：树节点只提供层级关系，属性来自 groups 数组
    return { ...data, uuid: treeNode.uuid, isOpen: treeNode.isOpen, children: treeNode.children ?? data.children ?? [] }
  }
  function buildGroup(treeNode, parentPivot, parentNode) {
    const data = resolveGroupData(treeNode) || {}
    const g = new THREE.Group()
    g.rotation.order = eulerOrderFor()
    const pivot = data.origin ?? [0, 0, 0]
    g.position.set(pivot[0] - parentPivot[0], pivot[1] - parentPivot[1], pivot[2] - parentPivot[2])
    if (data.rotation && data.rotation.some((v) => v !== 0)) {
      g.rotation.set(
        THREE.MathUtils.degToRad(data.rotation[0]),
        THREE.MathUtils.degToRad(data.rotation[1]),
        THREE.MathUtils.degToRad(data.rotation[2]),
      )
    }
    g.name = data.name || 'group'
    g.visible = data.visibility !== false
    indexNode(data.uuid, g)
    parentNode.add(g)
    for (const child of data.children || []) buildNode(child, pivot, g)
  }

  // 递归构建 outliner 树
  function buildNode(node, parentPivot, parentNode) {
    // 分组节点（对象）或元素/组引用（字符串）
    if (node && typeof node === 'object') {
      if (node.children !== undefined || groupMap.has(node.uuid)) {
        buildGroup(node, parentPivot, parentNode)
      }
      return
    }
    if (typeof node === 'string') {
      const el = elementMap.get(node)
      if (el) {
        buildElement(el, parentPivot, parentNode)
        return
      }
      const grp = groupMap.get(node)
      if (grp) {
        buildGroup(grp, parentPivot, parentNode)
        return
      }
    }
  }

  // Blockbench 4.10+ 的 bbmodel 元素不再带 type 字段（cube 信息在 el.from/to），需兼容新旧两种结构
  function isCubeEl(el) {
    if (!el) return false
    if (el.type !== undefined) return el.type === 'cube'
    return el.cube !== false && Array.isArray(el.from) && Array.isArray(el.to)
  }

  // 判断元素是否至少有一个可渲染面：hitbox 等 6 面 texture:null 的元素在 Blockbench 中不显示
  function hasRenderableFace(el) {
    if (!el.faces || typeof el.faces !== 'object') return true
    const faces = FACE_ORDER.map((key) => el.faces[key]).filter((face) => face !== undefined)
    if (!faces.length) return false
    return faces.some((face) => face && face.texture !== null && face.texture !== false)
  }

  function buildElement(el, parentPivot, parentNode) {
    // mesh 自由网格元素（Blockbench 4.6+，此前被跳过导致整模报「无可渲染网格」）
    if (el && el.type === 'mesh') {
      if (el.visibility === false) return
      const built = buildMeshElement(el, texList, resolution)
      if (!built) return
      built.node.position.set(
        built.pivot[0] - parentPivot[0],
        built.pivot[1] - parentPivot[1],
        built.pivot[2] - parentPivot[2],
      )
      indexNode(el.uuid, built.node)
      parentNode.add(built.node)
      return
    }
    if (!isCubeEl(el)) {
      if (el && el.type !== undefined) console.warn('[bbmodel] 暂不支持元素类型:', el.type, el.name)
      return
    }
    // Blockbench cube_outline 元素：编辑器线框/碰撞箱，始终为 from>to 的负尺寸立方体，
    // 不应作为实心立方体渲染（否则会包裹并遮挡内部实体，如 emoji 的黄底遮住表情脸）。
    if ((el.name || '').toLowerCase().includes('outline')) return
    if (!hasRenderableFace(el)) return
    const data = el
    const effBoxUV = typeof data.box_uv === 'boolean' ? data.box_uv : defaultBoxUV
    const cubeEl = { ...data, box_uv: effBoxUV }
    const { mesh, pivot } = buildCube(cubeEl, texList, resolution)
    mesh.position.set(pivot[0] - parentPivot[0], pivot[1] - parentPivot[1], pivot[2] - parentPivot[2])
    mesh.visible = data.visibility !== false
    indexNode(data.uuid, mesh)
    parentNode.add(mesh)
  }

  // outliner 缺失时兜底：所有元素平铺在根节点（Blockbench 同款回退）
  const outliner = json.outliner?.length ? json.outliner : json.elements.map((e) => e.uuid)
  for (const node of outliner) buildNode(node, [0, 0, 0], root)

  // 记录所有节点的静止姿态（动画叠加基准）
  root.traverse((o) => {
    if (o === root) return
    o.userData.rest = { pos: o.position.clone(), rot: o.rotation.clone(), scale: o.scale.clone() }
  })

  // 水平居中 + 底部对齐地面（y=0）
  const bbox = new THREE.Box3().setFromObject(root)
  const center = new THREE.Vector3()
  const size = new THREE.Vector3()
  if (isFinite(bbox.min.x)) {
    bbox.getCenter(center)
    bbox.getSize(size)
    const wrapper = new THREE.Group()
    wrapper.position.set(-center.x, -bbox.min.y, -center.z)
    // 把 root 的子节点挪进 wrapper 保持世界位置
    while (root.children.length) wrapper.add(root.children[0])
    root.add(wrapper)
  }

  const elementCount = json.elements.filter((e) => isCubeEl(e) || e?.type === 'mesh').length

  return {
    group: root,
    info: {
      name: json.name || json.model_identifier || '未命名模型',
      format,
      elementCount,
      textureCount: texList.filter(Boolean).length,
      animations: parseAnimations(json.animations),
      size: { x: size.x, y: size.y, z: size.z },
    },
  }
}
