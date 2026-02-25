import { invoke } from '@tauri-apps/api/core';
import { CliAuthInitResponse, CliAuthStatus, CliStatus } from '../types/settings';

export const cliProvidersApi = {
    /**
     * Запустить процесс OAuth Device Flow для указанного провайдера
     */
    async authStart(provider: string): Promise<CliAuthInitResponse> {
        return await invoke('cli_auth_start', { provider });
    },

    /**
     * Опросить статус авторизации по коду устройства
     */
    async authPoll(provider: string, deviceCode: string, codeVerifier?: string): Promise<CliAuthStatus> {
        return await invoke('cli_auth_poll', { provider, deviceCode, codeVerifier });
    },

    /**
     * Сохранить токены в Keychain
     */
    async saveToken(
        provider: string,
        accessToken: string,
        refreshToken: string | null,
        expiresAt: number,
        resourceUrl: string | null
    ): Promise<void> {
        return await invoke('cli_save_token', {
            provider,
            accessToken,
            refreshToken,
            expiresAt,
            resourceUrl
        });
    },

    /**
     * Получить текущий статус авторизации и использование лимитов
     */
    async getStatus(provider: string): Promise<CliStatus> {
        return await invoke('cli_get_status', { provider });
    },

    /**
     * Удалить токены и выйти из аккаунта
     */
    async logout(provider: string): Promise<void> {
        return await invoke('cli_logout', { provider });
    }
};
