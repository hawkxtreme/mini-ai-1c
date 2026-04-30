import { useState, useEffect, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { Minus, Square, X } from 'lucide-react';
import type { BslDiagnostic } from '../../api/bsl';
import { useSettings } from '../../contexts/SettingsContext';
import { useBsl } from '../../contexts/BslContext';
import { useChat } from '../../contexts/ChatContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { getConfiguratorApplySupport } from '../../api/configurator';
import { findFirstIntroducedParseError, formatIntroducedParseErrorMessage } from '../../utils/bslSyntaxGuard';
import { CodeSidePanel } from '../CodeSidePanel';
import { SettingsPanel } from '../SettingsPanel';
import { ConflictDialog } from '../ui/ConflictDialog';
import { Header } from './Header';
import { ChatArea } from '../chat/ChatArea';
import { OnboardingWizard } from '../Onboarding/OnboardingWizard';
import type { OverlayQuickActionSessionPayload } from '../../types/quickActionSessions';
import { useCodeSession } from '../../hooks/useCodeSession';
import logo from '../../assets/logo.png';

interface OverlayDiffPayload {
    diffContent: string;
    originalCode?: string | null;
    confHwnd: number;
    useSelectAll: boolean;
}

interface OverlayExplainPayload {
    confHwnd: number;
    scope: 'selection' | 'current_method' | 'module';
    code: string;
    originalCode?: string | null;
    runtimeId?: string | null;
}

export function MainLayout() {
    const { settings } = useSettings();
    const { status: bslStatus, analyzeCode } = useBsl();
    const { clearChat, exportChat, isLoading } = useChat();
    const { pasteCode, checkSelection, getCode } = useConfigurator();

    const [viewMode, setViewMode] = useState<'assistant' | 'split' | 'code'>('assistant');
    const [showSettings, setShowSettings] = useState(false);
    const [nodeAvailable, setNodeAvailable] = useState<boolean | null>(null);
    const [settingsTab, setSettingsTab] = useState<'llm' | 'configurator' | 'bsl' | 'mcp' | 'debug' | undefined>(undefined);
    const [isApplying, setIsApplying] = useState(false);
    const [isValidating, setIsValidating] = useState(false);
    const [diagnostics, setDiagnostics] = useState<any[]>([]);
    const [selectedDiagnostics, setSelectedDiagnostics] = useState<any[] | null>(null);
    const [showConflictDialog, setShowConflictDialog] = useState(false);
    const [selectionActive, setSelectionActive] = useState(true);
    const [activeDiffContent, setActiveDiffContent] = useState('');
    const [activeQuickActionSession, setActiveQuickActionSession] = useState<OverlayQuickActionSessionPayload | null>(null);
    const {
        state: codeSession,
        stateRef: codeSessionRef,
        actions: {
            loadFromConfigurator,
            applySucceeded,
            syncBaseline,
            userEdit,
            applyAICode,
            acceptDiff,
            clearContext,
            clearAll,
        },
    } = useCodeSession();
    const {
        configuratorCode: lastConfiguratorCode,
        baselineCode: uiBaselineCode,
        workingCode: modifiedCode,
        loadedContextCode,
        isContextSelection,
    } = codeSession;

    // useMemo + try/catch защищает от ошибки "Cannot read properties of undefined (reading 'metadata')"
    // которая возникает если Tauri IPC не инициализирован (первый рендер / dev-режим браузера)
    const appWindow = useMemo(() => {
        try {
            return getCurrentWindow();
        } catch (e) {
            console.warn('[MainLayout] getCurrentWindow() failed:', e);
            return null;
        }
    }, []);

    // Check Node.js availability on startup
    useEffect(() => {
        invoke<string | null>('check_node_version_cmd')
            .then(ver => setNodeAvailable(ver !== null))
            .catch(() => setNodeAvailable(false));
    }, []);

    // Диагностика ошибок
    useEffect(() => {
        const handleError = (event: ErrorEvent) => {
            if (event.message?.includes('metadata')) {
                console.error('Критическая ошибка UI (metadata):', {
                    message: event.message,
                    error: event.error,
                    stack: event.error?.stack,
                    settings_exists: !!settings,
                    bslStatus_exists: !!bslStatus,
                });
            }
        };
        window.addEventListener('error', handleError);
        return () => window.removeEventListener('error', handleError);
    }, [settings, bslStatus]);

    // Problem #4: Listen for external diff reset events
    useEffect(() => {
        const unlisten = listen<string>('RESET_DIFF', (event) => {
            console.log('Diff Reset Event received', event.payload?.length);
            syncBaseline(event.payload || '');
            setActiveDiffContent('');
        });
        return () => {
            unlisten.then(fn => fn());
        };
    }, [syncBaseline]);

    // Listen for overlay -> main window diff handoff.
    useEffect(() => {
        const unlisten = listen<OverlayDiffPayload>('open-diff-from-overlay', (event) => {
            const baseCode = event.payload.originalCode ?? codeSessionRef.current.configuratorCode ?? '';
            setActiveQuickActionSession(null);
            loadFromConfigurator(baseCode, !event.payload.useSelectAll);
            setActiveDiffContent(event.payload.diffContent || '');
            setViewMode(prev => prev === 'assistant' ? 'split' : prev);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, [codeSessionRef, loadFromConfigurator]);

    const ensureExpandedWindow = useCallback(async () => {
        if (!appWindow) return;

        const size = await appWindow.innerSize();
        const factor = await appWindow.scaleFactor();
        const logicalWidth = size.width / factor;
        const logicalHeight = size.height / factor;

        if (logicalWidth < 500) {
            await appWindow.setSize(new LogicalSize(700, logicalHeight));
        }
    }, [appWindow]);

    useEffect(() => {
        const unlisten = listen<OverlayExplainPayload>('open-explain-from-overlay', (event) => {
            const explainCode = (event.payload.code || event.payload.originalCode || '').trim();
            if (!explainCode) {
                return;
            }

            void ensureExpandedWindow().catch((e) => {
                console.warn('[MainLayout] Failed to expand window for explain handoff:', e);
            });

            setActiveQuickActionSession(null);
            loadFromConfigurator(explainCode, event.payload.scope === 'selection');
            setDiagnostics([]);
            setActiveDiffContent('');
            setViewMode('assistant');
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, [ensureExpandedWindow, loadFromConfigurator]);

    useEffect(() => {
        const unlisten = listen<OverlayQuickActionSessionPayload>('open-quick-action-session-from-overlay', (event) => {
            const sessionCode = (event.payload.code || event.payload.originalCode || '').trim();
            if (!sessionCode) {
                return;
            }

            const baseCode = event.payload.originalCode?.trim() || sessionCode;

            void ensureExpandedWindow().catch((e) => {
                console.warn('[MainLayout] Failed to expand window for quick action handoff:', e);
            });

            setActiveQuickActionSession(event.payload);
            loadFromConfigurator(baseCode, event.payload.scope === 'selection');
            setDiagnostics(event.payload.diagnostics ?? []);
            setActiveDiffContent('');
            setViewMode('split');
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, [ensureExpandedWindow, loadFromConfigurator]);

    // Analysis effect — runs only when modifiedCode changes AND we are not streaming
    useEffect(() => {
        if (!modifiedCode || isLoading) return;
        const runAnalysis = async () => {
            setIsValidating(true);
            try {
                const results = await analyzeCode(modifiedCode);
                setDiagnostics(results || []);
            } catch (e) {
                console.error('Analysis failed:', e);
            } finally {
                setIsValidating(false);
            }
        };

        const timer = setTimeout(runAnalysis, 2000);
        return () => clearTimeout(timer);
    }, [modifiedCode, analyzeCode, isLoading]);

    const ensureQuickActionDirectApplyAvailable = useCallback(async (
        writeSession: OverlayQuickActionSessionPayload | null,
        useSelectAll: boolean,
        originalContent?: string,
    ) => {
        if (
            writeSession?.mode !== 'write'
            || settings?.configurator?.editor_bridge_enabled !== true
            || !writeSession.writeIntent
        ) {
            return;
        }

        const support = await getConfiguratorApplySupport(
            writeSession.confHwnd,
            useSelectAll,
            writeSession.action,
            writeSession.writeIntent,
            originalContent,
        );

        if (support.canApplyDirectly) {
            return;
        }

        throw new Error(
            support.reason
            || 'Семантическое применение изменений в 1С сейчас недоступно. Проверьте EditorBridge/Scintilla или отключите опцию быстрых действий в 1С Конфигураторе.',
        );
    }, [settings?.configurator?.editor_bridge_enabled]);

    void ensureQuickActionDirectApplyAvailable;

    const handleApply = useCallback(async () => {
        setIsApplying(true);
        try {
            const writeSession = activeQuickActionSession?.mode === 'write' ? activeQuickActionSession : null;
            const useSelectAll = writeSession
                ? writeSession.useSelectAll
                : (!lastConfiguratorCode || lastConfiguratorCode.trim().length === 0);
            const originalContent = writeSession?.originalCode || lastConfiguratorCode || undefined;

            // Проверка конфликта на фронтенде до вставки — читаем текущее содержимое
            // через тот же путь (Ctrl+C), что использовался при захвате
            if (originalContent) {
                try {
                    const currentContent = await getCode(useSelectAll);
                    if (currentContent.trim() !== originalContent.trim()) {
                        const isActive = await checkSelection();
                        setSelectionActive(isActive);
                        setShowConflictDialog(true);
                        return;
                    }
                } catch {
                    // не удалось прочитать — продолжаем без проверки
                }
            }

            await pasteCode(
                modifiedCode,
                useSelectAll,
                originalContent,
                writeSession
                    ? {
                        action: writeSession.action,
                        writeIntent: writeSession.writeIntent,
                        caretLine: writeSession.caretLine ?? null,
                        methodStartLine: writeSession.methodStartLine ?? null,
                        methodName: writeSession.methodName ?? null,
                        runtimeId: writeSession.runtimeId ?? null,
                        forceLegacyApply: true,
                    }
                    : { forceLegacyApply: true },
            );
            applySucceeded();
            setActiveQuickActionSession(null);
        } catch (e: any) {
            const errorMsg = typeof e === 'string' ? e : e?.message || String(e);
            console.error('Apply failed', e);
            alert('Ошибка применения: ' + errorMsg);
        } finally {
            setIsApplying(false);
        }
    }, [activeQuickActionSession, applySucceeded, checkSelection, getCode, lastConfiguratorCode, modifiedCode, pasteCode]);

    const handleConflictApplyToAll = useCallback(async () => {
        setShowConflictDialog(false);
        setIsApplying(true);
        try {
            const writeSession = activeQuickActionSession?.mode === 'write' ? activeQuickActionSession : null;
            await pasteCode(
                modifiedCode,
                true,
                undefined,
                writeSession
                    ? {
                        action: writeSession.action,
                        writeIntent: writeSession.writeIntent,
                        caretLine: writeSession.caretLine ?? null,
                        methodStartLine: writeSession.methodStartLine ?? null,
                        methodName: writeSession.methodName ?? null,
                        runtimeId: writeSession.runtimeId ?? null,
                        forceLegacyApply: true,
                    }
                    : { forceLegacyApply: true },
            );
            setActiveQuickActionSession(null);
        } catch (e: any) {
            alert('Ошибка применения: ' + (e?.message || String(e)));
        } finally {
            setIsApplying(false);
        }
    }, [activeQuickActionSession, modifiedCode, pasteCode]);

    const handleConflictApplyToSelection = useCallback(async () => {
        setShowConflictDialog(false);
        setIsApplying(true);
        try {
            const writeSession = activeQuickActionSession?.mode === 'write' ? activeQuickActionSession : null;
            await pasteCode(
                modifiedCode,
                false,
                undefined,
                writeSession
                    ? {
                        action: writeSession.action,
                        writeIntent: writeSession.writeIntent,
                        caretLine: writeSession.caretLine ?? null,
                        methodStartLine: writeSession.methodStartLine ?? null,
                        methodName: writeSession.methodName ?? null,
                        runtimeId: writeSession.runtimeId ?? null,
                        forceLegacyApply: true,
                    }
                    : { forceLegacyApply: true },
            );
            setActiveQuickActionSession(null);
        } catch (e: any) {
            alert('Ошибка применения: ' + (e?.message || String(e)));
        } finally {
            setIsApplying(false);
        }
    }, [activeQuickActionSession, modifiedCode, pasteCode]);

    const handleCodeLoaded = useCallback((code: string, isSelection: boolean) => {
        setActiveQuickActionSession(null);
        loadFromConfigurator(code, isSelection);
        setDiagnostics([]);
        setActiveDiffContent('');
        setViewMode(prev => prev === 'assistant' ? 'split' : prev);
    }, [loadFromConfigurator]);

    const handleCommitCode = useCallback((code: string) => {
        applyAICode(code);
        acceptDiff();
        setActiveDiffContent('');
    }, [acceptDiff, applyAICode]);

    const handleChatApplyCode = useCallback((code: string) => {
        applyAICode(code);
        acceptDiff();
        setActiveDiffContent('');
        setViewMode(prev => prev === 'assistant' ? 'split' : prev);
    }, [acceptDiff, applyAICode]);

    const handleValidateChatAppliedCode = useCallback(async (baseCode: string, candidateCode: string) => {
        try {
            const [baseDiagnostics, candidateDiagnostics] = await Promise.all([
                analyzeCode(baseCode),
                analyzeCode(candidateCode),
            ]);

            const introducedParseError = findFirstIntroducedParseError(
                baseDiagnostics as BslDiagnostic[],
                candidateDiagnostics,
            );

            if (!introducedParseError) {
                return null;
            }

            return formatIntroducedParseErrorMessage(introducedParseError);
        } catch (error) {
            const details = error instanceof Error ? error.message : String(error);
            return details
                ? `Применение отменено: не удалось проверить синтаксис BSL перед применением. ${details}`
                : 'Применение отменено: не удалось проверить синтаксис BSL перед применением.';
        }
    }, [analyzeCode]);

    const handleActiveDiffChange = useCallback((content: string) => {
        setActiveDiffContent(content);
        if (content) {
            setViewMode(prev => prev === 'assistant' ? 'split' : prev);
        }
    }, []);

    const handleNewChat = useCallback(() => {
        clearChat();
        setActiveQuickActionSession(null);
        clearAll();
        setDiagnostics([]);
        setActiveDiffContent('');
    }, [clearAll, clearChat]);

    const [appVersion, setAppVersion] = useState<string>('');
    useEffect(() => {
        getVersion().then(v => setAppVersion(v)).catch(() => {});
    }, []);

    const minimize = () => appWindow?.minimize();
    const maximize = async () => {
        const isMaximized = await appWindow?.isMaximized();
        isMaximized ? appWindow?.unmaximize() : appWindow?.maximize();
    };
    const close = () => appWindow?.close();

    useEffect(() => {
        (window as any).__MINI_AI_LAYOUT_TEST__ = {
            setDiagnosticsState: (payload: {
                diagnostics?: any[];
                selectedDiagnostics?: any[] | null;
            }) => {
                if (payload.diagnostics !== undefined) {
                    setDiagnostics(payload.diagnostics);
                }
                if (Object.prototype.hasOwnProperty.call(payload, 'selectedDiagnostics')) {
                    setSelectedDiagnostics(payload.selectedDiagnostics ?? null);
                }
            },
            getDiagnosticsState: () => ({
                diagnostics,
                selectedDiagnostics,
                viewMode,
                baselineCode: uiBaselineCode,
                modifiedCode,
            }),
        };

        return () => {
            delete (window as any).__MINI_AI_LAYOUT_TEST__;
        };
    }, [diagnostics, modifiedCode, selectedDiagnostics, uiBaselineCode, viewMode]);

    return (
        <div className="flex flex-col h-screen bg-transparent relative overflow-hidden">
            <SettingsPanel isOpen={showSettings} onClose={() => setShowSettings(false)} initialTab={settingsTab as any} />

            <div className="relative h-10 bg-[#09090b] flex items-center justify-between px-4 border-b border-[#27272a] select-none z-50">
                <div data-tauri-drag-region className="absolute inset-0 z-0" />
                <div className="relative z-10 flex items-center gap-2 pointer-events-none">
                    <img src={logo} alt="Logo" className="w-5 h-5" />
                    <span className="text-sm font-medium text-zinc-300">Mini AI 1C</span>
                    {appVersion && <span className="text-xs text-zinc-600">v{appVersion}</span>}
                </div>
                <div className="relative z-50 flex items-center gap-1 pointer-events-auto">
                    <button onClick={minimize} className="p-1.5 hover:bg-zinc-800 rounded text-zinc-400 hover:text-white"><Minus className="w-4 h-4" /></button>
                    <button onClick={maximize} className="p-1.5 hover:bg-zinc-800 rounded text-zinc-400 hover:text-white"><Square className="w-3 h-3" /></button>
                    <button onClick={close} className="p-1.5 hover:bg-red-900/50 hover:text-red-200 rounded text-zinc-400"><X className="w-4 h-4" /></button>
                </div>
            </div>

            <div className="flex-1 flex flex-col relative overflow-hidden">
                {(settings && !settings.onboarding_completed) && (
                    <OnboardingWizard onComplete={() => window.location.reload()} />
                )}

                <Header
                    bslStatus={bslStatus}
                    nodeAvailable={nodeAvailable}
                    viewMode={viewMode}
                    onViewModeChange={setViewMode}
                    onNewChat={handleNewChat}
                    onExportChat={exportChat}
                    onOpenSettings={(tab) => { if (tab) setSettingsTab(tab as any); setShowSettings(true); }}
                />

                <div className="flex flex-1 overflow-hidden bg-[#09090b] relative">
                    <div className={`flex flex-1 overflow-hidden transition-all duration-300 ${viewMode === 'code' ? 'hidden' : 'opacity-100'}`}>
                        <ChatArea
                            originalCode={uiBaselineCode}
                            modifiedCode={modifiedCode}
                            loadedContextCode={loadedContextCode}
                            isContextSelection={isContextSelection}
                            onClearContext={clearContext}
                            onPrepareDiffBase={syncBaseline}
                            onApplyCode={handleChatApplyCode}
                            onValidateAppliedCode={handleValidateChatAppliedCode}
                            onCommitCode={handleCommitCode}
                            onCodeLoaded={handleCodeLoaded}
                            diagnostics={diagnostics}
                            selectedDiagnostics={selectedDiagnostics}
                            onOpenSettings={(tab) => {
                                setSettingsTab(tab as any);
                                setShowSettings(true);
                            }}
                            onActiveDiffChange={handleActiveDiffChange}
                            activeDiffContent={activeDiffContent}
                        />
                    </div>

                    <div className={`${viewMode === 'assistant' ? 'hidden' : 'flex'} ${viewMode === 'code' ? 'flex-1 w-full' : ''} z-40 h-full transition-all duration-300`}>
                        <CodeSidePanel
                            isOpen={viewMode !== 'assistant'}
                            isFullWidth={viewMode === 'code'}
                            onClose={() => setViewMode('assistant')}
                            originalCode={uiBaselineCode}
                            modifiedCode={modifiedCode}
                            onModifiedCodeChange={userEdit}
                            diagnostics={diagnostics}
                            onApply={handleApply}
                            isApplying={isApplying}
                            isValidating={isValidating}
                            activeDiffContent={activeDiffContent}
                            onActiveDiffChange={setActiveDiffContent}
                            onCommitCode={handleCommitCode}
                            onDiagnosticSelectionChange={setSelectedDiagnostics}
                        />
                    </div>
                </div>

                <ConflictDialog
                    isOpen={showConflictDialog}
                    selectionActive={selectionActive}
                    onClose={() => setShowConflictDialog(false)}
                    onApplyToAll={handleConflictApplyToAll}
                    onApplyToSelection={handleConflictApplyToSelection}
                />
            </div>
        </div>
    );
}
