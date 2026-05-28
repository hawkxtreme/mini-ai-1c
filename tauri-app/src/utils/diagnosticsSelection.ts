export interface DiagnosticSelectionItem {
    line: number;
    message: string;
    severity?: string;
}

export interface DiagnosticsSelectionResolution<T extends DiagnosticSelectionItem> {
    effectiveDiagnostics: T[];
    selectionWasExplicit: boolean;
    selectionWasStale: boolean;
}

export function getDiagnosticSelectionKey(diagnostic: DiagnosticSelectionItem): string {
    return `${diagnostic.line}:${diagnostic.severity ?? ''}:${diagnostic.message}`;
}

export function resolveEffectiveSelectedDiagnostics<T extends DiagnosticSelectionItem>(
    diagnostics: T[],
    selectedDiagnostics?: T[] | null,
): DiagnosticsSelectionResolution<T> {
    if (selectedDiagnostics === null || selectedDiagnostics === undefined) {
        return {
            effectiveDiagnostics: diagnostics,
            selectionWasExplicit: false,
            selectionWasStale: false,
        };
    }

    const currentDiagnosticKeys = new Set(diagnostics.map(getDiagnosticSelectionKey));
    const matchingSelectedDiagnostics = selectedDiagnostics.filter((diagnostic) =>
        currentDiagnosticKeys.has(getDiagnosticSelectionKey(diagnostic)),
    );

    if (matchingSelectedDiagnostics.length !== selectedDiagnostics.length) {
        return {
            effectiveDiagnostics: diagnostics,
            selectionWasExplicit: false,
            selectionWasStale: true,
        };
    }

    return {
        effectiveDiagnostics: matchingSelectedDiagnostics,
        selectionWasExplicit: true,
        selectionWasStale: false,
    };
}
