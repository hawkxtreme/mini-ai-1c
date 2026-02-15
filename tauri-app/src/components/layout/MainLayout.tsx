import { useState, useEffect, useCallback, useMemo } from 'react';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
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
import logo from '../../assets/logo.png';

export function MainLayout() {
    const { settings } = useSettings();
    const { status: bslStatus, analyzeCode } = useBsl();
    const { clearChat } = useChat();
    const { pasteCode, checkSelection } = useConfigurator();

    const [showSidePanel, setShowSidePanel] = useState(false);
    const [showSettings, setShowSettings] = useState(false);
    const [isApplying, setIsApplying] = useState(false);
    const [isValidating, setIsValidating] = useState(false);

    const [originalCode, setOriginalCode] = useState('');
    const [modifiedCode, setModifiedCode] = useState('');
    const [diagnostics, setDiagnostics] = useState<any[]>([]);
    const [showConflictDialog, setShowConflictDialog] = useState(false);
    const [selectionActive, setSelectionActive] = useState(true);

    const appWindow = getCurrentWindow();

    // Window resizing logic
    useEffect(() => {
        if (showSidePanel) {
            const ensureWidth = async () => {
                try {
                    const factor = await appWindow.scaleFactor();
                    const innerSize = await appWindow.innerSize();
                    const logicalWidth = innerSize.width / factor;
                    const logicalHeight = innerSize.height / factor;
                    const minWidthRequired = 950;
                    if (logicalWidth < minWidthRequired) {
                        await appWindow.setSize(new LogicalSize(minWidthRequired, logicalHeight));
                    }
                } catch (error) {
                    console.error('Failed to resize window:', error);
                }
            };
            ensureWidth();
        }
    }, [showSidePanel]);

    // Analysis effect
    useEffect(() => {
        if (showSidePanel && modifiedCode) {
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
    }, [modifiedCode, showSidePanel]);

    const handleApply = useCallback(async () => {
        setIsApplying(true);
        try {
            // If original is empty (new module), force select all for clean write
            const useSelectAll = !originalCode || originalCode.trim().length === 0;
            await pasteCode(modifiedCode, useSelectAll, originalCode || undefined);
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
        setShowSidePanel(true);
    }, []);

    const minimize = () => appWindow.minimize();
    const maximize = async () => {
        const isMaximized = await appWindow.isMaximized();
        isMaximized ? appWindow.unmaximize() : appWindow.maximize();
    };
    const close = () => appWindow.close();

    return (
        <div className="flex flex-col h-screen bg-transparent">
            <SettingsPanel isOpen={showSettings} onClose={() => setShowSettings(false)} />

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

            <Header
                bslStatus={bslStatus}
                showSidePanel={showSidePanel}
                toggleSidePanel={() => setShowSidePanel(!showSidePanel)}
                onClearChat={() => {
                    clearChat();
                    setOriginalCode('');
                    setModifiedCode('');
                    setDiagnostics([]);
                }}
                onOpenSettings={() => setShowSettings(true)}
                onCodeLoaded={handleCodeLoaded}
            />

            <div className="flex flex-1 overflow-hidden bg-[#09090b] relative">
                <ChatArea
                    modifiedCode={modifiedCode}
                    onApplyCode={useCallback((code: string) => {
                        setModifiedCode(code);
                        setShowSidePanel(true);
                    }, [])}
                    onCodeLoaded={handleCodeLoaded}
                    diagnostics={diagnostics}
                />

                <div className={`z-40 h-full border-l border-[#27272a] transition-all duration-300 ${showSidePanel ? 'flex' : 'hidden'}`}>
                    <CodeSidePanel
                        isOpen={showSidePanel}
                        onClose={() => setShowSidePanel(false)}
                        originalCode={originalCode}
                        modifiedCode={modifiedCode}
                        onModifiedCodeChange={setModifiedCode}
                        diagnostics={diagnostics}
                        onApply={handleApply}
                        isApplying={isApplying}
                        isValidating={isValidating}
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
    );
}
