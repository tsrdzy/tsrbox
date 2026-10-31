use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{Connection, Row};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn now_iso() -> String {
    crate::config::now_iso()
}

pub fn is_folder_empty(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true,
    }
}

pub fn is_valid_library(dir: &Path) -> bool {
    dir.exists()
        && dir.join("library.json").is_file()
        && dir.join("imodel.db").is_file()
        && dir.join("models").is_dir()
}

pub fn load_library(dir: &Path) -> Option<Value> {
    let text = fs::read_to_string(dir.join("library.json")).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_library(dir: &Path, library: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(library).map_err(|e| e.to_string())?;
    fs::write(dir.join("library.json"), text).map_err(|e| e.to_string())
}

pub fn init_library_dirs(dir: &Path) -> Result<Value, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !is_folder_empty(dir) {
        return Err("目标文件夹非空，无法创建资源库".to_string());
    }

    for i in 0..256u32 {
        let shard = format!("{:02x}", i);
        fs::create_dir_all(dir.join("models").join(shard)).map_err(|e| e.to_string())?;
    }

    let library = json!({
        "name": "模型资源库",
        "createdAt": now_iso(),
        "version": "2.0.0"
    });
    save_library(dir, &library)?;
    Ok(library)
}

/// 内容指纹：模型文件 + 全部贴图文件（按文件名排序），不含动作文件。
/// 用于精细化查重：只有模型与贴图都相同才视为同一份内容。
pub fn content_key(paths: &[PathBuf]) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"tsrbox-content-key-v1");
    for path in paths {
        let bytes = fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 从已入库模型的 files 记录计算内容指纹（主文件 + texture 角色文件）
pub fn model_content_key(root: &Path, id: &str, files: &Value) -> Option<String> {
    let dir = model_dir(root, id);
    let entries = files.as_array()?;
    let mut main: Option<PathBuf> = None;
    let mut textures: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let role = entry.get("role").and_then(|value| value.as_str()).unwrap_or("");
        let name = entry.get("name").and_then(|value| value.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        match role {
            "main" => main = Some(path),
            "texture" => textures.push(path),
            _ => {}
        }
    }
    let main = main?;
    textures.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut all = Vec::with_capacity(textures.len() + 1);
    all.push(main);
    all.extend(textures);
    content_key(&all).ok()
}

/// 回填旧库缺失的 content_key（只处理空值，一次打开库扫描）
pub fn backfill_content_keys(root: &Path, conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT id, files FROM models WHERE content_key IS NULL OR content_key = ''")
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "[]".to_string()),
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();
    drop(stmt);

    for (id, files_text) in rows {
        let files: Value = serde_json::from_str(&files_text).unwrap_or(json!([]));
        if let Some(key) = model_content_key(root, &id, &files) {
            let _ = conn.execute(
                "UPDATE models SET content_key = ?1 WHERE id = ?2",
                rusqlite::params![key, id],
            );
        }
    }
    Ok(())
}

pub fn get_model_by_content_key(conn: &Connection, key: &str) -> Option<Value> {
    conn.query_row(
        "SELECT * FROM models WHERE content_key = ?1 AND (deleted_at IS NULL OR deleted_at = '') LIMIT 1",
        [key],
        |row| model_from_row(row),
    )
    .ok()
}

// ===== 回收站 =====

pub const RECYCLE_RETENTION_DAYS: i64 = 30;

pub fn list_deleted_models(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM models WHERE deleted_at IS NOT NULL AND deleted_at != '' \
             ORDER BY deleted_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| model_from_row(row))
        .map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    for row in rows {
        models.push(row.map_err(|e| e.to_string())?);
    }
    Ok(models)
}

pub fn soft_delete_model(conn: &Connection, id: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE models SET deleted_at = ?1 WHERE id = ?2 AND (deleted_at IS NULL OR deleted_at = '')",
            rusqlite::params![now_iso(), id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("模型不存在或已在回收站中".to_string());
    }
    Ok(())
}

pub fn restore_model(conn: &Connection, id: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE models SET deleted_at = NULL WHERE id = ?1",
            [id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("模型不存在".to_string());
    }
    Ok(())
}

pub fn delete_model_permanently(root: &Path, conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM models WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    let _ = delete_model_dir(root, id);
    Ok(())
}

pub fn empty_recycle(root: &Path, conn: &Connection) -> Result<usize, String> {
    let mut stmt = conn
        .prepare("SELECT id FROM models WHERE deleted_at IS NOT NULL AND deleted_at != ''")
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();
    drop(stmt);
    for id in &ids {
        conn.execute("DELETE FROM models WHERE id = ?1", [id])
            .map_err(|e| e.to_string())?;
        let _ = delete_model_dir(root, id);
    }
    Ok(ids.len())
}

/// 清理超过保留期的回收站模型（打开资源库时调用）
pub fn purge_expired_deleted(root: &Path, conn: &Connection) -> Result<usize, String> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(RECYCLE_RETENTION_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut stmt = conn
        .prepare(
            "SELECT id FROM models WHERE deleted_at IS NOT NULL AND deleted_at != '' AND deleted_at < ?1",
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map([cutoff.as_str()], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();
    drop(stmt);
    for id in &ids {
        conn.execute("DELETE FROM models WHERE id = ?1", [id])
            .map_err(|e| e.to_string())?;
        let _ = delete_model_dir(root, id);
    }
    Ok(ids.len())
}

pub fn shard(id: &str) -> String {
    id.chars().take(2).collect::<String>().to_lowercase()
}

pub fn model_dir(root: &Path, id: &str) -> PathBuf {
    root.join("models").join(shard(id)).join(id)
}

pub fn file_ext(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// 支持的模型扩展名
pub const MODEL_EXTS: [&str; 3] = ["bbmodel", "json", "vox"];

/// 判断 .json 是否为纯动画文件（仅含 animations / animation_controllers，无模型内容）
fn is_animation_json(path: &Path) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return false,
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    let has_model = obj.contains_key("minecraft:geometry")
        || obj.contains_key("elements")
        || obj.contains_key("bones")
        || obj.contains_key("textures")
        || obj.contains_key("parent")
        || obj.keys().any(|key| key.starts_with("geometry."));
    let has_anim = obj.contains_key("animations") || obj.contains_key("animation_controllers");
    has_anim && !has_model
}

pub fn validate_model(path: &Path) -> Result<(), String> {
    let ext = file_ext(path);
    if !MODEL_EXTS.contains(&ext.as_str()) {
        return Err("仅支持 .bbmodel / .json / .vox 文件".to_string());
    }
    if ext == "json" && is_animation_json(path) {
        return Err("这是动画文件，请在模型上传页作为动作文件添加".to_string());
    }
    Ok(())
}

/// 校验贴图 / 动作关联文件的扩展名
pub fn validate_aux_file(path: &Path, role: &str) -> Result<(), String> {
    let ext = file_ext(path);
    match role {
        "texture" if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") => Ok(()),
        "texture" => Err("贴图文件仅支持 png / jpg / jpeg / webp".to_string()),
        "animation" if ext == "json" => Ok(()),
        "animation" => Err("动作文件仅支持 .json".to_string()),
        _ => Err("未知的关联文件类型".to_string()),
    }
}

/// 读取文件为上传页暂存数据（不复制、不写库）
pub fn staged_file(path: &Path, role: &str) -> Result<Value, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string();
    let bytes = fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    let size = bytes.len() as u64;
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&name)
        .to_string();
    Ok(json!({
        "path": path.to_string_lossy(),
        "name": name,
        "fileName": name,
        "defaultName": stem,
        "fileType": file_ext(path),
        "fileSize": size,
        "role": role,
        "dataBase64": STANDARD.encode(bytes)
    }))
}

/// 把贴图/动作等关联文件复制进模型目录，返回文件记录（同名时自动追加序号）
pub fn add_aux_file(root: &Path, id: &str, source: &Path, role: &str) -> Result<Value, String> {
    validate_aux_file(source, role)?;
    let dir = model_dir(root, id);
    if !dir.is_dir() {
        return Err("模型目录不存在".to_string());
    }
    let ext = file_ext(source);

    let original = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string();
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let mut name = original;
    let mut dest = dir.join(&name);
    let mut index = 1;
    while dest.exists() {
        name = format!("{stem}({index}).{ext}");
        dest = dir.join(&name);
        index += 1;
    }
    fs::copy(source, &dest).map_err(|e| format!("复制文件失败: {e}"))?;
    let size = fs::metadata(&dest).map_err(|e| e.to_string())?.len();
    Ok(json!({ "name": name, "role": role, "size": size }))
}

pub fn remove_aux_file(root: &Path, id: &str, name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "cover.png"
        || name == "thumbnail.png"
    {
        return Err("无效的文件名".to_string());
    }
    let path = model_dir(root, id).join(name);
    if path.is_file() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 拷贝源文件到模型分片目录，返回上传页所需的数据（尚未写入数据库）
pub fn create_model_from_path(root: &Path, source: &Path, hash: &str) -> Result<Value, String> {
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string();
    let default_name = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&file_name)
        .to_string();

    let dir = model_dir(root, hash);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(&file_name);
    fs::copy(source, &dest).map_err(|e| format!("复制文件失败: {e}"))?;
    let size = fs::metadata(&dest).map_err(|e| e.to_string())?.len();
    let bytes = fs::read(&dest).map_err(|e| e.to_string())?;
    let data_base64 = STANDARD.encode(bytes);
    let ext = file_ext(source);

    Ok(json!({
        "duplicate": false,
        "id": hash,
        "fileName": file_name,
        "fileType": ext,
        "fileSize": size,
        "files": [{ "name": file_name, "role": "main", "size": size }],
        "uploadTime": now_iso(),
        "dataBase64": data_base64,
        "auxFiles": [],
        "defaultName": default_name
    }))
}

pub fn pending_file_for(source: &Path, hash: &str) -> Value {
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
        "ext": file_ext(source),
        "hash": hash,
        "contentKey": hash,
        "defaultName": default_name
    })
}

/// 拷贝主文件与关联文件（贴图 / 动作）到模型目录，返回上传页所需的数据
pub fn create_model_with_aux(
    root: &Path,
    source: &Path,
    id: &str,
    aux: &[(PathBuf, String)],
) -> Result<Value, String> {
    let mut record = create_model_from_path(root, source, id)?;
    let mut files: Vec<Value> = record
        .get("files")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for (path, role) in aux {
        let info = add_aux_file(root, id, path, role)?;
        files.push(info.clone());
    }
    record["files"] = Value::Array(files);
    record["contentKey"] = json!(id);
    Ok(record)
}

pub fn parse_json(value: Option<String>, fallback: Value) -> Value {
    value
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(fallback)
}

pub fn model_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let dimensions: Option<String> = row.get("dimensions")?;
    let view_params: Option<String> = row.get("view_params")?;
    let tags: Option<String> = row.get("tags")?;
    let files: Option<String> = row.get("files")?;
    let cover: i64 = row.get("cover")?;
    let animation_count: i64 = row.get::<_, Option<i64>>("animation_count")?.unwrap_or(0);

    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "name": row.get::<_, String>("name")?,
        "description": row.get::<_, String>("description")?,
        "fileName": row.get::<_, String>("file_name")?,
        "fileType": row.get::<_, String>("file_type")?,
        "fileSize": row.get::<_, i64>("file_size")?,
        "dimensions": parse_json(dimensions, Value::Null),
        "uploadTime": row.get::<_, Option<String>>("upload_time")?.unwrap_or_default(),
        "cover": cover != 0,
        "viewParams": parse_json(view_params, Value::Null),
        "tags": parse_json(tags, json!([])),
        "files": parse_json(files, json!([])),
        "animationCount": animation_count,
        "contentKey": row.get::<_, Option<String>>("content_key")?.unwrap_or_default(),
        "folderId": row.get::<_, Option<String>>("folder_id")?.unwrap_or_default(),
        "deletedAt": row.get::<_, Option<String>>("deleted_at")?.unwrap_or_default()
    }))
}

pub fn get_model(conn: &Connection, id: &str) -> Option<Value> {
    conn.query_row("SELECT * FROM models WHERE id = ?1", [id], |row| {
        model_from_row(row)
    })
    .ok()
}

pub fn list_models(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM models WHERE deleted_at IS NULL OR deleted_at = ''")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| model_from_row(row))
        .map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    for row in rows {
        models.push(row.map_err(|e| e.to_string())?);
    }
    Ok(models)
}

pub fn cover_data_url(root: &Path, id: &str) -> Option<String> {
    let bytes = fs::read(model_dir(root, id).join("cover.png")).ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

pub fn save_cover(root: &Path, id: &str, kind: &str, base64: &str) -> Result<String, String> {
    let dir = model_dir(root, id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let clean = base64
        .split_once("base64,")
        .map(|(_, data)| data)
        .unwrap_or(base64);
    let bytes = STANDARD
        .decode(clean)
        .map_err(|e| format!("封面数据无效: {e}"))?;
    let name = format!("{kind}.png");
    fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
    Ok(name)
}

pub fn read_cover(root: &Path, id: &str, kind: &str) -> Option<String> {
    let bytes = fs::read(model_dir(root, id).join(format!("{kind}.png"))).ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

pub fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn value_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(|item| item.as_i64()).unwrap_or(0)
}

pub fn value_json(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .map(|item| {
            if item.is_null() {
                fallback.to_string()
            } else {
                item.to_string()
            }
        })
        .unwrap_or_else(|| fallback.to_string())
}

pub fn value_optional_json(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::Null) | None => None,
        Some(item) => Some(item.to_string()),
    }
}

pub fn value_optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
}

pub fn upsert_model(conn: &Connection, meta: &Value) -> Result<(), String> {
    let tags_text = value_json(meta, "tags", "[]");
    let has_cover = match meta.get("cover") {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    };
    let deleted_at = value_optional_string(meta, "deletedAt");
    conn.execute(
        r#"
        INSERT OR REPLACE INTO models
          (id, name, description, file_name, file_type, file_size, dimensions,
           upload_time, cover, view_params, tags, files, animation_count, content_key,
           folder_id, deleted_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        "#,
        rusqlite::params![
            value_string(meta, "id"),
            value_string(meta, "name"),
            value_string(meta, "description"),
            value_string(meta, "fileName"),
            value_string(meta, "fileType"),
            value_i64(meta, "fileSize"),
            value_optional_json(meta, "dimensions"),
            value_string(meta, "uploadTime"),
            if has_cover { 1 } else { 0 },
            value_optional_json(meta, "viewParams"),
            tags_text,
            value_json(meta, "files", "[]"),
            value_i64(meta, "animationCount"),
            value_string(meta, "contentKey"),
            value_string(meta, "folderId"),
            deleted_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let tags = meta
        .get("tags")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for tag in tags {
        if let Some(name) = tag.as_str() {
            let _ = conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [name]);
        }
    }

    Ok(())
}

pub fn merge_json(base: &mut Value, patch: &Value) {
    if let (Some(base_map), Some(patch_map)) = (base.as_object_mut(), patch.as_object()) {
        for (key, value) in patch_map {
            base_map.insert(key.clone(), value.clone());
        }
    }
}

pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|char| {
            if char == '\\'
                || char == '/'
                || char == ':'
                || char == '*'
                || char == '?'
                || char == '"'
                || char == '<'
                || char == '>'
                || char == '|'
            {
                '_'
            } else {
                char
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "model".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn delete_model_dir(root: &Path, id: &str) -> Result<(), String> {
    let dir = model_dir(root, id);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn insert_tag(conn: &Connection, tag: &str) -> Result<(), String> {
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [tag])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_tags(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM tags ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row.map_err(|e| e.to_string())?);
    }
    Ok(tags)
}

pub fn remove_tag(conn: &Connection, tag: &str) -> Result<(), String> {
    conn.execute("DELETE FROM tags WHERE name = ?1", [tag])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ===== 文件夹 =====

fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "name": row.get::<_, String>("name")?,
        "color": row.get::<_, Option<String>>("color")?.unwrap_or_default(),
        "createdAt": row.get::<_, String>("created_at")?,
        "parentId": row.get::<_, Option<String>>("parent_id")?.unwrap_or_default(),
        "sortOrder": row.get::<_, Option<i64>>("sort_order")?.unwrap_or(0)
    }))
}

pub fn get_folder(conn: &Connection, id: &str) -> Option<Value> {
    conn.query_row(
        "SELECT * FROM folders WHERE id = ?1",
        [id],
        |row| folder_from_row(row),
    )
    .ok()
}

pub fn list_folders(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM folders ORDER BY sort_order ASC, created_at ASC, name ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| folder_from_row(row))
        .map_err(|e| e.to_string())?;
    let mut folders = Vec::new();
    for row in rows {
        folders.push(row.map_err(|e| e.to_string())?);
    }
    Ok(folders)
}

/// 同一父文件夹下不允许重名
fn ensure_unique_folder_name(
    conn: &Connection,
    name: &str,
    exclude_id: &str,
    parent_id: &str,
) -> Result<(), String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM folders WHERE name = ?1 AND id != ?2 AND COALESCE(parent_id, '') = ?3",
            rusqlite::params![name, exclude_id, parent_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Err("同一层级下已存在同名文件夹".to_string());
    }
    Ok(())
}

/// 防止把文件夹移动到自身或自己的子孙节点
fn ensure_folder_move_allowed(conn: &Connection, id: &str, target_parent: &str) -> Result<(), String> {
    if target_parent.is_empty() {
        return Ok(());
    }
    if target_parent == id {
        return Err("不能把文件夹移动到自身".to_string());
    }
    let mut current = target_parent.to_string();
    for _ in 0..256 {
        if current == id {
            return Err("不能把文件夹移动到其子文件夹".to_string());
        }
        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM folders WHERE id = ?1",
                [&current],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        match parent {
            Some(next) if !next.is_empty() => current = next,
            _ => break,
        }
    }
    Ok(())
}

pub fn create_folder(
    conn: &Connection,
    name: &str,
    color: &str,
    parent_id: &str,
) -> Result<Value, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("文件夹名称不能为空".to_string());
    }
    if !parent_id.is_empty() && get_folder(conn, parent_id).is_none() {
        return Err("父文件夹不存在".to_string());
    }
    ensure_unique_folder_name(conn, trimmed, "", parent_id)?;
    let id = format!(
        "f{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let created_at = now_iso();
    // 同级末尾追加
    let sort_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders WHERE COALESCE(parent_id, '') = ?1",
            [parent_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO folders (id, name, color, created_at, parent_id, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, trimmed, color, created_at, parent_id, sort_order],
    )
    .map_err(|e| e.to_string())?;
    Ok(get_folder(conn, &id)
        .unwrap_or_else(|| json!({ "id": id, "name": trimmed, "color": color, "createdAt": created_at, "parentId": parent_id, "sortOrder": sort_order })))
}

pub fn update_folder(
    conn: &Connection,
    id: &str,
    name: Option<&str>,
    color: Option<&str>,
    parent_id: Option<&str>,
) -> Result<Value, String> {
    let mut folder = get_folder(conn, id).ok_or_else(|| "文件夹不存在".to_string())?;
    let current_parent = value_string(&folder, "parentId");

    if let Some(next_parent) = parent_id {
        let target = next_parent.trim();
        if target != current_parent {
            ensure_folder_move_allowed(conn, id, target)?;
            if !target.is_empty() && get_folder(conn, target).is_none() {
                return Err("父文件夹不存在".to_string());
            }
            ensure_unique_folder_name(conn, &value_string(&folder, "name"), id, target)?;
            folder["parentId"] = json!(target);
            let next_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders WHERE COALESCE(parent_id, '') = ?1",
                    [target],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            folder["sortOrder"] = json!(next_order);
        }
    }

    let parent = value_string(&folder, "parentId");
    if let Some(next_name) = name {
        let trimmed = next_name.trim();
        if trimmed.is_empty() {
            return Err("文件夹名称不能为空".to_string());
        }
        ensure_unique_folder_name(conn, trimmed, id, &parent)?;
        folder["name"] = json!(trimmed);
    }
    if let Some(next_color) = color {
        folder["color"] = json!(next_color);
    }
    conn.execute(
        "UPDATE folders SET name = ?1, color = ?2, parent_id = ?3, sort_order = ?4 WHERE id = ?5",
        rusqlite::params![
            value_string(&folder, "name"),
            value_string(&folder, "color"),
            parent,
            value_i64(&folder, "sortOrder"),
            id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(get_folder(conn, id).unwrap_or(folder))
}

/// 移动文件夹到指定父级并插入到 index 位置（同父级即调整顺序）
pub fn move_folder(
    conn: &Connection,
    id: &str,
    parent_id: &str,
    index: i64,
) -> Result<Value, String> {
    let folder = get_folder(conn, id).ok_or_else(|| "文件夹不存在".to_string())?;
    let target_parent = parent_id.trim();
    if !target_parent.is_empty() && get_folder(conn, target_parent).is_none() {
        return Err("父文件夹不存在".to_string());
    }
    ensure_folder_move_allowed(conn, id, target_parent)?;
    ensure_unique_folder_name(conn, &value_string(&folder, "name"), id, target_parent)?;

    // 取目标父级下除自身外的顺序列表
    let mut stmt = conn
        .prepare(
            "SELECT id FROM folders WHERE COALESCE(parent_id, '') = ?1 AND id != ?2 \
             ORDER BY sort_order ASC, created_at ASC, name ASC",
        )
        .map_err(|e| e.to_string())?;
    let mut siblings: Vec<String> = stmt
        .query_map(rusqlite::params![target_parent, id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();
    drop(stmt);

    let insert_at = index.max(0) as usize;
    let insert_at = insert_at.min(siblings.len());
    siblings.insert(insert_at, id.to_string());

    for (position, folder_id) in siblings.iter().enumerate() {
        conn.execute(
            "UPDATE folders SET parent_id = ?1, sort_order = ?2 WHERE id = ?3",
            rusqlite::params![target_parent, position as i64, folder_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(get_folder(conn, id).unwrap_or(folder))
}

/// 删除文件夹：模型与子文件夹都移到其父文件夹（顶层则回到「全部文件」）
pub fn delete_folder(conn: &Connection, id: &str) -> Result<(), String> {
    let folder = get_folder(conn, id).ok_or_else(|| "文件夹不存在".to_string())?;
    let parent = value_string(&folder, "parentId");
    conn.execute(
        "UPDATE folders SET parent_id = ?1 WHERE parent_id = ?2",
        rusqlite::params![parent, id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE models SET folder_id = ?1 WHERE folder_id = ?2",
        rusqlite::params![parent, id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM folders WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 批量把模型移入文件夹；folder_id 为空表示移回「全部文件」
pub fn move_models_to_folder(
    conn: &Connection,
    ids: &[String],
    folder_id: &str,
) -> Result<usize, String> {
    if !folder_id.is_empty() && get_folder(conn, folder_id).is_none() {
        return Err("文件夹不存在".to_string());
    }
    let mut moved = 0;
    for id in ids {
        moved += conn
            .execute(
                "UPDATE models SET folder_id = ?1 WHERE id = ?2",
                rusqlite::params![folder_id, id],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(moved)
}

/// 跨格式查重：同名但不同文件格式（如 bbmodel / json）
pub fn find_model_by_name_other_format(
    conn: &Connection,
    name: &str,
    ext: &str,
) -> Option<Value> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT * FROM models WHERE lower(trim(name)) = lower(trim(?1)) AND lower(file_type) != lower(?2) AND (deleted_at IS NULL OR deleted_at = '') LIMIT 1",
        rusqlite::params![trimmed, ext],
        |row| model_from_row(row),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_roundtrip_and_cover_flag() {
        let dir = std::env::temp_dir().join(format!("tsrbox-store-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        init_library_dirs(&dir).unwrap();
        let conn = crate::db::open(&dir).unwrap();

        let source = dir.join("cube.bbmodel");
        fs::write(&source, "{\"meta\":{}}").unwrap();
        let hash = content_key(&[source.clone()]).unwrap();
        let mut meta = create_model_from_path(&dir, &source, &hash).unwrap();
        meta["name"] = json!("Cube");
        meta["cover"] = json!("cover.png");
        meta["animationCount"] = json!(2);
        meta["tags"] = json!(["a"]);
        meta["uploadTime"] = json!(now_iso());

        upsert_model(&conn, &meta).unwrap();
        let models = list_models(&conn).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["name"], "Cube");
        assert_eq!(models[0]["cover"].as_bool(), Some(true));
        assert_eq!(models[0]["animationCount"].as_i64(), Some(2));
        assert_eq!(models[0]["tags"], json!(["a"]));

        save_cover(&dir, &hash, "cover", "data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert!(cover_data_url(&dir, &hash).is_some());

        drop(conn);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_model_and_aux_files() {
        let dir = std::env::temp_dir().join(format!("tsrbox-aux-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_library_dirs(&dir).unwrap();

        let source = dir.join("entity.json");
        fs::write(
            &source,
            r#"{"minecraft:geometry":[{"description":{},"bones":[]}]}"#,
        )
        .unwrap();
        assert!(validate_model(&source).is_ok());
        let vox = dir.join("model.vox");
        fs::write(&vox, b"VOX ").unwrap();
        assert!(validate_model(&vox).is_ok());

        let animation = dir.join("walk.animation.json");
        fs::write(
            &animation,
            r#"{"format_version":"1.8.0","animations":{"animation.walk":{}}}"#,
        )
        .unwrap();
        assert!(validate_model(&animation).is_err());

        let hash = content_key(&[source.clone()]).unwrap();
        create_model_from_path(&dir, &source, &hash).unwrap();

        let texture = dir.join("skin.png");
        fs::write(&texture, [0u8, 1, 2]).unwrap();
        let first = add_aux_file(&dir, &hash, &texture, "texture").unwrap();
        let second = add_aux_file(&dir, &hash, &texture, "texture").unwrap();
        assert_eq!(first["role"], "texture");
        assert_ne!(first["name"], second["name"]);
        assert!(model_dir(&dir, &hash)
            .join(first["name"].as_str().unwrap())
            .is_file());

        remove_aux_file(&dir, &hash, first["name"].as_str().unwrap()).unwrap();
        assert!(!model_dir(&dir, &hash)
            .join(first["name"].as_str().unwrap())
            .exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn content_key_tracks_model_and_textures() {
        let dir = std::env::temp_dir().join(format!("tsrbox-key-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_library_dirs(&dir).unwrap();

        let model = dir.join("m.json");
        fs::write(&model, r#"{"minecraft:geometry":[{"description":{},"bones":[]}]}"#).unwrap();
        let texture_a = dir.join("a.png");
        fs::write(&texture_a, [1u8, 2, 3]).unwrap();
        let texture_b = dir.join("b.png");
        fs::write(&texture_b, [4u8, 5, 6]).unwrap();

        let key_a = content_key(&[model.clone(), texture_a.clone()]).unwrap();
        assert_eq!(
            key_a,
            content_key(&[model.clone(), texture_a.clone()]).unwrap()
        );
        assert_ne!(
            key_a,
            content_key(&[model.clone(), texture_b.clone()]).unwrap()
        );

        let record = create_model_with_aux(
            &dir,
            &model,
            &key_a,
            &[(texture_a.clone(), "texture".to_string())],
        )
        .unwrap();
        let files = record["files"].clone();
        assert_eq!(model_content_key(&dir, &key_a, &files).unwrap(), key_a);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folders_crud_and_move() {
        let dir = std::env::temp_dir().join(format!("tsrbox-folder-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_library_dirs(&dir).unwrap();
        let conn = crate::db::open(&dir).unwrap();

        let source = dir.join("cube.bbmodel");
        fs::write(&source, "{\"meta\":{}}").unwrap();
        let key = content_key(&[source.clone()]).unwrap();
        let mut meta = create_model_from_path(&dir, &source, &key).unwrap();
        meta["name"] = json!("Cube");
        meta["uploadTime"] = json!(now_iso());
        upsert_model(&conn, &meta).unwrap();

        let folder = create_folder(&conn, "角色", "#409EFF", "").unwrap();
        let folder_id = value_string(&folder, "id");
        assert!(!folder_id.is_empty());
        assert_eq!(list_folders(&conn).unwrap().len(), 1);
        assert!(create_folder(&conn, "角色", "", "").is_err());

        let child = create_folder(&conn, "主角", "", &folder_id).unwrap();
        let child_id = value_string(&child, "id");
        assert_eq!(value_string(&child, "parentId"), folder_id);
        assert!(create_folder(&conn, "主角", "", &folder_id).is_err());
        assert!(
            update_folder(&conn, &folder_id, None, None, Some(&child_id)).is_err(),
            "不能把父文件夹移动到自己的子文件夹"
        );

        assert_eq!(
            move_models_to_folder(&conn, &[key.clone()], &folder_id).unwrap(),
            1
        );
        assert_eq!(
            value_string(&get_model(&conn, &key).unwrap(), "folderId"),
            folder_id
        );

        let renamed = update_folder(
            &conn,
            &folder_id,
            Some("生物"),
            Some("#67C23A"),
            Some(""),
        )
        .unwrap();
        assert_eq!(value_string(&renamed, "name"), "生物");
        assert_eq!(value_string(&renamed, "color"), "#67C23A");

        delete_folder(&conn, &folder_id).unwrap();
        assert_eq!(list_folders(&conn).unwrap().len(), 1);
        assert_eq!(
            value_string(&get_folder(&conn, &child_id).unwrap(), "parentId"),
            ""
        );
        assert_eq!(
            value_string(&get_model(&conn, &key).unwrap(), "folderId"),
            ""
        );

        drop(conn);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folders_reorder_and_nest() {
        let dir = std::env::temp_dir().join(format!("tsrbox-folder-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_library_dirs(&dir).unwrap();
        let conn = crate::db::open(&dir).unwrap();

        let a = value_string(&create_folder(&conn, "A", "", "").unwrap(), "id");
        let b = value_string(&create_folder(&conn, "B", "", "").unwrap(), "id");
        let c = value_string(&create_folder(&conn, "C", "", "").unwrap(), "id");

        let root_names = |conn: &Connection| -> Vec<String> {
            list_folders(conn)
                .unwrap()
                .iter()
                .filter(|folder| value_string(folder, "parentId").is_empty())
                .map(|folder| value_string(folder, "name"))
                .collect()
        };
        assert_eq!(root_names(&conn), vec!["A", "B", "C"]);

        // C 拖到最前
        move_folder(&conn, &c, "", 0).unwrap();
        assert_eq!(root_names(&conn), vec!["C", "A", "B"]);

        // A 拖到最后
        move_folder(&conn, &a, "", 2).unwrap();
        assert_eq!(root_names(&conn), vec!["C", "B", "A"]);

        // B 放进 C 里
        move_folder(&conn, &b, &c, 0).unwrap();
        let moved = get_folder(&conn, &b).unwrap();
        assert_eq!(value_string(&moved, "parentId"), c);
        assert_eq!(value_i64(&moved, "sortOrder"), 0);

        // 不能把 C 拖进自己的孩子 B
        assert!(move_folder(&conn, &c, &b, 0).is_err());

        drop(conn);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recycle_bin_soft_delete_restore_and_purge() {
        let dir = std::env::temp_dir().join(format!("tsrbox-recycle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_library_dirs(&dir).unwrap();
        let conn = crate::db::open(&dir).unwrap();

        let source = dir.join("cube.bbmodel");
        fs::write(&source, "{\"meta\":{}}").unwrap();
        let key = content_key(&[source.clone()]).unwrap();
        let mut meta = create_model_from_path(&dir, &source, &key).unwrap();
        meta["name"] = json!("Cube");
        meta["uploadTime"] = json!(now_iso());
        upsert_model(&conn, &meta).unwrap();

        soft_delete_model(&conn, &key).unwrap();
        assert!(list_models(&conn).unwrap().is_empty());
        assert_eq!(list_deleted_models(&conn).unwrap().len(), 1);
        assert!(!value_string(&get_model(&conn, &key).unwrap(), "deletedAt").is_empty());

        restore_model(&conn, &key).unwrap();
        assert_eq!(list_models(&conn).unwrap().len(), 1);
        assert!(list_deleted_models(&conn).unwrap().is_empty());

        // 超过保留期 → 打开库时永久清理
        soft_delete_model(&conn, &key).unwrap();
        conn.execute(
            "UPDATE models SET deleted_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
            [&key],
        )
        .unwrap();
        assert_eq!(purge_expired_deleted(&dir, &conn).unwrap(), 1);
        assert!(list_deleted_models(&conn).unwrap().is_empty());
        assert!(!model_dir(&dir, &key).exists());

        drop(conn);
        fs::remove_dir_all(&dir).unwrap();
    }
}
