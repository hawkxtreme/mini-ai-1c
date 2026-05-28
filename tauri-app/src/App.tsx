import React, { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { SettingsProvider, ProfileProvider, ConfiguratorProvider, BslProvider, ChatProvider } from './contexts';
import { MainLayout } from './components/layout/MainLayout';
import { useSettings } from './contexts/SettingsContext';
import { useQuickActions } from './hooks/useQuickActions';
import { installPerformanceDiagnostics } from './utils/performanceDiagnostics';

/** Mounts the quick-actions listener (needs Settings + Profiles context). */
function QuickActionsMount() {
  useQuickActions();
  return null;
}

function ConfiguratorOverlayMount() {
  const { settings } = useSettings();
  const overlayEnabled = settings?.configurator?.editor_bridge_enabled === true;

  useEffect(() => {
    const unlisten = listen<{ x: number; y: number; hwnd: number; childHwnd: number }>(
      'configurator-rclick',
      async ({ payload }) => {
        if (!overlayEnabled) {
          return;
        }

        try {
          await invoke('show_overlay', {
            confHwnd: payload.hwnd,
            cursorX: payload.x,
            cursorY: payload.y,
            state: {
              phase: 'menu',
              confHwnd: payload.hwnd,
              useSelectAll: false,
              targetX: payload.x,
              targetY: payload.y,
              targetChildHwnd: payload.childHwnd,
            },
          });
        } catch (e) {
          console.error('[App] show_overlay error:', e);
        }
      },
    );

    return () => {
      unlisten.then(f => f());
    };
  }, [overlayEnabled]);

  useEffect(() => {
    if (!overlayEnabled) {
      void invoke('hide_overlay', { confHwnd: null }).catch(() => {});
    }
  }, [overlayEnabled]);

  return null;
}

function App() {
  useEffect(() => {
    installPerformanceDiagnostics();

    // Disable browser context menu inside the app
    const handleContextMenu = (e: MouseEvent) => { e.preventDefault(); };
    document.addEventListener('contextmenu', handleContextMenu);

return () => {
      document.removeEventListener('contextmenu', handleContextMenu);
    };
  }, []);

  return (
    <SettingsProvider>
      <ProfileProvider>
        <ConfiguratorProvider>
          <BslProvider>
            <ChatProvider>
              <ConfiguratorOverlayMount />
              <QuickActionsMount />
              <MainLayout />
            </ChatProvider>
          </BslProvider>
        </ConfiguratorProvider>
      </ProfileProvider>
    </SettingsProvider>
  );
}

export default App;
