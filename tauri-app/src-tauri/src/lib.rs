//! Mini AI 1C Agent - Tauri Application
//!
//! AI-ассистент для разработки на платформе 1С:Предприятие

mod ai_client;
mod bsl_client;
mod bsl_installer;
mod chat_history;
mod commands;
mod history_manager;
#[cfg(windows)]
mod configurator;
mod crypto;
// Hotkeys removed
// mod hotkeys;
mod llm_profiles;
mod llm;
mod mcp_client;
mod settings;

use commands::*;

use tauri::{Manager, tray::TrayIconBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_mcp_bridge::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(tokio::sync::Mutex::new(crate::bsl_client::BSLClient::new()))
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
            approve_tool,
            reject_tool,
            undo_last_change,
            analyze_bsl,
            format_bsl,
            find_configurator_windows_cmd,
            get_code_from_configurator,
            get_active_fragment_cmd,
            paste_code_to_configurator,
            // Chat history
            get_chat_sessions,
            get_active_chat,
            create_chat,
            switch_chat,
            delete_chat,
            save_chat_message,
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
            // MCP
            get_mcp_tools,
            call_mcp_tool,
            test_mcp_connection,
            get_mcp_server_statuses,
            get_mcp_server_logs,
        ])
        .setup(|app| {
            // Setup Tray Icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Mini AI 1C")
                .build(app)?;

            // Hotkeys removed


            // Start BSL Language Server using managed state
            let app_handle = app.handle().clone();
             
            // Start settings watcher for reactive MCP
            crate::mcp_client::start_settings_watcher();

            tauri::async_runtime::spawn(async move {
                // Wait a bit for app to fully start
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                
                let client_state = app_handle.state::<tokio::sync::Mutex<crate::bsl_client::BSLClient>>();
                let mut client = client_state.lock().await;
                
                if let Err(e) = client.start_server() {
                    eprintln!("Failed to start BSL LS: {}", e);
                } else {
                    println!("BSL LS started");
                    // Try to connect immediately
                    if let Err(e) = client.connect().await {
                         eprintln!("Failed to connect to BSL LS: {}", e);
                    } else {
                         println!("BSL LS connected");
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
