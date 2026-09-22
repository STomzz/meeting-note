//! Tauri 命令层：薄封装，真正的逻辑都在 `bnu-core`。

use bnu_core::db;
use bnu_core::notes::{self, NoteMeta, ScanStats, SearchHit};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    conn: Mutex<Connection>,
    vault: Mutex<PathBuf>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn read_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn write_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

#[tauri::command]
fn get_vault(state: State<'_, AppState>) -> String {
    state.vault.lock().unwrap().to_string_lossy().to_string()
}

#[tauri::command]
fn set_vault(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let p = PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    std::fs::create_dir_all(&p).map_err(err)?;
    if !p.is_dir() {
        return Err("不是有效的文件夹".into());
    }
    let conn = state.conn.lock().unwrap();
    write_setting(&conn, "vault", &p.to_string_lossy()).map_err(err)?;
    *state.vault.lock().unwrap() = p.clone();
    Ok(p.to_string_lossy().to_string())
}

#[tauri::command]
fn scan_vault(state: State<'_, AppState>) -> Result<ScanStats, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::scan_vault(&conn, &root).map_err(err)
}

#[tauri::command]
fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteMeta>, String> {
    let conn = state.conn.lock().unwrap();
    notes::list_notes(&conn).map_err(err)
}

#[tauri::command]
fn read_note(id: String, state: State<'_, AppState>) -> Result<String, String> {
    let root = state.vault.lock().unwrap().clone();
    notes::read_note(&root, &id).map_err(err)
}

#[tauri::command]
fn write_note(id: String, content: String, state: State<'_, AppState>) -> Result<NoteMeta, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::write_note(&conn, &root, &id, &content).map_err(err)
}

#[tauri::command]
fn create_note(folder: String, title: String, state: State<'_, AppState>) -> Result<NoteMeta, String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::create_note(&conn, &root, &folder, &title).map_err(err)
}

#[tauri::command]
fn delete_note(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = state.vault.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap();
    notes::delete_note(&conn, &root, &id).map_err(err)
}

#[tauri::command]
fn search_notes(
    query: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchHit>, String> {
    let conn = state.conn.lock().unwrap();
    notes::search(&conn, &query, limit.unwrap_or(30)).map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("index.sqlite"))?;

            let default_vault = app.path().document_dir()?.join("BNU-Notes");
            std::fs::create_dir_all(&default_vault)?;
            let vault = read_setting(&conn, "vault")
                .map(PathBuf::from)
                .unwrap_or(default_vault);
            std::fs::create_dir_all(&vault).ok();

            app.manage(AppState {
                conn: Mutex::new(conn),
                vault: Mutex::new(vault),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_vault,
            set_vault,
            scan_vault,
            list_notes,
            read_note,
            write_note,
            create_note,
            delete_note,
            search_notes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
