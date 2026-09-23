mod acrylic;
mod commands;
mod error;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = commands::init_state(commands::settings_default_path());
            app.manage(state);

            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(hwnd) = window.hwnd() {
                    acrylic::enable(hwnd.0 as isize);
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_projects,
            commands::create_project,
            commands::remove_project,
            commands::preview_apply,
            commands::apply_mod,
            commands::list_layers,
            commands::preview_rollback,
            commands::rollback_top,
            commands::remove_layer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Folder Backup");
}
