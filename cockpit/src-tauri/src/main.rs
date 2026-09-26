//! Tauri backend: Rust core as the Tauri backend (Phase 4).
//! Exposes the DB read projection; all state stays in studio.db.

use studio_core::state::StateStore;

#[tauri::command]
fn snapshot(db_path: String) -> Result<String, String> {
    let store = StateStore::open(&db_path).map_err(|e| e.to_string())?;
    let snap = store.snapshot().map_err(|e| e.to_string())?;
    serde_json::to_string(&snap).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![snapshot])
        .run(tauri::generate_context!())
        .expect("tauri cockpit failed");
}
