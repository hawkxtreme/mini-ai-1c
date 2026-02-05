# ТЗ: Управление настройками

## Описание
Централизованное хранение настроек приложения с persistence в JSON.

## Структура настроек

### 1. ConfiguratorSettings
```rust
struct ConfiguratorSettings {
    window_title_pattern: String,  // "Конфигуратор"
    selected_window_hwnd: Option<isize>,
    capture_on_hotkey: bool,       // true
    hotkey: String,                // "Ctrl+Shift+1"
}
```

### 2. BSLServerSettings
```rust
struct BSLServerSettings {
    jar_path: String,
    auto_download: bool,
    websocket_port: u16,  // 8025
    java_path: String,    // "java"
    enabled: bool,
}
```

### 3. UISettings
```rust
struct UISettings {
    theme: String,           // "dark"
    minimize_to_tray: bool,
    start_minimized: bool,
    window_width: u32,
    window_height: u32,
    window_x: i32,
    window_y: i32,
}
```

## Tauri Commands

```rust
#[tauri::command]
fn get_settings() -> AppSettings;

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<(), Error>;

#[tauri::command]
fn reset_settings() -> Result<(), Error>;
```

## Хранение
- Путь: `%APPDATA%/MiniAI1C/settings.json`
- Формат: JSON с pretty-print
