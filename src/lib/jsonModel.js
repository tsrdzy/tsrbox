/**
 * .json 模型解析：Bedrock geometry / Java Block & Item。
 * 坐标系按原始模型坐标（Bedrock 1 像素 = 1 单位；Java 1 单位 = 1/16 格），
 * 统一由 ModelViewer 缩放 1/16 并居中。
 */
import * as THREE from 'three'
import { FACE_ORDER, createModelMaterial, deg, toNumber, toVec3, writeFaceUVs } from './geomCommon.js'

/** 判断 JSON 模型类型：bedrock | java | animation | unknown */
export function detectJsonKind(json) {
  if (!json || typeof json !== 'object' || Array.isArray(json)) return 'unknown'
  if (json['minecraft:geometry'] || Object.keys(json).some((key) => key.startsWith('geometry.'))) {
    return 'bedrock'
  }
  if (Array.isArray(json.bones)) return 'bedrock'
  if (json.elements || json.textures || json.parent || json.display || json.groups) return 'java'
  if (json.animations || json.animation_controllers) return 'animation'
  return 'unknown'
}

function resolveBedrockGeometry(json) {
  if (Array.isArray(json['minecraft:geometry'])) return json['minecraft:geometry'][0]
  if (json['minecraft:geometry'] && typeof json['minecraft:geometry'] === 'object') {
    return json['minecraft:geometry']
  }
  const key = Object.keys(json).find((item) => item.startsWith('geometry.'))
  if (key && json[key] && typeof json[key] === 'object') return json[key]
  if (Array.isArray(json.bones)) return json
  return null
}

/** 逐面 UV 缺省尺寸：按方块各面实际像素尺寸推断 */
function faceDefaultSize(size, face) {
  const [w, h, d] = size
  if (face === 'east' || face === 'west') return [d, h]
  if (face === 'north' || face === 'south') return [w, h]
  return [w, d]
}

/** Bedrock 方块 UV：box UV 展开公式，与 Blockbench 一致 */
function bedrockCubeUVRects(cube, size) {
  const uv = cube.uv
  if (uv && typeof uv === 'object' && !Array.isArray(uv)) {
    const rects = {}
    for (const face of FACE_ORDER) {
      const item = uv[face]
      if (!item) continue
      let u = 0
      let v = 0
      let faceSize = faceDefaultSize(size, face)
      if (Array.isArray(item)) {
        u = toNumber(item[0])
        v = toNumber(item[1])
      } else {
        u = toNumber(item.uv?.[0])
        v = toNumber(item.uv?.[1])
        if (Array.isArray(item.uv_size)) faceSize = [toNumber(item.uv_size[0]), toNumber(item.uv_size[1])]
      }
      let rect = [u, v, u + faceSize[0], v + faceSize[1]]
      // Blockbench 在导入时把 up/down 的 UV 旋转 180°，这里保持一致
      if (face === 'up' || face === 'down') rect = [rect[2], rect[3], rect[0], rect[1]]
      rects[face] = rect
    }
    return rects
  }

  if (!Array.isArray(uv)) return null
  const [u, v] = [toNumber(uv[0]), toNumber(uv[1])]
  const w = Math.floor(size[0] + 1e-7)
  const h = Math.floor(size[1] + 1e-7)
  const d = Math.floor(size[2] + 1e-7)
  return {
    east: [u, v + d, u + d, v + d + h],
    west: [u + d + w, v + d, u + 2 * d + w, v + d + h],
    north: [u + d, v + d, u + d + w, v + d + h],
    south: [u + 2 * d + w, v + d, u + 2 * d + 2 * w, v + d + h],
    // 与 Blockbench boxUVRects 的 up/down 方向一致（负尺寸矩形）
    up: [u + d + w, v + d, u + d, v],
    down: [u + 2 * d + w, v, u + d + w, v + d],
  }
}

function addBedrockCube(boneNode, cube, bonePivot, texWidth, texHeight, material) {
  const origin = toVec3(cube.origin)
  const size = toVec3(cube.size).map((v) => Math.abs(v))
  const uvRects = bedrockCubeUVRects(cube, size)
  // 无 UV（如 hitbox 的 uv:{}）的方块在 Blockbench 中同样不渲染
  if (!uvRects || !Object.keys(uvRects).length) return 0

  const inflate = toNumber(cube.inflate)
  const dims = size.map((v) => Math.max(0.001, v + inflate * 2))
  const geometry = new THREE.BoxGeometry(dims[0], dims[1], dims[2])
  writeFaceUVs(geometry, uvRects, texWidth, texHeight)

  const mesh = new THREE.Mesh(geometry, material)
  mesh.name = String(cube.name || '')
  // Bedrock 的 X 轴方向与 Blockbench 相反，方块中心与枢轴都要镜像
  const center = [
    -(origin[0] - inflate + dims[0] / 2),
    origin[1] - inflate + dims[1] / 2,
    origin[2] - inflate + dims[2] / 2,
  ]
  // Bedrock 的旋转轴约定与 Blockbench 相反：x、y 取反，z 保持
  const rawRotation = toVec3(cube.rotation)
  const rotation = [-rawRotation[0], -rawRotation[1], rawRotation[2]]
  const hasRotation = rotation.some((v) => v !== 0)

  if (hasRotation) {
    const rawPivot = Array.isArray(cube.pivot) ? toVec3(cube.pivot) : center
    const pivot = [-rawPivot[0], rawPivot[1], rawPivot[2]]
    const holder = new THREE.Group()
    holder.position.set(pivot[0] - bonePivot[0], pivot[1] - bonePivot[1], pivot[2] - bonePivot[2])
    holder.rotation.order = 'ZYX'
    holder.rotation.set(deg(rotation[0]), deg(rotation[1]), deg(rotation[2]))
    mesh.position.set(center[0] - pivot[0], center[1] - pivot[1], center[2] - pivot[2])
    holder.add(mesh)
    boneNode.add(holder)
  } else {
    mesh.position.set(
      center[0] - bonePivot[0],
      center[1] - bonePivot[1],
      center[2] - bonePivot[2],
    )
    boneNode.add(mesh)
  }
  return 1
}

/**
 * 构建 Bedrock geometry 模型
 * @returns {{group: THREE.Group, bones: Map, textureSize: {width:number,height:number}, elementCount:number}}
 */
export function buildBedrockModel(json, textures) {
  const textureList = Array.isArray(textures) ? textures.filter(Boolean) : []
  const texture = textureList[0]?.texture || null
  const geo = resolveBedrockGeometry(json)
  if (!geo) throw new Error('无法识别的 Bedrock 模型 JSON')

  const description = geo.description || {}
  const texWidth = Math.max(1, toNumber(description.texture_width ?? geo.texturewidth, 64))
  const texHeight = Math.max(1, toNumber(description.texture_height ?? geo.textureheight, 64))
  const material = createModelMaterial(texture)
  const bones = Array.isArray(geo.bones) ? geo.bones : []

  const root = new THREE.Group()
  const boneNodes = new Map()
  const boneRecords = new Map()
  let elementCount = 0

  for (const bone of bones) {
    const node = new THREE.Group()
    node.name = String(bone.name || '')
    node.rotation.order = 'ZYX'
    // Bedrock 骨骼枢轴 x 取反，与 Blockbench/three.js 的旋转方向对齐
    const rawPivot = toVec3(bone.pivot)
    boneNodes.set(bone.name, {
      node,
      pivot: [-rawPivot[0], rawPivot[1], rawPivot[2]],
      bone,
    })
    boneRecords.set(String(bone.name || '').toLowerCase(), {
      object: node,
      basePosition: new THREE.Vector3(),
      baseRotation: new THREE.Euler(0, 0, 0, 'ZYX'),
      baseScale: new THREE.Vector3(1, 1, 1),
    })
  }

  for (const bone of bones) {
    const entry = boneNodes.get(bone.name)
    if (!entry) continue
    const { node, pivot } = entry
    const parent = bone.parent ? boneNodes.get(bone.parent) : null
    const parentPivot = parent ? parent.pivot : [0, 0, 0]
    node.position.set(pivot[0] - parentPivot[0], pivot[1] - parentPivot[1], pivot[2] - parentPivot[2])
    const rawRotation = toVec3(bone.rotation)
    node.rotation.set(
      deg(-rawRotation[0]),
      deg(-rawRotation[1]),
      deg(rawRotation[2]),
    )

    for (const cube of bone.cubes || []) {
      elementCount += addBedrockCube(node, cube, pivot, texWidth, texHeight, material)
    }

    ;(parent ? parent.node : root).add(node)

    const record = boneRecords.get(String(bone.name || '').toLowerCase())
    if (record) {
      record.basePosition.copy(node.position)
      record.baseRotation.copy(node.rotation)
      record.baseScale.copy(node.scale)
    }
  }

  return { group: root, bones: boneRecords, textureSize: { width: texWidth, height: texHeight }, elementCount }
}

/**
 * 构建 Java Block/Item 模型
 * Java UV 固定使用 0-16 纹理空间（与贴图实际分辨率无关）。
 */
export function buildJavaModel(json, textures) {
  const textureList = Array.isArray(textures) ? textures.filter(Boolean) : []
  const refMap = json.textures && typeof json.textures === 'object' ? json.textures : {}
  const fallbackTexture = textureList[0]?.texture || null
  const materialCache = new Map()

  const fileStem = (name) =>
    String(name || '')
      .replace(/\.[^.]+$/, '')
      .toLowerCase()

  function resolveFaceTexture(faceData) {
    const ref = String(faceData?.texture || '').replace(/^#/, '')
    const link = refMap[ref]
    if (typeof link === 'string' && textureList.length) {
      const base = link.replace(/^minecraft:/, '').split('/').pop().toLowerCase()
      const hit = textureList.find((item) => fileStem(item.name) === base)
      if (hit) return hit.texture
    }
    return fallbackTexture
  }

  function materialFor(texture) {
    const key = texture ? texture.uuid : 'none'
    if (!materialCache.has(key)) materialCache.set(key, createModelMaterial(texture))
    return materialCache.get(key)
  }

  const root = new THREE.Group()
  const elements = Array.isArray(json.elements) ? json.elements : []
  let elementCount = 0

  for (const element of elements) {
    const from = toVec3(element.from)
    const to = toVec3(element.to)
    const inflate = toNumber(element.inflate)
    const low = from.map((v) => v - inflate)
    const high = to.map((v) => v + inflate)
    const dims = high.map((v, i) => Math.max(0.001, Math.abs(v - low[i])))
    const geometry = new THREE.BoxGeometry(dims[0], dims[1], dims[2])

    const rects = {}
    const rotations = {}
    for (const face of FACE_ORDER) {
      const faceData = element.faces?.[face]
      if (!faceData || !Array.isArray(faceData.uv)) continue
      rects[face] = faceData.uv.slice(0, 4)
      if (faceData.rotation) rotations[face] = faceData.rotation
    }
    writeFaceUVs(geometry, rects, 16, 16, rotations)

    // 每个面按 textures 映射选择贴图，缺省使用第一张
    const materials = FACE_ORDER.map((face) => materialFor(resolveFaceTexture(element.faces?.[face])))
    const mesh = new THREE.Mesh(geometry, materials)
    const center = low.map((v, i) => (v + high[i]) / 2)
    const rotation = element.rotation || {}
    const origin = Array.isArray(rotation.origin) ? toVec3(rotation.origin) : [0, 0, 0]
    let angles = [0, 0, 0]
    if (rotation.axis && rotation.angle !== undefined) {
      const axis = { x: 0, y: 1, z: 2 }[rotation.axis] ?? 1
      angles[axis] = toNumber(rotation.angle)
    } else {
      angles = [toNumber(rotation.x), toNumber(rotation.y), toNumber(rotation.z)]
    }

    if (angles.some((v) => v !== 0)) {
      const holder = new THREE.Group()
      holder.position.set(origin[0], origin[1], origin[2])
      holder.rotation.order = 'ZYX'
      holder.rotation.set(deg(angles[0]), deg(angles[1]), deg(angles[2]))
      mesh.position.set(center[0] - origin[0], center[1] - origin[1], center[2] - origin[2])
      holder.add(mesh)
      root.add(holder)
    } else {
      mesh.position.set(center[0], center[1], center[2])
      root.add(mesh)
    }
    elementCount++
  }

  return { group: root, bones: new Map(), textureSize: { width: 16, height: 16 }, elementCount }
}
