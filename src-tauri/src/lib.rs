// dews — desktop RSS reader. Ported data layer (Fever deliberately absent) + command bridge.
pub mod commands;
pub mod data;
pub mod desktop;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::new())
        .setup(|_app| {
            // The tray exists only on Windows: macOS uses the Dock icon, which
            // is already the platform convention for a hidden window.
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = desktop::setup_tray(_app.handle()) {
                    eprintln!("创建系统托盘失败：{e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::save_settings,
            commands::get_fonts,
            commands::is_portable,
            commands::set_unread_badge,
            commands::search_articles,
            commands::get_article_html,
            commands::fetch_full_text,
            commands::mark_article,
            commands::mark_articles_read,
            commands::sync_all,
            commands::sync_subscriptions,
            commands::sync_feed_articles,
            commands::add_feed,
            commands::check_feed,
            commands::remove_feed,
            commands::create_group,
            commands::rename_group,
            commands::reorder_groups,
            commands::delete_group,
            commands::set_feed_group,
            commands::import_opml,
            commands::export_opml,
            commands::validate_account,
            commands::save_account,
            commands::delete_account,
            commands::open_external,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(desktop::handle_run_event);
}
