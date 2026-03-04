export interface BslDiagnostic {
    line: number;
    message: string;
    severity: string;
}

export interface CodeSidePanelProps {
    isOpen: boolean;
    onClose: () => void;
    originalCode: string;
    modifiedCode: string;
    onModifiedCodeChange: (code: string) => void;
    diagnostics: BslDiagnostic[];
    onApply: () => void;
    isApplying: boolean;
    isValidating: boolean;
    activeDiffContent?: string;
    onActiveDiffChange?: (content: string) => void;
    onDiffRejected?: () => void;
    isFullWidth?: boolean;
}
