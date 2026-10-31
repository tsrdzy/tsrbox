use rusqlite::{Connection, Result as SqlResult};
use std::{fs, path::Path};

pub fn open(library_path: &Path) -> Result<Connection, String> {
    let db_path = library_path.join("imodel.db");
    let _ = fs::remove_file(library_path.join("imodel.db-wal"));
    let _ = fs::remove_file(library_path.join("imodel.db-shm"));

    let conn = Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {e}"))?;
    let _: String = conn
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=OFF;")
        .map_err(|e| e.to_string())?;
    create_tables(&conn).map_err(|e| e.to_string())?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS models (
          id TEXT PRIMARY KEY,
          name TEXT DEFAULT '',
          description TEXT DEFAULT '',
          file_name TEXT DEFAULT '',
          file_type TEXT DEFAULT '',
          file_size INTEGER DEFAULT 0,
          dimensions TEXT,
          upload_time TEXT,
          cover INTEGER DEFAULT 0,
          view_params TEXT,
          tags TEXT DEFAULT '[]',
          files TEXT DEFAULT '[]',
          animation_count INTEGER DEFAULT 0,
          content_key TEXT,
          folder_id TEXT DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_models_tags ON models(tags);

        CREATE TABLE IF NOT EXISTS folders (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          color TEXT DEFAULT '',
          created_at TEXT NOT NULL,
          parent_id TEXT DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS tags (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS operation_logs (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          timestamp TEXT NOT NULL,
          operation_type TEXT NOT NULL,
          module TEXT NOT NULL,
          detail TEXT DEFAULT '',
          status TEXT DEFAULT 'success',
          error_message TEXT DEFAULT '',
          device_info TEXT DEFAULT '{}'
        );

        CREATE INDEX IF NOT EXISTS idx_operation_logs_timestamp ON operation_logs(timestamp);
        CREATE INDEX IF NOT EXISTS idx_operation_logs_type ON operation_logs(operation_type);
        CREATE INDEX IF NOT EXISTS idx_operation_logs_module ON operation_logs(module);
        "#,
    )?;

    // 旧库补列：动画数量
    let has_animation_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('models') WHERE name = 'animation_count'",
        [],
        |row| row.get(0),
    )?;
    if has_animation_count == 0 {
        conn.execute(
            "ALTER TABLE models ADD COLUMN animation_count INTEGER DEFAULT 0",
            [],
        )?;
    }

    // 旧库补列：模型 + 贴图内容指纹，用于精细化查重
    let has_content_key: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('models') WHERE name = 'content_key'",
        [],
        |row| row.get(0),
    )?;
    if has_content_key == 0 {
        conn.execute("ALTER TABLE models ADD COLUMN content_key TEXT", [])?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_models_content_key ON models(content_key)",
        [],
    )?;

    // 旧库补列：所属文件夹
    let has_folder_id: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('models') WHERE name = 'folder_id'",
        [],
        |row| row.get(0),
    )?;
    if has_folder_id == 0 {
        conn.execute(
            "ALTER TABLE models ADD COLUMN folder_id TEXT DEFAULT ''",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_models_folder_id ON models(folder_id)",
        [],
    )?;

    // 旧库补列：父文件夹（多级文件夹）
    let has_parent_id: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('folders') WHERE name = 'parent_id'",
        [],
        |row| row.get(0),
    )?;
    if has_parent_id == 0 {
        conn.execute(
            "ALTER TABLE folders ADD COLUMN parent_id TEXT DEFAULT ''",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_folders_parent_id ON folders(parent_id)",
        [],
    )?;

    // 旧库补列：回收站（软删除时间，空 = 未删除）
    let has_deleted_at: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('models') WHERE name = 'deleted_at'",
        [],
        |row| row.get(0),
    )?;
    if has_deleted_at == 0 {
        conn.execute("ALTER TABLE models ADD COLUMN deleted_at TEXT", [])?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_models_deleted_at ON models(deleted_at)",
        [],
    )?;

    // 旧库补列：同级排序（支持手动拖拽调整顺序）
    let has_sort_order: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('folders') WHERE name = 'sort_order'",
        [],
        |row| row.get(0),
    )?;
    if has_sort_order == 0 {
        conn.execute(
            "ALTER TABLE folders ADD COLUMN sort_order INTEGER DEFAULT 0",
            [],
        )?;
        // 按原有创建顺序回填，迁移后顺序保持不变
        let mut stmt = conn.prepare("SELECT id FROM folders ORDER BY created_at ASC, name ASC")?;
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|row| row.ok())
            .collect();
        drop(stmt);
        for (index, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE folders SET sort_order = ?1 WHERE id = ?2",
                rusqlite::params![index as i64, id],
            )?;
        }
    }

    Ok(())
}
