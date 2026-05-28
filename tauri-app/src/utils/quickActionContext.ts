import type { QuickActionAction, QuickActionCaptureScope } from '../types/quickActionSessions';

export interface QuickActionEditorContextSnapshot {
    available: boolean;
    has_selection: boolean;
    selection_text: string;
    current_method_name: string | null;
    current_method_text: string | null;
    module_text: string;
    caret_line: number;
    method_start_line: number | null;
    primary_runtime_id?: string | null;
}

export interface ResolvedQuickActionCapture {
    scope: QuickActionCaptureScope;
    promptCode: string;
    originalCode: string;
    useSelectAll: boolean;
    caretLine?: number;
    methodStartLine?: number | null;
    methodName?: string | null;
    runtimeId?: string | null;
    bslAnalysisCode?: string;
}

function hasText(value?: string | null): boolean {
    return (value ?? '').trim().length > 0;
}

export function shouldSyncQuickActionToClickTarget(
    action: QuickActionAction,
    hasSelection: boolean,
): boolean {
    if (action === 'describe') {
        return false;
    }

    return !hasSelection;
}

export function resolveCaptureFromEditorContext(
    ctx: QuickActionEditorContextSnapshot,
    action: QuickActionAction,
): ResolvedQuickActionCapture | null {
    if (!ctx.available) {
        return null;
    }

    const currentMethodText = hasText(ctx.current_method_text) ? ctx.current_method_text! : null;
    const moduleText = hasText(ctx.module_text) ? ctx.module_text : null;
    const selectionText = hasText(ctx.selection_text) ? ctx.selection_text : null;

    if (action === 'describe') {
        if (!currentMethodText) {
            return null;
        }

        return {
            scope: 'current_method',
            promptCode: currentMethodText,
            originalCode: currentMethodText,
            useSelectAll: false,
            caretLine: ctx.caret_line,
            methodStartLine: ctx.method_start_line,
            methodName: ctx.current_method_name,
            runtimeId: ctx.primary_runtime_id ?? null,
        };
    }

    if (ctx.has_selection && selectionText) {
        return {
            scope: 'selection',
            promptCode: selectionText,
            originalCode: selectionText,
            useSelectAll: false,
            runtimeId: ctx.primary_runtime_id ?? null,
            bslAnalysisCode: currentMethodText ?? moduleText ?? undefined,
        };
    }

    if (currentMethodText) {
        return {
            scope: 'current_method',
            promptCode: currentMethodText,
            originalCode: currentMethodText,
            useSelectAll: false,
            caretLine: ctx.caret_line,
            methodStartLine: ctx.method_start_line,
            methodName: ctx.current_method_name,
            runtimeId: ctx.primary_runtime_id ?? null,
        };
    }

    if (moduleText) {
        return {
            scope: 'module',
            promptCode: moduleText,
            originalCode: moduleText,
            useSelectAll: true,
            runtimeId: ctx.primary_runtime_id ?? null,
        };
    }

    return null;
}
