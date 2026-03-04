import { useState, useCallback, useEffect } from 'react';

export function useResizing(initialWidth: number) {
    const [width, setWidth] = useState(initialWidth);
    const [isResizing, setIsResizing] = useState(false);
    const [isExpanded, setIsExpanded] = useState(true);

    const startResizing = useCallback((e: React.MouseEvent) => {
        setIsResizing(true);
        e.preventDefault();
    }, []);

    const stopResizing = useCallback(() => {
        setIsResizing(false);
    }, []);

    const resize = useCallback((e: MouseEvent) => {
        if (isResizing) {
            const newWidth = window.innerWidth - e.clientX;
            // Constrain width between 280 and 80% of window
            if (newWidth > 280 && newWidth < window.innerWidth * 0.8) {
                setWidth(newWidth);
                if (newWidth > 400) setIsExpanded(true);
                else setIsExpanded(false);
            }
        }
    }, [isResizing]);

    useEffect(() => {
        if (isResizing) {
            window.addEventListener('mousemove', resize);
            window.addEventListener('mouseup', stopResizing);
        } else {
            window.removeEventListener('mousemove', resize);
            window.removeEventListener('mouseup', stopResizing);
        }
        return () => {
            window.removeEventListener('mousemove', resize);
            window.removeEventListener('mouseup', stopResizing);
        };
    }, [isResizing, resize, stopResizing]);

    return {
        width,
        setWidth,
        isResizing,
        isExpanded,
        setIsExpanded,
        startResizing
    };
}
