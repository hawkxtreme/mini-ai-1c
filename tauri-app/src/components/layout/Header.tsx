import { Settings, PanelRight, Trash2, Maximize2, Minimize2, Pin, MessageSquare, Columns, Code2 } from 'lucide-react';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { useState, useEffect, useRef } from 'react';

interface HeaderProps {
    bslStatus: { connected: boolean } | null;
    viewMode: 'assistant' | 'split' | 'code';
    onViewModeChange: (mode: 'assistant' | 'split' | 'code') => void;
    onClearChat: () => void;
    onOpenSettings: () => void;
    onCodeLoaded: (code: string, isSelection: boolean) => void;
}

export function Header({ bslStatus, viewMode, onViewModeChange, onClearChat, onOpenSettings, onCodeLoaded }: HeaderProps) {
    const [isCompact, setIsCompact] = useState(false);
    const { snapToConfigurator } = useConfigurator();
    const sliderRef = useRef<HTMLDivElement>(null);
    const isDragging = useRef(false);

    useEffect(() => {
        const updateCompactStatus = () => {
            setIsCompact(window.innerWidth < 500);
        };

        window.addEventListener('resize', updateCompactStatus);
        updateCompactStatus(); // Initial check

        return () => window.removeEventListener('resize', updateCompactStatus);
    }, []);

    const toggleCompactMode = async () => {
        const appWindow = getCurrentWindow();
        const size = await appWindow.innerSize();
        const factor = await appWindow.scaleFactor();
        const logicalWidth = size.width / factor;
        const currentHeight = size.height / factor;

        const goingToCompact = logicalWidth >= 500; // If current width is >= 500, we are going to compact (400)
        const newWidth = goingToCompact ? 400 : 700;

        if (goingToCompact && viewMode !== 'assistant') {
            onViewModeChange('assistant'); // Close side panel when going compact
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
                    <div className="w-[36px] flex justify-center"><MessageSquare className="w-3.5 h-3.5" /></div>
                    <div className="w-[36px] flex justify-center"><Columns className="w-3.5 h-3.5" /></div>
                    <div className="w-[36px] flex justify-center"><Code2 className="w-3.5 h-3.5" /></div>
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
