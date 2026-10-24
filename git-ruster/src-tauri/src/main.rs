// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod explorer;
mod git;
mod graph;
mod pty;
mod shell;
mod util;
mod watcher;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch (e.g. from the Explorer context menu)
        // forwards its arguments here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(path) = config::path_from_args(&argv) {
                config::open_path(app, &path);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(config::ConfigState::load(app.handle()));
            app.manage(pty::Terminals::default());
            app.manage(watcher::WatcherState::default());
            let args: Vec<String> = std::env::args().collect();
            if let Some(path) = config::path_from_args(&args) {
                config::open_path(app.handle(), &path);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config,
            config::add_folders,
            config::remove_folder,
            config::set_selected,
            config::save_settings,
            config::save_ui,
            config::save_repo_view,
            config::list_folders,
            git::repo_info,
            git::graph,
            git::status,
            git::commit_details,
            git::compare_files,
            git::read_blob,
            git::read_work_file,
            git::write_work_file,
            git::conflict_versions,
            git::read_merge_msg,
            git::last_commit_message,
            git::write_temp_file,
            git::open_in_explorer,
            pty::term_open,
            pty::term_write,
            pty::term_resize,
            pty::term_close,
            pty::term_run,
            watcher::watch_repo,
            explorer::context_menu_status,
            explorer::context_menu_install,
            explorer::context_menu_uninstall,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Git Ruster");
}
