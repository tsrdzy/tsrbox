use crate::{config, convert, db, device, dragout, store, AppState};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter, Manager, State};

// ===== 通用辅助 =====

fn get_root(state: &AppState) -> Result<PathBuf, String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?;
    config
        .library_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| "未设置资源库".to_string())
}

fn save_config(state: &AppState) -> Result<(), String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .clone();
    config::save(&state.config_path, &config)
}

fn open_db(state: &AppState, path: &Path) -> Result<(), String> {
    let mut guard = state.db.lock().map_err(|_| "数据库锁失败".to_string())?;
    *guard = None;
    let conn = db::open(path)?;
    let _ = store::backfill_content_keys(path, &conn);
    // 回收站超过保留期的模型在打开库时自动永久清理
    let _ = store::purge_expired_deleted(path, &conn);
    *guard = Some(conn);
    Ok(())
}

fn ensure_db(state: &AppState, path: &Path) -> Result<(), String> {
    let mut guard = state.db.lock().map_err(|_| "数据库锁失败".to_string())?;
    if guard.is_none() {
        let conn = db::open(path)?;
        let _ = store::backfill_content_keys(path, &conn);
        let _ = store::purge_expired_deleted(path, &conn);
        *guard = Some(conn);
    }
    Ok(())
}

fn close_db(state: &AppState) {
    if let Ok(mut guard) = state.db.lock() {
        *guard = None;
    }
}

fn with_db<T>(
    state: &AppState,
    run: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state.db.lock().map_err(|_| "数据库锁失败".to_string())?;
    let conn = guard.as_ref().ok_or_else(|| "未打开资源库".to_string())?;
    run(conn)
}

fn log_operation(
    state: &AppState,
    operation_type: &str,
    module: &str,
    detail: Value,
    status: &str,
    error_message: &str,
) {
    let guard = match state.db.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let conn = match guard.as_ref() {
        Some(conn) => conn,
        None => return,
    };
    let detail_text = match &detail {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let device_info = state
        .device_info
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_else(|| json!({}))
        .to_string();
    let _ = conn.execute(
        r#"INSERT INTO operation_logs
           (timestamp, operation_type, module, detail, status, error_message, device_info)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        rusqlite::params![
            config::now_iso(),
            operation_type,
            module,
            detail_text,
            status,
            error_message,
            device_info
        ],
    );
}

/// 解析前端传来的关联文件列表
fn parse_aux(aux: &[Value]) -> Result<Vec<(PathBuf, String)>, String> {
    let mut result = Vec::new();
    for item in aux {
        let path = item
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "关联文件路径缺失".to_string())?;
        let role = item.get("role").and_then(|value| value.as_str()).unwrap_or("");
        let source = PathBuf::from(path);
        store::validate_aux_file(&source, role)?;
        result.push((source, role.to_string()));
    }
    Ok(result)
}

/// 内容指纹：主文件 + 全部贴图（按文件名排序），动作文件不参与
fn content_key_for(source: &Path, aux: &[(PathBuf, String)]) -> Result<String, String> {
    let mut textures: Vec<PathBuf> = aux
        .iter()
        .filter(|(_, role)| role == "texture")
        .map(|(path, _)| path.clone())
        .collect();
    textures.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut paths = Vec::with_capacity(textures.len() + 1);
    paths.push(source.to_path_buf());
    paths.extend(textures);
    store::content_key(&paths)
}

fn pending_with_aux(source: &Path, key: &str, aux: &[(PathBuf, String)]) -> Value {
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    let default_name = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    json!({
        "path": source.to_string_lossy(),
        "name": name,
        "ext": store::file_ext(source),
        "hash": key,
        "contentKey": key,
        "defaultName": default_name,
        "aux": aux.iter().map(|(path, role)| json!({
            "path": path.to_string_lossy(),
            "role": role
        })).collect::<Vec<_>>()
    })
}

fn upload_result(
    state: &AppState,
    source: &Path,
    key: &str,
    aux: &[(PathBuf, String)],
) -> Result<Value, String> {
    store::validate_model(source)?;
    let root = get_root(state)?;
    if let Some(model) = with_db(state, |conn| Ok(store::get_model_by_content_key(conn, key)))? {
        return Ok(json!({
            "duplicate": true,
            "reason": "content",
            "existingModel": model,
            "pendingFile": pending_with_aux(source, key, aux)
        }));
    }

    // 跨格式查重：同名模型（如 bbmodel 与 json 是同一模型的不同格式）
    let default_name = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    let ext = store::file_ext(source);
    if let Some(model) = with_db(state, |conn| {
        Ok(store::find_model_by_name_other_format(
            conn,
            &default_name,
            &ext,
        ))
    })? {
        return Ok(json!({
            "duplicate": true,
            "reason": "format",
            "existingModel": model,
            "pendingFile": pending_with_aux(source, key, aux)
        }));
    }

    let record = match store::create_model_with_aux(&root, source, key, aux) {
        Ok(record) => record,
        Err(error) => {
            let _ = store::delete_model_dir(&root, key);
            return Err(error);
        }
    };
    log_operation(
        state,
        "upload",
        "models",
        json!({
            "fileName": record["fileName"].clone(),
            "fileType": record["fileType"].clone()
        }),
        "success",
        "",
    );
    Ok(record)
}

// ===== 资源库 =====

/// 创建资源库的默认路径：文档目录下的「模型资源库」，已存在且非空时自动追加序号
#[tauri::command]
pub fn library_default_path(app: AppHandle) -> Result<String, String> {
    let base = app.path().document_dir().map_err(|e| e.to_string())?;
    let mut candidate = base.join("模型资源库");
    let mut index = 1;
    while candidate.exists() && !store::is_folder_empty(&candidate) {
        index += 1;
        candidate = base.join(format!("模型资源库{index}"));
    }
    Ok(candidate.to_string_lossy().to_string())
}

#[tauri::command]
pub fn library_status(state: State<'_, AppState>) -> Result<Value, String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .clone();

    if let Some(path_text) = config.library_path.clone() {
        let path = PathBuf::from(&path_text);
        if store::is_valid_library(&path) {
            ensure_db(&state, &path)?;
            if let Some(library) = store::load_library(&path) {
                return Ok(json!({
                    "initialized": true,
                    "libraryPath": path_text,
                    "library": library,
                    "libraries": config.libraries
                }));
            }
        }
    }

    Ok(json!({
        "initialized": false,
        "libraryPath": null,
        "library": null,
        "libraries": config.libraries
    }))
}

#[tauri::command]
pub fn library_list(state: State<'_, AppState>) -> Result<Value, String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?;
    serde_json::to_value(&config.libraries).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_create(
    state: State<'_, AppState>,
    folder_path: String,
    name: String,
) -> Result<Value, String> {
    let folder = PathBuf::from(&folder_path);
    let mut library = store::init_library_dirs(&folder)?;
    let trimmed_name = name.trim().to_string();
    if !trimmed_name.is_empty() {
        library["name"] = json!(trimmed_name);
        store::save_library(&folder, &library)?;
    }

    open_db(&state, &folder)?;
    let path_text = folder.to_string_lossy().to_string();
    let library_name = library
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "读取配置失败".to_string())?;
        config.library_path = Some(path_text.clone());
        config::add_or_touch_library(&mut config, &path_text, &library_name);
    }
    save_config(&state)?;

    log_operation(
        &state,
        "create",
        "library",
        json!({ "folderPath": path_text, "name": library_name }),
        "success",
        "",
    );

    let libraries = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .libraries
        .clone();
    Ok(json!({
        "libraryPath": path_text,
        "library": library,
        "libraries": libraries
    }))
}

#[tauri::command]
pub fn library_open(state: State<'_, AppState>, folder_path: String) -> Result<Value, String> {
    let folder = PathBuf::from(&folder_path);
    if !store::is_valid_library(&folder) {
        return Err("所选文件夹不是有效的资源库，可能已损坏或被删除".to_string());
    }
    open_db(&state, &folder)?;
    let library = store::load_library(&folder).ok_or_else(|| "资源库不存在".to_string())?;
    let path_text = folder.to_string_lossy().to_string();
    let library_name = library
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "读取配置失败".to_string())?;
        config.library_path = Some(path_text.clone());
        config::add_or_touch_library(&mut config, &path_text, &library_name);
    }
    save_config(&state)?;

    log_operation(
        &state,
        "open",
        "library",
        json!({ "folderPath": path_text }),
        "success",
        "",
    );

    let libraries = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .libraries
        .clone();
    Ok(json!({
        "libraryPath": path_text,
        "library": library,
        "libraries": libraries
    }))
}

#[tauri::command]
pub fn library_remove(state: State<'_, AppState>, folder_path: String) -> Result<Value, String> {
    let mut clear_current = false;
    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "读取配置失败".to_string())?;
        config.libraries.retain(|item| item.path != folder_path);
        if config.library_path.as_deref() == Some(folder_path.as_str()) {
            config.library_path = None;
            clear_current = true;
        }
    }
    log_operation(
        &state,
        "delete",
        "library",
        json!({ "folderPath": folder_path }),
        "success",
        "",
    );
    if clear_current {
        close_db(&state);
    }
    save_config(&state)?;
    let libraries = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .libraries
        .clone();
    Ok(json!(libraries))
}

#[tauri::command]
pub fn library_close(state: State<'_, AppState>) -> Result<Value, String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "读取配置失败".to_string())?;
        config.library_path = None;
    }
    save_config(&state)?;
    log_operation(&state, "close", "library", json!({}), "success", "");
    close_db(&state);
    let libraries = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .libraries
        .clone();
    Ok(json!({ "libraryPath": null, "libraries": libraries }))
}

#[tauri::command]
pub fn library_rename(
    state: State<'_, AppState>,
    folder_path: String,
    new_name: String,
) -> Result<Value, String> {
    let folder = PathBuf::from(&folder_path);
    let mut library = store::load_library(&folder).ok_or_else(|| "资源库不存在".to_string())?;
    library["name"] = json!(new_name);
    store::save_library(&folder, &library)?;

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "读取配置失败".to_string())?;
        if let Some(item) = config
            .libraries
            .iter_mut()
            .find(|item| item.path == folder_path)
        {
            item.name = new_name.clone();
        }
    }
    save_config(&state)?;

    log_operation(
        &state,
        "rename",
        "library",
        json!({ "folderPath": folder_path, "newName": new_name }),
        "success",
        "",
    );

    let libraries = state
        .config
        .lock()
        .map_err(|_| "读取配置失败".to_string())?
        .libraries
        .clone();
    Ok(json!({
        "libraryPath": folder_path,
        "library": library,
        "libraries": libraries
    }))
}

#[tauri::command]
pub fn library_update_settings(
    state: State<'_, AppState>,
    settings: Value,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let mut library = store::load_library(&root).ok_or_else(|| "资源库不存在".to_string())?;

    let mut merged = library
        .get("settings")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default();
    if let Value::Object(patch) = settings {
        for (key, value) in patch {
            merged.insert(key, value);
        }
    }
    library["settings"] = Value::Object(merged);
    store::save_library(&root, &library)?;
    Ok(library)
}

// ===== 标签 =====

#[tauri::command]
pub fn tags_list(state: State<'_, AppState>) -> Result<Value, String> {
    with_db(&state, |conn| Ok(json!(store::list_tags(conn)?)))
}

#[tauri::command]
pub fn tags_add(state: State<'_, AppState>, tag: String) -> Result<Value, String> {
    with_db(&state, |conn| {
        store::insert_tag(conn, &tag)?;
        Ok(json!(store::list_tags(conn)?))
    })
}

#[tauri::command]
pub fn tags_remove(state: State<'_, AppState>, tag: String) -> Result<Value, String> {
    with_db(&state, |conn| {
        store::remove_tag(conn, &tag)?;
        Ok(json!(store::list_tags(conn)?))
    })
}

// ===== 文件夹 =====

#[tauri::command]
pub fn folders_list(state: State<'_, AppState>) -> Result<Value, String> {
    with_db(&state, |conn| Ok(json!(store::list_folders(conn)?)))
}

#[tauri::command]
pub fn folders_create(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
    parent_id: Option<String>,
) -> Result<Value, String> {
    let folder = with_db(&state, |conn| {
        store::create_folder(
            conn,
            &name,
            &color.unwrap_or_default(),
            &parent_id.unwrap_or_default(),
        )
    })?;
    log_operation(
        &state,
        "create",
        "folders",
        json!({
            "id": folder["id"].clone(),
            "name": folder["name"].clone(),
            "parentId": folder["parentId"].clone()
        }),
        "success",
        "",
    );
    Ok(folder)
}

#[tauri::command]
pub fn folders_update(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    color: Option<String>,
    parent_id: Option<String>,
) -> Result<Value, String> {
    with_db(&state, |conn| {
        store::update_folder(
            conn,
            &id,
            name.as_deref(),
            color.as_deref(),
            parent_id.as_deref(),
        )
    })
}

#[tauri::command]
pub fn folders_move(
    state: State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
    index: Option<i64>,
) -> Result<Value, String> {
    with_db(&state, |conn| {
        store::move_folder(
            conn,
            &id,
            parent_id.as_deref().unwrap_or(""),
            index.unwrap_or(0),
        )
    })
}

#[tauri::command]
pub fn folders_delete(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    with_db(&state, |conn| store::delete_folder(conn, &id))?;
    log_operation(
        &state,
        "delete",
        "folders",
        json!({ "id": id }),
        "success",
        "",
    );
    Ok(json!(true))
}

#[tauri::command]
pub fn models_move_folder(
    state: State<'_, AppState>,
    ids: Vec<String>,
    folder_id: Option<String>,
) -> Result<Value, String> {
    let target = folder_id.unwrap_or_default();
    let moved = with_db(&state, |conn| {
        store::move_models_to_folder(conn, &ids, &target)
    })?;
    log_operation(
        &state,
        "move",
        "folders",
        json!({ "count": moved, "folderId": target }),
        "success",
        "",
    );
    Ok(json!({ "moved": moved }))
}

// ===== 模型 =====

#[tauri::command]
pub fn models_list(state: State<'_, AppState>) -> Result<Value, String> {
    let root = get_root(&state)?;
    let mut models = with_db(&state, |conn| store::list_models(conn))?;
    for model in &mut models {
        let id = model
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let has_cover = model
            .get("cover")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if has_cover {
            if let Some(cover) = store::cover_data_url(&root, &id) {
                model["coverBase64"] = json!(cover);
            }
        }
    }
    Ok(json!(models))
}

#[tauri::command]
pub fn models_stage_model(path: String) -> Result<Value, String> {
    let source = PathBuf::from(&path);
    store::validate_model(&source)?;
    store::staged_file(&source, "main")
}

#[tauri::command]
pub fn models_stage_aux(path: String, role: String) -> Result<Value, String> {
    let source = PathBuf::from(&path);
    store::validate_aux_file(&source, &role)?;
    store::staged_file(&source, &role)
}

#[tauri::command]
pub fn models_upload_by_path(state: State<'_, AppState>, path: String) -> Result<Value, String> {
    let source = PathBuf::from(&path);
    let key = content_key_for(&source, &[])?;
    upload_result(&state, &source, &key, &[])
}

#[tauri::command]
pub fn models_upload_with_aux(
    state: State<'_, AppState>,
    path: String,
    aux: Vec<Value>,
) -> Result<Value, String> {
    let source = PathBuf::from(&path);
    store::validate_model(&source)?;
    let aux_files = parse_aux(&aux)?;
    let key = content_key_for(&source, &aux_files)?;
    upload_result(&state, &source, &key, &aux_files)
}

#[tauri::command]
pub fn models_overwrite(
    state: State<'_, AppState>,
    existing_model: Value,
    pending_file: Value,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let old_id = existing_model
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if old_id.is_empty() {
        return Err("旧模型信息缺失".to_string());
    }
    let source = pending_file
        .get("path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "待覆盖文件信息缺失".to_string())?;
    store::validate_model(&source)?;
    let aux_files = parse_aux(
        pending_file
            .get("aux")
            .and_then(|value| value.as_array())
            .map(|items| items.as_slice())
            .unwrap_or(&[]),
    )?;
    let key = pending_file
        .get("contentKey")
        .or_else(|| pending_file.get("hash"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .unwrap_or_default();
    let key = if key.is_empty() {
        content_key_for(&source, &aux_files)?
    } else {
        key
    };

    store::delete_model_dir(&root, &old_id)?;
    with_db(&state, |conn| {
        conn.execute("DELETE FROM models WHERE id = ?1", [&old_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    store::create_model_with_aux(&root, &source, &key, &aux_files)
}

fn expand_paths(input_paths: &[String]) -> Vec<PathBuf> {
    // 批量/目录拖拽仅收 BBMODEL / VOX；JSON 模型必须走专属上传页补充贴图与动作
    fn is_batch_model(path: &Path) -> bool {
        path.is_file() && matches!(store::file_ext(path).as_str(), "bbmodel" | "vox")
    }

    fn walk(dir: &Path, output: &mut Vec<PathBuf>) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, output);
            } else if is_batch_model(&path) {
                output.push(path);
            }
        }
    }

    let mut files = Vec::new();
    for input in input_paths {
        let path = PathBuf::from(input);
        if path.is_dir() {
            walk(&path, &mut files);
        } else if is_batch_model(&path) {
            files.push(path);
        }
    }
    files
}

fn batch_upload_impl(app: &AppHandle, paths: Vec<String>) -> Result<Value, String> {
    let state = app.state::<AppState>();
    let root = get_root(&state)?;
    let files = expand_paths(&paths);
    if files.is_empty() {
        return Ok(json!({ "total": 0, "items": [], "invalidFormat": true }));
    }

    let total = files.len();
    let mut items = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();

    for (index, source) in files.iter().enumerate() {
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let default_name = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();

        let _ = app.emit(
            "batch-upload-progress",
            json!({
                "current": index + 1,
                "total": total,
                "fileName": file_name,
                "stage": "copying"
            }),
        );

        let result = (|| -> Result<Value, String> {
            store::validate_model(source)?;
            // 批量仅收 BBMODEL / VOX，内容指纹即主文件内容
            let key = store::content_key(&[source.clone()])?;

            if seen_hashes.contains(&key) {
                return Ok(json!({
                    "skipped": true,
                    "fileName": file_name,
                    "defaultName": default_name,
                    "message": "批次内重复"
                }));
            }

            if let Some(existing) = with_db(&state, |conn| Ok(store::get_model_by_content_key(conn, &key)))? {
                seen_hashes.insert(key.clone());
                return Ok(json!({
                    "duplicate": true,
                    "reason": "content",
                    "existingModel": existing,
                    "pendingFile": store::pending_file_for(source, &key),
                    "fileName": file_name,
                    "defaultName": default_name
                }));
            }

            let ext = store::file_ext(source);
            if let Some(existing) = with_db(&state, |conn| {
                Ok(store::find_model_by_name_other_format(
                    conn,
                    &default_name,
                    &ext,
                ))
            })? {
                seen_hashes.insert(key.clone());
                return Ok(json!({
                    "duplicate": true,
                    "reason": "format",
                    "existingModel": existing,
                    "pendingFile": store::pending_file_for(source, &key),
                    "fileName": file_name,
                    "defaultName": default_name
                }));
            }

            seen_hashes.insert(key.clone());
            store::create_model_from_path(&root, source, &key)
        })();

        match result {
            Ok(item) => items.push(item),
            Err(error) => items.push(json!({
                "error": true,
                "fileName": file_name,
                "defaultName": default_name,
                "message": error
            })),
        }
    }

    let success_count = items
        .iter()
        .filter(|item| {
            !item
                .get("duplicate")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && !item
                    .get("skipped")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                && !item
                    .get("error")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
        })
        .count();
    let failed_count = items
        .iter()
        .filter(|item| {
            item.get("error")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
        .count();
    log_operation(
        &state,
        "batch_upload",
        "models",
        json!({
            "count": total,
            "successCount": success_count,
            "failedCount": failed_count
        }),
        if failed_count == 0 {
            "success"
        } else {
            "partial"
        },
        "",
    );

    Ok(json!({ "total": total, "items": items }))
}

#[tauri::command]
pub async fn models_batch_upload_by_paths(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<Value, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || batch_upload_impl(&handle, paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn models_load_file(state: State<'_, AppState>, model: Value) -> Result<Value, String> {
    let root = get_root(&state)?;
    let id = model
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let file_name = model
        .get("fileName")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let dir = store::model_dir(&root, id);
    let bytes = fs::read(dir.join(file_name)).map_err(|e| format!("读取模型文件失败: {e}"))?;
    let file_type = model
        .get("fileType")
        .and_then(|value| value.as_str())
        .unwrap_or("bbmodel");
    let files = model.get("files").cloned().unwrap_or_else(|| json!([]));

    // 关联文件（贴图 / 动作）一并读取，前端预览时直接使用
    let mut aux_files = Vec::new();
    if let Some(entries) = files.as_array() {
        for entry in entries {
            let name = entry.get("name").and_then(|value| value.as_str()).unwrap_or("");
            let role = entry.get("role").and_then(|value| value.as_str()).unwrap_or("");
            if role == "main" || name.is_empty() {
                continue;
            }
            if let Ok(data) = fs::read(dir.join(name)) {
                aux_files.push(json!({
                    "name": name,
                    "role": role,
                    "size": data.len(),
                    "dataBase64": STANDARD.encode(data)
                }));
            }
        }
    }

    Ok(json!({
        "dataBase64": STANDARD.encode(bytes),
        "fileType": file_type,
        "files": files,
        "auxFiles": aux_files
    }))
}

#[tauri::command]
pub fn models_add_aux_file(
    state: State<'_, AppState>,
    id: String,
    path: String,
    role: String,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let info = store::add_aux_file(&root, &id, &PathBuf::from(&path), &role)?;

    // 已入库的模型直接同步 files 字段；上传页尚未保存的模型由前端随 meta 一起提交
    if let Some(record) = with_db(&state, |conn| Ok(store::get_model(conn, &id)))? {
        let mut files = record
            .get("files")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        files.push(info.clone());
        let files_value = Value::Array(files);
        let key = store::model_content_key(&root, &id, &files_value).unwrap_or_default();
        with_db(&state, |conn| {
            conn.execute(
                "UPDATE models SET files = ?1, content_key = ?2 WHERE id = ?3",
                rusqlite::params![files_value.to_string(), key, id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
    }

    log_operation(
        &state,
        "add_aux_file",
        "models",
        json!({ "id": id, "role": role, "name": info.get("name").cloned().unwrap_or_default() }),
        "success",
        "",
    );
    Ok(info)
}

#[tauri::command]
pub fn models_remove_aux_file(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    store::remove_aux_file(&root, &id, &name)?;

    if let Some(record) = with_db(&state, |conn| Ok(store::get_model(conn, &id)))? {
        let mut files = record
            .get("files")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        files.retain(|entry| entry.get("name").and_then(|value| value.as_str()) != Some(name.as_str()));
        let files_value = Value::Array(files);
        let key = store::model_content_key(&root, &id, &files_value).unwrap_or_default();
        with_db(&state, |conn| {
            conn.execute(
                "UPDATE models SET files = ?1, content_key = ?2 WHERE id = ?3",
                rusqlite::params![files_value.to_string(), key, id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
    }
    Ok(json!(true))
}

#[tauri::command]
pub fn models_save_image(
    state: State<'_, AppState>,
    id: String,
    kind: String,
    base64: String,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let name = store::save_cover(&root, &id, &kind, &base64)?;
    if kind == "cover" {
        let _ = with_db(&state, |conn| {
            conn.execute("UPDATE models SET cover = 1 WHERE id = ?1", [&id])
                .map_err(|e| e.to_string())?;
            Ok(())
        });
    }
    Ok(json!(name))
}

#[tauri::command]
pub fn models_read_image(
    state: State<'_, AppState>,
    id: String,
    has_cover: bool,
    has_thumb: bool,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let kind = if has_cover {
        "cover"
    } else if has_thumb {
        "thumbnail"
    } else {
        return Ok(Value::Null);
    };
    Ok(store::read_cover(&root, &id, kind)
        .map(Value::String)
        .unwrap_or(Value::Null))
}

#[tauri::command]
pub fn models_save(state: State<'_, AppState>, mut meta: Value) -> Result<Value, String> {
    let root = get_root(&state)?;
    let id = meta
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return Err("模型信息缺失".to_string());
    }
    let files = meta.get("files").cloned().unwrap_or_else(|| json!([]));
    if let Some(key) = store::model_content_key(&root, &id, &files) {
        meta["contentKey"] = json!(key);
    }
    with_db(&state, |conn| {
        store::upsert_model(conn, &meta)?;
        Ok(meta.clone())
    })
}

#[tauri::command]
pub fn models_update(
    state: State<'_, AppState>,
    id: String,
    patch: Value,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    with_db(&state, |conn| {
        let mut current = store::get_model(conn, &id).ok_or_else(|| "模型不存在".to_string())?;
        store::merge_json(&mut current, &patch);
        let files = current.get("files").cloned().unwrap_or_else(|| json!([]));
        if let Some(key) = store::model_content_key(&root, &id, &files) {
            current["contentKey"] = json!(key);
        }
        store::upsert_model(conn, &current)?;
        Ok(current)
    })
}

#[tauri::command]
pub fn models_delete(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    // 软删除：移入回收站，保留 RECYCLE_RETENTION_DAYS 天后自动清理
    with_db(&state, |conn| store::soft_delete_model(conn, &id))?;
    log_operation(
        &state,
        "delete",
        "models",
        json!({ "id": id, "fileName": "", "recycle": true }),
        "success",
        "",
    );
    Ok(json!(true))
}

#[tauri::command]
pub fn models_list_deleted(state: State<'_, AppState>) -> Result<Value, String> {
    let root = get_root(&state)?;
    let mut models = with_db(&state, |conn| store::list_deleted_models(conn))?;
    for model in &mut models {
        let id = model
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let has_cover = model
            .get("cover")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if has_cover {
            if let Some(cover) = store::cover_data_url(&root, &id) {
                model["coverBase64"] = json!(cover);
            }
        }
    }
    Ok(json!({
        "models": models,
        "retentionDays": store::RECYCLE_RETENTION_DAYS
    }))
}

#[tauri::command]
pub fn models_restore(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    with_db(&state, |conn| store::restore_model(conn, &id))?;
    log_operation(
        &state,
        "restore",
        "models",
        json!({ "id": id }),
        "success",
        "",
    );
    Ok(json!(true))
}

#[tauri::command]
pub fn models_delete_forever(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let root = get_root(&state)?;
    with_db(&state, |conn| {
        store::delete_model_permanently(&root, conn, &id)
    })?;
    log_operation(
        &state,
        "delete",
        "models",
        json!({ "id": id, "permanent": true }),
        "success",
        "",
    );
    Ok(json!(true))
}

#[tauri::command]
pub fn models_empty_recycle(state: State<'_, AppState>) -> Result<Value, String> {
    let root = get_root(&state)?;
    let removed = with_db(&state, |conn| store::empty_recycle(&root, conn))?;
    log_operation(
        &state,
        "delete",
        "models",
        json!({ "recycle": "empty", "count": removed }),
        "success",
        "",
    );
    Ok(json!({ "removed": removed }))
}

fn model_display_name(model: &Value) -> String {
    model
        .get("name")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| model.get("fileName").and_then(|value| value.as_str()))
        .unwrap_or("model")
        .to_string()
}

/// 拆分模型记录中的主文件 / 贴图 / 动作文件
fn collect_model_file_roles(model: &Value) -> (String, Vec<String>, Vec<String>) {
    let mut main = String::new();
    let mut textures = Vec::new();
    let mut animations = Vec::new();
    if let Some(files) = model.get("files").and_then(|value| value.as_array()) {
        for entry in files {
            let name = entry.get("name").and_then(|value| value.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            match entry
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("")
            {
                "main" => {
                    if main.is_empty() {
                        main = name.to_string();
                    }
                }
                "texture" => textures.push(name.to_string()),
                "animation" => animations.push(name.to_string()),
                _ => {}
            }
        }
    }
    if main.is_empty() {
        main = model
            .get("fileName")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
    }
    (main, textures, animations)
}

/// 前端只传了精简信息时，用数据库记录补全（含 files）
fn resolve_model(state: &AppState, model: &Value) -> Value {
    let id = model
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if id.is_empty() {
        return model.clone();
    }
    let record = with_db(state, |conn| Ok(store::get_model(conn, id)))
        .ok()
        .flatten();
    match record {
        Some(mut record) => {
            if let Some(name) = model
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
            {
                record["name"] = json!(name);
            }
            if let Some(file_name) = model
                .get("fileName")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
            {
                record["fileName"] = json!(file_name);
            }
            record
        }
        None => model.clone(),
    }
}

fn format_label(format: convert::Format) -> &'static str {
    match format {
        convert::Format::Original => "原始",
        convert::Format::Bbmodel => "BBMODEL",
        convert::Format::Bedrock => "基岩版 JSON",
        convert::Format::Java => "Java 版 JSON",
        convert::Format::Generic => "通用 JSON",
    }
}

/// 复制模型目录下的全部文件（原始格式导出）
fn export_model_files(
    root: &Path,
    model: &Value,
    target_root: &Path,
) -> Result<Value, String> {
    let id = model
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let model_name = model_display_name(model);

    let source_dir = store::model_dir(root, &id);
    if !source_dir.is_dir() {
        return Err("模型文件夹不存在".to_string());
    }

    let safe_name = store::sanitize_file_name(&model_name);
    let target_dir = target_root.join(safe_name);
    fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let entries = fs::read_dir(&source_dir).map_err(|e| e.to_string())?;
    let mut exported = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        if file_name == "cover.png" || file_name == "thumbnail.png" {
            continue;
        }
        fs::copy(&path, target_dir.join(&file_name)).map_err(|e| e.to_string())?;
        exported.push(file_name);
    }

    Ok(json!({
        "targetDir": target_dir.to_string_lossy(),
        "fileCount": exported.len(),
        "files": exported
    }))
}

fn export_model_dir(
    root: &Path,
    model: &Value,
    target_root: &Path,
    format: convert::Format,
) -> Result<Value, String> {
    if format.is_original() {
        return export_model_files(root, model, target_root);
    }

    let id = model
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let model_name = model_display_name(model);
    let source_dir = store::model_dir(root, id);
    let (main, textures, animations) = collect_model_file_roles(model);
    if main.is_empty() || !source_dir.is_dir() {
        return Err("模型主文件缺失".to_string());
    }
    // 目标格式与模型自身格式一致时直接复制原文件（避免无损重排）
    if convert::model_format(&source_dir, &main) == Some(format) {
        return export_model_files(root, model, target_root);
    }

    let result = convert::convert_model(
        &source_dir,
        &main,
        &textures,
        &animations,
        format,
        &model_name,
    );

    match result {
        Ok(converted) => {
            let model_name = model_display_name(model);
            let target_dir = target_root.join(store::sanitize_file_name(&model_name));
            fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
            let mut files = Vec::new();
            for (name, bytes) in &converted.files {
                fs::write(target_dir.join(name), bytes).map_err(|e| e.to_string())?;
                files.push(name.clone());
            }
            Ok(json!({
                "targetDir": target_dir.to_string_lossy(),
                "fileCount": files.len(),
                "files": files,
                "converted": true,
                "format": format.name(),
                "warnings": converted.warnings
            }))
        }
        Err(error) => {
            let mut value = export_model_files(root, model, target_root)?;
            if let Some(object) = value.as_object_mut() {
                object.insert("converted".to_string(), json!(false));
                object.insert("format".to_string(), json!(format.name()));
                object.insert(
                    "warnings".to_string(),
                    json!([format!(
                        "无法转换为{}格式：{}，已导出原始文件",
                        format_label(format),
                        error
                    )]),
                );
            }
            Ok(value)
        }
    }
}

#[tauri::command]
pub fn models_export(
    state: State<'_, AppState>,
    model: Value,
    target_root: String,
    format: Option<String>,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let model = resolve_model(&state, &model);
    export_model_dir(
        &root,
        &model,
        &PathBuf::from(&target_root),
        convert::Format::parse(format.as_deref().unwrap_or("original")),
    )
}

#[tauri::command]
pub fn models_export_batch(
    state: State<'_, AppState>,
    models: Vec<Value>,
    target_root: String,
    format: Option<String>,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let target = PathBuf::from(&target_root);
    let format = convert::Format::parse(format.as_deref().unwrap_or("original"));
    let mut count = 0;
    let mut failed = Vec::new();
    let mut warnings = Vec::new();
    for model in &models {
        let model = resolve_model(&state, model);
        match export_model_dir(&root, &model, &target, format) {
            Ok(result) => {
                count += 1;
                if let Some(list) = result.get("warnings").and_then(|value| value.as_array()) {
                    for item in list {
                        if let Some(text) = item.as_str() {
                            warnings.push(json!({
                                "name": model_display_name(&model),
                                "message": text
                            }));
                        }
                    }
                }
            }
            Err(error) => failed.push(json!({
                "name": model_display_name(&model),
                "message": error
            })),
        }
    }
    Ok(json!({
        "count": count,
        "total": models.len(),
        "targetDir": target_root,
        "format": format.name(),
        "failed": failed,
        "warnings": warnings
    }))
}

// ===== 操作日志 =====

fn filter_logs(options: &Value) -> (String, Vec<SqlValue>) {
    let mut clause = String::new();
    let mut params: Vec<SqlValue> = Vec::new();

    let text_filters = [
        ("type", "operation_type"),
        ("status", "status"),
        ("module", "module"),
    ];
    for (key, column) in text_filters {
        if let Some(value) = options
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            clause.push_str(&format!(" AND {column} = ?"));
            params.push(SqlValue::Text(value.to_string()));
        }
    }
    for (key, column) in [("startDate", "timestamp"), ("endDate", "timestamp")] {
        if let Some(value) = options
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            clause.push_str(&format!(
                " AND {column} {} ?",
                if key == "startDate" { ">=" } else { "<=" }
            ));
            params.push(SqlValue::Text(value.to_string()));
        }
    }
    (clause, params)
}

fn log_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let detail: Option<String> = row.get("detail")?;
    let device_info: Option<String> = row.get("device_info")?;
    Ok(json!({
        "id": row.get::<_, i64>("id")?,
        "timestamp": row.get::<_, String>("timestamp")?,
        "operation_type": row.get::<_, String>("operation_type")?,
        "module": row.get::<_, String>("module")?,
        "detail": store::parse_json(detail, Value::Null),
        "status": row.get::<_, String>("status")?,
        "error_message": row.get::<_, String>("error_message")?,
        "device_info": store::parse_json(device_info, json!({}))
    }))
}

fn query_logs(conn: &Connection, options: &Value, paginated: bool) -> Result<(Value, i64), String> {
    let (clause, params) = filter_logs(options);
    let limit = options
        .get("limit")
        .and_then(|value| value.as_i64())
        .unwrap_or(100);
    let offset = options
        .get("offset")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);

    let mut query =
        format!("SELECT * FROM operation_logs WHERE 1=1{clause} ORDER BY timestamp DESC");
    let mut query_params = params.clone();
    if paginated {
        query.push_str(" LIMIT ? OFFSET ?");
        query_params.push(SqlValue::Integer(limit));
        query_params.push(SqlValue::Integer(offset));
    }

    let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(query_params), |row| {
            log_from_row(row)
        })
        .map_err(|e| e.to_string())?;
    let mut logs = Vec::new();
    for row in rows {
        logs.push(row.map_err(|e| e.to_string())?);
    }

    let count_sql = format!("SELECT COUNT(*) FROM operation_logs WHERE 1=1{clause}");
    let count: i64 = conn
        .query_row(&count_sql, rusqlite::params_from_iter(params), |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;

    Ok((json!(logs), count))
}

#[tauri::command]
pub fn logs_query(state: State<'_, AppState>, options: Value) -> Result<Value, String> {
    with_db(&state, |conn| {
        let (logs, count) = query_logs(conn, &options, true)?;
        Ok(json!({ "logs": logs, "count": count }))
    })
}

#[tauri::command]
pub fn logs_export(state: State<'_, AppState>, options: Value) -> Result<Value, String> {
    with_db(&state, |conn| {
        let (logs, _count) = query_logs(conn, &options, false)?;
        Ok(logs)
    })
}

#[tauri::command]
pub fn logs_clear(
    state: State<'_, AppState>,
    before_date: Option<String>,
) -> Result<Value, String> {
    with_db(&state, |conn| {
        let changes = match before_date {
            Some(date) => conn.execute("DELETE FROM operation_logs WHERE timestamp < ?1", [date]),
            None => conn.execute("DELETE FROM operation_logs", []),
        }
        .map_err(|e| e.to_string())?;
        Ok(json!(changes))
    })
}

#[tauri::command]
pub fn logs_write_file(path: String, content: String) -> Result<Value, String> {
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(json!(path))
}

#[tauri::command]
pub fn logs_get_device_info(state: State<'_, AppState>) -> Result<Value, String> {
    let mut guard = state
        .device_info
        .lock()
        .map_err(|_| "设备信息锁失败".to_string())?;
    if guard.is_none() {
        *guard = Some(device::collect());
    }
    Ok(guard.clone().unwrap_or_else(|| json!({})))
}

// ===== 拖出文件到桌面 =====

/// 拖出格式默认值来自资源库设置（library.json -> settings.dragFormat）
fn drag_format_setting(root: &Path) -> convert::Format {
    let Some(library) = store::load_library(root) else {
        return convert::Format::Original;
    };
    let text = library
        .get("settings")
        .and_then(|value| value.get("dragFormat"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    convert::Format::parse(text)
}

/// 原始格式拖出：主文件 + 关联的贴图与动作文件
fn drag_original_paths(root: &Path, state: &AppState, items: &[Value]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let file_name = item
            .get("fileName")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if id.len() < 2
            || file_name.is_empty()
            || id.contains('/')
            || id.contains('\\')
            || file_name.contains('/')
            || file_name.contains('\\')
        {
            continue;
        }
        let path = store::model_dir(root, id).join(file_name);
        if path.is_file() {
            paths.push(path.clone());
        }
        // JSON / VOX 模型拖出时，把关联的贴图与动作文件一并带上
        if let Ok(Some(record)) = with_db(state, |conn| Ok(store::get_model(conn, id))) {
            if let Some(files) = record.get("files").and_then(|value| value.as_array()) {
                for entry in files {
                    let role = entry.get("role").and_then(|value| value.as_str()).unwrap_or("");
                    let name = entry.get("name").and_then(|value| value.as_str()).unwrap_or("");
                    if role == "main" || name.is_empty() {
                        continue;
                    }
                    let aux = store::model_dir(root, id).join(name);
                    if aux.is_file() && !paths.contains(&aux) {
                        paths.push(aux);
                    }
                }
            }
        }
    }
    paths
}

fn unique_stem(dir: &Path, base: &str) -> String {
    let mut stem = base.to_string();
    let mut index = 2;
    loop {
        let taken = fs::read_dir(dir)
            .ok()
            .map(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(stem.as_str())
                })
            })
            .unwrap_or(false);
        if !taken {
            return stem;
        }
        stem = format!("{base} ({index})");
        index += 1;
    }
}

/// 按指定格式转换后拖出（转换结果写入系统临时目录）
fn drag_converted_paths(
    root: &Path,
    state: &AppState,
    items: &[Value],
    format: convert::Format,
) -> Vec<PathBuf> {
    let cache = std::env::temp_dir().join("tsrbox-drag-cache");
    let _ = fs::remove_dir_all(&cache);
    if fs::create_dir_all(&cache).is_err() {
        return drag_original_paths(root, state, items);
    }

    let mut paths = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if id.len() < 2 || id.contains('/') || id.contains('\\') {
            continue;
        }
        let record = with_db(state, |conn| Ok(store::get_model(conn, id)))
            .ok()
            .flatten();
        let Some(record) = record else {
            continue;
        };
        let source_dir = store::model_dir(root, id);
        let (main, textures, animations) = collect_model_file_roles(&record);
        if main.is_empty() || !source_dir.is_dir() {
            continue;
        }
        let stem = unique_stem(&cache, &store::sanitize_file_name(&model_display_name(&record)));
        if convert::model_format(&source_dir, &main) == Some(format) {
            // 目标格式与源格式一致，直接拖出原始文件
            copy_model_files(
                &source_dir, &cache, &stem, &main, &textures, &animations, &mut paths,
            );
            continue;
        }
        match convert::convert_model(&source_dir, &main, &textures, &animations, format, &stem) {
            Ok(converted) => {
                for (file_name, bytes) in converted.files {
                    let path = cache.join(file_name);
                    if fs::write(&path, bytes).is_ok() {
                        paths.push(path);
                    }
                }
            }
            Err(_) => {
                // 转换失败时回退为原始文件
                copy_model_files(
                    &source_dir, &cache, &stem, &main, &textures, &animations, &mut paths,
                );
            }
        }
    }
    paths
}

fn copy_model_files(
    source_dir: &Path,
    cache: &Path,
    stem: &str,
    main: &str,
    textures: &[String],
    animations: &[String],
    paths: &mut Vec<PathBuf>,
) {
    let names = std::iter::once(main.to_string())
        .chain(textures.iter().cloned())
        .chain(animations.iter().cloned());
    for file_name in names {
        if file_name.is_empty() {
            continue;
        }
        let dest = if file_name.starts_with(stem) {
            cache.join(&file_name)
        } else {
            cache.join(format!("{stem}_{file_name}"))
        };
        if fs::copy(source_dir.join(&file_name), &dest).is_ok() {
            paths.push(dest);
        }
    }
}

#[tauri::command]
pub fn resources_start_drag(
    state: State<'_, AppState>,
    items: Vec<Value>,
    format: Option<String>,
) -> Result<Value, String> {
    let root = get_root(&state)?;
    let target = match format.as_deref() {
        Some(text) if !text.trim().is_empty() => convert::Format::parse(text),
        _ => drag_format_setting(&root),
    };
    let paths = if target.is_original() {
        drag_original_paths(&root, &state, &items)
    } else {
        drag_converted_paths(&root, &state, &items, target)
    };
    if paths.is_empty() {
        return Ok(json!(false));
    }
    dragout::start_drag(paths)?;
    Ok(json!(true))
}
