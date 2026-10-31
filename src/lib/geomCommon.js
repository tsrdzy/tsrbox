/**
 * JSON / VOX 模型解析的通用工具：贴图、材质、BoxGeometry 的逐面 UV 写入。
 * 面顺序与 bbmodel 解析器保持一致：+x,-x,+y,-y,+z,-z → east,west,up,down,south,north
 */
import * as THREE from 'three'

export const FACE_ORDER = ['east', 'west', 'up', 'down', 'south', 'north']

export function toNumber(value, fallback = 0) {
  const n = Number(value)
  return Number.isFinite(n) ? n : fallback
}

/** 解析 [x,y,z] / 单个数字 / molang 字符串为三元数组 */
export function toVec3(value, fallback = 0) {
  if (Array.isArray(value)) {
    return [toNumber(value[0], fallback), toNumber(value[1], fallback), toNumber(value[2], fallback)]
  }
  const n = toNumber(value, fallback)
  return [n, n, n]
}

export function deg(value) {
  return THREE.MathUtils.degToRad(toNumber(value))
}

/** 从 data URL 加载贴图（Image 解码，保留像素锐利度） */
export function loadTextureFromDataUrl(dataUrl) {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const texture = new THREE.Texture(img)
      texture.colorSpace = THREE.SRGBColorSpace
      texture.magFilter = THREE.NearestFilter
      texture.minFilter = THREE.NearestFilter
      texture.generateMipmaps = false
      texture.needsUpdate = true
      resolve(texture)
    }
    img.onerror = () => reject(new Error('贴图加载失败'))
    img.src = dataUrl
  })
}

/** 未上传贴图时使用的占位材质 */
export function createModelMaterial(texture) {
  return new THREE.MeshStandardMaterial({
    color: texture ? 0xffffff : 0xb0b0c0,
    map: texture || null,
    metalness: 0.05,
    roughness: 0.8,
    side: THREE.DoubleSide,
    transparent: !!texture,
    alphaTest: texture ? 0.05 : 0,
  })
}

function toCoord(value) {
  return Number.isFinite(Number(value)) ? Number(value) : 0
}

/**
 * 按逐面矩形写入 BoxGeometry 的 UV。
 * 保留矩形的原始方向（uv 坐标可能倒序），以支持镜像 / 反向 UV。
 * @param {THREE.BoxGeometry} geometry
 * @param {Record<string, number[]>} rects 面名 → [u1,v1,u2,v2]（纹理像素坐标）
 * @param {number} texWidth
 * @param {number} texHeight
 * @param {Record<string, number>} rotations 面名 → 旋转角度（90 的倍数）
 */
export function writeFaceUVs(geometry, rects, texWidth, texHeight, rotations = {}) {
  if (!rects) return
  const uvAttr = geometry.attributes.uv
  FACE_ORDER.forEach((face, faceIndex) => {
    const raw = rects[face]
    if (!raw || raw.length < 4) return
    const [x1, y1, x2, y2] = raw.map(toCoord)
    const left = x1 / texWidth
    const right = x2 / texWidth
    const top = 1 - y1 / texHeight
    const bottom = 1 - y2 / texHeight
    const corners = [
      [left, top],
      [right, top],
      [left, bottom],
      [right, bottom],
    ]
    let turns = Math.round(toNumber(rotations[face]) / 90) % 4
    while (turns > 0) {
      const first = corners[0]
      corners[0] = corners[2]
      corners[2] = corners[3]
      corners[3] = corners[1]
      corners[1] = first
      turns--
    }
    const base = faceIndex * 8
    for (let i = 0; i < 4; i++) {
      uvAttr.array[base + i * 2] = corners[i][0]
      uvAttr.array[base + i * 2 + 1] = corners[i][1]
    }
  })
  uvAttr.needsUpdate = true
}

/** data URL / base64 文本转 Uint8Array */
export function base64ToBytes(base64) {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

export function base64ToText(base64) {
  return new TextDecoder().decode(base64ToBytes(base64))
}

export function textureMime(name) {
  const ext = String(name || '').toLowerCase().split('.').pop()
  if (ext === 'jpg' || ext === 'jpeg') return 'image/jpeg'
  if (ext === 'webp') return 'image/webp'
  return 'image/png'
}
