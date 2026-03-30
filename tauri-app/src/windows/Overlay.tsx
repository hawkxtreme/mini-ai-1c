/**
 * Overlay.tsx - AI context menu popup for 1C Configurator.
 *
 * The menu is intentionally styled closer to a native Windows context menu:
 * restrained chrome, aligned shortcuts, compact spacing and controlled height.
 */

import { Fragment, type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { VoiceInputControl } from '../components/voice/VoiceInputControl';

type Phase = 'menu' | 'input' | 'loading' | 'result';
type ResultType = 'comment' | 'diff' | 'explain_only';
type WriteIntent =
    | 'replace_selection'
    | 'replace_current_method'
    | 'insert_before_current_method'
    | 'replace_module';
type ActionId = 'describe' | 'elaborate' | 'fix' | 'explain' | 'review';

interface OverlayState {
    phase: Phase;
    action?: ActionId;
    resultType?: ResultType;
    preview?: string;
    resultCode?: string;
    diffContent?: string;
    confHwnd: number;
    originalCode?: string;
    useSelectAll: boolean;
    writeIntent?: WriteIntent;
    canApplyDirectly?: boolean;
    applyUnavailableReason?: string;
    preferredWriter?: string;
    caretLine?: number;
    methodStartLine?: number | null;
    methodName?: string | null;
    runtimeId?: string | null;
    targetX?: number | null;
    targetY?: number | null;
    targetChildHwnd?: number | null;
}

interface EditorContext {
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

function normalizeOptionalText(text?: string | null): string {
    return (text ?? '').replace(/\r\n/g, '\n').trim();
}

function sameOptionalString(left?: string | null, right?: string | null): boolean {
    const leftText = (left ?? '').trim();
    const rightText = (right ?? '').trim();
    if (!leftText || !rightText) {
        return true;
    }

    return leftText === rightText;
}

function buildApplyContextChangedState(current: OverlayState): OverlayState {
    return {
        ...current,
        phase: 'result',
        resultType: 'explain_only',
        canApplyDirectly: false,
        preview:
            'Контекст изменился, пока готовился результат. Откройте меню заново на нужной процедуре и повторите применение.',
    };
}

async function resolveFreshApplyPayload(current: OverlayState): Promise<{
    useSelectAll: boolean;
    originalCode: string;
    caretLine?: number | null;
    methodStartLine?: number | null;
    methodName?: string | null;
    runtimeId?: string | null;
}> {
    const ctx = await invoke<EditorContext>('get_editor_context_cmd', { hwnd: current.confHwnd });
    if (!ctx.available) {
        throw new Error('Контекст редактора 1С недоступен.');
    }

    if (current.writeIntent === 'replace_selection') {
        const selectionText = normalizeOptionalText(ctx.selection_text);
        if (!ctx.has_selection || !selectionText) {
            throw new Error('Выделение изменилось. Откройте меню заново и повторите применение.');
        }

        if (normalizeOptionalText(current.originalCode) !== selectionText) {
            throw new Error('Выделение изменилось. Откройте меню заново и повторите применение.');
        }

        return {
            useSelectAll: false,
            originalCode: ctx.selection_text,
            runtimeId: ctx.primary_runtime_id ?? current.runtimeId ?? null,
        };
    }

    if (current.writeIntent === 'replace_module') {
        const moduleText = normalizeOptionalText(ctx.module_text);
        if (!moduleText) {
            throw new Error('Текст модуля недоступен. Откройте меню заново и повторите применение.');
        }

        if (normalizeOptionalText(current.originalCode) !== moduleText) {
            throw new Error('Модуль изменился. Откройте меню заново и повторите применение.');
        }

        return {
            useSelectAll: true,
            originalCode: ctx.module_text,
            runtimeId: ctx.primary_runtime_id ?? current.runtimeId ?? null,
        };
    }

    if (!sameOptionalString(ctx.primary_runtime_id, current.runtimeId)) {
        throw new Error('Активный фрагмент изменился. Откройте меню заново и повторите применение.');
    }

    const originalMethodText = normalizeOptionalText(current.originalCode);
    const moduleText = normalizeOptionalText(ctx.module_text);
    if (!originalMethodText || !moduleText.includes(originalMethodText)) {
        throw new Error('Текст целевой процедуры изменился. Откройте меню заново и повторите применение.');
    }

    return {
        useSelectAll: false,
        originalCode: current.originalCode ?? '',
        caretLine: current.caretLine ?? ctx.caret_line,
        methodStartLine: current.methodStartLine ?? ctx.method_start_line ?? null,
        methodName: current.methodName ?? ctx.current_method_name ?? null,
        runtimeId: current.runtimeId ?? ctx.primary_runtime_id ?? null,
    };
}

function IconComment() {
    return (
        <svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <path d="M3.5 4.25a.75.75 0 0 1 .75-.75h7.5a.75.75 0 1 1 0 1.5h-7.5a.75.75 0 0 1-.75-.75Z" fill="currentColor" />
            <path d="M3.5 8a.75.75 0 0 1 .75-.75h5.25a.75.75 0 1 1 0 1.5H4.25A.75.75 0 0 1 3.5 8Z" fill="currentColor" />
            <path d="M3.5 11.75a.75.75 0 0 1 .75-.75H10a.75.75 0 1 1 0 1.5H4.25a.75.75 0 0 1-.75-.75Z" fill="currentColor" />
            <path d="M13 2.75A1.75 1.75 0 0 0 11.25 1h-8.5A1.75 1.75 0 0 0 1 2.75v8.5C1 12.216 1.784 13 2.75 13H4.5v2l2.372-2H11.25A1.75 1.75 0 0 0 13 11.25v-8.5Zm-1.5 8.5a.25.25 0 0 1-.25.25H6.25L6 11.72 5.28 11H2.75a.25.25 0 0 1-.25-.25v-8.5A.25.25 0 0 1 2.75 2h8.5a.25.25 0 0 1 .25.25v8.5Z" fill="currentColor" />
        </svg>
    );
}

function IconPencil() {
    return (
        <svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <path d="m11.84 1.97 2.19 2.19a1.5 1.5 0 0 1 0 2.12l-7.8 7.8L3 14.75l.67-3.23 7.8-7.8a1.5 1.5 0 0 1 2.12 0Zm-7.1 10.1-.28 1.36 1.36-.28 6.94-6.94-1.08-1.08-6.94 6.94Z" fill="currentColor" />
        </svg>
    );
}

function IconFix() {
    return (
        <svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <path d="M8 1.25a6.75 6.75 0 1 0 0 13.5A6.75 6.75 0 0 0 8 1.25Zm0 12A5.25 5.25 0 1 1 8 2.75a5.25 5.25 0 0 1 0 10.5Z" fill="currentColor" />
            <path d="M8 4.25a.875.875 0 0 1 .875.875v3.25a.875.875 0 1 1-1.75 0v-3.25A.875.875 0 0 1 8 4.25Z" fill="currentColor" />
            <circle cx="8" cy="11.25" r="1" fill="currentColor" />
        </svg>
    );
}

function IconQuestion() {
    return (
        <svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <path d="M8 1.25a6.75 6.75 0 1 0 0 13.5A6.75 6.75 0 0 0 8 1.25Zm0 12A5.25 5.25 0 1 1 8 2.75a5.25 5.25 0 0 1 0 10.5Z" fill="currentColor" />
            <path d="M7.89 10.9a.95.95 0 1 0 0 1.9.95.95 0 0 0 0-1.9Zm-.5-1.62v-.37c0-.9.52-1.43 1.2-1.9.55-.38.92-.7.92-1.33 0-.74-.55-1.2-1.35-1.2-.73 0-1.25.32-1.62.93a.75.75 0 0 1-1.29-.77A3.24 3.24 0 0 1 8.18 3c1.7 0 2.94 1.01 2.94 2.58 0 1.28-.74 1.93-1.46 2.42-.55.38-.77.63-.77 1.13v.15a.75.75 0 0 1-1.5 0Z" fill="currentColor" />
        </svg>
    );
}

function IconReview() {
    return (
        <svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <path d="M7 2.25a4.75 4.75 0 1 0 2.98 8.45l3.16 3.15a.75.75 0 0 0 1.06-1.06l-3.15-3.16A4.75 4.75 0 0 0 7 2.25Zm0 8A3.25 3.25 0 1 1 7 3.75a3.25 3.25 0 0 1 0 6.5Z" fill="currentColor" />
        </svg>
    );
}

const ACTIONS: Array<{
    id: ActionId;
    icon: ReactNode;
    label: string;
    key: string | null;
    group: number;
}> = [
    { id: 'describe', icon: <IconComment />, label: 'Описание', key: 'F1', group: 0 },
    { id: 'elaborate', icon: <IconPencil />, label: 'Доработать...', key: 'F2', group: 0 },
    { id: 'fix', icon: <IconFix />, label: 'Исправить', key: 'F3', group: 1 },
    { id: 'explain', icon: <IconQuestion />, label: 'Объяснить', key: null, group: 2 },
    { id: 'review', icon: <IconReview />, label: 'Ревью кода', key: null, group: 2 },
];

const ACTION_LABEL: Record<ActionId, string> = {
    describe: 'Генерирую описание',
    elaborate: 'Дорабатываю код',
    fix: 'Исправляю ошибки',
    explain: 'Объясняю код',
    review: 'Провожу ревью',
};

const KEY_TO_ACTION: Partial<Record<string, ActionId>> = {
    F1: 'describe',
    F2: 'elaborate',
    F3: 'fix',
};

const MENU_WIDTH = 316;
const MENU_MAX_HEIGHT = 420;
const MENU_LIST_MAX_HEIGHT = 348;
const INPUT_WIDTH = 396;
const INPUT_MIN_HEIGHT = 306;
const INPUT_MAX_HEIGHT = 380;
const LOADING_WIDTH = 404;
const LOADING_HEIGHT = 168;
const RESULT_WIDTH = 500;
const EXPLAIN_RESULT_WIDTH = 548;
const RESULT_MAX_HEIGHT = 472;
const EXPLAIN_RESULT_MAX_HEIGHT = 520;

let runtimeStarted = false;
let runtimeApplyState: ((state: OverlayState) => void) | null = null;
let runtimeFocusBound = false;

async function refreshRuntimePendingState() {
    const pendingState = await invoke<OverlayState | null>('get_pending_overlay_state');
    if (pendingState) {
        runtimeApplyState?.(pendingState);
    }
}

async function startOverlayRuntime() {
    if (runtimeStarted) {
        return;
    }

    runtimeStarted = true;

    await listen<OverlayState>('overlay-state', (event) => {
        runtimeApplyState?.(event.payload);
    });

    if (!runtimeFocusBound) {
        runtimeFocusBound = true;
        window.addEventListener('focus', () => {
            void refreshRuntimePendingState();
        });
    }

    await invoke('overlay_ready');
    await refreshRuntimePendingState();
}

function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
}

function resizeMenu(root: HTMLDivElement) {
    const header = root.querySelector('.overlay-menu-header') as HTMLElement | null;
    const list = root.querySelector('.overlay-actions-list') as HTMLElement | null;
    if (!list) return;

    const visibleListHeight = Math.min(list.scrollHeight, MENU_LIST_MAX_HEIGHT);
    const totalHeight =
        (header?.offsetHeight ?? 40) +
        visibleListHeight +
        8;

    void invoke('resize_overlay', {
        width: MENU_WIDTH,
        height: clamp(totalHeight, 228, MENU_MAX_HEIGHT),
    });
}

function resizeInput(root: HTMLDivElement) {
    const header = root.querySelector('.overlay-panel-header') as HTMLElement | null;
    const body = root.querySelector('.overlay-input-body') as HTMLElement | null;
    const buttons = root.querySelector('.overlay-btn-row') as HTMLElement | null;
    const textarea = root.querySelector('.overlay-textarea') as HTMLElement | null;

    const totalHeight = clamp(
        (header?.offsetHeight ?? 48) +
        Math.max(body?.scrollHeight ?? 0, textarea?.scrollHeight ?? 0) +
        (buttons?.offsetHeight ?? 44) +
        20,
        INPUT_MIN_HEIGHT,
        INPUT_MAX_HEIGHT,
    );

    void invoke('resize_overlay', {
        width: INPUT_WIDTH,
        height: totalHeight,
    });
}

function resizeResult(root: HTMLDivElement, explainOnly?: boolean) {
    const header = root.querySelector('.overlay-result-header') as HTMLElement | null;
    const previewShell = root.querySelector('.overlay-preview-shell') as HTMLElement | null;
    const preview = root.querySelector('.overlay-preview') as HTMLElement | null;
    const buttons = root.querySelector('.overlay-btn-row') as HTMLElement | null;

    const headerHeight = header?.offsetHeight ?? 48;
    const buttonsHeight = buttons?.offsetHeight ?? (explainOnly ? 52 : 92);
    const shellPadding = previewShell ? 24 : 0;
    const previewNaturalHeight = preview ? preview.scrollHeight : 0;
    const previewVisibleHeight = Math.min(previewNaturalHeight, explainOnly ? 360 : 252);
    const totalHeight = clamp(
        headerHeight + buttonsHeight + shellPadding + previewVisibleHeight + 18,
        explainOnly ? 236 : 324,
        explainOnly ? EXPLAIN_RESULT_MAX_HEIGHT : RESULT_MAX_HEIGHT,
    );

    void invoke('resize_overlay', {
        width: explainOnly ? EXPLAIN_RESULT_WIDTH : RESULT_WIDTH,
        height: totalHeight,
    });
}

function resizeForState(root: HTMLDivElement, nextState: OverlayState) {
    if (nextState.phase === 'menu') {
        resizeMenu(root);
        return;
    }

    if (nextState.phase === 'input') {
        resizeInput(root);
        return;
    }

    if (nextState.phase === 'loading') {
        void invoke('resize_overlay', { width: LOADING_WIDTH, height: LOADING_HEIGHT });
        return;
    }

    resizeResult(root, nextState.resultType === 'explain_only');
}

export function OverlayWindow() {
    const [state, setState] = useState<OverlayState | null>(null);
    const [inputText, setInputText] = useState('');
    const [applying, setApplying] = useState(false);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const rootRef = useRef<HTMLDivElement>(null);
    const phaseRef = useRef<Phase>('menu');
    const stateRef = useRef<OverlayState | null>(null);
    const applyingRef = useRef(false);

    const appendVoiceTaskText = useCallback((text: string) => {
        setInputText(prev => prev + (prev ? ' ' : '') + text);
    }, []);

    stateRef.current = state;
    applyingRef.current = applying;
    runtimeApplyState = applyIncomingState;
    if (!runtimeStarted) {
        void startOverlayRuntime();
    }

    function scheduleResize(nextState: OverlayState) {
        const run = (attempt: number) => {
            const root = rootRef.current;
            if (!root) {
                return;
            }

            resizeForState(root, nextState);

            if (attempt < 3) {
                requestAnimationFrame(() => run(attempt + 1));
            }
        };

        requestAnimationFrame(() => run(0));
    }

    function applyIncomingState(nextState: OverlayState) {
        setState(nextState);
        phaseRef.current = nextState.phase;
        scheduleResize(nextState);
        if (nextState.phase === 'input') {
            setTimeout(() => inputRef.current?.focus(), 40);
        }
    }

    useLayoutEffect(() => {
        const onKey = (event: KeyboardEvent) => {
            const phase = phaseRef.current;

            if (event.key === 'Escape') {
                event.preventDefault();
                void closeOverlay();
                return;
            }

            if (phase === 'menu' && KEY_TO_ACTION[event.key]) {
                event.preventDefault();
                void handleAction(KEY_TO_ACTION[event.key] as ActionId);
                return;
            }

            if (phase === 'result') {
                if (event.key === 'Enter' && !event.shiftKey) {
                    event.preventDefault();
                    void handleApply();
                    return;
                }

                if (event.key === 'd' || event.key === 'D') {
                    event.preventDefault();
                    void handleShowDiff();
                }
            }
        };

        const onBlur = () => {
            if (phaseRef.current === 'loading' || phaseRef.current === 'result') {
                return;
            }

            setTimeout(() => void closeOverlay(), 80);
        };

        window.addEventListener('keydown', onKey);
        window.addEventListener('blur', onBlur);

        return () => {
            window.removeEventListener('keydown', onKey);
            window.removeEventListener('blur', onBlur);
        };
    }, []);

    useEffect(() => {
        const root = rootRef.current;
        if (!root || !state) return;

        const frame = requestAnimationFrame(() => {
            resizeForState(root, state);
        });

        return () => cancelAnimationFrame(frame);
    }, [state]);

    async function closeOverlay(restoreFocus = false) {
        await invoke('hide_overlay', {
            confHwnd: stateRef.current?.confHwnd ?? null,
            restoreFocus,
        });
    }

    async function handleAction(actionId: ActionId) {
        const current = stateRef.current;
        if (!current) return;

        if (actionId === 'elaborate') {
            const nextState: OverlayState = { ...current, phase: 'input', action: 'elaborate' };
            setState(nextState);
            phaseRef.current = 'input';
            scheduleResize(nextState);
            await invoke('update_overlay_state', { state: nextState });
            return;
        }

        const nextState: OverlayState = { ...current, phase: 'loading', action: actionId };
        setState(nextState);
        phaseRef.current = 'loading';
        scheduleResize(nextState);
        await invoke('update_overlay_state', { state: nextState });
        await invoke('emit_to_main', {
            event: 'quick-action-from-overlay',
            payload: {
                action: actionId,
                confHwnd: current.confHwnd,
                targetX: current.targetX ?? null,
                targetY: current.targetY ?? null,
                targetChildHwnd: current.targetChildHwnd ?? null,
            },
        });
    }

    async function handleElaborateSubmit() {
        if (!inputText.trim()) return;

        const current = stateRef.current;
        if (!current) return;

        const nextState: OverlayState = { ...current, phase: 'loading', action: 'elaborate' };
        setState(nextState);
        phaseRef.current = 'loading';
        scheduleResize(nextState);
        await invoke('update_overlay_state', { state: nextState });
        await invoke('emit_to_main', {
            event: 'quick-action-from-overlay',
            payload: {
                action: 'elaborate',
                task: inputText,
                confHwnd: current.confHwnd,
                targetX: current.targetX ?? null,
                targetY: current.targetY ?? null,
                targetChildHwnd: current.targetChildHwnd ?? null,
            },
        });
        setInputText('');
    }

    async function handleApply() {
        const current = stateRef.current;
        if (
            !current?.resultCode ||
            !current.action ||
            !current.writeIntent ||
            applyingRef.current ||
            current.canApplyDirectly === false
        ) {
            return;
        }

        setApplying(true);
        try {
            await closeOverlay(true);
            await new Promise((resolve) => window.setTimeout(resolve, 140));
            const freshApply = await resolveFreshApplyPayload(current);
            await invoke('paste_code_to_configurator', {
                hwnd: current.confHwnd,
                code: current.resultCode,
                useSelectAll: freshApply.useSelectAll,
                originalContent: freshApply.originalCode ?? null,
                action: current.action,
                writeIntent: current.writeIntent,
                caretLine: freshApply.caretLine ?? null,
                methodStartLine: freshApply.methodStartLine ?? null,
                methodName: freshApply.methodName ?? null,
                runtimeId: freshApply.runtimeId ?? null,
            });
        } catch (error) {
            const message = String(error);
            const errorState = /изменил|изменилось|недоступен|не удалось/i.test(message)
                ? buildApplyContextChangedState(current)
                : {
                ...current,
                preview: `Ошибка вставки:\n${String(error)}`,
                resultType: 'explain_only' as const,
                phase: 'result' as const,
            };
            stateRef.current = errorState;
            phaseRef.current = 'result';
            setState(errorState);
            scheduleResize(errorState);
            await invoke('update_overlay_state', { state: errorState });
            await invoke('show_hidden_overlay');
        } finally {
            setApplying(false);
        }
    }

    async function handleShowDiff() {
        const current = stateRef.current;
        if (!current?.diffContent) return;

        await invoke('open_diff_from_overlay', {
            diffContent: current.diffContent,
            originalCode: current.originalCode ?? null,
            confHwnd: current.confHwnd,
            useSelectAll: current.useSelectAll,
        });
    }

    function renderMenu(disabled: boolean) {
        return (
            <div className="overlay-menu-shell">
                <div className="overlay-menu-header" data-tauri-drag-region>
                    <div className="overlay-brand">
                        <span className="overlay-brand-dot" />
                        <span className="overlay-brand-title">Mini AI 1C</span>
                    </div>

                    <button
                        type="button"
                        className="overlay-close-btn"
                        onClick={() => void closeOverlay()}
                        aria-label="Закрыть"
                    >
                        ×
                    </button>
                </div>

                <div className="overlay-menu-caption">
                    <div className="overlay-menu-title">Быстрые действия</div>
                </div>

                <div className="overlay-actions-list" role="menu">
                    {ACTIONS.map((action, index) => {
                        const showSeparator = index > 0 && action.group !== ACTIONS[index - 1].group;
                        return (
                            <Fragment key={action.id}>
                                {showSeparator && <div className="overlay-separator" />}
                                <button
                                    type="button"
                                    className="overlay-action-btn"
                                    onClick={() => void handleAction(action.id)}
                                    disabled={disabled}
                                >
                                    <span className="action-icon-frame">{action.icon}</span>
                                    <span className="action-label">{action.label}</span>
                                    {action.key && <span className="action-key">{action.key}</span>}
                                </button>
                            </Fragment>
                        );
                    })}
                </div>
            </div>
        );
    }

    if (!state) {
        return (
            <div className="overlay-root" ref={rootRef}>
                <div className="overlay-panel overlay-panel--loading overlay-panel--boot">
                    <div className="overlay-panel-header" data-tauri-drag-region>
                        <div>
                            <div className="overlay-panel-title">Mini AI 1C</div>
                            <div className="overlay-panel-subtitle">Подготовка меню</div>
                        </div>

                        <button
                            type="button"
                            className="overlay-close-btn"
                            onClick={() => void closeOverlay()}
                            aria-label="Закрыть"
                        >
                            ×
                        </button>
                    </div>

                    <div className="overlay-loading-body overlay-loading-body--boot">
                        <span className="spinner" />
                        <div className="overlay-loading-copy">
                            <div className="overlay-loading-title">Открываю быстрые действия...</div>
                            <div className="overlay-loading-subtitle">
                                Первый запуск может занять немного больше времени.
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="overlay-root" ref={rootRef}>
            {state.phase === 'menu' && renderMenu(false)}

            {state.phase === 'input' && (
                <div className="overlay-panel overlay-panel--input">
                    <div className="overlay-panel-header" data-tauri-drag-region>
                        <div>
                            <div className="overlay-panel-title">Доработать код</div>
                            <div className="overlay-panel-subtitle">Опишите, что нужно изменить</div>
                        </div>

                        <button
                            type="button"
                            className="overlay-close-btn"
                            onClick={() => void closeOverlay()}
                            aria-label="Закрыть"
                        >
                            ×
                        </button>
                    </div>

                    <div className="overlay-input-body">
                        <label className="overlay-input-label" htmlFor="overlay-task">
                            Задача для ассистента
                        </label>
                        <div className="overlay-textarea-shell">
                            <textarea
                            id="overlay-task"
                            ref={inputRef}
                            className="overlay-textarea"
                            value={inputText}
                            onChange={(event) => setInputText(event.target.value)}
                            placeholder="Например: добавь обработку ошибок и проверку параметров."
                            rows={4}
                            onKeyDown={(event) => {
                                if (event.key === 'Enter' && !event.shiftKey) {
                                    event.preventDefault();
                                    void handleElaborateSubmit();
                                }
                            }}
                        />
                            <div className="overlay-textarea-tools">
                                <VoiceInputControl
                                    onText={appendVoiceTaskText}
                                    selectedHwnd={state?.confHwnd ?? null}
                                    variant="overlay"
                                />
                            </div>
                        </div>
                    </div>

                    <div className="overlay-btn-row">
                        <button type="button" className="btn-primary" onClick={() => void handleElaborateSubmit()}>
                            Выполнить
                        </button>
                        <button type="button" className="btn-ghost" onClick={() => void closeOverlay()}>
                            Отмена
                        </button>
                    </div>
                </div>
            )}

            {state.phase === 'loading' && (
                <div className="overlay-panel overlay-panel--loading">
                    <div className="overlay-panel-header" data-tauri-drag-region>
                        <div>
                            <div className="overlay-panel-title">Mini AI 1C</div>
                            <div className="overlay-panel-subtitle">Обработка запроса</div>
                        </div>

                        <button
                            type="button"
                            className="overlay-close-btn"
                            onClick={() => void closeOverlay()}
                            aria-label="Закрыть"
                        >
                            ×
                        </button>
                    </div>

                    <div className="overlay-loading-body">
                        <span className="spinner" />
                        <div className="overlay-loading-copy">
                            <div className="overlay-loading-title">
                                {ACTION_LABEL[state.action ?? 'describe'] ?? 'Обрабатываю'}...
                            </div>
                            <div className="overlay-loading-subtitle">
                                Подготавливаю результат для активного фрагмента.
                            </div>
                        </div>
                    </div>

                    <div className="overlay-btn-row overlay-btn-row--compact">
                        <button type="button" className="btn-ghost" onClick={() => void closeOverlay()}>
                            Отменить
                        </button>
                    </div>
                </div>
            )}

            {state.phase === 'result' && (
                <div className="overlay-panel overlay-result">
                    <div className="overlay-result-header" data-tauri-drag-region>
                        <div className="overlay-result-title">{getResultTitle(state)}</div>
                        <button
                            type="button"
                            className="overlay-close-btn"
                            onClick={() => void closeOverlay()}
                            aria-label="Закрыть"
                        >
                            ×
                        </button>
                    </div>

                    {state.preview && (
                        <div className="overlay-preview-shell">
                            <pre className="overlay-preview">{state.preview}</pre>
                        </div>
                    )}

                    <div className="overlay-btn-row">
                        {state.resultType === 'explain_only' ? (
                            <button type="button" className="btn-primary" onClick={() => void closeOverlay()}>
                                Закрыть
                            </button>
                        ) : state.canApplyDirectly === false ? (
                            <>
                                {state.diffContent ? (
                                    <button type="button" className="btn-primary" onClick={() => void handleShowDiff()}>
                                        Открыть диф
                                    </button>
                                ) : (
                                    <button type="button" className="btn-primary" onClick={() => void closeOverlay()}>
                                        Закрыть
                                    </button>
                                )}
                                {state.diffContent && (
                                    <button type="button" className="btn-ghost" onClick={() => void closeOverlay()}>
                                        Позже
                                    </button>
                                )}
                            </>
                        ) : (
                            <>
                                <button
                                    type="button"
                                    className="btn-primary"
                                    onClick={() => void handleApply()}
                                    disabled={applying || !state.resultCode || !state.action || !state.writeIntent}
                                >
                                    {applying ? 'Применяю...' : 'Применить'}
                                </button>

                                {state.diffContent && (
                                    <button type="button" className="btn-secondary" onClick={() => void handleShowDiff()}>
                                        Диф
                                    </button>
                                )}

                                <button type="button" className="btn-ghost" onClick={() => void closeOverlay()}>
                                    Отмена
                                </button>
                            </>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}

function getResultTitle(state: OverlayState): string {
    if (state.action === 'describe' && state.resultType === 'explain_only' && !state.resultCode) {
        return 'Описание не подготовлено';
    }

    switch (state.action) {
        case 'describe':
            return 'Описание готово';
        case 'elaborate':
            return 'Изменения подготовлены';
        case 'fix':
            return 'Исправления готовы';
        case 'explain':
            return 'Объяснение готово';
        case 'review':
            return 'Ревью кода';
        default:
            return 'Готово';
    }
}
