use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRecord {
    pub path: String,
    pub name: String,
    pub created_at: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub library_path: Option<String>,
    pub libraries: Vec<LibraryRecord>,
    /// 插件私有数据（通用 KV，供前端插件持久化，核心不关心内容）
    pub plugin_data: std::collections::BTreeMap<String, serde_json::Value>,
}

pub fn load(path: &Path) -> AppConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn add_or_touch_library(config: &mut AppConfig, path: &str, name: &str) -> LibraryRecord {
    let now = now_iso();
    if let Some(item) = config.libraries.iter_mut().find(|item| item.path == path) {
        item.last_opened_at = now;
        if !name.is_empty() {
            item.name = name.to_string();
        }
        return item.clone();
    }
    let record = LibraryRecord {
        path: path.to_string(),
        name: if name.is_empty() {
            "我的模型库".to_string()
        } else {
            name.to_string()
        },
        created_at: now.clone(),
        last_opened_at: now,
    };
    config.libraries.push(record.clone());
    record
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("tsrbox-bbmodel-config.json")
}
