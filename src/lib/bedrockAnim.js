/**
 * Bedrock .animation.json 解析与播放。
 * 支持：
 * - bones.<bone>.<channel> = 数字 / 数组 / 字符串 / 带 pre|post 的对象 / 以时间为 key 的对象
 * - animation_length、loop
 * - 位置单位为模型像素，旋转角度为角度值
 */
import * as THREE from 'three'
import { toNumber } from './geomCommon.js'

const CHANNELS = ['rotation', 'position', 'scale']

function vectorOf(value) {
  if (Array.isArray(value)) {
    return [toNumber(value[0]), toNumber(value[1]), toNumber(value[2])]
  }
  const n = toNumber(value)
  return [n, n, n]
}

function normalizeChannel(raw) {
  if (raw === undefined || raw === null) return null

  if (Array.isArray(raw) || typeof raw === 'number' || typeof raw === 'string') {
    return [{ t: 0, v: vectorOf(raw), step: false }]
  }

  if (typeof raw === 'object') {
    // 单帧 pre/post 写法
    if ('pre' in raw || 'post' in raw) {
      const value = raw.post !== undefined ? raw.post : raw.pre
      return [{ t: 0, v: vectorOf(value), step: raw.lerp_mode === 'step' }]
    }
    const keyframes = []
    for (const [timestamp, value] of Object.entries(raw)) {
      const time = Number.parseFloat(timestamp)
      if (!Number.isFinite(time)) continue
      if (value && typeof value === 'object' && !Array.isArray(value)) {
        const inner = value.post !== undefined ? value.post : value.pre
        keyframes.push({ t: time, v: vectorOf(inner), step: value.lerp_mode === 'step' })
      } else {
        keyframes.push({ t: time, v: vectorOf(value), step: false })
      }
    }
    keyframes.sort((a, b) => a.t - b.t)
    return keyframes.length ? keyframes : null
  }
  return null
}

function sample(keyframes, time) {
  if (!keyframes || !keyframes.length) return null
  if (time <= keyframes[0].t) return keyframes[0].v
  const last = keyframes[keyframes.length - 1]
  if (time >= last.t) return last.v
  for (let i = 0; i < keyframes.length - 1; i++) {
    const current = keyframes[i]
    const next = keyframes[i + 1]
    if (time >= current.t && time <= next.t) {
      if (current.step) return current.v
      const span = next.t - current.t || 1
      const ratio = (time - current.t) / span
      return current.v.map((v, index) => v + (next.v[index] - v) * ratio)
    }
  }
  return last.v
}

/** 解析 .animation.json → [{name, length, loop, bones}] */
export function parseBedrockAnimations(json) {
  const animations = json && typeof json.animations === 'object' ? json.animations : null
  if (!animations) return []

  const result = []
  for (const [name, raw] of Object.entries(animations)) {
    if (!raw || typeof raw !== 'object') continue
    let maxTime = 0
    const bones = {}
    for (const [boneName, channels] of Object.entries(raw.bones || {})) {
      const normalized = {}
      for (const channel of CHANNELS) {
        const keyframes = normalizeChannel(channels?.[channel])
        if (keyframes && keyframes.length) {
          normalized[channel] = keyframes
          maxTime = Math.max(maxTime, keyframes[keyframes.length - 1].t)
        }
      }
      if (Object.keys(normalized).length) bones[boneName] = normalized
    }
    const length = toNumber(raw.animation_length, 0) || maxTime
    result.push({
      name,
      length: length > 0 ? length : Math.max(maxTime, 0.001),
      loop: !!raw.loop,
      bones,
    })
  }
  return result
}

export class BedrockAnimator {
  /** @param {Map<string, {object: THREE.Object3D, basePosition: THREE.Vector3, baseRotation: THREE.Euler, baseScale: THREE.Vector3}>} bones */
  constructor(bones) {
    this.bones = bones
    this.animation = null
    this.time = 0
  }

  setAnimation(animation) {
    this.reset()
    this.animation = animation
    this.time = 0
    this.apply(0)
  }

  reset() {
    this.animation = null
    this.time = 0
    for (const bone of this.bones.values()) {
      bone.object.position.copy(bone.basePosition)
      bone.object.rotation.copy(bone.baseRotation)
      bone.object.scale.copy(bone.baseScale)
    }
  }

  update(delta) {
    if (!this.animation) return
    this.time += delta
    let time = this.time
    const length = this.animation.length
    if (length > 0) {
      if (this.animation.loop) time %= length
      else if (time > length) time = length
    }
    this.apply(time)
  }

  apply(time) {
    if (!this.animation) return
    for (const [name, channels] of Object.entries(this.animation.bones)) {
      const bone = this.bones.get(name.toLowerCase())
      if (!bone) continue

      if (channels.rotation) {
        const value = sample(channels.rotation, time)
        if (value) {
          // Bedrock 动画旋转与几何体一致：x、y 取反，z 保持
          bone.object.rotation.set(
            bone.baseRotation.x - THREE.MathUtils.degToRad(value[0]),
            bone.baseRotation.y - THREE.MathUtils.degToRad(value[1]),
            bone.baseRotation.z + THREE.MathUtils.degToRad(value[2]),
            bone.baseRotation.order,
          )
        }
      }
      if (channels.position) {
        const value = sample(channels.position, time)
        if (value) {
          // 位移同样按 x 取反换算到 Blockbench 坐标
          bone.object.position.set(
            bone.basePosition.x - value[0],
            bone.basePosition.y + value[1],
            bone.basePosition.z + value[2],
          )
        }
      }
      if (channels.scale) {
        const value = sample(channels.scale, time)
        if (value) {
          bone.object.scale.set(
            bone.baseScale.x * (value[0] || 1),
            bone.baseScale.y * (value[1] || 1),
            bone.baseScale.z * (value[2] || 1),
          )
        }
      }
    }
  }
}
