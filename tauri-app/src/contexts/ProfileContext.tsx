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

    const loadProfiles = async () => {
        try {
            const data = await api.getProfiles();
            setStore(data);
        } catch (e) {
            console.error("Failed to load profiles:", e);
        }
    };

    useEffect(() => {
        loadProfiles();
    }, []);

    const handleSetActiveProfile = async (id: string) => {
        await api.setActiveProfile(id);
        await loadProfiles();
    };

    const handleSaveProfile = async (profile: LLMProfile, apiKey?: string) => {
        await api.saveProfile(profile, apiKey);
        await loadProfiles();
    };

    const handleDeleteProfile = async (id: string) => {
        await api.deleteProfile(id);
        await loadProfiles();
    };

    const activeProfile = store?.profiles.find(p => p.id === store.active_profile_id);

    return (
        <ProfileContext.Provider value={{
            profiles: store?.profiles || [],
            activeProfileId: store?.active_profile_id || 'default',
            activeProfile,
            loadProfiles,
            setActiveProfile: handleSetActiveProfile,
            saveProfile: handleSaveProfile,
            deleteProfile: handleDeleteProfile
        }}>
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
