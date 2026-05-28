import { parseConfiguratorTitle, parseConfiguratorTitleFull } from './configurator';

export interface ConfiguratorWindowDescriptor {
    hwnd: number;
    title: string;
    process_id: number;
}

export interface ConfiguratorWindowBinding {
    selected_window_hwnd: number | null;
    selected_window_pid?: number | null;
    selected_window_title?: string | null;
    selected_config_name?: string | null;
}

export type ConfiguratorBindingStatus =
    | 'unselected'
    | 'resolved'
    | 'rebound'
    | 'missing'
    | 'ambiguous';

export interface ConfiguratorBindingResolution<TWindow extends ConfiguratorWindowDescriptor> {
    status: ConfiguratorBindingStatus;
    activeWindow: TWindow | null;
    nextBinding: ConfiguratorWindowBinding;
    matchedBy: 'hwnd' | 'process_id' | 'title' | 'config_name' | null;
    candidates: TWindow[];
}

function sanitizePositiveInt(value: number | null | undefined): number | null {
    return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null;
}

function sanitizeString(value: string | null | undefined): string | null {
    const trimmed = value?.trim();
    return trimmed ? trimmed : null;
}

function normalizeKey(value: string | null | undefined): string | null {
    const sanitized = sanitizeString(value);
    if (!sanitized) return null;
    return sanitized.replace(/\s+/g, ' ').trim().toLowerCase();
}

function extractConfigName(title: string | null | undefined): string | null {
    const sanitizedTitle = sanitizeString(title);
    if (!sanitizedTitle) return null;

    const parsed = parseConfiguratorTitleFull(sanitizedTitle);
    if (parsed.config_name?.trim()) {
        return parsed.config_name.trim();
    }

    const fallback = parseConfiguratorTitle(sanitizedTitle).trim();
    if (!fallback || fallback === 'Конфигуратор' || fallback === 'Configurator' || fallback === '1C:Enterprise') {
        return null;
    }
    return fallback;
}

function bindingWithWindow<TWindow extends ConfiguratorWindowDescriptor>(
    binding: ConfiguratorWindowBinding,
    window: TWindow,
): ConfiguratorWindowBinding {
    return {
        ...binding,
        selected_window_hwnd: sanitizePositiveInt(window.hwnd),
        selected_window_pid: sanitizePositiveInt(window.process_id),
        selected_window_title: sanitizeString(window.title),
        selected_config_name: extractConfigName(window.title),
    };
}

function releaseMissingHwnd(binding: ConfiguratorWindowBinding): ConfiguratorWindowBinding {
    return {
        ...binding,
        selected_window_hwnd: null,
    };
}

function hasBindingIdentity(binding: ConfiguratorWindowBinding): boolean {
    return Boolean(
        sanitizePositiveInt(binding.selected_window_hwnd) ||
        sanitizePositiveInt(binding.selected_window_pid) ||
        sanitizeString(binding.selected_window_title) ||
        sanitizeString(binding.selected_config_name),
    );
}

export function bindConfiguratorWindow<TWindow extends ConfiguratorWindowDescriptor>(window: TWindow): ConfiguratorWindowBinding {
    return bindingWithWindow(
        {
            selected_window_hwnd: null,
            selected_window_pid: null,
            selected_window_title: null,
            selected_config_name: null,
        },
        window,
    );
}

export function areConfiguratorBindingsEqual(
    left: ConfiguratorWindowBinding,
    right: ConfiguratorWindowBinding,
): boolean {
    return sanitizePositiveInt(left.selected_window_hwnd) === sanitizePositiveInt(right.selected_window_hwnd)
        && sanitizePositiveInt(left.selected_window_pid) === sanitizePositiveInt(right.selected_window_pid)
        && sanitizeString(left.selected_window_title) === sanitizeString(right.selected_window_title)
        && sanitizeString(left.selected_config_name) === sanitizeString(right.selected_config_name);
}

export function getConfiguratorBindingDisplayTitle(
    binding: ConfiguratorWindowBinding,
    activeWindow?: Pick<ConfiguratorWindowDescriptor, 'title'> | null,
): string {
    const activeTitle = sanitizeString(activeWindow?.title);
    if (activeTitle) {
        return parseConfiguratorTitle(activeTitle);
    }

    const bindingConfigName = sanitizeString(binding.selected_config_name);
    if (bindingConfigName) {
        return bindingConfigName;
    }

    const bindingTitle = sanitizeString(binding.selected_window_title);
    if (bindingTitle) {
        return parseConfiguratorTitle(bindingTitle);
    }

    return 'Конфигуратор';
}

export function resolveConfiguratorBinding<TWindow extends ConfiguratorWindowDescriptor>(
    binding: ConfiguratorWindowBinding,
    windows: readonly TWindow[],
): ConfiguratorBindingResolution<TWindow> {
    const selectedHwnd = sanitizePositiveInt(binding.selected_window_hwnd);
    const selectedPid = sanitizePositiveInt(binding.selected_window_pid);
    const selectedTitleKey = normalizeKey(binding.selected_window_title);
    const selectedConfigKey = normalizeKey(binding.selected_config_name ?? extractConfigName(binding.selected_window_title));

    if (!hasBindingIdentity(binding)) {
        return {
            status: 'unselected',
            activeWindow: null,
            nextBinding: releaseMissingHwnd(binding),
            matchedBy: null,
            candidates: [],
        };
    }

    if (selectedHwnd) {
        const exactWindow = windows.find(window => sanitizePositiveInt(window.hwnd) === selectedHwnd);
        if (exactWindow) {
            return {
                status: 'resolved',
                activeWindow: exactWindow,
                nextBinding: bindingWithWindow(binding, exactWindow),
                matchedBy: 'hwnd',
                candidates: [exactWindow],
            };
        }
    }

    if (selectedPid) {
        const pidCandidates = windows.filter(window => sanitizePositiveInt(window.process_id) === selectedPid);
        if (pidCandidates.length === 1) {
            return {
                status: 'rebound',
                activeWindow: pidCandidates[0],
                nextBinding: bindingWithWindow(binding, pidCandidates[0]),
                matchedBy: 'process_id',
                candidates: pidCandidates,
            };
        }
        if (pidCandidates.length > 1) {
            return {
                status: 'ambiguous',
                activeWindow: null,
                nextBinding: releaseMissingHwnd(binding),
                matchedBy: 'process_id',
                candidates: pidCandidates,
            };
        }
    }

    if (selectedTitleKey) {
        const titleCandidates = windows.filter(window => normalizeKey(window.title) === selectedTitleKey);
        if (titleCandidates.length === 1) {
            return {
                status: 'rebound',
                activeWindow: titleCandidates[0],
                nextBinding: bindingWithWindow(binding, titleCandidates[0]),
                matchedBy: 'title',
                candidates: titleCandidates,
            };
        }
        if (titleCandidates.length > 1) {
            return {
                status: 'ambiguous',
                activeWindow: null,
                nextBinding: releaseMissingHwnd(binding),
                matchedBy: 'title',
                candidates: titleCandidates,
            };
        }
    }

    if (selectedConfigKey) {
        const configCandidates = windows.filter(window => normalizeKey(extractConfigName(window.title)) === selectedConfigKey);
        if (configCandidates.length === 1) {
            return {
                status: 'rebound',
                activeWindow: configCandidates[0],
                nextBinding: bindingWithWindow(binding, configCandidates[0]),
                matchedBy: 'config_name',
                candidates: configCandidates,
            };
        }
        if (configCandidates.length > 1) {
            return {
                status: 'ambiguous',
                activeWindow: null,
                nextBinding: releaseMissingHwnd(binding),
                matchedBy: 'config_name',
                candidates: configCandidates,
            };
        }
    }

    return {
        status: 'missing',
        activeWindow: null,
        nextBinding: releaseMissingHwnd(binding),
        matchedBy: null,
        candidates: [],
    };
}
