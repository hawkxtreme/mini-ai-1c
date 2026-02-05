# ТЗ: UI/UX Требования

## Дизайн

### Тема
- Тёмная тема по умолчанию
- Цвета: bg-zinc-900, accent-blue-500/600

### Основной Layout
```
┌─────────────────────────────────────┐
│  Header: Logo + Profile Selector    │
├─────────────────────────────────────┤
│                                     │
│           Chat Messages             │
│        (scrollable area)            │
│                                     │
├─────────────────────────────────────┤
│  [Analyze] [Format]                 │
├─────────────────────────────────────┤
│  Input: [____________] [Send]       │
└─────────────────────────────────────┘
```

## Компоненты

### 1. ChatMessage
- Avatar (user/assistant)
- Content с Markdown
- Timestamp
- Copy button

### 2. ChatInput
- Multiline textarea
- Send button
- Enter для отправки (Shift+Enter для новой строки)

### 3. SettingsPanel (Modal/Drawer)
- Tabs: General, LLM Profiles, BSL, Configurator
- Form inputs для каждой настройки

### 4. ProfileSelector
- Dropdown со списком профилей
- Индикатор активного профиля

## Технологии

- React 18+
- TypeScript
- TailwindCSS
- lucide-react (иконки)
- react-markdown (рендеринг)

## Accessibility

- Keyboard navigation
- Focus states
- ARIA labels
