use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub process_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorApplySupport {
    pub can_apply_directly: bool,
    pub preferred_writer: String,
    pub target_kind: Option<String>,
    pub reason: Option<String>,
}

#[tauri::command]
pub fn find_configurator_windows_cmd(pattern: String) -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::find_configurator_windows(&pattern)
            .into_iter()
            .map(|w| WindowInfo {
                hwnd: w.hwnd,
                title: w.title,
                process_id: w.process_id,
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = pattern;
        Vec::new()
    }
}

#[tauri::command]
pub fn check_selection_state(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        if let Ok((has_selection, _)) = crate::editor_bridge::get_selection(hwnd) {
            return has_selection;
        }

        use windows::Win32::Foundation::HWND;
        let parent = HWND(hwnd as *mut std::ffi::c_void);
        if let Some(sci) = crate::scintilla::find_scintilla_control(parent) {
            return crate::scintilla::sci_has_selection(sci);
        }

        use crate::configurator;
        configurator::is_selection_active(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

#[tauri::command]
pub fn get_editor_context_cmd(
    app_handle: AppHandle,
    hwnd: isize,
    skip_focus_restore: Option<bool>,
) -> Result<crate::editor_bridge::EditorContext, String> {
    #[cfg(windows)]
    {
        let mut ctx = match crate::editor_bridge::get_editor_context_with_read_preferences(
            hwnd, true, true,
        ) {
            Ok(ctx) => ctx,
            Err(error) => {
                crate::app_log!(
                    "[1C] Preferred clipboard-backed editor context unavailable - {}, falling back to raw EditorBridge context",
                    error
                );
                crate::editor_bridge::get_editor_context(hwnd)?
            }
        };

        if !ctx.has_selection && !ctx.module_text.trim().is_empty() {
            let mut candidate_lines = Vec::new();
            if ctx.caret_line >= 0 {
                candidate_lines.push(ctx.caret_line as usize);
            }
            if let Some(start_line) = ctx.method_start_line {
                if start_line >= 0 {
                    candidate_lines.push(start_line as usize);
                    candidate_lines.push(start_line as usize + 1);
                }
            }

            let clean_fragment = candidate_lines.into_iter().find_map(|line| {
                crate::scintilla::extract_active_fragment_info(&ctx.module_text, line)
            });

            if let Some(fragment) = clean_fragment {
                ctx.current_method_text = Some(fragment.text);
                ctx.method_start_line = Some(fragment.start_line as i32);
                ctx.method_end_line = Some(fragment.end_line as i32);
            } else {
                ctx.current_method_text = None;
            }
        }

        if !skip_focus_restore.unwrap_or(false) {
            restore_focus_to_app(&app_handle);
        }
        Ok(ctx)
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn sync_configurator_caret_to_point_cmd(
    hwnd: isize,
    screen_x: i32,
    screen_y: i32,
    child_hwnd: Option<isize>,
) -> Result<bool, String> {
    #[cfg(windows)]
    {
        use std::thread;
        use std::time::Duration;
        use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetWindowRect, SendMessageW, SetForegroundWindow, WindowFromPoint,
            GA_ROOT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        };

        fn make_lparam(x: i32, y: i32) -> LPARAM {
            LPARAM(((y as u32) << 16 | (x as u16 as u32)) as isize)
        }

        let top_hwnd = HWND(hwnd as *mut std::ffi::c_void);
        if top_hwnd.0.is_null() {
            return Err("Configurator HWND is required.".to_string());
        }

        let point = POINT {
            x: screen_x,
            y: screen_y,
        };

        let fallback_target = unsafe { WindowFromPoint(point) };
        let mut target_hwnd =
            HWND(child_hwnd.unwrap_or(fallback_target.0 as isize) as *mut std::ffi::c_void);
        if target_hwnd.0.is_null() {
            target_hwnd = top_hwnd;
        }

        let target_root = unsafe { GetAncestor(target_hwnd, GA_ROOT) };
        if !target_root.0.is_null() && target_root != top_hwnd {
            target_hwnd = top_hwnd;
        }

        unsafe {
            let _ = SetForegroundWindow(top_hwnd);
        }
        thread::sleep(Duration::from_millis(35));

        let mut window_rect = RECT::default();
        let mut message_target = target_hwnd;
        let converted = unsafe { GetWindowRect(message_target, &mut window_rect) }.is_ok();
        if !converted {
            message_target = top_hwnd;
            let converted_top = unsafe { GetWindowRect(message_target, &mut window_rect) }.is_ok();
            if !converted_top {
                return Err(
                    "Failed to translate click point to configurator coordinates.".to_string(),
                );
            }
        }

        let client_point = POINT {
            x: screen_x - window_rect.left,
            y: screen_y - window_rect.top,
        };
        let lparam = make_lparam(client_point.x, client_point.y);
        unsafe {
            let _ = SendMessageW(message_target, WM_MOUSEMOVE, WPARAM(0), lparam);
            let _ = SendMessageW(message_target, WM_LBUTTONDOWN, WPARAM(0x0001), lparam);
            let _ = SendMessageW(message_target, WM_LBUTTONUP, WPARAM(0), lparam);
        }

        thread::sleep(Duration::from_millis(45));
        crate::app_log!(
            "[1C] Synced caret to right-click point: top_hwnd={}, target_hwnd={}, screen=({}, {}) client=({}, {})",
            hwnd,
            message_target.0 as isize,
            screen_x,
            screen_y,
            client_point.x,
            client_point.y
        );
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let _ = screen_x;
        let _ = screen_y;
        let _ = child_hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn get_configurator_apply_support_cmd(
    hwnd: isize,
    action: Option<String>,
    write_intent: Option<String>,
    use_select_all: Option<bool>,
    original_content: Option<String>,
) -> Result<EditorApplySupport, String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;

        let select_all = use_select_all.unwrap_or(false);
        let requested_action = crate::semantic_bridge::parse_action_kind(action.as_deref())?;
        let requested_intent = crate::semantic_bridge::parse_write_intent(write_intent.as_deref())?;
        let quick_action_apply_policy = resolve_quick_action_apply_policy(
            crate::settings::load_settings()
                .configurator
                .editor_bridge_enabled,
            requested_action,
            requested_intent,
        );
        let parent = HWND(hwnd as *mut std::ffi::c_void);

        if !matches!(
            quick_action_apply_policy,
            QuickActionApplyPolicy::ForceLegacy
        ) {
            if let Ok(ctx) = crate::editor_bridge::get_editor_context(hwnd) {
                let target = match choose_bridge_paste_target(
                    &ctx,
                    requested_intent,
                    requested_action,
                    select_all,
                    original_content.as_deref(),
                ) {
                    Ok(target) => target,
                    Err(error) => {
                        return Ok(direct_apply_unavailable(
                            error,
                            requested_intent.map(semantic_target_name),
                        ));
                    }
                };

                let target_name = bridge_paste_target_name(&target).to_string();

                if bridge_supports_target(&ctx, &target) {
                    let writer = ctx
                        .capabilities
                        .as_ref()
                        .map(|capabilities| capabilities.write_mode.clone())
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "editor_bridge".to_string());
                    return Ok(direct_apply_available(&writer, Some(target_name)));
                }

                let reason = bridge_capability_reason(&ctx).unwrap_or_else(|| {
                "Надежное прямое применение недоступно: bridge не поддерживает эту операцию, а Scintilla не найден.".to_string()
            });
                if matches!(quick_action_apply_policy, QuickActionApplyPolicy::Default)
                    && crate::scintilla::find_scintilla_control(parent).is_some()
                {
                    return Ok(direct_apply_available("scintilla", Some(target_name)));
                }

                return Ok(direct_apply_unavailable(reason, Some(target_name)));
            }
        }

        if matches!(
            quick_action_apply_policy,
            QuickActionApplyPolicy::ForceLegacy
        ) {
            let inferred_intent = crate::semantic_bridge::infer_write_intent(
                requested_intent,
                requested_action,
                crate::semantic_bridge::ResolverContext {
                    has_selection: false,
                    has_current_method: false,
                    prefer_full_module: select_all,
                },
            );

            return match inferred_intent {
                Some(intent) => Ok(direct_apply_available(
                    "legacy_clipboard",
                    Some(semantic_target_name(intent)),
                )),
                None => Ok(direct_apply_unavailable(
                    "Для этого действия запись в редактор не требуется.".to_string(),
                    None,
                )),
            };
        }

        if matches!(
            quick_action_apply_policy,
            QuickActionApplyPolicy::ForceSemantic
        ) {
            let inferred_intent = crate::semantic_bridge::infer_write_intent(
                requested_intent,
                requested_action,
                crate::semantic_bridge::ResolverContext {
                    has_selection: false,
                    has_current_method: false,
                    prefer_full_module: select_all,
                },
            );

            return match inferred_intent {
                Some(intent) => Ok(direct_apply_unavailable(
                    "Semantic apply для quick actions требует доступный EditorBridge-контекст. Старый Scintilla writer отключён для этого режима, чтобы не допускать порчу кодировки.".to_string(),
                    Some(semantic_target_name(intent)),
                )),
                None => Ok(direct_apply_unavailable(
                    "Для этого действия запись в редактор не требуется.".to_string(),
                    None,
                )),
            };
        }

        if let Some(sci) = crate::scintilla::find_scintilla_control(parent) {
            let inferred_intent = crate::semantic_bridge::infer_write_intent(
                requested_intent,
                requested_action,
                crate::semantic_bridge::ResolverContext {
                    has_selection: crate::scintilla::sci_has_selection(sci),
                    has_current_method: crate::scintilla::sci_get_active_fragment_info(sci)
                        .ok()
                        .flatten()
                        .is_some(),
                    prefer_full_module: select_all,
                },
            );

            return match inferred_intent {
                Some(intent) => Ok(direct_apply_available(
                    "scintilla",
                    Some(semantic_target_name(intent)),
                )),
                None => Ok(direct_apply_unavailable(
                    "Для этого действия запись в редактор не требуется.".to_string(),
                    None,
                )),
            };
        }

        let inferred_intent = crate::semantic_bridge::infer_write_intent(
            requested_intent,
            requested_action,
            crate::semantic_bridge::ResolverContext {
                has_selection: false,
                has_current_method: false,
                prefer_full_module: select_all,
            },
        );

        match inferred_intent {
            Some(intent) => Ok(direct_apply_unavailable(
                "Надежное прямое применение недоступно: нет semantic bridge write и не найден Scintilla. Используйте диф.".to_string(),
                Some(semantic_target_name(intent)),
            )),
            None => Ok(direct_apply_unavailable(
                "Для этого действия запись в редактор не требуется.".to_string(),
                None,
            )),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let _ = action;
        let _ = write_intent;
        let _ = use_select_all;
        let _ = original_content;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn diagnose_editor_bridge_cmd(hwnd: isize) -> Result<String, String> {
    #[cfg(windows)]
    {
        let report = crate::editor_bridge::diagnose_editor(hwnd)?;
        crate::app_log!("[Bridge][DIAG]\n{}", report);
        Ok(report)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[cfg(windows)]
fn restore_focus_to_app(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_focus();
        crate::app_log!("[1C] Focus restored to mini-ai-1c");
    }
}

#[cfg(windows)]
const CONFLICT_MESSAGE: &str =
    "CONFLICT: Код в Конфигураторе был изменён с момента последнего чтения. Получите код заново перед применением.";

#[cfg(windows)]
fn ensure_content_not_changed(
    original_content: Option<&str>,
    current_content: &str,
) -> Result<(), String> {
    if let Some(original) = original_content {
        let original_hash = crate::configurator::calculate_content_hash(original);
        let current_hash = crate::configurator::calculate_content_hash(current_content);
        if original_hash != current_hash {
            return Err(CONFLICT_MESSAGE.to_string());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickActionApplyPolicy {
    Default,
    ForceSemantic,
    ForceLegacy,
}

fn resolve_quick_action_apply_policy(
    editor_bridge_enabled: bool,
    requested_action: Option<crate::semantic_bridge::QuickActionKind>,
    requested_intent: Option<crate::semantic_bridge::SemanticWriteIntent>,
) -> QuickActionApplyPolicy {
    if requested_action.is_none() && requested_intent.is_none() {
        return QuickActionApplyPolicy::Default;
    }

    if editor_bridge_enabled {
        QuickActionApplyPolicy::ForceSemantic
    } else {
        QuickActionApplyPolicy::ForceLegacy
    }
}

#[cfg(windows)]
enum BridgePasteTarget<'a> {
    FullModule(&'a str),
    Selection(&'a str),
    CurrentMethod(&'a str),
    InsertBeforeCurrentMethod(&'a str),
}

#[cfg(windows)]
fn editor_context_has_current_method(ctx: &crate::editor_bridge::EditorContext) -> bool {
    ctx.current_method_text
        .as_deref()
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false)
}

#[cfg(windows)]
fn choose_bridge_paste_target<'a>(
    ctx: &'a crate::editor_bridge::EditorContext,
    requested_intent: Option<crate::semantic_bridge::SemanticWriteIntent>,
    requested_action: Option<crate::semantic_bridge::QuickActionKind>,
    select_all: bool,
    original_content: Option<&'a str>,
) -> Result<BridgePasteTarget<'a>, String> {
    let resolved_intent = crate::semantic_bridge::infer_write_intent(
        requested_intent,
        requested_action,
        crate::semantic_bridge::ResolverContext {
            has_selection: ctx.has_selection,
            has_current_method: editor_context_has_current_method(ctx),
            prefer_full_module: select_all,
        },
    )
    .ok_or_else(|| "Для этого действия запись в редактор не требуется.".to_string())?;

    let target = match resolved_intent {
        crate::semantic_bridge::SemanticWriteIntent::ReplaceSelection => {
            if ctx.has_selection {
                BridgePasteTarget::Selection(&ctx.selection_text)
            } else {
                return Err(
                    "Выделение в редакторе 1С больше недоступно. Повторите действие, не меняя фокус."
                        .to_string(),
                );
            }
        }
        crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod => {
            let method_text = ctx
                .current_method_text
                .as_deref()
                .or(original_content)
                .ok_or_else(|| {
                    "Не удалось определить текущую процедуру или функцию.".to_string()
                })?;
            BridgePasteTarget::CurrentMethod(method_text)
        }
        crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod => {
            let method_text = ctx
                .current_method_text
                .as_deref()
                .or(original_content)
                .ok_or_else(|| {
                    "Не удалось определить текущую процедуру для вставки описания.".to_string()
                })?;
            BridgePasteTarget::InsertBeforeCurrentMethod(method_text)
        }
        crate::semantic_bridge::SemanticWriteIntent::ReplaceModule => {
            if let Some(original) = original_content {
                let original_hash = crate::configurator::calculate_content_hash(original);
                let module_hash = crate::configurator::calculate_content_hash(&ctx.module_text);
                if original_hash != module_hash
                    && !select_all
                    && editor_context_has_current_method(ctx)
                {
                    let method_text = ctx.current_method_text.as_deref().ok_or_else(|| {
                        "Не удалось определить текущую процедуру или функцию.".to_string()
                    })?;
                    return Ok(BridgePasteTarget::CurrentMethod(method_text));
                }
            }
            BridgePasteTarget::FullModule(&ctx.module_text)
        }
    };

    Ok(target)
}

#[cfg(windows)]
fn bridge_paste_target_name(target: &BridgePasteTarget<'_>) -> &'static str {
    match target {
        BridgePasteTarget::FullModule(_) => "full_module",
        BridgePasteTarget::Selection(_) => "selection",
        BridgePasteTarget::CurrentMethod(_) => "current_method",
        BridgePasteTarget::InsertBeforeCurrentMethod(_) => "insert_before_current_method",
    }
}

#[cfg(windows)]
fn semantic_target_name(intent: crate::semantic_bridge::SemanticWriteIntent) -> String {
    intent.as_str().to_string()
}

#[cfg(windows)]
fn direct_apply_unavailable(reason: String, target_kind: Option<String>) -> EditorApplySupport {
    EditorApplySupport {
        can_apply_directly: false,
        preferred_writer: "diff_only".to_string(),
        target_kind,
        reason: Some(reason),
    }
}

#[cfg(windows)]
fn direct_apply_available(writer: &str, target_kind: Option<String>) -> EditorApplySupport {
    EditorApplySupport {
        can_apply_directly: true,
        preferred_writer: writer.to_string(),
        target_kind,
        reason: None,
    }
}

#[cfg(windows)]
fn bridge_supports_target(
    ctx: &crate::editor_bridge::EditorContext,
    target: &BridgePasteTarget<'_>,
) -> bool {
    let Some(capabilities) = ctx.capabilities.as_ref() else {
        return true;
    };

    match target {
        BridgePasteTarget::FullModule(_) => capabilities.can_replace_module,
        BridgePasteTarget::Selection(_) => capabilities.can_replace_selection,
        BridgePasteTarget::CurrentMethod(_) => capabilities.can_replace_current_method,
        BridgePasteTarget::InsertBeforeCurrentMethod(_) => capabilities.can_insert_before_method,
    }
}

#[cfg(windows)]
fn bridge_capability_reason(ctx: &crate::editor_bridge::EditorContext) -> Option<String> {
    ctx.capabilities
        .as_ref()
        .map(|capabilities| capabilities.diagnostic_message.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn log_bridge_write_result(result: &crate::editor_bridge::EditorWriteResult) {
    crate::app_log!(
        "[1C] EditorBridge write result: applied={}, operation={}, target={}, before_hash={}, after_hash={}, fallback_used={}, writer_kind={}, message={}",
        result.applied,
        result.operation_kind,
        result.target_kind,
        result.before_hash,
        result.after_hash,
        result.fallback_used,
        result.writer_kind,
        result.diagnostic_message
    );
}

#[cfg(windows)]
fn bridge_method_hints_with_fallback(
    ctx: &crate::editor_bridge::EditorContext,
    requested: &crate::editor_bridge::EditorMethodHints,
) -> crate::editor_bridge::EditorMethodHints {
    crate::editor_bridge::EditorMethodHints {
        caret_line: requested.caret_line.or(Some(ctx.caret_line)),
        method_start_line: requested.method_start_line.or(ctx.method_start_line),
        method_name: requested
            .method_name
            .clone()
            .or_else(|| ctx.current_method_name.clone()),
        runtime_id: requested
            .runtime_id
            .clone()
            .or_else(|| ctx.primary_runtime_id.clone()),
    }
}

#[cfg(windows)]
fn hints_contain_semantic_method_identity(hints: &crate::editor_bridge::EditorMethodHints) -> bool {
    hints.method_start_line.is_some()
        || hints.caret_line.is_some()
        || hints
            .method_name
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

#[cfg(windows)]
fn can_attempt_hint_based_bridge_semantic_write(
    intent: Option<crate::semantic_bridge::SemanticWriteIntent>,
    original_content: Option<&str>,
    hints: &crate::editor_bridge::EditorMethodHints,
) -> bool {
    matches!(
        intent,
        Some(crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod)
            | Some(crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod)
    ) && original_content
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        && hints_contain_semantic_method_identity(hints)
}

#[cfg(windows)]
fn execute_hint_based_bridge_semantic_write(
    hwnd: isize,
    code: &str,
    intent: crate::semantic_bridge::SemanticWriteIntent,
    hints: &crate::editor_bridge::EditorMethodHints,
) -> Result<crate::editor_bridge::EditorWriteResult, String> {
    match intent {
        crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod => {
            crate::editor_bridge::replace_current_method(hwnd, code, Some(hints))
        }
        crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod => {
            crate::editor_bridge::insert_before_method(hwnd, code, Some(hints))
        }
        _ => Err(
            "Hint-based EditorBridge write supports only method-scoped semantic actions."
                .to_string(),
        ),
    }
}

#[cfg(windows)]
fn build_legacy_semantic_clipboard_module(
    current_module: &str,
    code: &str,
    intent: crate::semantic_bridge::SemanticWriteIntent,
    original_content: Option<&str>,
    hints: &crate::editor_bridge::EditorMethodHints,
) -> Result<Option<String>, String> {
    let Some(method_start_line) = hints.method_start_line else {
        return Ok(None);
    };
    if method_start_line < 0 {
        return Ok(None);
    }

    let Some(captured_method_text) = original_content.filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    let updated_module = match intent {
        crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod => {
            crate::semantic_bridge::replace_method_in_module_from_capture(
                current_module,
                method_start_line as usize,
                captured_method_text,
                code,
            )?
        }
        crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod => {
            crate::semantic_bridge::insert_before_method_in_module_from_capture(
                current_module,
                method_start_line as usize,
                captured_method_text,
                code,
            )?
        }
        _ => return Ok(None),
    };

    Ok(Some(updated_module))
}

#[cfg(windows)]
fn try_legacy_semantic_clipboard_rewrite(
    hwnd: isize,
    code: &str,
    intent: crate::semantic_bridge::SemanticWriteIntent,
    original_content: Option<&str>,
    hints: &crate::editor_bridge::EditorMethodHints,
) -> Result<Option<(String, String)>, String> {
    let current_module = crate::configurator::get_selected_code(hwnd, true)?;
    let updated_module = build_legacy_semantic_clipboard_module(
        &current_module,
        code,
        intent,
        original_content,
        hints,
    )?;

    Ok(updated_module.map(|value| (current_module, value)))
}

#[cfg(windows)]
const BRIDGE_VERIFY_ATTEMPTS: usize = 12;
#[cfg(windows)]
const BRIDGE_VERIFY_INITIAL_DELAY_MS: u64 = 180;
#[cfg(windows)]
const BRIDGE_VERIFY_DELAY_MS: u64 = 250;

#[cfg(windows)]
fn bridge_verification_matches(
    target: &BridgePasteTarget<'_>,
    before_module_hash: &str,
    expected_hash: &str,
    after_ctx: &crate::editor_bridge::EditorContext,
) -> bool {
    let after_module_hash = crate::configurator::calculate_content_hash(&after_ctx.module_text);

    match target {
        BridgePasteTarget::FullModule(_) => after_module_hash == expected_hash,
        BridgePasteTarget::Selection(_) => after_module_hash != before_module_hash,
        BridgePasteTarget::CurrentMethod(_) => {
            if let Some(method_text) = after_ctx.current_method_text.as_deref() {
                let after_method_hash = crate::configurator::calculate_content_hash(method_text);
                if after_method_hash == expected_hash {
                    return true;
                }
            }

            after_module_hash != before_module_hash
        }
        BridgePasteTarget::InsertBeforeCurrentMethod(_) => after_module_hash != before_module_hash,
    }
}

#[cfg(windows)]
fn verify_bridge_write(
    bridge_hwnd: isize,
    before_ctx: &crate::editor_bridge::EditorContext,
    target: &BridgePasteTarget<'_>,
    code: &str,
    write_result: Option<&crate::editor_bridge::EditorWriteResult>,
) -> Result<(), String> {
    let expected_hash = crate::configurator::calculate_content_hash(code);
    let before_target_text = match target {
        BridgePasteTarget::FullModule(text)
        | BridgePasteTarget::Selection(text)
        | BridgePasteTarget::CurrentMethod(text)
        | BridgePasteTarget::InsertBeforeCurrentMethod(text) => *text,
    };
    let before_target_hash = crate::configurator::calculate_content_hash(before_target_text);
    let before_module_hash = crate::configurator::calculate_content_hash(&before_ctx.module_text);

    crate::app_log!(
        "[1C] Bridge verification start: target={}, hwnd={}, target_hash={}, expected_hash={}, module_hash={}",
        bridge_paste_target_name(target),
        bridge_hwnd,
        before_target_hash,
        expected_hash,
        before_module_hash
    );

    if before_target_hash == expected_hash {
        crate::app_log!(
            "[1C] Bridge verification skipped: target content already equals requested code"
        );
        return Ok(());
    }

    if let Some(result) = write_result {
        if result.applied {
            crate::app_log!(
                "[1C] Bridge verification trusted transport result: writer_kind={}, after_hash={}, expected_hash={}, target={}",
                result.writer_kind,
                result.after_hash,
                expected_hash,
                bridge_paste_target_name(target)
            );
            return Ok(());
        }
    }

    let mut last_after_ctx: Option<crate::editor_bridge::EditorContext> = None;
    let mut last_error: Option<String> = None;

    for attempt in 0..BRIDGE_VERIFY_ATTEMPTS {
        let delay_ms = if attempt == 0 {
            BRIDGE_VERIFY_INITIAL_DELAY_MS
        } else {
            BRIDGE_VERIFY_DELAY_MS
        };
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));

        match crate::editor_bridge::get_editor_context(bridge_hwnd) {
            Ok(after_ctx) => {
                let after_module_hash =
                    crate::configurator::calculate_content_hash(&after_ctx.module_text);
                let after_method_hash = after_ctx
                    .current_method_text
                    .as_deref()
                    .map(crate::configurator::calculate_content_hash);

                crate::app_log!(
                    "[1C] Bridge verification poll {}/{}: target={}, hwnd={}, after_module_hash={}, after_method_hash={}, current_method_present={}, selection_present={}",
                    attempt + 1,
                    BRIDGE_VERIFY_ATTEMPTS,
                    bridge_paste_target_name(target),
                    bridge_hwnd,
                    after_module_hash,
                    after_method_hash.as_deref().unwrap_or("<none>"),
                    after_ctx.current_method_text.is_some(),
                    after_ctx.has_selection
                );

                if bridge_verification_matches(
                    target,
                    &before_module_hash,
                    &expected_hash,
                    &after_ctx,
                ) {
                    return Ok(());
                }

                last_after_ctx = Some(after_ctx);
            }
            Err(error) => {
                crate::app_log!(
                    "[1C] Bridge verification poll {}/{} failed: {}",
                    attempt + 1,
                    BRIDGE_VERIFY_ATTEMPTS,
                    error
                );
                last_error = Some(error);
            }
        }
    }

    if let Some(after_ctx) = last_after_ctx {
        let after_module_hash = crate::configurator::calculate_content_hash(&after_ctx.module_text);

        match target {
            BridgePasteTarget::FullModule(_) => Err(
                "Вставка не подтвердилась в ожидаемом состоянии: EditorBridge изменил редактор, но текст модуля не совпал с ожидаемым за окно верификации."
                    .to_string(),
            ),
            BridgePasteTarget::Selection(_) => Err(
                "Вставка не подтвердилась: текст модуля в 1С не изменился за окно верификации после replace_selection."
                    .to_string(),
            ),
            BridgePasteTarget::CurrentMethod(_) => {
                if after_module_hash != before_module_hash {
                    crate::app_log!(
                        "[1C] Bridge verification fallback accepted after timeout: module hash changed for current_method target"
                    );
                    Ok(())
                } else {
                    Err(
                        "Вставка не подтвердилась: код текущей процедуры в 1С не изменился за окно верификации."
                            .to_string(),
                    )
                }
            }
            BridgePasteTarget::InsertBeforeCurrentMethod(_) => {
                if after_module_hash != before_module_hash {
                    crate::app_log!(
                        "[1C] Bridge verification fallback accepted after timeout: module hash changed for insert_before_current_method target"
                    );
                    Ok(())
                } else {
                    Err(
                        "Вставка не подтвердилась: модуль в 1С не изменился за окно верификации после вставки перед процедурой."
                            .to_string(),
                    )
                }
            }
        }
    } else {
        Err(format!(
            "Не удалось проверить результат вставки через EditorBridge после {} попыток: {}",
            BRIDGE_VERIFY_ATTEMPTS,
            last_error.unwrap_or_else(|| "неизвестная ошибка верификации".to_string())
        ))
    }
}

#[tauri::command]
pub fn get_code_from_configurator(
    app_handle: AppHandle,
    hwnd: isize,
    use_select_all: Option<bool>,
    skip_focus_restore: Option<bool>,
) -> Result<String, String> {
    crate::app_log!(
        "[1C] get_code (HWND: {}, select_all: {:?})",
        hwnd,
        use_select_all
    );
    #[cfg(windows)]
    {
        use crate::configurator;
        use windows::Win32::Foundation::HWND;

        let select_all = use_select_all.unwrap_or(false);
        let parent = HWND(hwnd as *mut std::ffi::c_void);

        match if select_all {
            crate::editor_bridge::get_text(hwnd)
        } else {
            match crate::editor_bridge::get_selection(hwnd) {
                Ok((true, selection_text)) => Ok(selection_text),
                Ok((false, _)) => crate::editor_bridge::get_text(hwnd),
                Err(error) => Err(error),
            }
        } {
            Ok(result) => {
                crate::app_log!("[1C] EditorBridge found - reading code via UIAutomation");
                return Ok(result);
            }
            Err(error) => {
                crate::app_log!("[1C] EditorBridge unavailable - {}", error);
            }
        }

        if let Some(sci) = crate::scintilla::find_scintilla_control(parent) {
            crate::app_log!("[1C] Scintilla found - reading via SCI_GETTEXT/SCI_GETSELTEXT");
            let result = if select_all || !crate::scintilla::sci_has_selection(sci) {
                crate::scintilla::sci_get_text(sci)
            } else {
                crate::scintilla::sci_get_seltext(sci)
            };
            return result;
        }

        crate::app_log!("[1C] Scintilla not found - falling back to clipboard");
        let result = configurator::get_selected_code(hwnd, select_all);
        if !skip_focus_restore.unwrap_or(false) {
            restore_focus_to_app(&app_handle);
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        let _ = use_select_all;
        let _ = skip_focus_restore;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn get_active_fragment_cmd(
    app_handle: AppHandle,
    hwnd: isize,
    skip_focus_restore: Option<bool>,
) -> Result<String, String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        use windows::Win32::Foundation::HWND;

        let parent = HWND(hwnd as *mut std::ffi::c_void);

        match crate::editor_bridge::get_selection(hwnd) {
            Ok((true, selection_text)) => {
                crate::app_log!("[1C] EditorBridge found - returning active selection");
                return Ok(selection_text);
            }
            Ok((false, _)) => {}
            Err(error) => {
                crate::app_log!("[1C] EditorBridge selection unavailable - {}", error);
            }
        }

        match crate::editor_bridge::get_current_method(hwnd) {
            Ok(method) => {
                crate::app_log!("[1C] EditorBridge found - returning current method");
                return Ok(method.text);
            }
            Err(error) => {
                crate::app_log!("[1C] EditorBridge current method unavailable - {}", error);
            }
        }

        match crate::editor_bridge::get_text(hwnd) {
            Ok(module_text) => {
                crate::app_log!("[1C] EditorBridge found - falling back to full module");
                return Ok(module_text);
            }
            Err(error) => {
                crate::app_log!("[1C] EditorBridge full module unavailable - {}", error);
            }
        }

        if let Some(sci) = crate::scintilla::find_scintilla_control(parent) {
            crate::app_log!("[1C] Scintilla found - reading active fragment");
            return crate::scintilla::sci_get_active_fragment(sci);
        }

        crate::app_log!("[1C] Scintilla not found - falling back to clipboard for active fragment");
        let result = configurator::get_active_fragment(hwnd);
        if !skip_focus_restore.unwrap_or(false) {
            restore_focus_to_app(&app_handle);
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        let _ = skip_focus_restore;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub async fn paste_code_to_configurator<R: Runtime>(
    app_handle: AppHandle<R>,
    hwnd: isize,
    code: String,
    use_select_all: Option<bool>,
    original_content: Option<String>,
    action: Option<String>,
    write_intent: Option<String>,
    caret_line: Option<i32>,
    method_start_line: Option<i32>,
    method_name: Option<String>,
    runtime_id: Option<String>,
    force_legacy_apply: Option<bool>,
) -> Result<(), String> {
    crate::app_log!("[1C] paste_code (HWND: {}, len: {})", hwnd, code.len());
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;
        use windows::Win32::Foundation::HWND;

        let select_all = use_select_all.unwrap_or(false);
        let force_legacy_apply = force_legacy_apply.unwrap_or(false);
        let requested_action = crate::semantic_bridge::parse_action_kind(action.as_deref())?;
        let requested_intent = crate::semantic_bridge::parse_write_intent(write_intent.as_deref())?;
        let quick_action_apply_policy = resolve_quick_action_apply_policy(
            crate::settings::load_settings()
                .configurator
                .editor_bridge_enabled,
            requested_action,
            requested_intent,
        );
        let requested_hints = crate::editor_bridge::EditorMethodHints {
            caret_line,
            method_start_line,
            method_name,
            runtime_id,
        };
        let resolved_intent = crate::semantic_bridge::infer_write_intent(
            requested_intent,
            requested_action,
            crate::semantic_bridge::ResolverContext {
                has_selection: false,
                has_current_method: original_content
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                    || hints_contain_semantic_method_identity(&requested_hints),
                prefer_full_module: select_all,
            },
        );
        let can_try_hint_based_bridge = can_attempt_hint_based_bridge_semantic_write(
            resolved_intent,
            original_content.as_deref(),
            &requested_hints,
        );
        let parent = HWND(hwnd as *mut std::ffi::c_void);
        let mut bridge_context_error: Option<String> = None;
        let mut bridge_write_failure: Option<String> = None;
        let mut legacy_semantic_failure: Option<String> = None;

        let bridge_result: Option<Result<(Option<String>, Result<(), String>), String>> =
            if force_legacy_apply
                || matches!(
                    quick_action_apply_policy,
                    QuickActionApplyPolicy::ForceLegacy
                )
            {
                if force_legacy_apply {
                    crate::app_log!(
                    "[1C] Apply requested with forceLegacyApply=true - skipping EditorBridge and using legacy clipboard writer"
                );
                } else {
                    crate::app_log!(
                    "[1C] Quick action apply: editor bridge disabled in settings, forcing legacy clipboard writer"
                );
                }
                None
            } else {
                match crate::editor_bridge::get_editor_context(hwnd) {
                    Ok(ctx) => {
                        crate::app_log!(
                        "[1C] EditorBridge found - applying semantic write (requested_hwnd={}, bridge_hwnd={}, title='{}')",
                        hwnd,
                        ctx.conf_hwnd,
                        ctx.window_title
                    );
                        let bridge_hwnd = ctx.conf_hwnd as isize;

                        let target = choose_bridge_paste_target(
                            &ctx,
                            requested_intent,
                            requested_action,
                            select_all,
                            original_content.as_deref(),
                        )?;
                        let current_target = match &target {
                            BridgePasteTarget::FullModule(text)
                            | BridgePasteTarget::Selection(text)
                            | BridgePasteTarget::CurrentMethod(text)
                            | BridgePasteTarget::InsertBeforeCurrentMethod(text) => *text,
                        };
                        let current_target_hash =
                            crate::configurator::calculate_content_hash(current_target);
                        let requested_hash = crate::configurator::calculate_content_hash(&code);

                        crate::app_log!(
                        "[1C] Bridge target selected: target={}, target_hash={}, requested_hash={}, has_selection={}, current_method={}",
                        bridge_paste_target_name(&target),
                        current_target_hash,
                        requested_hash,
                        ctx.has_selection,
                        ctx.current_method_name.as_deref().unwrap_or("<none>")
                    );

                        if let Some(capabilities) = ctx.capabilities.as_ref() {
                            crate::app_log!(
                            "[1C] Bridge capabilities: read_mode={}, write_mode={}, replace_selection={}, replace_current_method={}, insert_before_method={}, replace_module={}",
                            capabilities.read_mode,
                            capabilities.write_mode,
                            capabilities.can_replace_selection,
                            capabilities.can_replace_current_method,
                            capabilities.can_insert_before_method,
                            capabilities.can_replace_module
                        );
                        }

                        if !bridge_supports_target(&ctx, &target) {
                            crate::app_log!(
                            "[1C] EditorBridge reports write unsupported for target={} - falling through to Scintilla/legacy fallback",
                            bridge_paste_target_name(&target)
                        );
                            let reason = bridge_capability_reason(&ctx).unwrap_or_else(|| {
                                format!(
                                    "EditorBridge не подтвердил semantic write для target={}.",
                                    bridge_paste_target_name(&target)
                                )
                            });
                            crate::app_log!("[1C] EditorBridge capability reason: {}", reason);
                            bridge_context_error = Some(reason);
                            None
                        } else {
                            let write_result = match ensure_content_not_changed(
                                original_content.as_deref(),
                                current_target,
                            ) {
                                Ok(()) => {
                                    crate::app_log!(
                                    "[1C] EditorBridge context resolved - using bridge semantic writer as the primary apply path"
                                );
                                    match &target {
                                        BridgePasteTarget::FullModule(_) => {
                                            let result = crate::editor_bridge::replace_module(
                                                bridge_hwnd,
                                                &code,
                                            );
                                            if let Ok(write_result) = &result {
                                                log_bridge_write_result(write_result);
                                                verify_bridge_write(
                                                    bridge_hwnd,
                                                    &ctx,
                                                    &target,
                                                    &code,
                                                    Some(write_result),
                                                )?;
                                            }
                                            result.map(|_| ())
                                        }
                                        BridgePasteTarget::Selection(_) => {
                                            let result = crate::editor_bridge::replace_selection(
                                                bridge_hwnd,
                                                &code,
                                            );
                                            if let Ok(write_result) = &result {
                                                log_bridge_write_result(write_result);
                                                verify_bridge_write(
                                                    bridge_hwnd,
                                                    &ctx,
                                                    &target,
                                                    &code,
                                                    Some(write_result),
                                                )?;
                                            }
                                            result.map(|_| ())
                                        }
                                        BridgePasteTarget::CurrentMethod(_) => {
                                            let hints = bridge_method_hints_with_fallback(
                                                &ctx,
                                                &requested_hints,
                                            );
                                            let result =
                                                crate::editor_bridge::replace_current_method(
                                                    bridge_hwnd,
                                                    &code,
                                                    Some(&hints),
                                                );
                                            if let Ok(write_result) = &result {
                                                log_bridge_write_result(write_result);
                                                verify_bridge_write(
                                                    bridge_hwnd,
                                                    &ctx,
                                                    &target,
                                                    &code,
                                                    Some(write_result),
                                                )?;
                                            }
                                            result.map(|_| ())
                                        }
                                        BridgePasteTarget::InsertBeforeCurrentMethod(_) => {
                                            let hints = bridge_method_hints_with_fallback(
                                                &ctx,
                                                &requested_hints,
                                            );
                                            let result = crate::editor_bridge::insert_before_method(
                                                bridge_hwnd,
                                                &code,
                                                Some(&hints),
                                            );
                                            if let Ok(write_result) = &result {
                                                log_bridge_write_result(write_result);
                                                verify_bridge_write(
                                                    bridge_hwnd,
                                                    &ctx,
                                                    &target,
                                                    &code,
                                                    Some(write_result),
                                                )?;
                                            }
                                            result.map(|_| ())
                                        }
                                    }
                                }
                                Err(error) => Err(error),
                            };

                            Some(Ok((Some(ctx.module_text.clone()), write_result)))
                        }
                    }
                    Err(error) => {
                        crate::app_log!("[1C] EditorBridge unavailable for paste - {}", error);
                        bridge_context_error = Some(format!("EditorBridge недоступен: {}", error));
                        None
                    }
                }
            };

        if let Some(bridge_res) = bridge_result {
            let (snapshot, write_result) = bridge_res?;
            if write_result.is_ok() {
                if let Some(current_code) = snapshot {
                    history_manager::save_snapshot(hwnd, current_code).await;
                }
                let _ = app_handle.emit("RESET_DIFF", code.clone());
                return write_result;
            }
            // Bridge write failed — log and fall through to Scintilla/clipboard
            if let Err(ref e) = write_result {
                crate::app_log!(
                    "[1C] Bridge write failed, falling through to clipboard: {}",
                    e
                );
                bridge_write_failure = Some(e.clone());
            }
        }

        let scintilla = if !force_legacy_apply
            && matches!(quick_action_apply_policy, QuickActionApplyPolicy::Default)
        {
            crate::scintilla::find_scintilla_control(parent).map(|sci| sci.0 as isize)
        } else {
            None
        };

        if bridge_write_failure.is_none()
            && scintilla.is_none()
            && can_try_hint_based_bridge
            && bridge_context_error.is_some()
            && !force_legacy_apply
            && !matches!(
                quick_action_apply_policy,
                QuickActionApplyPolicy::ForceLegacy
            )
        {
            let semantic_intent = resolved_intent.expect("checked by can_try_hint_based_bridge");
            crate::app_log!(
                "[1C] Trying hint-based EditorBridge semantic write fallback with preserved method hints"
            );

            let snapshot_code = crate::editor_bridge::get_text(hwnd).ok();
            match execute_hint_based_bridge_semantic_write(
                hwnd,
                &code,
                semantic_intent,
                &requested_hints,
            ) {
                Ok(write_result) => {
                    log_bridge_write_result(&write_result);
                    if let Some(current_code) = snapshot_code {
                        history_manager::save_snapshot(hwnd, current_code).await;
                    }
                    let _ = app_handle.emit("RESET_DIFF", code.clone());
                    return Ok(());
                }
                Err(error) => {
                    crate::app_log!(
                        "[1C] Hint-based EditorBridge semantic write fallback failed: {}",
                        error
                    );
                    bridge_write_failure = Some(error);
                }
            }
        }

        let scintilla_result: Option<Result<(Option<String>, Result<(), String>), String>> =
            if let Some(sci_raw) = scintilla {
                let sci = HWND(sci_raw as *mut std::ffi::c_void);
                crate::app_log!("[1C] Scintilla found - evaluating semantic write fallback");

                let snapshot_and_check: Result<(Option<String>, Result<(), String>), String> =
                    match crate::scintilla::sci_get_text(sci) {
                        Ok(current_code) => {
                            let inferred_intent = crate::semantic_bridge::infer_write_intent(
                                requested_intent,
                                requested_action,
                                crate::semantic_bridge::ResolverContext {
                                    has_selection: crate::scintilla::sci_has_selection(sci),
                                    has_current_method:
                                        crate::scintilla::sci_get_active_fragment_info(sci)
                                            .ok()
                                            .flatten()
                                            .is_some(),
                                    prefer_full_module: select_all,
                                },
                            );

                            let write_result = match inferred_intent {
                                Some(crate::semantic_bridge::SemanticWriteIntent::ReplaceSelection) => {
                                    let selection_text = crate::scintilla::sci_get_seltext(sci)?;
                                    ensure_content_not_changed(
                                        original_content.as_deref(),
                                        &selection_text,
                                    )?;
                                    crate::scintilla::sci_replace_sel(sci, &code)
                                }
                                Some(crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod) => {
                                    let fragment = crate::scintilla::sci_get_active_fragment_info(sci)?
                                        .ok_or_else(|| "Не удалось определить текущую процедуру или функцию.".to_string())?;
                                    ensure_content_not_changed(
                                        original_content.as_deref(),
                                        &fragment.text,
                                    )?;
                                    let updated_module = crate::semantic_bridge::replace_method_in_module(
                                        &current_code,
                                        fragment.start_line,
                                        fragment.end_line,
                                        &code,
                                    )?;
                                    crate::scintilla::sci_set_text(sci, &updated_module)
                                }
                                Some(crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod) => {
                                    let fragment = crate::scintilla::sci_get_active_fragment_info(sci)?
                                        .ok_or_else(|| "Не удалось определить текущую процедуру для вставки описания.".to_string())?;
                                    ensure_content_not_changed(
                                        original_content.as_deref(),
                                        &fragment.text,
                                    )?;
                                    let updated_module = crate::semantic_bridge::insert_before_method_in_module(
                                        &current_code,
                                        fragment.start_line,
                                        &code,
                                    )?;
                                    crate::scintilla::sci_set_text(sci, &updated_module)
                                }
                                Some(crate::semantic_bridge::SemanticWriteIntent::ReplaceModule) => {
                                    ensure_content_not_changed(
                                        original_content.as_deref(),
                                        &current_code,
                                    )?;
                                    crate::scintilla::sci_set_text(sci, &code)
                                }
                                None => Err("Для этого действия запись в редактор не требуется.".to_string()),
                            };

                            Ok((Some(current_code), write_result))
                        }
                        Err(error) => Err(error),
                    };

                Some(snapshot_and_check)
            } else {
                None
            };

        if let Some(sci_res) = scintilla_result {
            let (snapshot, write_result) = sci_res?;
            if let Some(current_code) = snapshot {
                history_manager::save_snapshot(hwnd, current_code).await;
            }
            if write_result.is_ok() {
                let _ = app_handle.emit("RESET_DIFF", code.clone());
            }
            return write_result;
        }

        if let Some(intent) = resolved_intent {
            if matches!(
                intent,
                crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod
                    | crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod
            ) {
                crate::app_log!(
                    "[1C] Trying legacy semantic clipboard fallback with reconstructed full-module rewrite"
                );

                match try_legacy_semantic_clipboard_rewrite(
                    hwnd,
                    &code,
                    intent,
                    original_content.as_deref(),
                    &requested_hints,
                ) {
                    Ok(Some((snapshot_code, updated_module))) => {
                        let paste_result = configurator::paste_code(hwnd, &updated_module, true);
                        history_manager::save_snapshot(hwnd, snapshot_code).await;
                        if paste_result.is_ok() {
                            let _ = app_handle.emit("RESET_DIFF", code.clone());
                        }
                        return paste_result;
                    }
                    Ok(None) => {
                        crate::app_log!(
                            "[1C] Legacy semantic clipboard fallback skipped: insufficient method hints or captured method text"
                        );
                    }
                    Err(error) => {
                        crate::app_log!(
                            "[1C] Legacy semantic clipboard fallback failed: {}",
                            error
                        );
                        legacy_semantic_failure = Some(error);
                    }
                }
            }
        }

        if !force_legacy_apply
            && matches!(
                quick_action_apply_policy,
                QuickActionApplyPolicy::ForceSemantic
            )
        {
            return Err(legacy_semantic_failure
                .or(bridge_write_failure)
                .or(bridge_context_error)
                .unwrap_or_else(|| {
                    "Семантическое применение для быстрого действия сейчас недоступно. Проверьте доступность EditorBridge/Scintilla или отключите опцию быстрых действий в 1С Конфигураторе.".to_string()
                }));
        }

        crate::app_log!("[1C] Scintilla not found - falling back to clipboard");

        let clipboard_intent = crate::semantic_bridge::infer_write_intent(
            requested_intent,
            requested_action,
            crate::semantic_bridge::ResolverContext {
                has_selection: false,
                has_current_method: false,
                prefer_full_module: select_all,
            },
        );

        if !force_legacy_apply
            && matches!(
                clipboard_intent,
                Some(crate::semantic_bridge::SemanticWriteIntent::ReplaceCurrentMethod)
                    | Some(crate::semantic_bridge::SemanticWriteIntent::InsertBeforeCurrentMethod)
            )
        {
            return Err(legacy_semantic_failure
                .or(bridge_write_failure)
                .or(bridge_context_error)
                .unwrap_or_else(|| {
                    "Семантическое применение к текущей процедуре сейчас недоступно без EditorBridge или Scintilla. Используйте \"Диф\"."
                        .to_string()
                }));
        }

        let snapshot_code = configurator::get_selected_code(hwnd, select_all).ok();
        if !force_legacy_apply {
            if let Some(ref current_code) = snapshot_code {
                ensure_content_not_changed(original_content.as_deref(), current_code)?;
            }
        }

        let paste_result = configurator::paste_code(hwnd, &code, select_all);

        if let Some(current_code) = snapshot_code {
            history_manager::save_snapshot(hwnd, current_code).await;
        }

        if paste_result.is_ok() {
            let _ = app_handle.emit("RESET_DIFF", code);
        }

        paste_result
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        let _ = code;
        let _ = use_select_all;
        let _ = original_content;
        let _ = action;
        let _ = write_intent;
        let _ = caret_line;
        let _ = method_start_line;
        let _ = method_name;
        let _ = runtime_id;
        let _ = force_legacy_apply;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn align_with_configurator(app_handle: AppHandle, hwnd: isize) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        let ai_window = app_handle
            .get_webview_window("main")
            .ok_or("Main window not found")?;
        let ai_hwnd = ai_window.hwnd().map_err(|e| e.to_string())?;

        configurator::align_windows(hwnd, ai_hwnd.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub async fn undo_last_change(hwnd: isize) -> Result<(), String> {
    crate::app_log!("[1C] undo_last_change (HWND: {})", hwnd);
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;

        if let Some(snapshot) = history_manager::pop_snapshot(hwnd).await {
            match crate::editor_bridge::set_text(hwnd, &snapshot.original_code) {
                Ok(write_result) => {
                    log_bridge_write_result(&write_result);
                    Ok(())
                }
                Err(error) => {
                    crate::app_log!("[1C] EditorBridge unavailable for undo - {}", error);
                    configurator::paste_code(hwnd, &snapshot.original_code, true)
                }
            }
        } else {
            Err("No history for this window".to_string())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn probe_scintilla(hwnd: isize) -> Result<String, String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        let parent = HWND(hwnd as *mut std::ffi::c_void);
        match crate::scintilla::find_scintilla_control(parent) {
            Some(sci) => Ok(format!("Scintilla found: HWND={}", sci.0 as isize)),
            None => Err("Scintilla control not found in child windows".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Windows only".to_string())
    }
}

#[tauri::command]
pub fn set_configurator_rdp_mode(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::set_rdp_mode(enabled);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

#[tauri::command]
pub fn set_configurator_editor_bridge_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::mouse_hook::set_editor_bridge_enabled(enabled);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EditorBridgeStatus {
    pub bridge: bool,
}

/// Check whether EditorBridge.exe is available.
/// EditorBridge is a self-contained single-file exe — no .NET installation required.
#[tauri::command]
pub fn check_editor_bridge_status<R: Runtime>(_app: AppHandle<R>) -> EditorBridgeStatus {
    let bridge = crate::editor_bridge::find_exe()
        .map(|path| path.exists())
        .unwrap_or(false);

    EditorBridgeStatus { bridge }
}

#[tauri::command]
pub fn restart_editor_bridge_cmd() -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::editor_bridge::restart()
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Download EditorBridge.exe from GitHub releases and save path to settings.
/// Returns absolute path to the downloaded exe.
#[tauri::command]
pub async fn install_editor_bridge_cmd(app: tauri::AppHandle) -> Result<String, String> {
    crate::editor_bridge_installer::download_editor_bridge(app).await
}

/// Feature #6: Return BSL insertion context at the current cursor position.
#[tauri::command]
pub fn get_insertion_context_cmd(hwnd: isize) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    {
        crate::editor_bridge::get_insertion_context(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Feature #6: Insert text before the given line in the BSL module.
#[tauri::command]
pub fn insert_at_line_cmd(
    hwnd: isize,
    line: i64,
    text: String,
) -> Result<crate::editor_bridge::EditorWriteResult, String> {
    #[cfg(windows)]
    {
        crate::editor_bridge::insert_at_line(hwnd, line, &text)
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, line, text);
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Feature #6: Append text after the last КонецПроцедуры/КонецФункции in the BSL module.
#[tauri::command]
pub fn append_to_module_cmd(
    hwnd: isize,
    text: String,
) -> Result<crate::editor_bridge::EditorWriteResult, String> {
    #[cfg(windows)]
    {
        crate::editor_bridge::append_to_module(hwnd, &text)
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, text);
        Err("Configurator integration is only available on Windows".to_string())
    }
}

#[tauri::command]
pub fn send_hotkey_cmd(hwnd: isize, key: u16, modifiers: Vec<u16>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::send_hotkey(hwnd, key, modifiers);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let _ = key;
        let _ = modifiers;
        Err("Hotkeys are only available on Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::build_legacy_semantic_clipboard_module;
    use super::{resolve_quick_action_apply_policy, QuickActionApplyPolicy};
    #[cfg(windows)]
    use crate::editor_bridge::EditorMethodHints;
    use crate::semantic_bridge::{QuickActionKind, SemanticWriteIntent};

    #[test]
    fn quick_action_policy_uses_semantic_apply_when_bridge_enabled() {
        assert_eq!(
            resolve_quick_action_apply_policy(
                true,
                Some(QuickActionKind::Fix),
                Some(SemanticWriteIntent::ReplaceCurrentMethod),
            ),
            QuickActionApplyPolicy::ForceSemantic
        );
    }

    #[test]
    fn quick_action_policy_uses_legacy_apply_when_bridge_disabled() {
        assert_eq!(
            resolve_quick_action_apply_policy(
                false,
                Some(QuickActionKind::Elaborate),
                Some(SemanticWriteIntent::ReplaceModule),
            ),
            QuickActionApplyPolicy::ForceLegacy
        );
    }

    #[test]
    fn quick_action_policy_keeps_default_for_regular_apply() {
        assert_eq!(
            resolve_quick_action_apply_policy(true, None, None),
            QuickActionApplyPolicy::Default
        );
    }

    #[test]
    #[cfg(windows)]
    fn legacy_semantic_clipboard_module_rewrites_method_from_capture() {
        let hints = EditorMethodHints {
            caret_line: Some(2),
            method_start_line: Some(2),
            method_name: Some("Тест".to_string()),
            runtime_id: None,
        };
        let current_module =
            "Перем X;\r\n\r\nПроцедура Тест()\r\n    Сообщить(1);\r\nКонецПроцедуры\r\n";

        let updated = build_legacy_semantic_clipboard_module(
            current_module,
            "Процедура Тест()\n    Сообщить(2);\nКонецПроцедуры",
            SemanticWriteIntent::ReplaceCurrentMethod,
            Some("Процедура Тест()\n    Сообщить(1);\nКонецПроцедуры"),
            &hints,
        )
        .unwrap()
        .expect("legacy module should be rebuilt");

        assert_eq!(
            updated,
            "Перем X;\r\n\r\nПроцедура Тест()\r\n    Сообщить(2);\r\nКонецПроцедуры"
        );
    }

    #[test]
    #[cfg(windows)]
    fn legacy_semantic_clipboard_module_inserts_unicode_comment_before_method() {
        let hints = EditorMethodHints {
            caret_line: Some(2),
            method_start_line: Some(2),
            method_name: Some("Тест".to_string()),
            runtime_id: None,
        };
        let current_module =
            "Перем X;\n\nПроцедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры\n";

        let updated = build_legacy_semantic_clipboard_module(
            current_module,
            "// Формирует XML...",
            SemanticWriteIntent::InsertBeforeCurrentMethod,
            Some("Процедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры"),
            &hints,
        )
        .unwrap()
        .expect("legacy module should be rebuilt");

        assert_eq!(
            updated,
            "Перем X;\n\n// Формирует XML...\nПроцедура Тест()\n    Сообщить(\"Привет\");\nКонецПроцедуры\n"
        );
    }

    #[test]
    #[cfg(windows)]
    fn legacy_semantic_clipboard_module_requires_method_start_line() {
        let hints = EditorMethodHints {
            caret_line: Some(2),
            method_start_line: None,
            method_name: Some("Тест".to_string()),
            runtime_id: None,
        };

        let updated = build_legacy_semantic_clipboard_module(
            "Процедура Тест()\nКонецПроцедуры\n",
            "// Описание",
            SemanticWriteIntent::InsertBeforeCurrentMethod,
            Some("Процедура Тест()\nКонецПроцедуры"),
            &hints,
        )
        .unwrap();

        assert!(updated.is_none());
    }
}
