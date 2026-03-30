/**
 * useQuickActions - orchestrates AI quick actions triggered from Overlay.
 *
 * Listens for "quick-action-from-overlay", captures code from Configurator,
 * calls the LLM, and sends the prepared result back to the overlay window.
 */

import { useEffect, useRef } from 'react';
import { emit, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { useSettings } from '../contexts/SettingsContext';
import { useProfiles } from '../contexts/ProfileContext';
import { analyzeBsl, type BslDiagnostic } from '../api/bsl';
import {
  getConfiguratorApplySupport,
  type ConfiguratorApplySupport,
} from '../api/configurator';
import type {
  OverlayQuickActionSessionPayload,
  QuickActionAction,
  QuickActionCaptureScope,
  QuickActionWriteIntent,
} from '../types/quickActionSessions';
import { applyDiffWithDiagnostics } from '../utils/diffViewer';

interface QuickActionEvent {
  action: QuickActionAction;
  confHwnd: number;
  task?: string;
  targetX?: number | null;
  targetY?: number | null;
  targetChildHwnd?: number | null;
}

interface ExplainOverlayPayload {
  confHwnd: number;
  scope: CaptureScope;
  code: string;
  originalCode: string;
  runtimeId?: string | null;
}

interface OverlayApplyEvent {
  action: 'describe' | 'elaborate' | 'fix';
  confHwnd: number;
  resultCode: string;
  originalCode?: string | null;
  useSelectAll: boolean;
  writeIntent: WriteIntent;
  caretLine?: number | null;
  methodStartLine?: number | null;
  methodName?: string | null;
  runtimeId?: string | null;
  targetX?: number | null;
  targetY?: number | null;
  targetChildHwnd?: number | null;
}

type ResultType = 'comment' | 'diff' | 'explain_only';
type WriteIntent = QuickActionWriteIntent;
type CaptureScope = QuickActionCaptureScope;


interface OverlayStateUpdate {
  phase: string;
  action?: string;
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

interface CaptureResult {
  scope: CaptureScope;
  promptCode: string;
  originalCode: string;
  useSelectAll: boolean;
  caretLine?: number;
  methodStartLine?: number | null;
  methodName?: string | null;
  runtimeId?: string | null;
  /** Full method/module text for BSL error analysis when promptCode is a partial selection */
  bslAnalysisCode?: string;
}

function normalizeOptionalText(text?: string | null): string {
  return normalizeLineEndings(text ?? '').trim();
}

function hasVisibleText(value?: string | null): boolean {
  return (value ?? '').trim().length > 0;
}

function sameOptionalString(left?: string | null, right?: string | null): boolean {
  if (!hasVisibleText(left) || !hasVisibleText(right)) {
    return true;
  }

  return (left ?? '').trim() === (right ?? '').trim();
}

function sameCaptureContext(current: CaptureResult, captured: CaptureResult): boolean {
  if (current.scope !== captured.scope) {
    return false;
  }

  if (normalizeOptionalText(current.originalCode) !== normalizeOptionalText(captured.originalCode)) {
    return false;
  }

  if (
    current.methodStartLine != null &&
    captured.methodStartLine != null &&
    current.methodStartLine !== captured.methodStartLine
  ) {
    return false;
  }

  if (!sameOptionalString(current.methodName, captured.methodName)) {
    return false;
  }

  if (!sameOptionalString(current.runtimeId, captured.runtimeId)) {
    return false;
  }

  return true;
}

function mergeCaptureForApply(base: CaptureResult, fresh: CaptureResult): CaptureResult {
  return {
    ...base,
    caretLine: fresh.caretLine ?? base.caretLine,
    methodStartLine: fresh.methodStartLine ?? base.methodStartLine,
    methodName: fresh.methodName ?? base.methodName,
    runtimeId: fresh.runtimeId ?? base.runtimeId,
  };
}

function hasStableCurrentMethodIdentity(capture: CaptureResult): boolean {
  if (capture.scope !== 'current_method') {
    return true;
  }

  return capture.methodStartLine != null || hasVisibleText(capture.runtimeId);
}

function buildContextChangedState(
  action: QuickActionEvent['action'],
  confHwnd: number,
  capture: CaptureResult,
  target?: Pick<QuickActionEvent, 'targetX' | 'targetY' | 'targetChildHwnd'>,
): OverlayStateUpdate {
  return {
    phase: 'result',
    action,
    resultType: 'explain_only',
    preview:
      'Текущую процедуру больше не удалось надежно определить. Проверьте, что курсор остался в нужном методе, и повторите генерацию после обновления контекста.',
    confHwnd,
    originalCode: capture.originalCode,
    useSelectAll: capture.useSelectAll,
    canApplyDirectly: false,
    caretLine: capture.caretLine,
    methodStartLine: capture.methodStartLine,
    methodName: capture.methodName,
    runtimeId: capture.runtimeId,
    targetX: target?.targetX ?? null,
    targetY: target?.targetY ?? null,
    targetChildHwnd: target?.targetChildHwnd ?? null,
  };
}

function buildOverlayMessageState(args: {
  action: QuickActionEvent['action'];
  confHwnd: number;
  preview: string;
  capture?: CaptureResult;
  target?: Pick<QuickActionEvent, 'targetX' | 'targetY' | 'targetChildHwnd'>;
}): OverlayStateUpdate {
  const { action, confHwnd, preview, capture, target } = args;

  return {
    phase: 'result',
    action,
    resultType: 'explain_only',
    preview,
    confHwnd,
    originalCode: capture?.originalCode,
    useSelectAll: capture?.useSelectAll ?? false,
    canApplyDirectly: false,
    caretLine: capture?.caretLine,
    methodStartLine: capture?.methodStartLine,
    methodName: capture?.methodName,
    runtimeId: capture?.runtimeId,
    targetX: target?.targetX ?? null,
    targetY: target?.targetY ?? null,
    targetChildHwnd: target?.targetChildHwnd ?? null,
  };
}

async function refreshQuickActionContext(
  confHwnd: number,
  action: QuickActionEvent['action'],
  capture: CaptureResult,
): Promise<{ changed: boolean; freshCapture: CaptureResult }> {
  const freshCapture = await captureQuickActionContext(confHwnd, action);
  return {
    changed: !sameCaptureContext(freshCapture, capture),
    freshCapture,
  };
}

function scopeFromWriteIntent(writeIntent: WriteIntent): CaptureScope {
  switch (writeIntent) {
    case 'replace_selection':
      return 'selection';
    case 'insert_before_current_method':
    case 'replace_current_method':
      return 'current_method';
    case 'replace_module':
    default:
      return 'module';
  }
}

function buildCaptureFromOverlayApplyEvent(payload: OverlayApplyEvent): CaptureResult {
  const originalCode = payload.originalCode ?? '';
  return {
    scope: scopeFromWriteIntent(payload.writeIntent),
    promptCode: originalCode,
    originalCode,
    useSelectAll: payload.useSelectAll,
    caretLine: payload.caretLine ?? undefined,
    methodStartLine: payload.methodStartLine ?? null,
    methodName: payload.methodName ?? null,
    runtimeId: payload.runtimeId ?? null,
  };
}

function writeIntentFromCaptureScope(scope: CaptureScope): WriteIntent {
  switch (scope) {
    case 'selection':
      return 'replace_selection';
    case 'current_method':
      return 'replace_current_method';
    case 'module':
    default:
      return 'replace_module';
  }
}

async function handoffQuickActionSession(args: {
  action: OverlayQuickActionSessionPayload['action'];
  mode: OverlayQuickActionSessionPayload['mode'];
  confHwnd: number;
  capture: CaptureResult;
  task?: string | null;
  diagnostics?: BslDiagnostic[] | null;
  diagnosticsError?: string | null;
}): Promise<void> {
  const { action, mode, confHwnd, capture, task, diagnostics, diagnosticsError } = args;
  await invoke('focus_main_window_for_overlay_chat');

  await emit('open-quick-action-session-from-overlay', {
    action,
    mode,
    confHwnd,
    scope: capture.scope,
    code: capture.promptCode,
    originalCode: capture.originalCode,
    useSelectAll: capture.useSelectAll,
    writeIntent: mode === 'write' ? writeIntentFromCaptureScope(capture.scope) : undefined,
    caretLine: capture.caretLine ?? null,
    methodStartLine: capture.methodStartLine ?? null,
    methodName: capture.methodName ?? null,
    runtimeId: capture.runtimeId ?? null,
    task: task ?? null,
    diagnostics: diagnostics ?? null,
    diagnosticsError: diagnosticsError ?? null,
  } satisfies OverlayQuickActionSessionPayload);

  await invoke('hide_overlay', { confHwnd, restoreFocus: false });
}

async function applyPreparedQuickActionResult(args: {
  action: QuickActionEvent['action'];
  confHwnd: number;
  resultCode: string;
  writeIntent: WriteIntent;
  capture: CaptureResult;
}): Promise<'applied' | 'context_changed'> {
  const { action, confHwnd, resultCode, writeIntent, capture } = args;

  if (action === 'describe' && !hasStableCurrentMethodIdentity(capture)) {
    await invoke('update_overlay_state', {
      state: buildContextChangedState(action, confHwnd, capture),
    });
    return 'context_changed';
  }

  const {
    changed: applyContextChanged,
    freshCapture: applyFreshCapture,
  } = await refreshQuickActionContext(confHwnd, action, capture);

  if (applyContextChanged) {
    await invoke('update_overlay_state', {
      state: buildContextChangedState(action, confHwnd, capture),
    });
    return 'context_changed';
  }

  const applyCapture = mergeCaptureForApply(capture, applyFreshCapture);

  await invoke('paste_code_to_configurator', {
    hwnd: confHwnd,
    code: resultCode,
    useSelectAll: applyCapture.useSelectAll,
    originalContent: applyCapture.originalCode ?? null,
    action,
    writeIntent,
    caretLine: applyCapture.caretLine ?? null,
    methodStartLine: applyCapture.methodStartLine ?? null,
    methodName: applyCapture.methodName ?? null,
    runtimeId: applyCapture.runtimeId ?? null,
  });

  await invoke('hide_overlay', { confHwnd });
  return 'applied';
}

function appendApplyAvailabilityMessage(

  previewText: string,
  support: ConfiguratorApplySupport | undefined,
  resultType: ResultType,
): string {
  if (!support || support.canApplyDirectly || resultType === 'explain_only') {
    return previewText;
  }

  const reason = support.reason?.trim() || 'Надежное прямое применение сейчас недоступно.';
  return trimPreview([reason, '', previewText].join('\n'));
}

function resultTypeFor(action: string): ResultType {
  switch (action) {
    case 'describe':
      return 'comment';
    case 'explain':
    case 'review':
      return 'explain_only';
    case 'elaborate':
    case 'fix':
    default:
      return 'diff';
  }
}

function normalizeLineEndings(text: string): string {
  return text.replace(/\r\n/g, '\n');
}

function restoreLineEndings(template: string, text: string): string {
  return template.includes('\r\n') ? text.replace(/\n/g, '\r\n') : text;
}

function trimPreview(text: string): string {
  return text.split('\n').slice(0, 8).join('\n');
}

function extractCommentBlock(rawResult: string): string {
  const commentLines = rawResult
    .split('\n')
    .filter(line => line.trimStart().startsWith('//'));

  const commentBlock = commentLines.join('\n').trim();
  if (commentBlock) return commentBlock;

  const fallback = rawResult.trim();
  if (!fallback) {
    throw new Error('Модель не вернула блок описания.');
  }

  return fallback;
}

function insertCommentBeforeRoutine(code: string, commentBlock: string): string {
  const normalizedCode = normalizeLineEndings(code);
  const lines = normalizedCode.split('\n');
  const routineLine = lines.findIndex(line =>
    /^\s*(Процедура|Функция|Procedure|Function)\s+/i.test(line),
  );

  if (routineLine >= 0) {
    lines.splice(routineLine, 0, commentBlock);
    return restoreLineEndings(code, lines.join('\n'));
  }

  return restoreLineEndings(code, `${commentBlock}\n${normalizedCode}`);
}

function buildDiffFallbackPreview(rawDiff: string, failedCount: number, fuzzyCount: number): string {
  const reasons: string[] = [];
  if (failedCount > 0) {
    reasons.push(`непримененных блоков: ${failedCount}`);
  }
  if (fuzzyCount > 0) {
    reasons.push(`нечетких блоков: ${fuzzyCount}`);
  }

  const message = `Автовставка отключена: ${reasons.join(', ')}. Откройте "Диф" для ручной проверки.`;
  return trimPreview([message, '', rawDiff].join('\n'));
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
  method_end_line?: number | null;
  primary_runtime_id?: string | null;
}


async function captureQuickActionContext(
  confHwnd: number,
  action: QuickActionEvent['action'],
): Promise<CaptureResult> {
  try {
    const ctx = await invoke<EditorContext>('get_editor_context_cmd', { hwnd: confHwnd, skipFocusRestore: action === 'describe' });
    if (ctx.available) {
      if (action === 'describe' && ctx.current_method_text) {
        return { scope: 'current_method', promptCode: ctx.current_method_text, originalCode: ctx.current_method_text, useSelectAll: false, caretLine: ctx.caret_line, methodStartLine: ctx.method_start_line, methodName: ctx.current_method_name, runtimeId: ctx.primary_runtime_id };
      }
      if (ctx.has_selection && ctx.selection_text.trim()) {
        const bslAnalysisCode = ctx.current_method_text || (ctx.module_text.trim() ? ctx.module_text : undefined);
        return { scope: 'selection', promptCode: ctx.selection_text, originalCode: ctx.selection_text, useSelectAll: false, runtimeId: ctx.primary_runtime_id, bslAnalysisCode };
      }
      if (ctx.current_method_text) {
        return { scope: 'current_method', promptCode: ctx.current_method_text, originalCode: ctx.current_method_text, useSelectAll: false, caretLine: ctx.caret_line, methodStartLine: ctx.method_start_line, methodName: ctx.current_method_name, runtimeId: ctx.primary_runtime_id };
      }
      if (ctx.module_text.trim()) {
        return { scope: 'module', promptCode: ctx.module_text, originalCode: ctx.module_text, useSelectAll: true, runtimeId: ctx.primary_runtime_id };
      }
    }
  } catch (bridgeError) {
    console.warn('[useQuickActions] bridge context failed, falling back:', bridgeError);
  }

  if (action === 'describe') {
    const promptCode = await invoke<string>('get_active_fragment_cmd', {
      hwnd: confHwnd,
      skipFocusRestore: true,
    });
    const normalizedPrompt = normalizeLineEndings(promptCode).trim();

    if (!normalizedPrompt) {
      throw new Error(
        'Не удалось определить текущую процедуру для описания без EditorBridge или Scintilla. Откройте нужный метод и повторите попытку.',
      );
    }

    return {
      scope: 'current_method',
      promptCode,
      originalCode: promptCode,
      useSelectAll: false,
    };
  }

  const hasSelection = await invoke<boolean>('check_selection_state', { hwnd: confHwnd });
  if (hasSelection) {
    const selectedCode = await invoke<string>('get_code_from_configurator', { hwnd: confHwnd, useSelectAll: false, skipFocusRestore: true });
    return { scope: 'selection', promptCode: selectedCode, originalCode: selectedCode, useSelectAll: false };
  }

  const [promptCode, originalCode] = await Promise.all([
    invoke<string>('get_active_fragment_cmd', { hwnd: confHwnd, skipFocusRestore: true }),
    invoke<string>('get_code_from_configurator', { hwnd: confHwnd, useSelectAll: true, skipFocusRestore: true }),
  ]);
  const scope: CaptureScope = normalizeLineEndings(promptCode) === normalizeLineEndings(originalCode) ? 'module' : 'current_method';
  return { scope, promptCode, originalCode: scope === 'module' ? originalCode : promptCode, useSelectAll: scope === 'module' };
}


export function useQuickActions() {
  const { settings } = useSettings();
  const { activeProfile } = useProfiles();
  const latestRequestIdRef = useRef(0);

  useEffect(() => {
    const quickActionUnlisten = listen<QuickActionEvent>('quick-action-from-overlay', async (event) => {
      const requestId = latestRequestIdRef.current + 1;
      latestRequestIdRef.current = requestId;
      const isStaleRequest = () => latestRequestIdRef.current !== requestId;
      const { action, confHwnd, task, targetX, targetY, targetChildHwnd } = event.payload;
      const autoApply = settings?.configurator?.editor_bridge_auto_apply ?? false;

      try {
        const shouldSyncToClickTarget =
          typeof targetX === 'number' &&
          typeof targetY === 'number' &&
          ((action === 'describe') || !(await invoke<boolean>('check_selection_state', { hwnd: confHwnd })));

        if (shouldSyncToClickTarget) {
          try {
            await invoke('sync_configurator_caret_to_point_cmd', {
              hwnd: confHwnd,
              screenX: targetX,
              screenY: targetY,
              childHwnd: targetChildHwnd ?? null,
            });
          } catch (syncError) {
            console.warn('[useQuickActions] failed to sync caret to click target:', syncError);
          }
        }

        const capture = await captureQuickActionContext(confHwnd, action);
        if (isStaleRequest()) return;

        if (action === 'explain') {
          await invoke('focus_main_window_for_overlay_chat');
          await emit('open-explain-from-overlay', {
            confHwnd,
            scope: capture.scope,
            code: capture.promptCode,
            originalCode: capture.originalCode,
            runtimeId: capture.runtimeId ?? null,
          } satisfies ExplainOverlayPayload);
          await invoke('hide_overlay', { confHwnd, restoreFocus: false });
          return;
        }

        if (action === 'fix') {
          let diagnostics: BslDiagnostic[] | null = null;
          let diagnosticsError: string | null = null;
          try {
            diagnostics = await analyzeBsl(capture.bslAnalysisCode ?? capture.promptCode);
          } catch (diagnosticsFailure) {
            diagnosticsError = String(diagnosticsFailure);
            console.warn('[useQuickActions] diagnostics lookup failed for fix action:', diagnosticsFailure);
          }

          if (isStaleRequest()) return;

          if (diagnostics && diagnostics.length === 0 && capture.scope !== 'selection') {
            await invoke('update_overlay_state', {
              state: buildOverlayMessageState({
                action,
                confHwnd,
                preview: 'Ошибок не найдено для выбранного фрагмента.',
                capture,
                target: event.payload,
              }),
            });
            return;
          }

          await handoffQuickActionSession({
            action,
            mode: 'write',
            confHwnd,
            capture,
            diagnostics,
            diagnosticsError,
          });
          return;
        }

        if (action === 'elaborate') {
          await handoffQuickActionSession({
            action,
            mode: 'write',
            confHwnd,
            capture,
            task: task?.trim() || null,
          });
          return;
        }

        if (action === 'review') {
          await handoffQuickActionSession({
            action,
            mode: 'chat',
            confHwnd,
            capture,
          });
          return;
        }

        const prompt = buildPrompt(action, capture.promptCode, task);
        const rawResult = await invoke<string>('quick_chat_invoke', { prompt });
        if (isStaleRequest()) return;

        const { changed: contextChanged, freshCapture } = await refreshQuickActionContext(
          confHwnd,
          action,
          capture,
        );
        if (contextChanged) {
          if (isStaleRequest()) return;
          await invoke('update_overlay_state', { state: buildContextChangedState(action, confHwnd, capture, event.payload) });
          return;
        }
        const effectiveCapture = mergeCaptureForApply(capture, freshCapture);

        const resultType = resultTypeFor(action);
        let resultCode: string | undefined;
        let diffContent: string | undefined;
        let previewText = rawResult;
        let writeIntent: WriteIntent | undefined;
        let applySupport: ConfiguratorApplySupport | undefined;

        if (action === 'describe') {
          const commentBlock = extractCommentBlock(rawResult);
          if (effectiveCapture.scope === 'selection') {
            resultCode = insertCommentBeforeRoutine(effectiveCapture.promptCode, commentBlock);
            writeIntent = 'replace_selection';
          } else if (effectiveCapture.scope === 'current_method') {
            resultCode = commentBlock;
            writeIntent = 'insert_before_current_method';
          } else {
            resultCode = insertCommentBeforeRoutine(effectiveCapture.promptCode, commentBlock);
            writeIntent = 'replace_module';
          }
          previewText = commentBlock;
        } else if (action === 'review') {
          resultCode = rawResult;
          previewText = rawResult;
        } else {
          diffContent = rawResult;
          const diffResult = applyDiffWithDiagnostics(effectiveCapture.promptCode, rawResult);
          if (diffResult.failedCount === 0 && diffResult.fuzzyCount === 0) {
            resultCode = diffResult.code;
            writeIntent = effectiveCapture.scope === 'selection'
              ? 'replace_selection'
              : effectiveCapture.scope === 'current_method'
                ? 'replace_current_method'
                : 'replace_module';
            previewText = rawResult;
          } else {
            previewText = buildDiffFallbackPreview(rawResult, diffResult.failedCount, diffResult.fuzzyCount);
          }
        }

        if (resultType !== 'explain_only' && resultCode && writeIntent) {
          try {
            applySupport = await getConfiguratorApplySupport(
              confHwnd,
              effectiveCapture.useSelectAll,
              action,
              writeIntent,
              effectiveCapture.originalCode,
            );
          } catch (applyError) {
            console.warn('[useQuickActions] apply support resolution failed:', applyError);
            applySupport = {
              canApplyDirectly: false,
              preferredWriter: 'diff_only',
              reason: `Не удалось определить доступность прямого применения результата: ${String(applyError)}`,
            };
          }
        }

        if (action === 'describe' && !hasStableCurrentMethodIdentity(effectiveCapture)) {
          applySupport = {
            canApplyDirectly: false,
            preferredWriter: 'diff_only',
            reason: 'Не удалось надежно определить текущую процедуру для прямой вставки. Проверьте результат и примените его вручную.',
          };
        }

        if (isStaleRequest()) return;
        previewText = appendApplyAvailabilityMessage(previewText, applySupport, resultType);

        if (action === 'describe' && autoApply && applySupport?.canApplyDirectly && resultCode && writeIntent) {
          await applyPreparedQuickActionResult({
            action,
            confHwnd,
            resultCode,
            writeIntent,
            capture: effectiveCapture,
          });
          return;
        }

        const resultState: OverlayStateUpdate = {
          phase: 'result',
          action,
          resultType,
          preview: trimPreview(previewText),
          resultCode,
          diffContent,
          confHwnd,
          originalCode: effectiveCapture.originalCode,
          useSelectAll: effectiveCapture.useSelectAll,
          writeIntent,
          canApplyDirectly: applySupport?.canApplyDirectly,
          applyUnavailableReason: applySupport?.reason ?? undefined,
          preferredWriter: applySupport?.preferredWriter,
          caretLine: effectiveCapture.caretLine,
          methodStartLine: effectiveCapture.methodStartLine,
          methodName: effectiveCapture.methodName,
          runtimeId: effectiveCapture.runtimeId,
          targetX,
          targetY,
          targetChildHwnd,
        };

        await invoke('update_overlay_state', { state: resultState });
      } catch (error) {
        if (isStaleRequest()) return;
        console.error('[useQuickActions] error:', error);
        await invoke('update_overlay_state', {
          state: {
            phase: 'result',
            action,
            resultType: 'explain_only' as ResultType,
            preview: `Ошибка: ${String(error)}`,
            confHwnd,
            useSelectAll: false,
            targetX,
            targetY,
            targetChildHwnd,
          } satisfies OverlayStateUpdate,
        });
      }
    });

    const overlayApplyUnlisten = listen<OverlayApplyEvent>(
      'apply-quick-action-result-from-overlay',
      async (event) => {
        const payload = event.payload;

        try {
          await applyPreparedQuickActionResult({
            action: payload.action,
            confHwnd: payload.confHwnd,
            resultCode: payload.resultCode,
            writeIntent: payload.writeIntent,
            capture: buildCaptureFromOverlayApplyEvent(payload),
          });
        } catch (error) {
          console.error('[useQuickActions] overlay apply error:', error);

          const capture = buildCaptureFromOverlayApplyEvent(payload);
          await invoke('update_overlay_state', {
            state: {
              phase: 'result',
              action: payload.action,
              resultType: 'explain_only' as ResultType,
              preview: `Ошибка вставки:\n${String(error)}`,
              confHwnd: payload.confHwnd,
              originalCode: capture.originalCode,
              useSelectAll: capture.useSelectAll,
              canApplyDirectly: false,
              caretLine: capture.caretLine,
              methodStartLine: capture.methodStartLine,
              methodName: capture.methodName,
              runtimeId: capture.runtimeId,
              targetX: payload.targetX ?? null,
              targetY: payload.targetY ?? null,
              targetChildHwnd: payload.targetChildHwnd ?? null,
            } satisfies OverlayStateUpdate,
          });
        }
      },
    );

    return () => {
      quickActionUnlisten.then(fn => fn());
      overlayApplyUnlisten.then(fn => fn());
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings, activeProfile]);
}

function buildPrompt(

  action: string,
  code: string,
  task?: string,
  diagnostics?: string,
): string {
  if (action === 'describe') {
    return `Ты — опытный 1С-разработчик. Сгенерируй комментарий к процедуре или функции строго по стандарту 1С «Описание процедур и функций» (#std453).

Требования к результату:
- Верни только блок комментария перед объявлением метода.
- Каждая строка результата должна начинаться с //.
- Если комментарий не помещается в одну строку, делай перенос по словам на следующую строку.
- Каждая перенесенная строка тоже должна начинаться с //.
- Ни одна строка готового комментария не должна превышать 120 символов.
- Секция "Описание" обязательна и должна объяснять назначение метода так, чтобы был понятен сценарий использования без чтения реализации.
- Описание должно начинаться с глагола.
- Не начинай описание со слов "Процедура", "Функция" и не повторяй имя метода, если это не добавляет смысла.
- Не пиши тавтологии и очевидные фразы, которые просто повторяют название метода или то, что это обработчик события.
- Секция "Параметры:" добавляется только если у метода есть параметры.
- Для каждого параметра используй формат:
//  ИмяПараметра - Тип1, Тип2 - осмысленное описание
- Тип параметра обязателен. Если точный тип неочевиден, определи наиболее вероятный по коду.
- Секция "Возвращаемое значение:" добавляется только для функции.
- Для простого возвращаемого значения используй формат:
//  Тип - описание
- Для составного возвращаемого значения каждый вариант пиши с новой строки:
//  - Тип - описание
- Не добавляй секцию "Пример".
- Не возвращай код метода, сигнатуру метода, markdown или пояснения вне комментария.

Код:
${code}

Верни только готовый комментарий по стандарту 1С.`;
  }

  switch (action) {
    case 'describe':
      return `Ты — опытный 1С-разработчик. Сгенерируй комментарий к процедуре или функции строго по стандарту 1С «Описание процедур и функций» (#std453).

Требования к результату:
- Верни только блок комментария перед объявлением метода.
- Каждая строка результата должна начинаться с //.
- Секция "Описание" обязательна и должна объяснять назначение метода так, чтобы был понятен сценарий использования без чтения реализации.
- Описание должно начинаться с глагола.
- Не начинай описание со слов "Процедура", "Функция" и не повторяй имя метода, если это не добавляет смысла.
- Не пиши тавтологии и очевидные фразы, которые просто повторяют название метода или то, что это обработчик события.
- Секция "Параметры:" добавляется только если у метода есть параметры.
- Для каждого параметра используй формат:
//  ИмяПараметра - Тип1, Тип2 - осмысленное описание
- Тип параметра обязателен. Если точный тип неочевиден, определи наиболее вероятный по коду.
- Секция "Возвращаемое значение:" добавляется только для функции.
- Для простого возвращаемого значения используй формат:
//  Тип - описание
- Для составного возвращаемого значения каждый вариант пиши с новой строки:
//  - Тип - описание
- Не добавляй секцию "Пример".
- Не возвращай код метода, сигнатуру метода, markdown или пояснения вне комментария.

Код:
${code}

Верни только готовый комментарий по стандарту 1С.`;

    case 'elaborate':
      return `Ты — опытный 1С-разработчик. Доработай процедуру/функцию.
Задача: ${task ?? 'улучши код'}

Код:
${code}

Верни результат в формате SEARCH/REPLACE.`;

    case 'fix':
      return `Ты — опытный 1С-разработчик. Исправь ошибки в коде.
${diagnostics ? `Диагностики BSL LS:\n${diagnostics}\n` : ''}Код:
${code}

Исправь только указанные ошибки. Верни в формате SEARCH/REPLACE.`;

    case 'explain':
      return `Объясни следующий 1С-код структурированно:

## Назначение
<одна строка>

## Параметры
- <Имя> (<Тип>) — <описание>

## Возвращаемое значение
<Тип> — <описание>

## Логика работы
1. <шаг>

Код:
${code}`;

    case 'review':
      return `Проведи код-ревью следующего 1С-кода:

## Критические проблемы
- <проблема или "нет">

## Потенциальные улучшения
- <улучшение>

## Производительность
- <замечание или "нет замечаний">

## Соответствие стандартам 1С
- <замечание>

Код:
${code}`;

    default:
      return code;
  }
}
