# ТЗ: Интеграция с 1С Конфигуратором

## Описание
Модуль для захвата/вставки кода из/в 1С Конфигуратор через глобальные горячие клавиши.

## Функциональные требования

### 1. Поиск окна Конфигуратора
- Поиск по заголовку "Конфигуратор"
- Поддержка нескольких окон
- Выбор конкретного окна (по HWND)

### 2. Захват кода
- Горячая клавиша: `Ctrl+Shift+1`
- Копирование выделенного текста (Ctrl+C)
- Отправка кода в чат агента

### 3. Вставка кода
- Вставка сгенерированного кода в Конфигуратор
- Ctrl+V в активное окно

## Технические требования (Windows/Rust)

### Crate: windows-rs
```rust
use windows::{
    Win32::UI::WindowsAndMessaging::*,
    Win32::Foundation::*,
};
```

### Tauri Commands
```rust
#[tauri::command]
fn list_configurator_windows() -> Vec<WindowInfo>;

#[tauri::command]
fn capture_code_from_configurator(hwnd: Option<isize>) -> Result<String, Error>;

#[tauri::command]
fn paste_code_to_configurator(code: String, hwnd: Option<isize>) -> Result<(), Error>;
```

### Регистрация горячих клавиш
```rust
// Использовать RegisterHotKey или tauri-plugin-global-shortcut
app.global_shortcut_manager()
    .register("Ctrl+Shift+1", handler)?;
```

## Платформа
> [!WARNING]
> Только Windows. Для Linux/Mac функционал недоступен.
