export function shouldResetApiKeyDraft(
    previousEditingId: string | null,
    nextEditingId: string | null,
): boolean {
    if (!nextEditingId) {
        return false;
    }

    return previousEditingId !== nextEditingId;
}
