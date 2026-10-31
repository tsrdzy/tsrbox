import { reactive } from 'vue'

/** 导出格式列表（与 Rust convert::Format 对应） */
export const EXPORT_FORMATS = ['original', 'bbmodel', 'bedrock', 'java', 'generic']

/**
 * 全局导出格式选择弹窗状态：
 * 由 App.vue 渲染弹窗，任意页面调用 askExportFormat() 等待用户选择。
 */
export const exportFormatDialog = reactive({
  visible: false,
  format: 'original',
  remember: false,
  resolve: null
})

export function askExportFormat(defaultFormat = 'original') {
  return new Promise((resolve) => {
    exportFormatDialog.format = EXPORT_FORMATS.includes(defaultFormat)
      ? defaultFormat
      : 'original'
    exportFormatDialog.remember = false
    exportFormatDialog.resolve = resolve
    exportFormatDialog.visible = true
  })
}

export function closeExportFormatDialog(format) {
  const resolve = exportFormatDialog.resolve
  exportFormatDialog.resolve = null
  exportFormatDialog.visible = false
  resolve?.(format || null)
}
