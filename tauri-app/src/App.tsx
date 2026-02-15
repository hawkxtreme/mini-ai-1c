import React, { useEffect } from 'react';
import { SettingsProvider, ProfileProvider, ConfiguratorProvider, BslProvider, ChatProvider } from './contexts';
import { MainLayout } from './components/layout/MainLayout';


function App() {
  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };
    document.addEventListener('contextmenu', handleContextMenu);
    return () => document.removeEventListener('contextmenu', handleContextMenu);
  }, []);

  return (
    <SettingsProvider>
      <ProfileProvider>
        <ConfiguratorProvider>
          <BslProvider>
            <ChatProvider>
              <MainLayout />
            </ChatProvider>
          </BslProvider>
        </ConfiguratorProvider>
      </ProfileProvider>
    </SettingsProvider>
  );
}

export default App;
