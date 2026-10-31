import { reactive, markRaw } from 'vue'

/**
 * 插件系统核心
 *
 * - 构建时通过 import.meta.glob 扫描 src/plugins/<插件名>/index.js
 * - 该目录被 .gitignore 忽略，不会提交到仓库；目录不存在时 glob 结果为空，
 *   应用自动以“纯核心”模式运行（无任何插件功能）
 * - 每个插件默认导出一个对象：{ name: string, setup(context): void }
 *
 * context 提供的能力：
 * - router / i18n            应用实例引用
 * - addRoute(route)          注册页面路由
 * - addMessages(locale,msg)  合并多语言文案
 * - storage                  通用 KV 持久化（window.api.pluginStorage）
 * - setBottomButton(obj)     覆盖左下角按钮显示（传入 reactive 对象，字段见下）
 * - registerBottomPanel(vue) 在左下角弹层菜单上方挂载一个组件（收到 prop: closePopover）
 * - registerBottomMenuItem(item) 在弹层菜单底部追加菜单项
 *
 * bottomButton 字段（reactive，插件直接改字段即可更新 UI）：
 * { type: 'icon' | 'avatar', icon, text, loading, avatarUrl, initial }
 *
 * bottomMenuItem 字段：
 * { id, icon, text, danger?, hidden?, onClick(closePopover) }
 */
const modules = import.meta.glob('./plugins/*/index.js', { eager: true })

export const pluginSlots = reactive({
  bottomButton: null,
  bottomPanels: [],
  bottomMenuItems: []
})

export function initPlugins(context) {
  const { router, i18n } = context

  const pluginContext = {
    router,
    i18n,
    storage: window.api.pluginStorage,
    addRoute: (route) => router.addRoute(route),
    addMessages: (locale, messages) => i18n.global.mergeLocaleMessage(locale, messages),
    setBottomButton: (button) => {
      pluginSlots.bottomButton = button
    },
    registerBottomPanel: (component) => {
      pluginSlots.bottomPanels.push(markRaw(component))
    },
    registerBottomMenuItem: (item) => {
      const index = pluginSlots.bottomMenuItems.findIndex((old) => old.id === item.id)
      if (index >= 0) pluginSlots.bottomMenuItems.splice(index, 1, item)
      else pluginSlots.bottomMenuItems.push(item)
    }
  }

  for (const [path, mod] of Object.entries(modules)) {
    const plugin = mod && mod.default
    if (!plugin || typeof plugin.setup !== 'function') {
      console.warn(`[Plugin] ${path} 缺少 default export { setup }`)
      continue
    }
    try {
      plugin.setup(pluginContext)
      console.info(`[Plugin] 已加载: ${plugin.name || path}`)
    } catch (e) {
      console.error(`[Plugin] 加载失败: ${path}`, e)
    }
  }
}
