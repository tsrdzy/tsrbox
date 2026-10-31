//! 插件通用 KV 存储命令
//! 核心只提供“按键存取 JSON”的能力，不包含任何具体业务逻辑；
//! 具体功能由前端插件（src/plugins/<name>）自行实现。
use crate::{config, AppState};
use serde_json::Value;
use tauri::State;

/// 读取插件数据（不存在返回 null）
#[tauri::command]
pub fn plugin_storage_get(state: State<'_, AppState>, key: String) -> Result<Option<Value>, String> {
    let config = state.config.lock().map_err(|_| "读取配置失败".to_string())?;
    Ok(config.plugin_data.get(&key).cloned())
}

/// 写入插件数据并持久化
#[tauri::command]
pub fn plugin_storage_set(
    state: State<'_, AppState>,
    key: String,
    value: Value,
) -> Result<(), String> {
    let config = {
        let mut guard = state.config.lock().map_err(|_| "读取配置失败".to_string())?;
        guard.plugin_data.insert(key, value);
        guard.clone()
    };
    config::save(&state.config_path, &config)
}

/// 删除插件数据并持久化
#[tauri::command]
pub fn plugin_storage_remove(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let config = {
        let mut guard = state.config.lock().map_err(|_| "读取配置失败".to_string())?;
        guard.plugin_data.remove(&key);
        guard.clone()
    };
    config::save(&state.config_path, &config)
}
