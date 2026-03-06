import React, { createContext, useContext, useEffect, useState } from 'react';
import * as api from '../api';

import { LLMProfile, ProfileStore } from '../api';

export type { LLMProfile, ProfileStore };

interface ProfileContextType {
    profiles: LLMProfile[];
    activeProfileId: string;
    activeProfile: LLMProfile | undefined;
    loadProfiles: () => Promise<void>;
    setActiveProfile: (id: string) => Promise<void>;
    saveProfile: (profile: LLMProfile, apiKey?: string) => Promise<void>;
    deleteProfile: (id: string) => Promise<void>;
}

const ProfileContext = createContext<ProfileContextType | undefined>(undefined);

export function ProfileProvider({ children }: { children: React.ReactNode }) {
    const [store, setStore] = useState<ProfileStore | null>(null);

    const loadProfiles = React.useCallback(async () => {
        try {
            const data = await api.getProfiles();
            setStore(data);
        } catch (e) {
            console.error("Failed to load profiles:", e);
        }
    }, []);

    useEffect(() => {
        loadProfiles();
    }, [loadProfiles]);

    const handleSetActiveProfile = React.useCallback(async (id: string) => {
        await api.setActiveProfile(id);
        await loadProfiles();
    }, [loadProfiles]);

    const handleSaveProfile = React.useCallback(async (profile: LLMProfile, apiKey?: string) => {
        await api.saveProfile(profile, apiKey);
        await loadProfiles();
    }, [loadProfiles]);

    const handleDeleteProfile = React.useCallback(async (id: string) => {
        await api.deleteProfile(id);
        await loadProfiles();
    }, [loadProfiles]);

    const activeProfile = React.useMemo(() =>
        store?.profiles.find(p => p.id === store.active_profile_id),
        [store]
    );

    const value = React.useMemo(() => ({
        profiles: store?.profiles || [],
        activeProfileId: store?.active_profile_id || 'default',
        activeProfile,
        loadProfiles,
        setActiveProfile: handleSetActiveProfile,
        saveProfile: handleSaveProfile,
        deleteProfile: handleDeleteProfile
    }), [store, activeProfile, loadProfiles, handleSetActiveProfile, handleSaveProfile, handleDeleteProfile]);

    return (
        <ProfileContext.Provider value={value}>
            {children}
        </ProfileContext.Provider>
    );
}

export function useProfiles() {
    const context = useContext(ProfileContext);
    if (context === undefined) {
        throw new Error('useProfiles must be used within a ProfileProvider');
    }
    return context;
}
