# ТЗ: AI Chat с потоковым выводом (Streaming)

## Описание
AI-чат для общения с LLM провайдерами с потоковым (streaming) выводом ответов.

## Функциональные требования

### 1. Интеграция с LLM API
- OpenAI-совместимый API (OpenAI, OpenRouter, Anthropic)
- Streaming через SSE/WebSocket
- Настраиваемый system prompt для 1С-контекста

### 2. UI чата
- История сообщений (user/assistant)
- Потоковый вывод: символы появляются по мере генерации
- Markdown рендеринг в ответах
- Подсветка синтаксиса BSL кода в блоках ```bsl

### 3. Системный промпт
```
Ты - AI-ассистент для разработки на платформе 1С:Предприятие.
Твои возможности:
- Анализ и рефакторинг кода на языке BSL (1С)
- Объяснение логики кода
- Поиск ошибок и предложение исправлений
- Написание нового кода по описанию
```

## Технические требования (Tauri/Rust)

### Backend (Rust)
```rust
#[tauri::command]
async fn stream_chat(
    messages: Vec<ChatMessage>,
    profile_id: String,
    app_handle: tauri::AppHandle
) -> Result<(), Error> {
    // Emit events: "chat-chunk", "chat-done", "chat-error"
}
```

### Frontend (TypeScript)
```typescript
import { listen } from '@tauri-apps/api/event';

listen('chat-chunk', (e) => appendMessage(e.payload));
listen('chat-done', () => setLoading(false));
```

## Данные

```typescript
interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}
```
