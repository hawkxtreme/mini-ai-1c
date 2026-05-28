//! EditorBridge installer
//! Downloads EditorBridge.exe from GitHub releases (hawkxtreme/1ConfBridge)

use serde::{Deserialize, Serialize};

use futures::StreamExt;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use crate::settings::{get_settings_dir, load_settings, save_settings};

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Serialize, Clone)]
struct DownloadProgress {
    progress: u64,
    total: u64,
    percent: u64,
}

/// Download EditorBridge.exe from GitHub releases.
/// Returns the absolute path to the downloaded exe.
pub async fn download_editor_bridge(app: AppHandle) -> Result<String, String> {
    crate::app_log!("[EditorBridge Installer] Starting download...");

    let _ = app.emit(
        "editor-bridge-download-progress",
        DownloadProgress {
            progress: 0,
            total: 0,
            percent: 0,
        },
    );

    let client = crate::http_client::http_client_builder()?
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    crate::app_log!("[EditorBridge Installer] Fetching latest release info...");
    let api_response = client
        .get("https://api.github.com/repos/hawkxtreme/mini-ai-1c/releases/latest")
        .header("User-Agent", "mini-ai-1c")
        .send()
        .await
        .map_err(|e| {
            format!(
                "Нет доступа к api.github.com: {}. \
                Проверьте подключение к интернету. \
                Если GitHub заблокирован файрволом — скачайте EditorBridge.exe вручную с \
                https://github.com/hawkxtreme/mini-ai-1c/releases/latest",
                e
            )
        })?;

    if !api_response.status().is_success() {
        let status = api_response.status();
        let body = api_response.text().await.unwrap_or_default();
        let msg = if status.as_u16() == 404 || body.contains("Not Found") {
            "Релиз EditorBridge ещё не опубликован. \
            Скачайте вручную после выхода релиза: \
            https://github.com/hawkxtreme/mini-ai-1c/releases/latest"
                .to_string()
        } else if status.as_u16() == 403 || body.contains("rate limit") {
            format!(
                "GitHub API rate limit ({}). Попробуйте позже или скачайте вручную: \
                https://github.com/hawkxtreme/mini-ai-1c/releases/latest",
                status
            )
        } else {
            format!(
                "GitHub API вернул ошибку {}. \
                Скачайте вручную: https://github.com/hawkxtreme/mini-ai-1c/releases/latest",
                status
            )
        };
        return Err(msg);
    }

    let release: GitHubRelease = api_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    crate::app_log!(
        "[EditorBridge Installer] Found release: {}",
        release.tag_name
    );

    let asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("EditorBridge.exe"))
        .ok_or_else(|| {
            format!(
                "EditorBridge.exe не найден в релизе {}. \
                Скачайте вручную: https://github.com/hawkxtreme/mini-ai-1c/releases/latest",
                release.tag_name
            )
        })?;

    let total_size = asset.size;
    crate::app_log!(
        "[EditorBridge Installer] Asset: {} ({} bytes)",
        asset.name,
        total_size
    );

    let bin_dir = get_settings_dir().join("bin");
    if !bin_dir.exists() {
        tokio::fs::create_dir_all(&bin_dir)
            .await
            .map_err(|e| format!("Failed to create bin dir: {}", e))?;
    }

    let target_path = bin_dir.join("EditorBridge.exe");
    crate::app_log!(
        "[EditorBridge Installer] Target path: {}",
        target_path.display()
    );

    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "mini-ai-1c")
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    // Kill running EditorBridge.exe before overwriting — file is locked while process is alive.
    crate::editor_bridge::stop();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    let mut file = tokio::fs::File::create(&target_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_percent: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Error downloading: {}", e))?;

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Error writing file: {}", e))?;

        downloaded += chunk.len() as u64;
        let percent = if total_size > 0 {
            (downloaded * 100) / total_size
        } else {
            0
        };

        if percent >= last_percent + 5 {
            crate::app_log!("[EditorBridge Installer] Progress: {}%", percent);
            let _ = app.emit(
                "editor-bridge-download-progress",
                DownloadProgress {
                    progress: downloaded,
                    total: total_size,
                    percent,
                },
            );
            last_percent = percent;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush: {}", e))?;

    let _ = app.emit(
        "editor-bridge-download-progress",
        DownloadProgress {
            progress: total_size,
            total: total_size,
            percent: 100,
        },
    );

    crate::app_log!("[EditorBridge Installer] Download complete!");

    let abs_path = tokio::fs::canonicalize(&target_path)
        .await
        .map_err(|e| format!("Failed to get absolute path: {}", e))?;

    let mut path_str = abs_path.to_string_lossy().to_string();

    #[cfg(windows)]
    if path_str.starts_with(r"\\?\") {
        path_str = path_str[4..].to_string();
    }

    let mut settings = load_settings();
    settings.configurator.editor_bridge_exe_path = path_str.clone();
    save_settings(&settings).map_err(|e| format!("Failed to save settings: {}", e))?;

    crate::app_log!("[EditorBridge Installer] Saved to settings: {}", path_str);

    Ok(path_str)
}
