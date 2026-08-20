use crate::settings;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::connect_async;

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
    pub runtime_info: String,
    pub server_version: String,
    pub server_path: String,
    pub workspace_path: String,
    pub active_port: u16,
    pub connected: bool,
    pub mcp_available: bool,
}

/// Notify BSL Language Server that a document was opened.
///
/// This is the editor's entry point into the LSP session, so it is also the place
/// where a dropped session gets healed — didChange/didClose stay non-blocking on
/// purpose (they fire per keystroke) and rely on the document cache replay instead.
#[tauri::command]
pub async fn bsl_did_open(
    uri: String,
    text: String,
    version: i32,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<(), String> {
    let mut client = state.inner().lock().await;
    crate::bsl_client::ensure_bsl_connected(&mut client).await?;
    client.bsl_did_open(uri, text, version).await
}

/// Notify BSL Language Server that a document changed
#[tauri::command]
pub async fn bsl_did_change(
    uri: String,
    text: String,
    version: i32,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<(), String> {
    let client = state.inner().lock().await;
    client.bsl_did_change(uri, text, version).await
}

/// Notify BSL Language Server that a document was closed
#[tauri::command]
pub async fn bsl_did_close(
    uri: String,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<(), String> {
    let client = state.inner().lock().await;
    client.bsl_did_close(uri).await
}

/// Analyze BSL code
#[tauri::command]
pub async fn analyze_bsl(
    code: String,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<Vec<BSLDiagnostic>, String> {
    let mut client = state.inner().lock().await;
    crate::bsl_client::ensure_bsl_connected(&mut client).await?;
    let diagnostics = client.analyze_code(&code, "frontend-analyze").await?;
    let ui_diagnostics = diagnostics
        .into_iter()
        .map(|d| BSLDiagnostic {
            line: d.range.start.line,
            character: d.range.start.character,
            message: d.message,
            severity: match d.severity {
                Some(1) => "error".to_string(),
                Some(2) => "warning".to_string(),
                Some(3) => "info".to_string(),
                _ => "hint".to_string(),
            },
        })
        .collect();
    Ok(ui_diagnostics)
}

/// Format BSL code
#[tauri::command]
pub async fn format_bsl(
    code: String,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<String, String> {
    let mut client = state.inner().lock().await;
    crate::bsl_client::ensure_bsl_connected(&mut client).await?;
    client.format_code(&code, "frontend-format").await
}

/// Check BSL LS status
#[tauri::command]
pub async fn check_bsl_status_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<BslStatus, String> {
    use crate::bsl_client::BSLClient;
    let settings = settings::load_settings();

    let native_path = settings.bsl_server.executable_path.trim();
    let native_installed = !native_path.is_empty() && std::path::Path::new(native_path).is_file();
    let legacy_installed = BSLClient::check_install(&settings.bsl_server.jar_path);
    let installed = native_installed || legacy_installed;
    let java_info = if native_installed {
        "Bundled runtime".to_string()
    } else {
        BSLClient::check_java(&settings.bsl_server.java_path)
    };
    let runtime_info = if native_installed {
        "Встроенный runtime официального Windows-пакета".to_string()
    } else {
        java_info.clone()
    };
    let server_path = if native_installed {
        native_path.to_string()
    } else {
        settings.bsl_server.jar_path.clone()
    };
    let workspace_path = if settings.bsl_server.workspace_path.trim().is_empty() {
        settings::get_settings_dir()
            .join("bsl-workspace")
            .to_string_lossy()
            .to_string()
    } else {
        settings.bsl_server.workspace_path.clone()
    };

    let (connected, mcp_available, active_port) = {
        let client = state.inner().lock().await;
        (
            client.is_connected(),
            client.is_official_mcp_available(),
            client
                .active_port()
                .unwrap_or(settings.bsl_server.websocket_port),
        )
    };

    Ok(BslStatus {
        installed,
        java_info,
        runtime_info,
        server_version: settings.bsl_server.installed_version,
        server_path,
        workspace_path,
        active_port,
        connected,
        mcp_available,
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
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<(), String> {
    {
        let mut client = state.inner().lock().await;
        client.stop();
    }

    // Wait for the old Java process to fully release the port before checking is_port_listening
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    {
        let mut client = state.inner().lock().await;
        client.start_server()?;
    }

    // Wait for BSL LS to initialize (Spring Boot takes ~4-5 seconds)
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

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

fn extract_server_version_line(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.to_ascii_lowercase().starts_with("version:"))
        .unwrap_or("version unknown")
        .to_string()
}

fn is_expected_mcp_probe_status(status: reqwest::StatusCode) -> bool {
    status.is_success() || status == reqwest::StatusCode::BAD_REQUEST
}

#[tauri::command]
pub async fn diagnose_bsl_ls_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
) -> Result<Vec<BslDiagnosticItem>, String> {
    let settings = settings::load_settings();
    let mut report = Vec::new();
    let (active_port, session_alive) = {
        let client = state.inner().lock().await;
        (
            client
                .active_port()
                .unwrap_or(settings.bsl_server.websocket_port),
            client.is_connected(),
        )
    };

    let native_path = std::path::Path::new(&settings.bsl_server.executable_path);
    if !settings.bsl_server.executable_path.trim().is_empty() && native_path.is_file() {
        let mut version_command = std::process::Command::new(native_path);
        version_command.arg("version");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            version_command.creation_flags(0x08000000);
        }

        match version_command.output() {
            Ok(output) if output.status.success() => {
                let output_text = format!(
                    "{} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                report.push(BslDiagnosticItem {
                    status: "ok".to_string(),
                    title: "BSL Language Server".to_string(),
                    message: format!(
                        "Нативный сервер успешно запущен: {}",
                        extract_server_version_line(&output_text)
                    ),
                    suggestion: None,
                });
            }
            Ok(output) => report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Ошибка запуска BSL Language Server".to_string(),
                message: format!("Команда version завершилась со статусом {}.", output.status),
                suggestion: Some(
                    "Переустановите официальный Windows-пакет кнопкой Download.".to_string(),
                ),
            }),
            Err(error) => report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Ошибка запуска BSL Language Server".to_string(),
                message: error.to_string(),
                suggestion: Some(
                    "Переустановите официальный Windows-пакет кнопкой Download.".to_string(),
                ),
            }),
        }

        report.push(BslDiagnosticItem {
            status: "ok".to_string(),
            title: "Runtime".to_string(),
            message: "Используется встроенный runtime официального Windows-пакета; внешняя Java не требуется.".to_string(),
            suggestion: None,
        });

        // Пробный сокет ниже проверяет только доступность порта. Без этой строки
        // отчёт показывал «всё в порядке», пока сессия приложения была мертва,
        // а диагностика в редакторе молчала.
        report.push(if session_alive {
            BslDiagnosticItem {
                status: "ok".to_string(),
                title: "Сессия LSP приложения".to_string(),
                message: "Приложение подключено к BSL LS, диагностика редактора активна."
                    .to_string(),
                suggestion: None,
            }
        } else {
            BslDiagnosticItem {
                status: "error".to_string(),
                title: "Нет активной сессии LSP".to_string(),
                message:
                    "Порт может отвечать, но у приложения нет живой сессии с BSL LS — \
                     проверка синтаксиса в редакторе работать не будет."
                        .to_string(),
                suggestion: Some(
                    "Нажмите Reconnect либо переоткройте панель кода — сессия поднимется заново."
                        .to_string(),
                ),
            }
        });

        let port = active_port;
        let ws_url = format!("ws://127.0.0.1:{port}/lsp");
        match tokio::time::timeout(Duration::from_secs(3), connect_async(&ws_url)).await {
            Ok(Ok(_)) => report.push(BslDiagnosticItem {
                status: "ok".to_string(),
                title: "LSP WebSocket".to_string(),
                message: format!("Соединение с {ws_url} установлено."),
                suggestion: None,
            }),
            Ok(Err(error)) => report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "LSP WebSocket недоступен".to_string(),
                message: error.to_string(),
                suggestion: Some("Нажмите Reconnect и повторите диагностику.".to_string()),
            }),
            Err(_) => report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Таймаут LSP WebSocket".to_string(),
                message: "Сервер не ответил за 3 секунды.".to_string(),
                suggestion: Some("Нажмите Reconnect и повторите диагностику.".to_string()),
            }),
        }

        let mcp_url = format!("http://127.0.0.1:{port}/mcp");
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        match http_client.get(&mcp_url).send().await {
            Ok(response) if is_expected_mcp_probe_status(response.status()) => {
                report.push(BslDiagnosticItem {
                    status: "ok".to_string(),
                    title: "Official MCP".to_string(),
                    message: format!(
                    "Endpoint {mcp_url} доступен; запрос без MCP-сессии ожидаемо вернул HTTP {}.",
                    response.status()
                ),
                    suggestion: None,
                })
            }
            Ok(response) => report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "Official MCP вернул ошибку".to_string(),
                message: format!("Endpoint {mcp_url} вернул HTTP {}.", response.status()),
                suggestion: Some("Нажмите Reconnect и повторите диагностику.".to_string()),
            }),
            Err(error) => report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "Official MCP недоступен".to_string(),
                message: error.to_string(),
                suggestion: Some(
                    "Убедитесь, что установлен BSL Language Server 1.x и выполнен Reconnect."
                        .to_string(),
                ),
            }),
        }

        return Ok(report);
    }

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
                    message: format!(
                        "Java найдена ({}), но не удалось определить мажорную версию.",
                        version_line
                    ),
                    suggestion: Some(
                        "Убедитесь, что у вас установлена Java 17 или выше.".to_string(),
                    ),
                });
            }
        }
        Err(e) => {
            report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Java не найдена".to_string(),
                message: format!(
                    "Ошибка при поиске Java по пути '{}': {}",
                    settings.bsl_server.java_path, e
                ),
                suggestion: Some(
                    "Установите Java 17+ и укажите корректный путь в настройках.".to_string(),
                ),
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
                    message: format!(
                        "Файл найден, но его размер ({:.2} МБ) слишком мал.",
                        size_mb
                    ),
                    suggestion: Some(
                        "Удалите файл и нажмите 'Download' в настройках BSL Server.".to_string(),
                    ),
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
                            suggestion: Some(
                                "Убедитесь, что Java установлена и путь к ней корректен."
                                    .to_string(),
                            ),
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
            suggestion: Some(
                "Нажмите 'Download' в настройках BSL Server для загрузки.".to_string(),
            ),
        });
    }

    let port = settings.bsl_server.websocket_port;
    let url = format!("http://127.0.0.1:{}", port);

    match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(_) => {
            report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "Сетевой порт".to_string(),
                message: format!(
                    "Порт {} свободен. Это значит, что сервер BSL сейчас НЕ запущен.",
                    port
                ),
                suggestion: Some(
                    "Попробуйте нажать 'Reconnect' или 'Save Settings' для запуска сервера."
                        .to_string(),
                ),
            });
        }
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
                    let code = status.as_u16();
                    // BSL LS is a WebSocket-only server; GET / returns 404 — that's expected.
                    let (item_status, message) = if status.is_success() || code == 404 {
                        ("ok", "Сервер запущен и отвечает.".to_string())
                    } else {
                        (
                            "warn",
                            format!("Сервер ответил с неожиданным статусом: {}.", code),
                        )
                    };
                    report.push(BslDiagnosticItem {
                        status: item_status.to_string(),
                        title: "HTTP ответ".to_string(),
                        message,
                        suggestion: None,
                    });
                }
                Err(e) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка HTTP".to_string(),
                        message: format!("Порт занят, но сервер не отвечает на HTTP запрос: {}", e),
                        suggestion: Some(
                            "Возможно, порт занят другим приложением или сервер завис.".to_string(),
                        ),
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
                }
                Ok(Err(e)) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка WebSocket".to_string(),
                        message: format!("Не удалось установить WebSocket соединение: {}", e),
                        suggestion: Some(
                            "Проверьте настройки брандмауэра или антивируса.".to_string(),
                        ),
                    });
                }
                Err(_) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Таймаут WebSocket".to_string(),
                        message: "Превышено время ожидания WebSocket рукопожатия (3 сек)."
                            .to_string(),
                        suggestion: Some("Попробуйте перезапустить приложение.".to_string()),
                    });
                }
            }
        }
    }

    Ok(report)
}

fn parse_java_major_version(version_output: &str) -> Option<u32> {
    for line in version_output.lines() {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                let ver_str = &line[start + 1..start + 1 + end];
                if ver_str.starts_with("1.") {
                    return ver_str.split('.').nth(1)?.parse().ok();
                }
                return ver_str.split('.').next()?.parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_version_diagnostics_omit_runtime_warnings() {
        let output = "version: 1.0.5\nWARNING: A terminally deprecated method was called";

        assert_eq!(extract_server_version_line(output), "version: 1.0.5");
    }

    #[test]
    fn mcp_probe_accepts_only_success_or_missing_session_response() {
        assert!(is_expected_mcp_probe_status(reqwest::StatusCode::OK));
        assert!(is_expected_mcp_probe_status(
            reqwest::StatusCode::BAD_REQUEST
        ));
        assert!(!is_expected_mcp_probe_status(
            reqwest::StatusCode::NOT_FOUND
        ));
        assert!(!is_expected_mcp_probe_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
    }
}
