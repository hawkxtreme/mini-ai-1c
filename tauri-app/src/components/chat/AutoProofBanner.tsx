import React, { useState, useEffect } from 'react';
import { X, Check } from 'lucide-react';

interface AutoProofBannerProps {
    onApply: () => void;
    onCancel: () => void;
}

export const AutoProofBanner: React.FC<AutoProofBannerProps> = ({
    onApply,
    onCancel
}) => {
    return (
        <div className="bg-blue-900/20 border border-blue-500/30 rounded-lg p-3 mb-4 animate-in fade-in slide-in-from-top-2">
            <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2 text-blue-200">
                    <Check size={16} className="text-blue-400" />
                    <span className="font-medium text-sm">
                        Код готов к применению
                    </span>
                </div>
            </div>

            <div className="flex gap-2 justify-end mt-2">
                <button
                    onClick={onCancel}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium 
                             text-gray-300 hover:text-white hover:bg-white/10 transition-colors"
                >
                    <X size={14} /> Отмена
                </button>
                <button
                    onClick={onApply}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium 
                             bg-blue-600 hover:bg-blue-500 text-white transition-colors shadow-sm"
                >
                    <Check size={14} /> Применить
                </button>
            </div>
        </div>
    );
};
