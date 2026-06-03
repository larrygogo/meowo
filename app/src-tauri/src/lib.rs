use cc_store::{ProjectOverview, Store, TaskCard};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    store: Mutex<Store>,
}

fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CC_KANBAN_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cc-kanban").join("board.db")
}

#[tauri::command]
fn get_overview(state: State<AppState>) -> Result<Vec<ProjectOverview>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.overview().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_project_tasks(state: State<AppState>, project_id: i64) -> Result<Vec<TaskCard>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.project_tasks(project_id).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = Store::open(db_path()).expect("打开 board.db 失败");
    tauri::Builder::default()
        .manage(AppState { store: Mutex::new(store) })
        .invoke_handler(tauri::generate_handler![get_overview, get_project_tasks])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
