//! Mini AI 1C Agent - Tauri Application
//!
//! AI-ассистент для разработки на платформе 1С:Предприятие

mod ai;
mod bsl_client;
mod bsl_installer;
mod commands;
#[cfg(windows)]
mod configurator;
mod crypto;
#[cfg(windows)]
mod editor_bridge;
#[cfg(windows)]
mod editor_bridge_installer;
mod history_manager;
mod http_client;
mod job_guard;
mod llm;
mod llm_profiles;
mod logger;
mod mcp_client;
#[cfg(windows)]
mod mouse_hook;
#[cfg(windows)]
mod scintilla;
mod semantic_bridge;
mod settings;

use std::sync::Arc;

use commands::*;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_mcp_bridge::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["overlay"])
                .build(),
        )
        .manage(Arc::new(tokio::sync::Mutex::new(
            crate::bsl_client::BSLClient::new(),
        )))
        .manage(crate::commands::ChatState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_profiles,
            save_profile,
            delete_profile,
            set_active_profile,
            stream_chat,
            stop_chat,
            interrupt_chat,
            compact_context,
            approve_tool,
            reject_tool,
            undo_last_change,
            analyze_bsl,
            format_bsl,
            find_configurator_windows_cmd,
            set_configurator_rdp_mode,
            set_configurator_editor_bridge_enabled,
            check_editor_bridge_status,
            restart_editor_bridge_cmd,
            install_editor_bridge_cmd,
            get_code_from_configurator,
            get_active_fragment_cmd,
            get_current_method_text_cmd,
            get_editor_context_cmd,
            get_configurator_apply_support_cmd,
            diagnose_editor_bridge_cmd,
            check_selection_state,
            sync_configurator_caret_to_point_cmd,
            paste_code_to_configurator,
            // Hotkeys
            // Hotkeys removed
            // LLM Utilities
            fetch_models_cmd,
            fetch_models_from_provider,
            fetch_models_for_profile,
            test_llm_connection_cmd,
            // BSL Utilities
            check_bsl_status_cmd,
            install_bsl_ls_cmd,
            reconnect_bsl_ls_cmd,
            diagnose_bsl_ls_cmd,
            check_java_cmd,
            check_node_version_cmd,
            resolve_node_path_cmd,
            check_node_path_cmd,
            complete_onboarding,
            reset_onboarding,
            restart_app_cmd,
            // MCP
            get_mcp_tools,
            list_mcp_tools,
            call_mcp_tool,
            test_mcp_connection,
            get_mcp_server_statuses,
            get_mcp_server_logs,
            save_debug_logs,
            write_frontend_log,
            delete_search_index,
            open_search_index_dir,
            align_with_configurator,
            send_hotkey_cmd,
            get_insertion_context_cmd,
            insert_at_line_cmd,
            append_to_module_cmd,
            // CLI Providers
            cli_auth_start,
            cli_auth_poll,
            cli_save_token,
            cli_logout,
            cli_get_status,
            cli_refresh_usage,
            // Settings export/import
            commands::settings::export_chat,
            commands::settings::export_settings,
            commands::settings::import_settings,
            commands::settings::validate_import_settings_file,
            commands::settings::import_settings_from_file,
            // 1С:Напарник
            clear_naparnik_session,
            // Scintilla diagnostics
            probe_scintilla,
            // Overlay / Quick Actions
            show_overlay,
            overlay_ready,
            get_pending_overlay_state,
            update_overlay_state,
            resize_overlay,
            hide_overlay,
            show_hidden_overlay,
            emit_to_main,
            open_diff_from_overlay,
            focus_main_window_for_overlay_chat,
            set_main_window_always_on_top,
            quick_chat_invoke,
        ])
        .setup(|app| {
            // Setup Tray Icon with context menu
            let quit_item = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&quit_item])?;

            let tray_handle = app.handle().clone();
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Mini AI 1C")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(move |_tray, event| {
                    match event {
                        // Left click or double click — show/focus window
                        // NOTE: do NOT match right-click here — set_focus() would steal focus
                        // from the tray popup menu and cause it to close instantly (Windows bug).
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            ..
                        }
                        | TrayIconEvent::DoubleClick { .. } => {
                            if let Some(window) = tray_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .on_menu_event({
                    let app_handle = app.handle().clone();
                    move |_, event| {
                        if event.id().as_ref() == "quit" {
                            if let Some(tray) = app_handle.tray_by_id("main-tray") {
                                let _ = tray.set_visible(false);
                            }
                            app_handle.exit(0);
                        }
                    }
                })
                .build(app)?;

            // Handle window close: hide tray icon then exit cleanly
            if let Some(main_window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                main_window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { .. } = event {
                        // Hide tray icon before exit to prevent ghost icon in Windows tray
                        if let Some(tray) = app_handle.tray_by_id("main-tray") {
                            let _ = tray.set_visible(false);
                        }
                        app_handle.exit(0);
                    }
                });
            }

            // Migration: com.miniai1c.agent → com.mini-ai-1c
            // The app identifier was changed; migrate old Tauri app data to the new folder.
            if let Some(roaming) = dirs::data_dir() {
                let old_dir = roaming.join("com.miniai1c.agent");
                let new_dir = roaming.join("com.mini-ai-1c");
                if old_dir.exists() && old_dir.is_dir() {
                    crate::app_log!(
                        "[MIGRATE] Migrating app data from {:?} to {:?}",
                        old_dir,
                        new_dir
                    );
                    if let Err(e) = migrate_dir(&old_dir, &new_dir) {
                        crate::app_log!("[MIGRATE] Migration error: {}", e);
                    } else {
                        // Remove old dir only if migration succeeded and it's now empty
                        let _ = std::fs::remove_dir_all(&old_dir);
                        crate::app_log!("[MIGRATE] Migration complete, removed old dir");
                    }
                }
            }

            // Start BSL Language Server using managed state
            let app_handle = app.handle().clone();

            // Clean up old history file if exists (Issue #11)
            let app_dir = app.path().app_data_dir().unwrap_or_default();
            let history_path = app_dir.join("chat_history.json");
            if history_path.exists() {
                let _ = std::fs::remove_file(history_path);
            }
            // Start settings watcher for reactive MCP
            crate::mcp_client::start_settings_watcher(app.handle().clone());

            // Install global mouse hook to detect right-click on 1C Configurator
            #[cfg(windows)]
            crate::mouse_hook::install_mouse_hook(app.handle().clone());

            #[cfg(windows)]
            {
                let current_settings = crate::settings::load_settings();
                crate::configurator::set_rdp_mode(current_settings.configurator.rdp_mode);
                crate::mouse_hook::set_rdp_mode(current_settings.configurator.rdp_mode);
                crate::mouse_hook::set_editor_bridge_enabled(
                    current_settings.configurator.editor_bridge_enabled,
                );
            }

            #[cfg(windows)]
            crate::editor_bridge::start_watchdog();

            tauri::async_runtime::spawn(async move {
                // Wait a bit for app to fully start
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let client_arc =
                    app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
                let client_inner = client_arc.inner().clone();
                crate::mcp_client::McpManager::register_internal_handler(
                    "bsl-ls",
                    Arc::new(crate::bsl_client::BSLMcpHandler::new(client_inner.clone())),
                )
                .await;

                let mut client = client_inner.lock().await;

                if let Err(e) = client.start_server() {
                    crate::app_log!(force: true, "Failed to start BSL LS: {}", e);
                } else {
                    crate::app_log!("BSL LS started");
                    // Try to connect immediately
                    if let Err(e) = client.connect().await {
                        crate::app_log!(force: true, "Failed to connect to BSL LS: {}", e);
                    } else {
                        crate::app_log!("BSL LS connected");
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Recursively copy all files from `src` to `dst`, skipping files that already exist in `dst`.
fn migrate_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            migrate_dir(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
