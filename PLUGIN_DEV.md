# tsrbox 插件开发文档

tsrbox 内置了一个轻量插件系统，用于承载**不适合公开的功能**（如登录授权、内部接口对接等）。
插件代码放在 `src/plugins/` 下，该目录已加入 `.gitignore`，**永远不会被提交到 Git 仓库**。

## 工作原理

- 核心在构建时通过 `import.meta.glob('./plugins/*/index.js')` 扫描插件（见 `src/pluginManager.js`）。
- 目录不存在或为空时，应用以「纯核心」模式运行，不会报错。
- 每个插件是一个文件夹，入口为 `index.js`，默认导出一个带 `setup` 的对象。
- 插件随主应用一起打包（`npm run build` / `tauri build`），无需单独构建。

## 快速开始

```
src/plugins/
└── my-feature/
    ├── index.js        # 入口（必需）
    ├── store.js        # 状态与业务逻辑（可选）
    ├── messages.js     # 多语言文案（可选）
    └── MyPanel.vue     # 组件（可选）
```

`src/plugins/my-feature/index.js`：

```js
export default {
  name: 'my-feature',
  setup(context) {
    // 在这里注册能力，见下方 API
  }
}
```

## context API

### 基础引用

| 属性 | 说明 |
| --- | --- |
| `context.router` | vue-router 实例 |
| `context.i18n` | vue-i18n 实例 |
| `context.storage` | 通用 KV 持久化（见下） |

### 注册路由

```js
import MyView from './MyView.vue'

context.addRoute({
  path: '/my-page',
  name: 'my-page',
  component: MyView,
  meta: { title: 'myPlugin.pageTitle', requiresAuth: true }
})
```

`requiresAuth: true` 表示需要已打开资源库（沿用核心路由守卫）。

### 注册多语言

```js
context.addMessages('zh-CN', { myPlugin: { title: '我的功能' } })
context.addMessages('en-US', { myPlugin: { title: 'My Feature' } })
```

合并后全局可用 `t('myPlugin.title')`。语言切换时已注册文案自动生效。

### 左下角按钮

```js
import { reactive } from 'vue'

const button = reactive({
  type: 'icon',          // 'icon' | 'avatar'
  icon: 'icon-yonghu',   // iconfont 类名
  text: '入口',
  loading: false,        // true 时图标旋转
  avatarUrl: '',         // type 为 avatar 时使用
  initial: '?'           // 无头像时的占位字符
})
context.setBottomButton(button)
// 之后直接改 button 的字段即可更新 UI（响应式）
button.text = '新文字'
```

不传时核心显示默认「更多」按钮。

### 弹层顶部面板

```js
import MyPanel from './MyPanel.vue'
context.registerBottomPanel(MyPanel)
```

面板组件渲染在左下角弹层菜单上方，会收到 prop `closePopover()`，执行动作后调用它关闭弹层。

### 弹层菜单项

```js
const item = reactive({
  id: 'my-item',          // 唯一 id，重复注册会覆盖
  icon: 'icon-shezhi',
  text: '菜单项',
  danger: false,          // true 显示为红色（如退出登录）
  hidden: false,          // 可响应式控制显隐
  onClick: (closePopover) => {
    closePopover()
    // ...
  }
})
context.registerBottomMenuItem(item)
```

菜单项渲染在核心菜单（教程/反馈/设置等）之后。

### 持久化存储

通用 KV，数据保存在应用配置目录的 `tsrbox-bbmodel-config.json` 的 `pluginData` 字段中：

```js
await context.storage.set('my-feature', { token: '...' })
const data = await context.storage.get('my-feature')   // 不存在返回 null
await context.storage.remove('my-feature')
```

- key 建议用插件名，避免冲突。
- value 必须是可 JSON 序列化的数据。

## 完整示例

参考登录插件 `src/plugins/auth/`（本地目录，不在仓库中）：

- `index.js`：注册路由/文案/按钮/面板/菜单项，watch 登录态同步 UI
- `store.js`：授权轮询逻辑 + `storage` 持久化
- `AuthPanel.vue`：弹层用户卡片
- `AccountView.vue`：个人中心页面
- `messages.js`：7 语言文案

## 注意事项

1. **不要把插件目录提交进仓库**：`src/plugins/` 已在 `.gitignore` 中，拉取公开代码的人没有该目录也能正常构建运行。
2. 核心只提供通用能力（KV 存储、插槽），**业务逻辑全部写在插件内**，这样核心仓库不含敏感实现。
3. 插件间通过不同的 storage key 和菜单项 id 隔离，互不干扰。
4. 插件加载失败只会打印 `console.error`，不影响核心与其他插件。
5. 插件需要网络请求时直接用 `fetch`（Tauri WebView 无 CSP 限制），需要打开浏览器用 `window.api.openExternal(url)`。
