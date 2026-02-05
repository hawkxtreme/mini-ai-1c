# ТЗ: BSL Language Server Integration

## Описание
Клиент для взаимодействия с BSL Language Server через WebSocket/LSP.

## Функциональные требования

### 1. Анализ кода (Diagnostics)
- Отправка BSL кода на анализ
- Получение списка ошибок/предупреждений
- Информация: строка, колонка, severity, сообщение

### 2. Форматирование кода
- Отправка кода для форматирования
- Получение отформатированного кода

### 3. Управление сервером
- Автоматический запуск BSL LS (JAR файл)
- Отслеживание статуса подключения
- Переподключение при обрыве

## Технические требования (Tauri/Rust)

### Структура диагностики
```rust
#[derive(Serialize)]
struct Diagnostic {
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
    severity: u8,  // 1=Error, 2=Warning, 3=Info, 4=Hint
    message: String,
    code: String,
}
```

### Tauri Commands
```rust
#[tauri::command]
fn analyze_bsl(code: String) -> Result<Vec<Diagnostic>, Error>;

#[tauri::command]
fn format_bsl(code: String) -> Result<String, Error>;

#[tauri::command]
fn get_bsl_status() -> BslServerStatus;
```

### Зависимости
- `tokio-tungstenite` для WebSocket
- BSL LS JAR файл (Java 11+)

## Протокол LSP

1. `initialize` → инициализация сессии
2. `textDocument/didOpen` → открытие документа
3. `textDocument/publishDiagnostics` → получение ошибок
4. `textDocument/formatting` → форматирование
