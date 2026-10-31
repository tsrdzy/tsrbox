# 更新日志

> 维护说明（给后续开发者/AI）：本文件与 [FEATURES.md](./FEATURES.md) 配套使用。
> 每次发版在文件顶部新增条目；版本号以 `package.json` 为准；历史条目由 git 提交整理。
> 条目分类统一为：新增 / 修改 / 修复 / 移除。

## [未发布] 0.2.6-tauri（Tauri 重构版）

### 新增

- 新增文件夹体系：`folders` 表 + `models.folder_id` 字段，只在数据库内分类，不创建/迁移磁盘目录。侧边栏提供「全部文件」默认分类（不可删除）与文件夹列表，支持新建、重命名、自定义颜色、删除（模型退回全部文件）、拖入模型、多选批量移动。旧库自动补列。
- 上传页重做为多模型行式暂存：每个模型占一行（左侧离屏渲染缩略图，右侧文件信息与关联文件），可分多次添加，底部「上传全部」统一提交；编辑已有模型保持单行并可展开编辑名称/标签/简介。
- 首页标签栏下新增筛选行：文件格式、是否含动画、是否有预览图、文件大小区间（MB）、上传时间区间，并与标签/文件夹筛选叠加，支持一键清空与结果计数。
- 首页卡片右键菜单恢复完整选项：打开详情、重命名、刷新预览图（单个/批量）、移动到文件夹、添加标签、导出（多选批量导出到同一目录）、删除。
- 新增 `models_export_batch` 命令与前端 `folders_* / models_move_folder` API。
- 使用 Tauri 2 + Rust 重构 tsrbox，项目位于 `f:\tauri\tsrbox`，前端复用 Electron 版裁剪后的 Vue 代码。
- Rust 后端实现资源库、模型、标签、操作日志、配置与设备信息命令。
- 使用 rusqlite（bundled SQLite）替代 better-sqlite3，并补充 `animation_count` 字段与旧库迁移。
- 使用 `tauri-plugin-dialog` 替代 Electron 文件对话框，`tauri-plugin-opener` 替代外链打开。
- 使用 Tauri 原生 `onDragDropEvent` 接收拖入文件路径；自写 Windows OLE `DoDragDrop` 实现文件拖出到桌面，替代 Electron `startDrag`。
- 日志导出改为系统保存对话框 + Rust 写文件。
- 新增 JSON 模型支持：Bedrock geometry（新旧格式）与 Java Block/Item，解析器位于 `src/lib/jsonModel.js`。
- 新增 VOX（MagicaVoxel）体素模型预览，使用 three.js `VOXLoader`。
- 新增 Bedrock `.animation.json` 解析与播放（`src/lib/bedrockAnim.js`）。
- 重做上传界面：单一拖拽/选择上传框，底部文件行展示主文件与贴图/动作，点击「上传」前只做暂存预览（`models_stage_model` / `models_stage_aux`），不会写入资源库。
- 首页拖拽上传仅接受 BBMODEL / VOX；拖入 JSON 会提示改用专属上传页。右上角上传按钮直接进入上传页。
- JSON / VOX 模型拖出到桌面时会自动带上关联的贴图与动作文件。
- 精细化查重：以「模型文件 + 全部贴图」内容指纹（`content_key`）判断重复，动作文件不参与；命中时提示是否继续上传并覆盖。旧库打开时自动回填指纹。
- 设置新增「一键重新生成全部预览图」，使用离屏 ModelViewer 按每个模型保存的视角重新渲染封面并显示进度。
- 首页模型卡片右键新增「刷新预览图」，可单独重建某个模型的封面。
- 应用图标改为透明背景，并重新生成 ico/icns/各尺寸 PNG。

### 修改

- 侧边栏由「模型资源库」单入口改为文件夹面板（196px）：顶部新建按钮，列表显示颜色点、名称与数量，右键可重命名/改色/删除；底部保留「更多」菜单。
- 卡片拖动逻辑拆分：拖动超过阈值后先进入待定状态，可拖到侧边栏文件夹完成应用内移动；指针移出窗口才发起系统 OLE 拖出，避免与文件夹拖放冲突。
- 上传页 `.json` 行支持按指针位置接收贴图/动作文件（`useDragUpload` 回传 drop 位置）；替换贴图后自动重新渲染缩略图。
- 批量/拖拽上传与覆盖现在会保留或继承当前文件夹（`folder_id`）。
- 自定义标题栏改用 `data-tauri-drag-region` 与 Tauri Window API。
- 配置存储改为 `app_config_dir/tsrbox-bbmodel-config.json`，与 Electron 版配置隔离。
- 为兼容本机 Rust 1.87，项目根加入 `.cargo/config.toml` 的 MSRV 回退解析，Tauri 固定到 2.11.x；前端 Tauri 包版本与之对齐。
- 语言包清理：移除图片/音频/字体/工具箱等已裁撤功能遗留的约 300 条死键（含 sidebar 旧入口与未使用的 support/removeTag 文案），7 个语言包仅保留模型库相关条目。
- 打包优化：ModelViewer 改为 `defineAsyncComponent` 异步加载，three.js 独立为 563 kB chunk，主包由 1.86 MB 降至 179 kB；`vite.config.js` 拆分 `element-plus` 与 `vue-vendor` 供应商包。
- 路由守卫改为静态引入 `useStore`，消除 Vite 动态/静态混合导入告警。
- 创建资源库的路径输入框提示改为「请选择一个空文件夹，资源库将创建在该目录」。
- 全局移除按钮、下拉菜单项与标签页的焦点框样式。
- 上传格式提示更新为 BBMODEL / JSON / VOX；首页拖拽仅 BBMODEL / VOX。
- 编辑/上传页底部文件列表与表单重排：输入框、标签输入与描述统一单列对齐，操作按钮固定在底部右侧。

### 修复

- 修复模型预览中的 `hitbox` 外框：bbmodel 中 6 面 `texture:null` 的元素、Bedrock JSON 中 `uv:{}` 的方块现在会被跳过，不再出现绿色包裹方块或整张贴图糊在模型外。
- 修复 JSON 模型逐面 UV 方向：保留负 `uv_size` 的镜像方向，up/down 面按 Blockbench 规则旋转 180°，贴图映射与 Blockbench 一致。
- 修复生产构建后主题初始化失效：`/src/assets/theme-init.js` 不会被 Vite 打包，安装包启动时脚本 404、深色主题要等 Vue 挂载才生效；现内联到 `index.html` 的 `<head>`，并删除该文件。
- 补全 6 个语言包缺失的文案：en-US 3 条，ja-JP / es-ES / fr-FR / de-DE / ru-RU 各 17 条（含快捷键名称），修复界面直接显示 i18n key 的问题。

### 移除

- Electron 主进程 / preload / electron-builder 配置及全部 Electron 依赖。
- 图片、音频、字体资源库与工具箱（延续 Electron 裁剪版的阉割范围）。

### 说明

- 仅支持 `.bbmodel`；支持资源库管理、上传/编辑/导出、封面、视角、动画、标签、搜索排序、框选多选、拖入拖出、快捷键、设置、日志与 7 种语言。
- 前端 `npm run build` 通过；Rust `cargo check` / `cargo build` 通过；应用已实际启动并验证自定义标题栏与资源库选择页；`cargo test --lib` 数据层测试通过。

## [未发布] 0.2.6-bbmodel（BBMODEL 专版）

### 新增

- 从 ipixel 拆分出独立项目 `tsrbox`（外层 electron 目录），定位为纯 Blockbench 模型资源库。

### 修改

- 模型资源库仅允许上传、存储与预览 `.bbmodel` 文件；文件对话框、批量扫描、拖拽上传的扩展名白名单同步收紧。
- 模型上传页移除辅助文件（MTL/贴图/动画/BIN）管理，bbmodel 贴图依赖内嵌 data URI。
- 7 个语言包的格式提示与关于页描述改为“仅支持 BBMODEL”。
- 依赖精简：移除图片/音频/字体/工具相关依赖（sharp、colorthief、wavesurfer、gif.js、jszip、file-saver、fflate、png-to-ico、image-conversion、electron-updater 等）。

### 移除

- 图片、音频、字体资源库及其主进程 IPC / 存储模块 / 前端页面 / 路由 / 侧边栏入口。
- 工具箱（9 个工具）及对应 IPC、多语言条目。
- 模型辅助文件增删功能与 `models:add-aux-file` / `models:remove-aux-file` IPC。
- ModelViewer 中 GLTF / OBJ / STL / FBX / Three.js JSON 加载分支与相关 loader 依赖。
- 遗留的 `InitView.vue`、`useUploadState.js` 等未引用文件。

### 说明

- 保留资源库管理、模型上传/编辑/导出、封面与视角、动画播放、标签、搜索排序、框选多选、原生拖出、快捷键、设置、操作日志、自动更新检查与 7 种语言。

## [未发布] 0.2.6（当前开发版）

### 新增

- 3D 模型资源库支持 Blockbench `.bbmodel` 格式上传、预览与动画播放（解析器 `src/renderer/src/lib/bbmodel.js`、播放器 `src/renderer/src/lib/bbanim.js`，Viewer 接入见 `ModelViewer.vue`）。
- 新增内部文档：`CHANGELOG.md`（本文件）、`FEATURES.md`（功能与架构清单）。
- 全局后台上传进度条（`stores/upload.js` + `components/UploadProgress.vue`）：上传不再阻塞界面，底部汇总进度可展开查看明细、可隐藏到后台继续加载。
- 资源卡片支持原生拖出：在卡片上按住拖动即可把源文件（含多选）拖到桌面或其它应用（`useNativeDrag` + 主进程 `resources:start-drag`）。
- 标签筛选改为单行标签条 + 最右侧“全部标签”弹窗（`components/TagFilterBar.vue`），支持搜索、多选、清空。
- 列表页框选仅在空白区域触发；在资源卡片上拖动改为拖出文件。

### 修改

- 上传入口扩展名同步追加 `bbmodel`：主进程文件/文件夹选择对话框、批量扫描、上传页、首页拖拽、7 个语言包的格式提示文案。
- `.bbmodel` 按 Blockbench 约定以 16 单位 = 1 格缩放到“格”单位，使卡片尺寸、取景与阴影表现一致。
- 预览尺寸调小时卡片自动精简信息：compact 隐藏标签/颜色/日期并缩小字号，tiny 仅保留名称（模型/图片/音频/字体四类列表）。
- 未选中任何资源时隐藏右侧多选操作条。
- 快捷键在四个资源列表页统一生效，修复此前仅图片页部分接入、且未读取自定义设置的问题。
- 软件更名为 `tsrbox`：`package.json`、`electron-builder.yml`（productName/executableName）、窗口标题、7 个语言包、初始化页/字体预览样例文案、README；应用图标换用 tsrcraft 像素 logo 并重新生成 `resources/icon.png|icon_256.png|icon.ico`。

### 修复

- 框选卡顿：抽出共享 `useBoxSelect` composable，mousemove 用 requestAnimationFrame 节流，拖动期间实时高亮卡片并对内容区加 `user-select:none`，消除高频重渲染与文本选中干扰（四个资源列表页统一接入）。
- 资源卡片无法拖出为文件：`useNativeDrag` 在卡片 mousedown 时立即 `preventDefault` 阻止浏览器默认图片/文本拖拽，确保 Electron `startDrag` 接管本次手势。
- 多选操作条被压缩、文字换行：`.filter-right/.batch-actions/.selected-count` 加 `flex-shrink:0` 与 `white-space:nowrap`，`.filter-left` 改为 `flex:1;min-width:0` 允许收缩。
- 标签条单独占一行：`TagFilterBar` 去掉 `max-width:46vw`，改为 `flex:1;min-width:0`，标签 chip 超出时裁剪而非换行。
- 框选掉帧：`useBoxSelect` 恢复“仅 mouseup 计算选中”的原始流畅策略——拖动过程中只通过 `selectionBoxRef` 直接写 DOM 更新框选框样式，不更新 `selectedIds`；mousedown 时一次性缓存卡片矩形，mouseup 时一次性计算最终选中集合。消除实时选中态引发的整格重渲染。
- 同窗口拖回重复上传：`useNativeDrag` 发起原生拖出时设置全局标记 `__tsrboxInternalDragOut`，`useDragUpload` 在 `drop`/`dragenter`/`dragover` 中检测该标记并跳过上传，避免把自己拖出的文件再次导入。

### 说明

- `.bbmodel` 贴图需内嵌（data URI）；外部引用的 png 不会加载，将以标记色显示。
- `.bbmodel` 动画自动出现在原有动画下拉面板并默认播放第 1 个。

## [0.2.5] - 2026-07-17

### 新增

- 列表页右键菜单：打开详细信息、删除、添加标签、全选、刷新。
- 全局快捷键：`Ctrl+A` 全选、`Ctrl+S` 导出选中、`Delete` 删除选中、方向键切换选择，支持自定义。
- 回到顶部按钮。
- 工具箱新增「图片转像素图」。

### 修改

- 修复部分语言文件文案错误。

## [0.2.4] - 2026-07-15

### 新增

- 颜色下拉框重做（颜色聚合、按色筛选与容差）。
- 工具箱新增「音频速度调整」「音视频分离」。

### 修复

- 图片资源库主色获取错误。

## [0.2.3] - 2026-07-14

### 修改

- 批量删除时按钮的显示问题。
- README 支持中英文。
- 模型详细信息、设置界面部分图标修复。

## [0.2.2] - 2026-07-14

### 新增

- 拖拽上传功能（模型 / 图片 / 音频 / 字体，支持多文件与文件夹）。

## [0.2.1] - 2026-07-12

### 新增

- 皮肤（主题色）功能：红、橙、黄、绿、蓝、紫、灰。
- `@` 路径符（路径输入快捷方式）。

### 修复

- 资源覆盖时报错。
- 关闭「关于 iPixel」界面时报错。

## [0.2.0] - 2026-07-12

### 新增

- 音频详细界面播放功能（播放/暂停、进度、速度、循环等）。
- 工具箱新增「GIF 生成器」。
- 操作日志采集更多电脑基础信息。
- 标题栏下拉框新增「创建资源库」。
- 启动时自动检测更新。
- 关于页新增爱发电链接。
- 设置界面显示优化。

### 修复

- 图片/音频/字体双击无法进入详细界面。
- 详细界面右侧介绍内容溢出。
- 图片/字体/音频上传重复文件缺少提示。
- 切换资源库过程中仍可切换到其他库。
- `upload.addTag` 未按语言翻译。
- 操作日志截取/覆盖不完整。
- 图片上传颜色获取错误。
- 从工具导入资源库后资源信息丢失。

## [0.1.8] - 2026-07-10

### 修改

- 更新软件描述与名称。

### 修复

- 若干小问题。

## [0.1.7] - 2026-07-10

### 修复

- 批量修复已知 BUG。

## [0.1.6] - 2026-07-09

### 新增

- 工具箱：图标（ICO）生成、图片压缩、图片格式转换。
- 显示设置界面优化。
- 音频编辑器循环播放文案改为图标。

## [0.1.5] - 2026-07-09

### 修复

- 操作日志无效。
- 顶部右侧资源控制按钮样式。
- 其他若干问题。

## [0.1.4] - 2026-07-09

### 新增

- 操作日志支持导出/导入，用于定位错误。
- 关于界面新增多个外链。

### 修复

- 检查更新异常。

## [0.1.3] - 2026-07-09

### 修改

- 项目更名为 ipixel。
- 更新 README。

## [0.1.2] - 2026-07-08

### 新增

- 图片、音频、字体资源库。
- 工具箱页面：图片合并、图片分割。
- 设置页面。
- 主题/字体字段隐藏显示。
- 预览大小、显示字段设置。

## [0.1.0] - 2026-07-07

### 新增

- 图片、音频、字体资源库基础能力。
- 侧边栏导航。

## [0.0.5] - 2026-07-05

### 新增

- 多语言支持（后扩展为 7 种语言）。

## [0.0.4] - 2026-07-05

### 新增

- 标签、格式显示具体数量与图标。

## [0.0.3] - 2026-07-05

### 修复

- 滚动与框选同时触发的冲突。
- 批量添加标签报错。

## [0.0.2] - 2026-07-05

### 新增

- 顶部搜索、排序、筛选。
- 单选、多选、框选。
- 批量删除、批量添加标签。

## [0.0.1] - 2026-07-05

### 修改

- 资源界面图标修复、资源库选择界面美化。
- 上传模型按钮改为 hover 展开，直接点击默认单个上传。
- 优化模型详细信息展示。

### 修复

- 添加辅助文件后预览图不刷新。

## 初始提交 - 2026-07-05

- 项目初始化（Electron + Vue 3 + Vite）。
