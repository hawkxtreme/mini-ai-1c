import React, { useEffect, useRef, useState } from 'react';
import { SlashCommand } from '../../types/settings';
import { Terminal, Command } from 'lucide-react';

interface CommandMenuProps {
    commands: SlashCommand[];
    onSelect: (command: SlashCommand) => void;
    onClose: () => void;
    anchorRect: DOMRect | null;
}

export function CommandMenu({ commands, onSelect, onClose, anchorRect }: CommandMenuProps) {
    const [selectedIndex, setSelectedIndex] = useState(0);
    const menuRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSelectedIndex(prev => (prev + 1) % commands.length);
            } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedIndex(prev => (prev - 1 + commands.length) % commands.length);
            } else if (e.key === 'Enter') {
                e.preventDefault();
                if (commands[selectedIndex]) {
                    onSelect(commands[selectedIndex]);
                }
            } else if (e.key === 'Escape') {
                e.preventDefault();
                onClose();
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [commands, selectedIndex, onSelect, onClose]);

    useEffect(() => {
        // Scroll selected item into view
        const selectedElement = menuRef.current?.children[selectedIndex] as HTMLElement;
        if (selectedElement) {
            selectedElement.scrollIntoView({ block: 'nearest' });
        }
    }, [selectedIndex]);

    if (commands.length === 0) return null;

    return (
        <div
            ref={menuRef}
            className="absolute bottom-full left-0 mb-2 w-full max-w-[580px] bg-[#1f1f23] border border-[#27272a] rounded-xl shadow-2xl z-50 overflow-hidden animate-in fade-in slide-in-from-bottom-2 duration-200 ring-1 ring-black/50"
            style={{
                maxHeight: '300px',
                overflowY: 'auto'
            }}
        >
            <div className="flex items-center gap-2 px-3 py-2 bg-[#27272a]/50 border-b border-[#27272a] text-[10px] text-zinc-500 uppercase tracking-wider font-bold">
                <Terminal size={12} className="text-blue-500" />
                Быстрые команды
            </div>
            {commands.map((cmd, index) => (
                <div
                    key={cmd.id}
                    onMouseDown={(e) => {
                        e.preventDefault();
                        onSelect(cmd);
                    }}
                    onMouseEnter={() => setSelectedIndex(index)}
                    className={`px-4 py-3 cursor-pointer transition-all flex items-start gap-4 ${index === selectedIndex ? 'bg-blue-600/10 border-l-2 border-blue-500' : 'hover:bg-[#27272a] border-l-2 border-transparent'
                        }`}
                >
                    <div className={`p-2 rounded-lg transition-colors ${index === selectedIndex ? 'bg-blue-500/20 text-blue-400' : 'bg-zinc-800 text-zinc-500'}`}>
                        <Command size={16} />
                    </div>
                    <div className="flex flex-col gap-0.5 min-w-0">
                        <div className="flex items-center gap-2">
                            <span className={`text-sm font-bold ${index === selectedIndex ? 'text-blue-400' : 'text-zinc-200'}`}>
                                /{cmd.command}
                            </span>
                            {cmd.is_system && (
                                <span className="text-[9px] bg-zinc-800 text-zinc-500 px-1.5 py-0.5 rounded border border-zinc-700/50 uppercase font-mono tracking-tighter">
                                    System
                                </span>
                            )}
                        </div>
                        <span className="text-[11px] text-zinc-500 line-clamp-2 leading-tight">
                            {cmd.description}
                        </span>
                    </div>
                </div>
            ))}
        </div>
    );
}
