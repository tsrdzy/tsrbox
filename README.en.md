# tsrbox

<div align="right">
  <a href="README.md">中文</a> | <a href="README.en.md">English</a>
</div>

## About

tsrbox is a local desktop resource manager for 3D models: Blockbench `.bbmodel`, `.json` models (Bedrock geometry / Java Block&Item) and MagicaVoxel `.vox` voxel models. All data is stored in local libraries, with cover preview, animation playback, tag classification, batch management and export.

## Features

- Manages BBMODEL, JSON and VOX models with a unified upload/storage/preview flow
- JSON models can be paired with textures and Bedrock `.animation.json` files on the upload page
- Precise duplicate detection by the combined content of model file + textures (animations excluded); prompt to continue/overwrite when identical
- Regenerate all covers from Settings, or right-click cards to refresh single/batch covers
- Drag-and-drop on the home page supports BBMODEL / VOX only; JSON models go through the dedicated upload page
- Multi-model upload page: one row per model (thumbnail, file info, related textures/animations); add several models and click "Upload All" once
- Sidebar folders: the default "All Files" cannot be deleted; folders support create, rename, custom color and delete. Drag models onto a folder or move a multi-selection (database-only classification, no files moved on disk)
- Filter row: format, has animations, has cover, file size range and upload time range, combined with tag/folder filters
- Card context menu: open details, rename, refresh cover, move to folder, add tag, export and delete
- Real-time three.js preview: orbit, zoom, pan, auto-rotate, floor, animation playback
- Automatic cover generation with the current view saved on upload
- Tags, keyword search, sorting, tag/folder filtering, box selection and batch operations
- Press and drag a card to drag the source file (multi-select supported) to the desktop or other apps
- Background upload progress bar that never blocks list interactions
- Dark/light theme, skin colors, 7 languages, customizable shortcuts
- Operation logs and device information
- All data stays local; no internet required

## Supported Formats

- Models: BBMODEL (Blockbench `.bbmodel`, textures must be embedded as data URI)
- Models: JSON (Bedrock geometry / Java Block&Item; textures added separately, animations via Bedrock `.animation.json`)
- Models: VOX (MagicaVoxel voxel models using the built-in palette)
- Metadata: SQLite database (`imodel.db`)
- File storage: sharded by SHA256 in `models/XX/` (`XX` = first two hash characters, `00-ff`)
- Library configuration: `library.json`

## System Requirements

- Windows 10+ / macOS 10.15+ / Linux
- Node.js 18+

## Usage

1. First launch: create an empty library or open an existing one
2. Upload models: click the upload button to open the dedicated page (multiple models supported); BBMODEL / VOX can also be dropped onto the home list
3. Add files: drop textures and `.animation.json` onto a model row, then click "Upload All"
4. Organize: create colored folders in the sidebar, drag models onto them or select models and use "Move to Folder"
5. Preview: double-click a card to open details, adjust view, play animations and save a cover
6. Edit: change name, description and tags in the detail page
7. Organize/search: search, format/animation/size/time/tag filters, sort, box select and batch operations
8. Export / drag out: export from the context menu or detail page, or drag source files out from cards

## Development

Requires Node.js 18+, Rust 1.87+, WebView2 (bundled with Windows 10/11) and MSVC build tools.

```bash
npm install
npm run tauri:dev    # start the Tauri dev environment
npm run tauri:build  # build the Windows NSIS installer
cd src-tauri && cargo test --lib   # verify the Rust data layer
```

`npm run dev` starts Vite only (port 1420); file dialogs, drag & drop and local persistence require `npm run tauri:dev`.
