import type { LLMProfile } from '../api/profiles';

export interface ModelMetadata {
    id: string;
    context_window?: number | null;
}

export interface ApplySelectedModelOptions {
    syncMaxTokens?: boolean;
}

export const applySelectedModelMetadata = (
    profile: LLMProfile,
    model: ModelMetadata,
    options: ApplySelectedModelOptions = {},
): LLMProfile => {
    const next: LLMProfile = {
        ...profile,
        model: model.id,
        context_window_override: model.context_window ?? profile.context_window_override,
    };
    if (options.syncMaxTokens && model.context_window) {
        next.max_tokens = model.context_window;
    }
    return next;
};

export const applyFetchedModelMetadata = (
    profile: LLMProfile,
    model: ModelMetadata,
): LLMProfile => ({
    ...profile,
    context_window_override: model.context_window ?? profile.context_window_override,
});

