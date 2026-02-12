import { Settings, PanelRight, Trash2, RefreshCw, Monitor, ChevronDown, FileText, MousePointerClick } from 'lucide-react';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { useState } from 'react';

interface HeaderProps {
    bslStatus: { connected: boolean } | null;
    showSidePanel: boolean;
    toggleSidePanel: () => void;
    onClearChat: () => void;
    onOpenSettings: () => void;
    onCodeLoaded: (code: string, isSelection: boolean) => void;
}

export function Header({ bslStatus, showSidePanel, toggleSidePanel, onClearChat, onOpenSettings, onCodeLoaded }: HeaderProps) {

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

            <div className="flex items-center gap-2">
                <button
                    onClick={toggleSidePanel}
                    className={`p-2 hover:bg-[#27272a] rounded-lg transition-colors ${showSidePanel ? 'text-blue-400 bg-blue-500/10' : 'text-zinc-400'}`}
                    title="Toggle Code Panel"
                >
                    <PanelRight className="w-4 h-4" />
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
