import { SettingsProvider, ProfileProvider, ConfiguratorProvider, BslProvider, ChatProvider } from './contexts';
import { MainLayout } from './components/layout/MainLayout';


function App() {
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
