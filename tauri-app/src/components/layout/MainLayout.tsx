import { useState, useEffect, useCallback, useMemo } from 'react';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { PanelRight, Trash2, Settings, Minus, Square, X } from 'lucide-react';
import { useSettings } from '../../contexts/SettingsContext';
import { useBsl } from '../../contexts/BslContext';
import { useChat } from '../../contexts/ChatContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { CodeSidePanel } from '../CodeSidePanel';
import { SettingsPanel } from '../SettingsPanel';
import { ConflictDialog } from '../ui/ConflictDialog';
import { Header } from './Header';
import { ChatArea } from '../chat/ChatArea';
import { OnboardingWizard } from '../Onboarding/OnboardingWizard';
import logo from '../../assets/logo.png';

export function MainLayout() {
    const { settings } = useSettings();
    const { status: bslStatus, analyzeCode } = useBsl();
    const { clearChat } = useChat();
    const { pasteCode, checkSelection } = useConfigurator();

    const [viewMode, setViewMode] = useState<'assistant' | 'split' | 'code'>('assistant');
    const [showSettings, setShowSettings] = useState(false);
    const [settingsTab, setSettingsTab] = useState<'llm' | 'configurator' | 'bsl' | 'mcp' | 'debug' | undefined>(undefined);
    const [isApplying, setIsApplying] = useState(false);
    const [isValidating, setIsValidating] = useState(false);

    const [originalCode, setOriginalCode] = useState('');
    const [modifiedCode, setModifiedCode] = useState('');
    const [diagnostics, setDiagnostics] = useState<any[]>([]);
    const [showConflictDialog, setShowConflictDialog] = useState(false);
    const [selectionActive, setSelectionActive] = useState(true);
    const [activeDiffContent, setActiveDiffContent] = useState(''); // Стейт для диффов

    const appWindow = getCurrentWindow();

    // Problem #4: Listen for external diff reset events
    useEffect(() => {
        const unlisten = listen<string>('RESET_DIFF', (event) => {
            console.log("Diff Reset Event received", event.payload.length);
            setOriginalCode(event.payload);
            setActiveDiffContent(''); // Сбрасываем диффы при новом коде
        });
        return () => {
            unlisten.then(fn => fn());
        };
    }, []);


    // Analysis effect
    useEffect(() => {
        if (viewMode !== 'assistant' && modifiedCode) {
            const runAnalysis = async () => {
                setIsValidating(true);
                try {
                    const results = await analyzeCode(modifiedCode);
                    setDiagnostics(results);
                } catch (e) {
                    console.error("Analysis failed", e);
                } finally {
                    setIsValidating(false);
                }
            };

            const timer = setTimeout(runAnalysis, 1000);
            return () => clearTimeout(timer);
        }
    }, [modifiedCode, viewMode]);

    const handleApply = useCallback(async () => {
        setIsApplying(true);
        try {
            // If original is empty (new module), force select all for clean write
            const useSelectAll = !originalCode || originalCode.trim().length === 0;
            await pasteCode(modifiedCode, useSelectAll, originalCode || undefined);
            // Сброс больше не нужен здесь, так как pasteCode вызовет событие RESET_DIFF
        } catch (e: any) {
            const errorMsg = typeof e === 'string' ? e : e?.message || String(e);
            if (errorMsg.includes('CONFLICT')) {
                const isActive = await checkSelection();
                setSelectionActive(isActive);
                setShowConflictDialog(true);
            } else {
                console.error("Apply failed", e);
                alert("Ошибка применения: " + errorMsg);
            }
        } finally {
            setIsApplying(false);
        }
    }, [modifiedCode, originalCode, pasteCode]);

    const handleConflictApplyToAll = useCallback(async () => {
        setShowConflictDialog(false);
        setIsApplying(true);
        try {
            // useSelectAll = true, originalContent = undefined (bypass hash check)
            await pasteCode(modifiedCode, true);
        } catch (e: any) {
            alert("Ошибка применения: " + (e?.message || String(e)));
        } finally {
            setIsApplying(false);
        }
    }, [modifiedCode, pasteCode]);

    const handleConflictApplyToSelection = useCallback(async () => {
        setShowConflictDialog(false);
        setIsApplying(true);
        try {
            // useSelectAll = false, originalContent = undefined (bypass hash check)
            await pasteCode(modifiedCode, false);
        } catch (e: any) {
            alert("Ошибка применения: " + (e?.message || String(e)));
        } finally {
            setIsApplying(false);
        }
    }, [modifiedCode, pasteCode]);

    const handleCodeLoaded = useCallback((code: string, isSelection: boolean) => {
        setOriginalCode(code);
        setModifiedCode(code);
        setActiveDiffContent(''); // Сброс при загрузке
        if (viewMode === 'assistant') setViewMode('split');
    }, [viewMode]);

    const minimize = () => appWindow.minimize();
    const maximize = async () => {
        const isMaximized = await appWindow.isMaximized();
        isMaximized ? appWindow.unmaximize() : appWindow.maximize();
    };
    const close = () => appWindow.close();

    return (
        <div className="flex flex-col h-screen bg-transparent relative overflow-hidden">
            <SettingsPanel isOpen={showSettings} onClose={() => setShowSettings(false)} initialTab={settingsTab as any} />

            {/* Custom Title Bar */}
            <div className="relative h-10 bg-[#09090b] flex items-center justify-between px-4 border-b border-[#27272a] select-none z-50">
                <div data-tauri-drag-region className="absolute inset-0 z-0" onMouseDown={() => appWindow.startDragging()} />
                <div className="relative z-10 flex items-center gap-2 pointer-events-none">
                    <img src={logo} alt="Logo" className="w-5 h-5" />
                    <span className="text-sm font-medium text-zinc-300">Mini AI 1C</span>
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
                    viewMode={viewMode}
                    onViewModeChange={setViewMode}
                    onClearChat={() => {
                        clearChat();
                        setOriginalCode('');
                        setModifiedCode('');
                        setDiagnostics([]);
                        setActiveDiffContent('');
                    }}
                    onOpenSettings={() => setShowSettings(true)}
                    onCodeLoaded={handleCodeLoaded}
                />

                <div className="flex flex-1 overflow-hidden bg-[#09090b] relative">
                    <div className={`flex flex-1 overflow-hidden transition-all duration-300 ${viewMode === 'code' ? 'hidden' : 'opacity-100 w-full'}`}>
                        <ChatArea
                            originalCode={originalCode}
                            modifiedCode={modifiedCode}
                            onApplyCode={useCallback((code: string) => {
                                setModifiedCode(code);
                                if (viewMode === 'assistant') setViewMode('split');
                            }, [viewMode])}
                            onCommitCode={useCallback((code: string) => {
                                // "Принять" из чата - значит сделать код новым бейзлайном
                                setOriginalCode(code);
                                setModifiedCode(code);
                                setActiveDiffContent('');
                                // Опционально: можно закрывать панель, но лучше оставить её для просмотра
                                // setShowSidePanel(false); 
                            }, [])}
                            onCodeLoaded={handleCodeLoaded}
                            diagnostics={diagnostics}
                            onOpenSettings={(tab) => {
                                setSettingsTab(tab as any);
                                setShowSettings(true);
                            }}
                            onActiveDiffChange={(content) => {
                                setActiveDiffContent(content);
                                if (content && viewMode === 'assistant') setViewMode('split'); // Авто-открытие панели если пришли диффы
                            }}
                            activeDiffContent={activeDiffContent}
                        />
                    </div>

                    <div className={`${viewMode === 'assistant' ? 'hidden' : 'flex'} ${viewMode === 'code' ? 'flex-1 w-full' : ''} z-40 h-full transition-all duration-300`}>
                        <CodeSidePanel
                            isOpen={viewMode !== 'assistant'}
                            isFullWidth={viewMode === 'code'}
                            onClose={() => setViewMode('assistant')}
                            originalCode={originalCode}
                            modifiedCode={modifiedCode}
                            onModifiedCodeChange={setModifiedCode}
                            diagnostics={diagnostics}
                            onApply={handleApply}
                            isApplying={isApplying}
                            isValidating={isValidating}
                            activeDiffContent={activeDiffContent}
                            onActiveDiffChange={setActiveDiffContent}
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
