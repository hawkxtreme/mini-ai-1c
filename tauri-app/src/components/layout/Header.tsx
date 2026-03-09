import { Settings, PanelRight, Trash2, Maximize2, Minimize2, Pin, MessageSquare, Columns, Code2, AlertTriangle, Bell, X, Info } from 'lucide-react';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { useSettings } from '../../contexts/SettingsContext';
import { useState, useEffect, useRef, useMemo } from 'react';

const PRESET_MCP_NOTIFICATIONS = [
    {
        id: 'builtin-1c-help',
        name: '1С:Справка',
        description: 'Официальная документация 1С прямо в чате — AI перестанет выдумывать несуществующие функции и методы.',
    },
    {
        id: 'builtin-1c-search',
        name: '1С:Поиск по конфигурации',
        description: 'Поиск процедур и объектов в вашей конфигурации. AI видит реальный код проекта и даёт точные ответы.',
    },
    {
        id: 'builtin-1c-naparnik',
        name: '1C:Напарник',
        description: 'Готовые паттерны и шаблоны кода 1С. Помогает следовать стандартам разработки.',
    },
    {
        id: 'builtin-1c-metadata',
        name: '1C:Метаданные',
        description: 'Структура метаданных конфигурации: справочники, реквизиты, ТЧ. AI видит схему базы и генерирует точный код.',
    },
];

interface HeaderProps {
    bslStatus: { connected: boolean } | null;
    nodeAvailable: boolean | null;
    viewMode: 'assistant' | 'split' | 'code';
    onViewModeChange: (mode: 'assistant' | 'split' | 'code') => void;
    onClearChat: () => void;
    onOpenSettings: (tab?: string) => void;
    onCodeLoaded: (code: string, isSelection: boolean) => void;
}

export function Header({ bslStatus, nodeAvailable, viewMode, onViewModeChange, onClearChat, onOpenSettings, onCodeLoaded }: HeaderProps) {
    const [isCompact, setIsCompact] = useState(false);
    const { snapToConfigurator } = useConfigurator();
    const { settings, updateSettings } = useSettings();
    const sliderRef = useRef<HTMLDivElement>(null);
    const isDragging = useRef(false);
    const [notifOpen, setNotifOpen] = useState(false);
    const notifRef = useRef<HTMLDivElement>(null);
    const [dismissed, setDismissed] = useState<Record<string, boolean>>({});

    useEffect(() => {
        const updateCompactStatus = () => {
            setIsCompact(window.innerWidth < 500);
        };

        window.addEventListener('resize', updateCompactStatus);
        updateCompactStatus();

        return () => window.removeEventListener('resize', updateCompactStatus);
    }, []);

    // Close dropdown on outside click
    useEffect(() => {
        const handler = (e: MouseEvent) => {
            if (notifRef.current && !notifRef.current.contains(e.target as Node)) {
                setNotifOpen(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, []);

    const notifications = useMemo(() => {
        if (!settings?.onboarding_completed) return [];
        return PRESET_MCP_NOTIFICATIONS.filter(n => {
            if (dismissed[n.id]) return false;
            if (localStorage.getItem(`mcp_notif_dismissed_${n.id}`) === 'true') return false;
            const server = settings.mcp_servers?.find(s => s.id === n.id);
            return server && !server.enabled;
        });
    }, [settings, dismissed]);

    const handleDismiss = (id: string) => {
        localStorage.setItem(`mcp_notif_dismissed_${id}`, 'true');
        setDismissed(prev => ({ ...prev, [id]: true }));
    };

    const handleEnable = async (id: string) => {
        if (!settings) return;
        handleDismiss(id);
        await updateSettings({
            ...settings,
            mcp_servers: settings.mcp_servers.map(s => s.id === id ? { ...s, enabled: true } : s),
        });
    };

    const toggleCompactMode = async () => {
        const appWindow = getCurrentWindow();
        const size = await appWindow.innerSize();
        const factor = await appWindow.scaleFactor();
        const logicalWidth = size.width / factor;
        const currentHeight = size.height / factor;

        const goingToCompact = logicalWidth >= 500;
        const newWidth = goingToCompact ? 400 : 700;

        if (goingToCompact && viewMode !== 'assistant') {
            onViewModeChange('assistant');
        }

        await appWindow.setSize(new LogicalSize(newWidth, currentHeight));
    };

    const handleMouseDown = (e: React.MouseEvent) => {
        isDragging.current = true;
        handleMouseMove(e as any);
    };

    const handleMouseMove = (e: MouseEvent | React.MouseEvent) => {
        if (!isDragging.current || !sliderRef.current) return;

        const rect = sliderRef.current.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const percentage = x / rect.width;

        if (percentage < 0.33) onViewModeChange('assistant');
        else if (percentage < 0.66) onViewModeChange('split');
        else onViewModeChange('code');
    };

    useEffect(() => {
        const handleMouseUp = () => {
            isDragging.current = false;
        };
        window.addEventListener('mouseup', handleMouseUp);
        window.addEventListener('mousemove', handleMouseMove);
        return () => {
            window.removeEventListener('mouseup', handleMouseUp);
            window.removeEventListener('mousemove', handleMouseMove);
        };
    }, []);

    return (
        <div className="flex items-center justify-between px-4 py-2 border-b border-[#27272a] bg-[#09090b]">
            <div className="flex items-center gap-3">
                {/* MCP notifications shutter */}
                {notifications.length > 0 && (
                    <div ref={notifRef} className="relative">
                        <button
                            onClick={() => setNotifOpen(v => !v)}
                            className="relative p-1.5 rounded-md bg-blue-500/10 border border-blue-500/20 hover:bg-blue-500/20 transition-colors"
                            title="Доступны новые MCP-инструменты"
                        >
                            <Bell className="w-3.5 h-3.5 text-blue-400" />
                            <span className="absolute -top-1 -right-1 w-3.5 h-3.5 bg-blue-500 rounded-full text-[8px] font-bold text-white flex items-center justify-center leading-none">
                                {notifications.length}
                            </span>
                        </button>

                        {notifOpen && (
                            <div className="absolute left-0 top-full mt-2 z-50 w-[280px] bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl overflow-hidden animate-in fade-in slide-in-from-top-2 duration-150">
                                <div className="flex items-center justify-between px-3 py-2.5 border-b border-zinc-800">
                                    <span className="text-[11px] font-semibold text-zinc-300 uppercase tracking-wider">Доступные инструменты</span>
                                    <button onClick={() => setNotifOpen(false)} className="text-zinc-500 hover:text-zinc-300 transition-colors">
                                        <X className="w-3.5 h-3.5" />
                                    </button>
                                </div>
                                <div className="flex flex-col divide-y divide-zinc-800/60">
                                    {notifications.map(n => (
                                        <div key={n.id} className="px-3 py-3">
                                            <div className="flex items-start gap-2 mb-2">
                                                <Info className="w-3.5 h-3.5 text-blue-400 shrink-0 mt-0.5" />
                                                <div className="flex-1 min-w-0">
                                                    <div className="flex items-center justify-between gap-1 mb-1">
                                                        <span className="text-[12px] font-semibold text-zinc-200">{n.name}</span>
                                                        <button
                                                            onClick={() => handleDismiss(n.id)}
                                                            className="text-zinc-600 hover:text-zinc-400 transition-colors shrink-0"
                                                            title="Не показывать снова"
                                                        >
                                                            <X className="w-3 h-3" />
                                                        </button>
                                                    </div>
                                                    <p className="text-[11px] text-zinc-500 leading-relaxed mb-2">{n.description}</p>
                                                    <button
                                                        onClick={() => handleEnable(n.id)}
                                                        className="text-[11px] font-medium px-2.5 py-1 rounded-md bg-blue-600/20 hover:bg-blue-600/40 text-blue-300 border border-blue-600/30 transition-colors"
                                                    >
                                                        Включить
                                                    </button>
                                                </div>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                                <div className="px-3 py-2 border-t border-zinc-800 bg-zinc-900/50">
                                    <button
                                        onClick={() => { onOpenSettings('mcp'); setNotifOpen(false); }}
                                        className="text-[11px] text-zinc-500 hover:text-zinc-300 transition-colors"
                                    >
                                        Открыть настройки MCP →
                                    </button>
                                </div>
                            </div>
                        )}
                    </div>
                )}

                {nodeAvailable === false && (
                    <div className="relative group">
                        <button className="p-1.5 rounded-md bg-yellow-500/10 border border-yellow-500/20 hover:bg-yellow-500/20 transition-colors">
                            <AlertTriangle className="w-3.5 h-3.5 text-yellow-400" />
                        </button>
                        <div className="absolute left-0 top-full mt-2 z-50 min-w-[220px] bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl p-3 hidden group-hover:block animate-in fade-in slide-in-from-top-2 duration-150">
                            <p className="text-xs font-semibold text-yellow-400 mb-1.5 flex items-center gap-1.5">
                                <AlertTriangle className="w-3 h-3" /> Проблемы системы
                            </p>
                            <ul className="space-y-1">
                                <li className="text-xs text-zinc-300 flex items-start gap-1.5">
                                    <span className="text-yellow-500 mt-0.5">•</span>
                                    Node.js не найден — встроенные MCP-серверы недоступны
                                </li>
                            </ul>
                            <p className="text-[10px] text-zinc-500 mt-2">Установите Node.js 18+ для работы MCP</p>
                        </div>
                    </div>
                )}
                <div className="flex items-center gap-2 px-2 py-1 rounded-md bg-zinc-900/50 border border-zinc-800/50">
                    <div className={`w-1.5 h-1.5 rounded-full ${!bslStatus ? 'bg-zinc-600 animate-pulse' : bslStatus.connected ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.4)]' : 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.4)]'}`} />
                    <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest hidden md:inline">BSL LS</span>
                    <span className="text-[10px] text-zinc-600 font-medium hidden md:inline">
                        {!bslStatus ? 'Initializing...' : bslStatus.connected ? 'Connected' : 'Offline'}
                    </span>
                </div>
            </div>

            {/* View Mode Switcher (Three-position Slider) */}
            <div
                ref={sliderRef}
                onMouseDown={handleMouseDown}
                className="relative bg-zinc-900 border border-zinc-800 rounded-full h-8 w-[120px] px-1 flex items-center cursor-pointer select-none group"
            >
                {/* Track Background Icons */}
                <div className="absolute inset-x-1 inset-y-0 flex items-center justify-between text-zinc-600">
                    <div
                        id="tour-mode-assistant"
                        onClick={() => onViewModeChange('assistant')}
                        className="w-[36px] flex justify-center hover:text-zinc-400 transition-colors cursor-pointer"
                    >
                        <MessageSquare className="w-3.5 h-3.5" />
                    </div>
                    <div
                        id="tour-mode-split"
                        onClick={() => onViewModeChange('split')}
                        className="w-[36px] flex justify-center hover:text-zinc-400 transition-colors cursor-pointer"
                    >
                        <Columns className="w-3.5 h-3.5" />
                    </div>
                    <div
                        id="tour-mode-code"
                        onClick={() => onViewModeChange('code')}
                        className="w-[36px] flex justify-center hover:text-zinc-400 transition-colors cursor-pointer"
                    >
                        <Code2 className="w-3.5 h-3.5" />
                    </div>
                </div>

                {/* Sliding Indicator */}
                <div
                    className={`absolute w-[36px] h-6 bg-zinc-700/50 border border-zinc-600 rounded-full shadow-lg transition-all duration-200 flex items-center justify-center z-10`}
                    style={{
                        left: viewMode === 'assistant' ? '4px' : viewMode === 'split' ? '42px' : '80px'
                    }}
                >
                    {viewMode === 'assistant' && <MessageSquare className="w-3.5 h-3.5 text-white" />}
                    {viewMode === 'split' && <Columns className="w-3.5 h-3.5 text-white" />}
                    {viewMode === 'code' && <Code2 className="w-3.5 h-3.5 text-white" />}
                </div>
            </div>

            <div className="flex items-center gap-2">
                <button
                    onClick={snapToConfigurator}
                    className="p-2 hover:bg-[#27272a] rounded-lg transition-colors text-zinc-400 group"
                    title="Привязать к окну Конфигуратора"
                >
                    <Pin className="w-4 h-4 group-hover:text-blue-400 transition-colors" />
                </button>
                <button
                    onClick={toggleCompactMode}
                    className="p-2 hover:bg-[#27272a] rounded-lg transition-colors text-zinc-400"
                    title={isCompact ? "Expand Window" : "Compact Window"}
                >
                    {isCompact ? <Maximize2 className="w-4 h-4" /> : <Minimize2 className="w-4 h-4" />}
                </button>
                <div className="w-px h-4 bg-[#27272a] mx-1" />
                <button
                    onClick={onClearChat}
                    className="p-2 hover:bg-[#27272a] rounded-lg transition-colors group"
                    title="Clear Chat & Editor"
                >
                    <Trash2 className="w-4 h-4 text-zinc-400 group-hover:text-red-400 transition-colors" />
                </button>
                <button
                    onClick={onOpenSettings}
                    className="p-2 hover:bg-[#27272a] rounded-lg transition-colors"
                    title="Settings"
                >
                    <Settings className="w-4 h-4 text-zinc-400" />
                </button>
            </div>
        </div>
    );
}
