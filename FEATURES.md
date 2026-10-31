# tsrbox 功能与架构清单（Tauri / BBMODEL 专版）

> 面向：后续开发/维护本项目的开发者与 AI。
> 配套：[CHANGELOG.md](./CHANGELOG.md)。改动代码后请同步更新对应章节与更新日志。
> 最后更新：2026-10-28

## 1. 项目定位与技术栈

由 Electron 版 tsrbox 重构而来的轻量 Tauri 版：只做 Blockbench `.bbmodel` 模型资源管理，不包含图片/音频/字体资源库和工具箱。

| 类别 | 选型 | 替代关系 |
| --- | --- | --- |
| 桌面框架 | Tauri 2（Rust + WebView2） | 替代 Electron |
| 前端 | Vue 3 + Vite 7 + element-plus + pinia + vue-router 5 + vue-i18n 9 | 原样复用 |
| 3D 渲染 | three 0.185（仅 bbmodel） | 原样复用 |
| 数据 | rusqlite（bundled SQLite） | 替代 better-sqlite3 |
| 配置 | Rust 自写 JSON 配置（`app_config_dir/tsrbox-bbmodel-config.json`） | 替代 electron-store |
| 文件对话框 | `tauri-plugin-dialog` | 替代 Electron dialog |
| 系统外链 | `tauri-plugin-opener` | 替代 shell.openExternal |
| 系统信息 | `sysinfo` + Windows PowerShell WMI 查询 | 替代 systeminformation |
| 拖出文件 | Windows OLE `DoDragDrop`（自写 `dragout.rs`） | 替代 Electron `startDrag` |
| 拖入文件 | Tauri 原生 `onDragDropEvent` | 替代 HTML5 dataTransfer |
| 语言 | zh-CN / en-US / ja-JP / es-ES / fr-FR / de-DE / ru-RU | 原样复用 |

Rust 1.87 环境下通过项目根 `.cargo/config.toml` 的 MSRV 回退解析自动选择兼容的 Tauri 2.11.x 依赖。

依赖与打包脚本见 [package.json](./package.json)、[src-tauri/tauri.conf.json](./src-tauri/tauri.conf.json)。

## 2. 目录与职责

```
src/                      Vue 前端（由 Electron 版复用并按 Tauri 适配）
  api.js                  前端 API 垫片：invoke / dialog / opener / 事件 / 窗口控制
  App.vue                 标题栏（data-tauri-drag-region）、侧边栏、主题、语言、外链
  router/index.js         /model、/settings、/shortcuts、/logs、/select-library
  views/
    HomeView.vue              模型列表（搜索/排序/筛选/文件夹/选择/批量操作/封面）
    UploadView.vue            模型上传与编辑（多模型行式暂存、贴图/动作关联、缩略图渲染）
    ModelLayout.vue           Home/Upload 切换
    SettingsView.vue          主题/皮肤/语言/更新检查/一键重新生成全部预览图
    ShortcutsView.vue         快捷键自定义
    LogsView.vue              操作日志查看/筛选/导出（系统保存对话框）
    LibrarySelectView.vue     资源库选择/新建/重命名/删除
  components/             ModelViewer、标签、全局上传进度等
  lib/bbmodel.js          Blockbench .bbmodel 解析器
  lib/bbanim.js           bbmodel 关键帧动画播放器
  lib/jsonModel.js        Bedrock geometry / Java Block&Item JSON 解析器
  lib/bedrockAnim.js      Bedrock .animation.json 解析与播放
  lib/geomCommon.js       JSON 模型通用贴图/材质/BoxUV 工具
  composables/            useStore、useDragUpload（Tauri 原生拖入）、useNativeDrag、useBoxSelect、useResourceShortcuts
  stores/settings.js      前端设置（主题/皮肤/语言/更新）
  stores/folders.js       文件夹状态（列表、当前文件夹、拖放悬停、批量移动）
  stores/upload.js        后台上传任务进度
  i18n/                   7 个语言包
src-tauri/                Rust 后端
  Cargo.toml
  tauri.conf.json         窗口（无边框 1280x820）、NSIS、CSP
  capabilities/default.json  窗口控制/对话框/外链权限
  icons/                  应用图标
  src/main.rs             入口
  src/lib.rs              插件注册、AppState、命令注册
  src/commands.rs         全部 IPC 命令（资源库/模型/标签/日志/拖出）
  src/store.rs            模型文件复制、哈希、封面读写、SQLite 行映射
  src/db.rs               建表与迁移（models / tags / operation_logs）
  src/config.rs           配置读写（libraryPath、libraries）
  src/device.rs           设备信息采集
  src/dragout.rs          Windows 原生文件拖出（OLE）
```

## 3. 运行与构建

```bash
npm install
npm run dev          # 仅前端（Vite 1420）
npm run tauri:dev    # Tauri 开发（推荐）
npm run build        # 前端构建校验
npm run tauri:build  # 出 Windows NSIS 安装包
```

## 4. 资源库体系

- 配置：`tsrbox-bbmodel-config.json`，保存 `libraryPath` 与 `libraries[]`。
- 每个资源库是独立目录，要求创建时为空；内部结构：
  - `library.json`：`name / createdAt / version / settings`
  - `imodel.db`：SQLite（models / tags / operation_logs）
  - `models/<hash 前2位>/<id>/`：模型原文件（`.bbmodel` / `.json` / `.vox`）、`cover.png`，以及 JSON 模型的贴图 / 动作关联文件
- 数据表：`models / folders / tags / operation_logs`；标签、文件列表、尺寸、视角参数以 JSON 字符串列存储；`animation_count` 记录动画数量；`content_key` 保存「主文件 + 贴图」内容指纹用于查重；`folder_id` 记录所属文件夹（`folders` 表仅存元数据，不移动磁盘文件）。
- 旧库打开时会自动补齐 `content_key` 与 `folder_id` 列，无需手动迁移。
- 打开数据库前清理 `-wal/-shm`，设置 `journal_mode=DELETE`、`synchronous=OFF`。
- 命令：`library_status/list/create/open/remove/close/rename/update_settings`；文件夹 `folders_list/create/update/delete`、`models_move_folder`。

## 5. 模型资源库（BBMODEL / JSON / VOX）

- 支持 `bbmodel`、`json`、`vox`：前端对话框、拖拽、后端命令三层校验；纯动画 `.json` 会被拒绝并提示作为动作文件添加。
- JSON 模型覆盖 Bedrock geometry（新旧格式）、Java Block/Item；`.animation.json` 作为动作文件关联，贴图 PNG/JPG/WEBP 作为贴图文件关联。
- 首页拖拽仅接受 BBMODEL / VOX；JSON 模型需点击右上角上传按钮进入专属上传页。
- 上传页采用多模型行式暂存：每个待上传模型占一行（左侧离屏渲染缩略图，右侧名称/格式/大小/尺寸/动画数），可继续添加多行；底部统一「上传全部」才复制入库并写数据库。
- 每一行的关联文件独立管理：JSON 行可拖入或选择贴图 / `.animation.json`，删除后自动重新渲染缩略图；编辑已有模型时保持单行并可展开修改名称、标签、简介。
- 侧边栏文件夹：`全部文件` 为默认虚拟分类（不可删除/重命名），文件夹可新建、重命名、自定义颜色、删除（删除后模型回到全部文件）；支持把选中模型拖入文件夹或批量「移动到文件夹」。文件夹仅写入数据库，不创建或迁移磁盘目录。
- 首页筛选行：文件格式、是否含动画、是否有预览图、文件大小区间（MB）、上传时间区间，可一键清空；与标签、文件夹筛选叠加。
- 卡片右键菜单：打开详情、重命名、刷新预览图、移动到文件夹、添加标签、导出（多选时批量导出到同一目录）、删除；多选时操作作用于选中集合。
- 关联文件：`models_add_aux_file` / `models_remove_aux_file` 复制到模型目录并记录在 `files` 字段。
- 查重：`content_key = SHA256(主文件 + 按名称排序的贴图文件)`；动作文件不参与，贴图不同视为不同模型；命中时提示是否继续上传（覆盖）。
- 封面：ModelViewer 渲染后截取 data URL，`models_save_image` 写入 `cover.png`。
- 列表：搜索、标签筛选、排序、预览尺寸/显示字段、单选/Ctrl/Shift/框选、原生拖出、批量删除/标签、快捷键。
- 详情：轨道控制、重置视角、保存视角与封面、动画下拉、导出到指定目录。
- 拖出：前端 `useNativeDrag` 拖动超过阈值后先进入待定状态；指针停在侧边栏文件夹时执行应用内移动，移出窗口后才调用 `resources_start_drag` → `dragout.rs` 的 Windows OLE `DoDragDrop`，支持多选文件。
- 拖入：`useDragUpload` 监听当前 Webview 的 `onDragDropEvent`，`drop` 时把路径与指针位置交给前端；上传页据此把贴图 / 动作文件挂到目标 JSON 行。应用自身拖出的文件通过全局标记避免重复导入。

## 6. 通用交互与全局设置

- 自定义标题栏：`decorations:false`，`data-tauri-drag-region` 拖动窗口，窗口控制走 `@tauri-apps/api/window`。
- 快捷键：全选/删除/导出/方向键，可在设置中自定义，输入框与弹窗聚焦时禁用。
- 后台静默上传（`stores/upload.js` + `UploadProgress.vue`），进度来自 Rust `batch-upload-progress` 事件。
- 设置：主题、皮肤色、语言、标题栏显示项、更新检查、一键重新生成全部预览图（离屏 ModelViewer 逐模型重新截图）；外链统一交给 `openUrl`。
- 预览图：首页模型卡片右键菜单可刷新单个或选中批量的预览图，用保存的视角重新渲染并写回封面。
- 侧边栏：资源库已打开时显示文件夹面板与底部「更多」菜单；点击文件夹跳转 `/model` 并按文件夹过滤。
- 日志导出使用系统保存对话框 + `logs_write_file`，不再依赖浏览器 Blob 下载。

## 7. 操作日志

- 写入当前库 `operation_logs`：时间、操作类型/模块、详情、状态、错误信息、`device_info`。
- `device_info`：sysinfo 获取 OS/CPU/内存/磁盘/网卡 MAC；PowerShell 补充 GPU、主板与主 IP。
- 日志页支持类型/模块/状态/时间筛选、详情查看、导出 JSON、清空。

## 8. 多语言

- 语言包：`src/i18n/*.js`（7 个），`main.js` 按设置切换，element-plus 同步 locale。
- 新增文案需同步全部语言文件；`upload.formats` 描述当前支持的 BBMODEL / JSON / VOX。

## 9. 维护注意与已知限制

- **权限**：新增插件命令必须在 `src-tauri/capabilities/default.json` 增加对应权限，否则前端 invoke 会被拒绝。
- **命令命名**：Rust 参数使用 snake_case，前端 invoke 使用 camelCase（Tauri 默认映射）。
- **同步/异步命令**：同步命令在主线程执行（`resources_start_drag` 依赖这一点）；批量上传为 `async fn` + `spawn_blocking`，通过事件回报进度。
- **Tauri 版本**：Rust 1.87 下由 MSRV 回退选择 Tauri 2.11.x；升级 Rust 后可移除 `.cargo/config.toml` 并升级到最新 Tauri。
- **前端 JS 包版本需与 Rust 插件版本同 minor**：当前 `@tauri-apps/api 2.11.1`、`plugin-dialog 2.7.3`、`plugin-opener 2.5.5`。
- **拖出仅 Windows 实现**：其他平台返回不支持提示；`dragout.rs` 仅实现 CF_HDROP，足够桌面/资源管理器接收文件。
- bbmodel 仅支持内嵌贴图；JSON 模型需要单独添加贴图与动作文件；VOX 使用内置调色板；外部粒子/声音特效不处理；非 cube 元素会跳过并告警。
- Bedrock 动画仅执行数值关键帧；molang 表达式按 0 处理，`pre/post` 取 `post`，`lerp_mode: step` 按阶跃采样。
- 资源库创建要求空目录；旧 Electron 库可打开（会自动补充 `animation_count` / `content_key` / `folder_id` 列）。
- 前端构建通过；Rust `cargo check` / `cargo build` 通过；`cargo test --lib` 覆盖模型入库、封面标记、动画数与标签往返、文件夹增删改与批量移动。
