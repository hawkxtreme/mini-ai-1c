import { Check } from 'lucide-react';
import { useSettings } from '@/contexts/SettingsContext';

interface FooterProps {
    onApply: () => void;
    isApplying: boolean;
    modifiedCode: string;
}

export function Footer({
    onApply,
    isApplying,
    modifiedCode
}: FooterProps) {
    const { settings } = useSettings();
    const isLightTheme = settings?.theme === 'light';

    return (
        <div className={`p-3 border-t flex items-center justify-between ${
            isLightTheme ? 'border-[#d4d4d8] bg-[#f4f4f5]' : 'border-[#27272a] bg-[#18181b]'
        }`}>
            <div className="text-[10px] text-zinc-500 flex items-center gap-2">
            </div>

            <button
                onClick={onApply}
                disabled={isApplying || !modifiedCode.trim()}
                className={`flex items-center gap-2 px-4 py-1.5 rounded text-xs font-medium transition-colors ${isApplying || !modifiedCode.trim()
                    ? isLightTheme
                        ? 'bg-[#e4e4e7] text-[#71717a] cursor-not-allowed'
                        : 'bg-[#27272a] text-zinc-500 cursor-not-allowed'
                    : 'bg-blue-600 hover:bg-blue-500 text-[#ffffff] shadow-lg shadow-blue-500/10'
                    }`}
                id="tour-apply"
            >
                {isApplying ? (
                    <>Applying...</>
                ) : (
                    <>
                        <Check className="w-3.5 h-3.5" />
                        Apply Changes
                    </>
                )}
            </button>
        </div>
    );
}
