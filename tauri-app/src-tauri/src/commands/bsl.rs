use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::settings;
use tokio_tungstenite::connect_async;
use std::time::Duration;

/// BSL analysis result for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSLDiagnostic {
    pub line: u32,
    pub character: u32,
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Serialize)]
pub struct BslStatus {
    pub installed: bool,
    pub java_info: String,
    pub connected: bool,
}

/// Analyze BSL code
#[tauri::command]
pub async fn analyze_bsl(
    code: String,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<Vec<BSLDiagnostic>, String> {
    crate::app_log!("[BSL] Requesting analysis of {} chars", code.len());
    let mut client = state.inner().lock().await;
    
    if !client.is_connected() {
        let _ = client.connect().await;
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let uri = format!("file:///temp_{}.bsl", timestamp);

    let diagnostics = client.analyze_code(&code, &uri).await?;
    
    let result: Vec<BSLDiagnostic> = diagnostics.iter().map(|d| BSLDiagnostic {
        line: d.range.start.line,
        character: d.range.start.character,
        message: d.message.clone(),
        severity: match d.severity {
            Some(1) => "error".to_string(),
            Some(2) => "warning".to_string(),
            Some(3) => "info".to_string(),
            _ => "hint".to_string(),
        },
    }).collect();
    
    Ok(result)
}

/// Format BSL code
#[tauri::command]
pub async fn format_bsl(
    code: String, 
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<String, String> {
    crate::app_log!("[BSL] Requesting format of {} chars", code.len());
    let mut client = state.inner().lock().await;
    
    if !client.is_connected() {
        let _ = client.connect().await;
    }
    
    client.format_code(&code, "file:///temp.bsl").await
}

/// Check BSL LS status
#[tauri::command]
pub async fn check_bsl_status_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<BslStatus, String> {
    use crate::bsl_client::BSLClient;
    let settings = settings::load_settings();
    
    let installed = BSLClient::check_install(&settings.bsl_server.jar_path);
    let java_info = BSLClient::check_java(&settings.bsl_server.java_path);
    
    let connected = if let Ok(client) = state.inner().try_lock() {
        client.is_connected()
    } else {
        false
    };
    
    Ok(BslStatus {
        installed,
        java_info,
        connected,
    })
}

/// Install (download) BSL Language Server
#[tauri::command]
pub async fn install_bsl_ls_cmd(app: tauri::AppHandle) -> Result<String, String> {
    crate::bsl_installer::download_bsl_ls(app).await
}

/// Reconnect BSL Language Server (stop and restart)
#[tauri::command]
pub async fn reconnect_bsl_ls_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<(), String> {
    {
        let mut client = state.inner().lock().await;
        client.stop();
        client.start_server()?;
    }
    
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    
    let mut client = state.inner().lock().await;
    client.connect().await?;
    
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BslDiagnosticItem {
    pub status: String,
    pub title: String,
    pub message: String,
    pub suggestion: Option<String>,
}

#[tauri::command]
pub async fn diagnose_bsl_ls_cmd() -> Vec<BslDiagnosticItem> {
    let settings = settings::load_settings();
    let mut report = Vec::new();

    let mut java_cmd = std::process::Command::new(&settings.bsl_server.java_path);
    java_cmd.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        java_cmd.creation_flags(0x08000000);
    }

    match java_cmd.output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let version_line = stderr.lines().next().unwrap_or("unknown").to_string();
            
            let java_version = parse_java_major_version(&stderr);
            if let Some(ver) = java_version {
                if ver < 17 {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Несовместимая версия Java".to_string(),
                        message: format!("Найдена Java {}, но требуется версия 17 или выше.", ver),
                        suggestion: Some("Установите Java 17+ (например, Eclipse Temurin) или winget install EclipseAdoptium.Temurin.17.JDK".to_string()),
                    });
                } else {
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "Java Runtime".to_string(),
                        message: format!("Найдена совместимая версия: {}", version_line),
                        suggestion: None,
                    });
                }
            } else {
                report.push(BslDiagnosticItem {
                    status: "warn".to_string(),
                    title: "Версия Java".to_string(),
                    message: format!("Java найдена ({}), но не удалось определить мажорную версию.", version_line),
                    suggestion: Some("Убедитесь, что у вас установлена Java 17 или выше.".to_string()),
                });
            }
        }
        Err(e) => {
            report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Java не найдена".to_string(),
                message: format!("Ошибка при поиске Java по пути '{}': {}", settings.bsl_server.java_path, e),
                suggestion: Some("Установите Java 17+ и укажите корректный путь в настройках.".to_string()),
            });
        }
    }

    let jar_path_str = &settings.bsl_server.jar_path;
    let jar_path = std::path::Path::new(jar_path_str);
    if jar_path.exists() {
        if let Ok(meta) = std::fs::metadata(jar_path) {
            let size_mb = meta.len() as f64 / 1024.0 / 1024.0;
            if size_mb < 1.0 {
                report.push(BslDiagnosticItem {
                    status: "error".to_string(),
                    title: "JAR файл поврежден".to_string(),
                    message: format!("Файл найден, но его размер ({:.2} МБ) слишком мал.", size_mb),
                    suggestion: Some("Удалите файл и нажмите 'Download' в настройках BSL Server.".to_string()),
                });
            } else {
                report.push(BslDiagnosticItem {
                    status: "ok".to_string(),
                    title: "BSL Server JAR".to_string(),
                    message: format!("Файл найден и готов к работе ({:.1} МБ).", size_mb),
                    suggestion: None,
                });

                let mut test_cmd = std::process::Command::new(&settings.bsl_server.java_path);
                test_cmd.args(["-jar", jar_path_str, "--help"]);
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    test_cmd.creation_flags(0x08000000);
                }

                match test_cmd.output() {
                    Ok(output) => {
                        if output.status.success() {
                            report.push(BslDiagnosticItem {
                                status: "ok".to_string(),
                                title: "Запуск сервера".to_string(),
                                message: "Тестовый запуск JAR прошел успешно.".to_string(),
                                suggestion: None,
                            });
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let error_msg = if stderr.contains("UnsupportedClassVersionError") {
                                "Несовместимая версия Java при попытке запуска JAR.".to_string()
                            } else {
                                format!("Сервер не запустился (код: {}).", output.status)
                            };
                            
                            report.push(BslDiagnosticItem {
                                status: "error".to_string(),
                                title: "Ошибка запуска JAR".to_string(),
                                message: error_msg,
                                suggestion: Some("Проверьте версию Java (требуется 17+) или целостность JAR-файла.".to_string()),
                            });
                        }
                    }
                    Err(e) => {
                        report.push(BslDiagnosticItem {
                            status: "error".to_string(),
                            title: "Ошибка выполнения".to_string(),
                            message: format!("Не удалось запустить процесс: {}", e),
                            suggestion: Some("Убедитесь, что Java установлена и путь к ней корректен.".to_string()),
                        });
                    }
                }
            }
        }
    } else {
        report.push(BslDiagnosticItem {
            status: "error".to_string(),
            title: "JAR файл не найден".to_string(),
            message: format!("По пути '{}' ничего не найдено.", jar_path_str),
            suggestion: Some("Нажмите 'Download' в настройках BSL Server для загрузки.".to_string()),
        });
    }

    let port = settings.bsl_server.websocket_port;
    let url = format!("http://127.0.0.1:{}", port);
    
    match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(_) => {
            report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "Сетевой порт".to_string(),
                message: format!("Порт {} свободен. Это значит, что сервер BSL сейчас НЕ запущен.", port),
                suggestion: Some("Попробуйте нажать 'Reconnect' или 'Save Settings' для запуска сервера.".to_string()),
            });
        },
        Err(_) => {
            report.push(BslDiagnosticItem {
                status: "ok".to_string(),
                title: "Сетевой порт".to_string(),
                message: format!("Порт {} занят (сервер запущен).", port),
                suggestion: None,
            });

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default();
            
            match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "HTTP ответ".to_string(),
                        message: format!("Сервер ответил по HTTP (статус: {}).", status),
                        suggestion: None,
                    });
                },
                Err(e) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка HTTP".to_string(),
                        message: format!("Порт занят, но сервер не отвечает на HTTP запрос: {}", e),
                        suggestion: Some("Возможно, порт занят другим приложением или сервер завис.".to_string()),
                    });
                }
            }

            let ws_url = format!("ws://127.0.0.1:{}/lsp", port);
            match tokio::time::timeout(Duration::from_secs(3), connect_async(&ws_url)).await {
                Ok(Ok(_)) => {
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "WebSocket соединение".to_string(),
                        message: "WebSocket рукопожатие прошло успешно.".to_string(),
                        suggestion: None,
                    });
                },
                Ok(Err(e)) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка WebSocket".to_string(),
                        message: format!("Не удалось установить WebSocket соединение: {}", e),
                        suggestion: Some("Проверьте настройки брандмауэра или антивируса.".to_string()),
                    });
                },
                Err(_) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Таймаут WebSocket".to_string(),
                        message: "Превышено время ожидания WebSocket рукопожатия (3 сек).".to_string(),
                        suggestion: Some("Попробуйте перезапустить приложение.".to_string()),
                    });
                }
            }
        }
    }

    report
}

fn parse_java_major_version(version_output: &str) -> Option<u32> {
    for line in version_output.lines() {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start+1..].find('"') {
                let ver_str = &line[start+1..start+1+end];
                if ver_str.starts_with("1.") {
                    return ver_str.split('.').nth(1)?.parse().ok();
                }
                return ver_str.split('.').next()?.parse().ok();
            }
        }
    }
    None
}
