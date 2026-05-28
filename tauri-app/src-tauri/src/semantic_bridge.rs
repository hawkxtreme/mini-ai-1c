use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickActionKind {
    Describe,
    Elaborate,
    Fix,
    Explain,
}

impl QuickActionKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "describe" => Some(Self::Describe),
            "elaborate" => Some(Self::Elaborate),
            "fix" => Some(Self::Fix),
            "explain" => Some(Self::Explain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticWriteIntent {
    ReplaceSelection,
    ReplaceCurrentMethod,
    InsertBeforeCurrentMethod,
    ReplaceModule,
}

impl SemanticWriteIntent {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "replace_selection" => Some(Self::ReplaceSelection),
            "replace_current_method" => Some(Self::ReplaceCurrentMethod),
            "insert_before_current_method" => Some(Self::InsertBeforeCurrentMethod),
            "replace_module" => Some(Self::ReplaceModule),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReplaceSelection => "replace_selection",
            Self::ReplaceCurrentMethod => "replace_current_method",
            Self::InsertBeforeCurrentMethod => "insert_before_current_method",
            Self::ReplaceModule => "replace_module",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolverContext {
    pub has_selection: bool,
    pub has_current_method: bool,
    pub prefer_full_module: bool,
}

pub fn parse_write_intent(value: Option<&str>) -> Result<Option<SemanticWriteIntent>, String> {
    match value {
        Some(raw) if !raw.trim().is_empty() => SemanticWriteIntent::parse(raw)
            .map(Some)
            .ok_or_else(|| format!("Неизвестный write intent: {}", raw)),
        _ => Ok(None),
    }
}

pub fn parse_action_kind(value: Option<&str>) -> Result<Option<QuickActionKind>, String> {
    match value {
        Some(raw) if !raw.trim().is_empty() => QuickActionKind::parse(raw)
            .map(Some)
            .ok_or_else(|| format!("Неизвестное quick action: {}", raw)),
        _ => Ok(None),
    }
}

pub fn infer_write_intent(
    explicit_intent: Option<SemanticWriteIntent>,
    action: Option<QuickActionKind>,
    ctx: ResolverContext,
) -> Option<SemanticWriteIntent> {
    if let Some(intent) = explicit_intent {
        return Some(intent);
    }

    if let Some(action) = action {
        return match action {
            QuickActionKind::Describe => {
                if ctx.has_current_method {
                    Some(SemanticWriteIntent::InsertBeforeCurrentMethod)
                } else if ctx.has_selection {
                    Some(SemanticWriteIntent::ReplaceSelection)
                } else {
                    Some(SemanticWriteIntent::ReplaceModule)
                }
            }
            QuickActionKind::Elaborate | QuickActionKind::Fix => {
                if ctx.has_selection {
                    Some(SemanticWriteIntent::ReplaceSelection)
                } else if ctx.has_current_method {
                    Some(SemanticWriteIntent::ReplaceCurrentMethod)
                } else {
                    Some(SemanticWriteIntent::ReplaceModule)
                }
            }
            QuickActionKind::Explain => None,
        };
    }

    if ctx.prefer_full_module {
        return Some(SemanticWriteIntent::ReplaceModule);
    }

    if ctx.has_selection {
        return Some(SemanticWriteIntent::ReplaceSelection);
    }

    if ctx.has_current_method {
        return Some(SemanticWriteIntent::ReplaceCurrentMethod);
    }

    Some(SemanticWriteIntent::ReplaceModule)
}

fn normalize_text_for_module(module_text: &str, text: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    if module_text.contains("\r\n") {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

fn preferred_newline(module_text: &str) -> &'static str {
    if module_text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn split_lines_inclusive(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').collect()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn line_span_len(text: &str) -> usize {
    split_lines_inclusive(text).len()
}

#[cfg_attr(not(test), allow(dead_code))]
fn line_range_text(
    module_text: &str,
    start_line: usize,
    line_count: usize,
) -> Result<String, String> {
    let lines = split_lines_inclusive(module_text);
    if lines.is_empty() || line_count == 0 {
        return Err("Не удалось определить диапазон строк для текущей процедуры.".to_string());
    }

    let end_exclusive = start_line.saturating_add(line_count);
    if start_line >= lines.len() || end_exclusive > lines.len() {
        return Err("Не удалось определить диапазон строк для текущей процедуры.".to_string());
    }

    Ok(lines[start_line..end_exclusive].concat())
}

#[cfg_attr(not(test), allow(dead_code))]
fn normalize_fragment_for_compare(module_text: &str, fragment_text: &str) -> String {
    normalize_text_for_module(module_text, fragment_text)
        .trim_end_matches(['\r', '\n'])
        .to_string()
}

pub fn replace_method_in_module(
    module_text: &str,
    start_line: usize,
    end_line: usize,
    replacement: &str,
) -> Result<String, String> {
    let lines: Vec<&str> = if module_text.is_empty() {
        Vec::new()
    } else {
        module_text.split_inclusive('\n').collect()
    };

    if lines.is_empty()
        || start_line >= lines.len()
        || end_line >= lines.len()
        || start_line > end_line
    {
        return Err("Не удалось собрать обновлённый модуль для текущей процедуры".to_string());
    }

    let prefix: String = lines[..start_line].concat();
    let suffix: String = if end_line + 1 < lines.len() {
        lines[end_line + 1..].concat()
    } else {
        String::new()
    };

    let normalized_replacement = normalize_text_for_module(module_text, replacement);
    let mut result =
        String::with_capacity(prefix.len() + normalized_replacement.len() + suffix.len() + 1);
    result.push_str(&prefix);
    result.push_str(&normalized_replacement);
    if !suffix.is_empty()
        && !normalized_replacement.ends_with('\n')
        && !normalized_replacement.ends_with("\r\n")
    {
        result.push_str(preferred_newline(module_text));
    }
    result.push_str(&suffix);
    Ok(result)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn replace_method_in_module_from_capture(
    module_text: &str,
    start_line: usize,
    captured_method_text: &str,
    replacement: &str,
) -> Result<String, String> {
    let line_count = line_span_len(captured_method_text);
    if line_count == 0 {
        return Err("Не удалось определить текущую процедуру для замены.".to_string());
    }

    let actual_fragment = line_range_text(module_text, start_line, line_count)?;
    let expected_fragment = normalize_fragment_for_compare(module_text, captured_method_text);
    let current_fragment = normalize_fragment_for_compare(module_text, &actual_fragment);
    if current_fragment != expected_fragment {
        return Err(
            "Текущая процедура изменилась с момента захвата. Повторите действие.".to_string(),
        );
    }

    replace_method_in_module(
        module_text,
        start_line,
        start_line + line_count - 1,
        replacement,
    )
}

pub fn insert_before_method_in_module(
    module_text: &str,
    start_line: usize,
    insertion: &str,
) -> Result<String, String> {
    let lines: Vec<&str> = if module_text.is_empty() {
        Vec::new()
    } else {
        module_text.split_inclusive('\n').collect()
    };

    if lines.is_empty() || start_line >= lines.len() {
        return Err("Не удалось вставить код перед текущей процедурой".to_string());
    }

    let prefix: String = lines[..start_line].concat();
    let suffix: String = lines[start_line..].concat();
    let mut normalized_insertion = normalize_text_for_module(module_text, insertion);
    if !normalized_insertion.ends_with('\n') && !normalized_insertion.ends_with("\r\n") {
        normalized_insertion.push_str(preferred_newline(module_text));
    }

    let mut result =
        String::with_capacity(prefix.len() + normalized_insertion.len() + suffix.len());
    result.push_str(&prefix);
    result.push_str(&normalized_insertion);
    result.push_str(&suffix);
    Ok(result)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn insert_before_method_in_module_from_capture(
    module_text: &str,
    start_line: usize,
    captured_method_text: &str,
    insertion: &str,
) -> Result<String, String> {
    let line_count = line_span_len(captured_method_text);
    if line_count == 0 {
        return Err("Не удалось определить текущую процедуру для вставки описания.".to_string());
    }

    let actual_fragment = line_range_text(module_text, start_line, line_count)?;
    let expected_fragment = normalize_fragment_for_compare(module_text, captured_method_text);
    let current_fragment = normalize_fragment_for_compare(module_text, &actual_fragment);
    if current_fragment != expected_fragment {
        return Err(
            "Текущая процедура изменилась с момента захвата. Повторите генерацию описания."
                .to_string(),
        );
    }

    insert_before_method_in_module(module_text, start_line, insertion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_describe_without_selection_prefers_insert_before_method() {
        let intent = infer_write_intent(
            None,
            Some(QuickActionKind::Describe),
            ResolverContext {
                has_selection: false,
                has_current_method: true,
                prefer_full_module: true,
            },
        );

        assert_eq!(intent, Some(SemanticWriteIntent::InsertBeforeCurrentMethod));
    }

    #[test]
    fn infer_describe_prefers_current_method_over_selection() {
        let intent = infer_write_intent(
            None,
            Some(QuickActionKind::Describe),
            ResolverContext {
                has_selection: true,
                has_current_method: true,
                prefer_full_module: false,
            },
        );

        assert_eq!(intent, Some(SemanticWriteIntent::InsertBeforeCurrentMethod));
    }

    #[test]
    fn infer_fix_with_selection_prefers_replace_selection() {
        let intent = infer_write_intent(
            None,
            Some(QuickActionKind::Fix),
            ResolverContext {
                has_selection: true,
                has_current_method: true,
                prefer_full_module: false,
            },
        );

        assert_eq!(intent, Some(SemanticWriteIntent::ReplaceSelection));
    }

    #[test]
    fn infer_without_action_uses_full_module_preference() {
        let intent = infer_write_intent(
            None,
            None,
            ResolverContext {
                has_selection: false,
                has_current_method: true,
                prefer_full_module: true,
            },
        );

        assert_eq!(intent, Some(SemanticWriteIntent::ReplaceModule));
    }

    #[test]
    fn replace_method_in_module_replaces_only_target_range() {
        let module =
            "Процедура A()\n\tСообщить(1);\nКонецПроцедуры\n\nПроцедура B()\nКонецПроцедуры\n";
        let updated = replace_method_in_module(
            module,
            0,
            2,
            "Процедура A()\n\tСообщить(2);\nКонецПроцедуры\n",
        )
        .unwrap();

        assert_eq!(
            updated,
            "Процедура A()\n\tСообщить(2);\nКонецПроцедуры\n\nПроцедура B()\nКонецПроцедуры\n"
        );
    }

    #[test]
    fn insert_before_method_in_module_inserts_block_before_method() {
        let module = "Перем X;\n\nПроцедура A()\nКонецПроцедуры\n";
        let updated = insert_before_method_in_module(module, 2, "// Описание процедуры").unwrap();

        assert_eq!(
            updated,
            "Перем X;\n\n// Описание процедуры\nПроцедура A()\nКонецПроцедуры\n"
        );
    }

    #[test]
    fn insert_before_method_in_module_preserves_crlf() {
        let module = "Перем X;\r\n\r\nПроцедура A()\r\nКонецПроцедуры\r\n";
        let updated =
            insert_before_method_in_module(module, 2, "// Описание\r\n// Строка 2").unwrap();

        assert_eq!(
            updated,
            "Перем X;\r\n\r\n// Описание\r\n// Строка 2\r\nПроцедура A()\r\nКонецПроцедуры\r\n"
        );
    }

    #[test]
    fn replace_method_in_module_from_capture_validates_fragment_before_rewrite() {
        let module = "Перем X;\r\n\r\nПроцедура Тест()\r\n    Сообщить(1);\r\nКонецПроцедуры\r\n";
        let captured = "Процедура Тест()\n    Сообщить(1);\nКонецПроцедуры";
        let updated = replace_method_in_module_from_capture(
            module,
            2,
            captured,
            "Процедура Тест()\n    Сообщить(2);\nКонецПроцедуры",
        )
        .unwrap();

        assert_eq!(
            updated,
            "Перем X;\r\n\r\nПроцедура Тест()\r\n    Сообщить(2);\r\nКонецПроцедуры"
        );
    }

    #[test]
    fn replace_method_in_module_from_capture_rejects_stale_fragment() {
        let module = "Процедура Тест()\n    Сообщить(2);\nКонецПроцедуры\n";
        let captured = "Процедура Тест()\n    Сообщить(1);\nКонецПроцедуры";
        let error = replace_method_in_module_from_capture(
            module,
            0,
            captured,
            "Процедура Тест()\nКонецПроцедуры",
        )
        .unwrap_err();

        assert!(error.contains("изменилась"));
    }

    #[test]
    fn insert_before_method_in_module_from_capture_preserves_unicode_prefix() {
        let module = "Перем X;\n\nПроцедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры\n";
        let captured = "Процедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры";
        let updated =
            insert_before_method_in_module_from_capture(module, 2, captured, "// Формирует XML...")
                .unwrap();

        assert_eq!(
            updated,
            "Перем X;\n\n// Формирует XML...\nПроцедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры\n"
        );
    }
}
