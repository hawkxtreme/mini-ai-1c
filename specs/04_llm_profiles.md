# ТЗ: Управление профилями LLM

## Описание
Хранение и управление профилями LLM провайдеров с шифрованием API ключей.

## Функциональные требования

### 1. CRUD профилей
- Создание/редактирование/удаление профилей
- Один профиль по умолчанию (нельзя удалить)
- Активный профиль для текущей сессии

### 2. Поддерживаемые провайдеры
- OpenAI (api.openai.com)
- Anthropic (api.anthropic.com)
- OpenRouter (openrouter.ai)
- Google (generativelanguage.googleapis.com)
- Custom (произвольный base_url)

### 3. Параметры профиля
- ID, Name
- Provider, Model
- API Key (зашифрованный)
- Base URL
- max_tokens, temperature

## Технические требования (Rust)

### Структуры
```rust
#[derive(Serialize, Deserialize)]
struct LLMProfile {
    id: String,
    name: String,
    provider: String,
    model: String,
    api_key_encrypted: String,
    base_url: Option<String>,
    max_tokens: u32,
    temperature: f32,
}
```

### Шифрование
```rust
// Crate: aes-gcm
fn encrypt_api_key(key: &str, master_key: &[u8]) -> String;
fn decrypt_api_key(encrypted: &str, master_key: &[u8]) -> String;
```

### Tauri Commands
```rust
#[tauri::command]
fn get_profiles() -> Vec<LLMProfile>;

#[tauri::command]
fn save_profile(profile: LLMProfile) -> Result<(), Error>;

#[tauri::command]
fn delete_profile(id: String) -> Result<(), Error>;

#[tauri::command]
fn set_active_profile(id: String) -> Result<(), Error>;
```

### Хранение
- JSON файл в AppData: `%APPDATA%/MiniAI1C/llm_profiles.json`
