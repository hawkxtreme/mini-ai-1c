//! BSL Language Server installer
//! Downloads and installs BSL LS from GitHub releases

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tauri::{AppHandle, Emitter};
use futures::StreamExt;

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

/// Download BSL Language Server from GitHub
/// Returns the absolute path to the downloaded JAR file
pub async fn download_bsl_ls(app: AppHandle) -> Result<String, String> {
    println!("[BSL Installer] Starting download...");
    
    // Emit initial progress
    let _ = app.emit("bsl-download-progress", DownloadProgress { progress: 0, total: 0, percent: 0 });
    
    // Create HTTP client with redirect support and timeout
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(600)) // 10 minutes timeout
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    
    // 1. Get latest release info from GitHub API
    println!("[BSL Installer] Fetching latest release info...");
    let release: GitHubRelease = client
        .get("https://api.github.com/repos/1c-syntax/bsl-language-server/releases/latest")
        .header("User-Agent", "mini-ai-1c")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    println!("[BSL Installer] Found release: {}", release.tag_name);

    // 2. Find the exec jar asset
    let asset = release.assets.iter()
        .find(|a| a.name.ends_with("-exec.jar"))
        .ok_or("Could not find *-exec.jar in the latest release")?;

    let total_size = asset.size;
    println!("[BSL Installer] Asset: {} ({} bytes)", asset.name, total_size);

    // 3. Determine install path (absolute path in app data dir)
    let bin_dir = get_settings_dir().join("bin");
    if !bin_dir.exists() {
        tokio::fs::create_dir_all(&bin_dir)
            .await
            .map_err(|e| format!("Failed to create bin dir: {}", e))?;
    }
    
    let target_path = bin_dir.join(&asset.name);
    println!("[BSL Installer] Target path: {}", target_path.display());

    // 4. Download file with progress
    println!("[BSL Installer] Downloading...");
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "mini-ai-1c")
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    // 5. Stream download with progress
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
        let percent = if total_size > 0 { (downloaded * 100) / total_size } else { 0 };
        
        // Emit progress every 5%
        if percent >= last_percent + 5 {
            println!("[BSL Installer] Progress: {}%", percent);
            let _ = app.emit("bsl-download-progress", DownloadProgress {
                progress: downloaded,
                total: total_size,
                percent,
            });
            last_percent = percent;
        }
    }

    file.flush().await.map_err(|e| format!("Failed to flush: {}", e))?;

    // Emit 100%
    let _ = app.emit("bsl-download-progress", DownloadProgress {
        progress: total_size,
        total: total_size,
        percent: 100,
    });

    println!("[BSL Installer] Download complete!");

    // 6. Get absolute path
    let abs_path = tokio::fs::canonicalize(&target_path)
        .await
        .map_err(|e| format!("Failed to get absolute path: {}", e))?;
    
    let mut path_str = abs_path.to_string_lossy().to_string();

    #[cfg(windows)]
    if path_str.starts_with(r"\\?\") {
        path_str = path_str[4..].to_string();
    }

    // 7. Save path to settings
    let mut settings = load_settings();
    settings.bsl_server.jar_path = path_str.clone();
    save_settings(&settings).map_err(|e| format!("Failed to save settings: {}", e))?;

    println!("[BSL Installer] Saved to settings: {}", path_str);

    Ok(path_str)
}
