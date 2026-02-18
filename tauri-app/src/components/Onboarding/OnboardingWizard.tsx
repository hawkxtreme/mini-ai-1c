import React, { useState, useEffect, useRef } from 'react';
import { useSettings } from '../../contexts/SettingsContext';
import { useProfiles } from '../../contexts/ProfileContext';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Check, Server, Brain, Monitor, ArrowRight, Download, Terminal, Cloud, LogOut, ChevronRight, ChevronLeft, Bot, FileText, PanelRight } from 'lucide-react';
import { LLMProfile } from '../../contexts/ProfileContext';

// --- Steps ---
type Step = 'welcome' | 'environment' | 'llm-setup' | 'mcp-setup' | 'tour' | 'finish';

interface OnboardingWizardProps {
    onComplete: () => void;
}

export const OnboardingWizard: React.FC<OnboardingWizardProps> = ({ onComplete }) => {
    const { settings, updateSettings } = useSettings();
    const { saveProfile, setActiveProfile, deleteProfile } = useProfiles();
    const [step, setStep] = useState<Step>('welcome');
    const wizardRef = useRef<HTMLDivElement>(null);
    const finishingRef = useRef(false);

    // Environment State
    const [javaStatus, setJavaStatus] = useState<'checking' | 'ok' | 'missing'>('checking');
    const [bslStatus, setBslStatus] = useState<'checking' | 'ok' | 'missing'>('checking');
    const [isDownloadingBsl, setIsDownloadingBsl] = useState(false);
    const [bslProgress, setBslProgress] = useState(0);

    // AI State
    const [selectedProvider, setSelectedProvider] = useState<string | null>(null);
    const [apiKey, setApiKey] = useState('');
    const [baseUrl, setBaseUrl] = useState('');
    const [modelName, setModelName] = useState('');
    const [naparnikToken, setNaparnikToken] = useState(''); // 1C:Naparnik Token

    // Tour State
    const [tourStep, setTourStep] = useState(0);
    const [showAbortConfirm, setShowAbortConfirm] = useState(false);
    const [downloadedJarPath, setDownloadedJarPath] = useState<string | null>(null);

    useEffect(() => {
        if (step === 'environment') {
            checkEnvironment();
        }
    }, [step]);

    const checkEnvironment = async () => {
        setJavaStatus('checking');
        setBslStatus('checking');

        try {
            const isJavaOk = await invoke<boolean>('check_java_cmd');
            setJavaStatus(isJavaOk ? 'ok' : 'missing');

            const bslStatus = await invoke<{ installed: boolean }>('check_bsl_status_cmd');
            setBslStatus(bslStatus.installed ? 'ok' : 'missing');
        } catch (e) {
            console.error(e);
            setJavaStatus('missing');
            setBslStatus('missing');
        }
    };

    const handleDownloadBsl = async () => {
        setIsDownloadingBsl(true);
        setBslProgress(0);

        try {
            // Слушаем прогресс от бэкенда
            const unlisten = await listen<{ percent: number }>('bsl-download-progress', (event) => {
                setBslProgress(event.payload.percent);
            });

            // Запускаем скачивание
            const jarPath = await invoke<string>('install_bsl_ls_cmd');
            setDownloadedJarPath(jarPath);

            setBslStatus('ok');
            if (unlisten) unlisten();
        } catch (e) {
            console.error('Download failed:', e);
            setBslStatus('missing');
        } finally {
            setIsDownloadingBsl(false);
        }
    };

    const handleFinish = async () => {
        finishingRef.current = true; // Мгновенный флаг для остановки поллинга
        setSpotlightRect(null);
        try {
            // Если settings нет в стейте, попробуем их получить принудительно (хотя они должны быть)
            // Но лучше полагаться на то, что SettingsContext всё равно сохранит результат

            const currentSettings = settings || {
                configurator: { window_title_pattern: "", selected_window_hwnd: 0 },
                bsl_server: {
                    enabled: false,
                    jar_path: "",
                    websocket_port: 9225,
                    java_path: "",
                    auto_download: true
                },
                mcp_servers: [],
                onboarding_completed: false
            };

            // Автоматическая активация MCP серверов при завершении
            const updatedServers = [...(currentSettings.mcp_servers || [])];

            // 1. BSL Language Server
            const bslEnvOk = javaStatus === 'ok' && bslStatus === 'ok';
            const bslIndex = updatedServers.findIndex(s => s.id === 'bsl-ls');
            if (bslIndex !== -1) {
                if (bslEnvOk && !updatedServers[bslIndex].enabled) {
                    updatedServers[bslIndex] = { ...updatedServers[bslIndex], enabled: true };
                }
            } else if (bslEnvOk) {
                updatedServers.push({
                    id: 'bsl-ls',
                    name: 'BSL Language Server',
                    enabled: true,
                    transport: 'internal',
                } as any);
            }

            // 2. 1C:Напарник (если введен токен)
            if (naparnikToken.trim()) {
                const nIndex = updatedServers.findIndex(s => s.id === 'builtin-1c-naparnik');
                if (nIndex !== -1) {
                    updatedServers[nIndex] = {
                        ...updatedServers[nIndex],
                        enabled: true,
                        env: { ...(updatedServers[nIndex].env || {}), 'ONEC_AI_TOKEN': naparnikToken }
                    };
                } else {
                    updatedServers.push({
                        id: 'builtin-1c-naparnik',
                        name: '1C:Напарник',
                        enabled: true,
                        transport: 'stdio',
                        command: 'npx',
                        args: ['--yes', 'tsx', 'src/mcp-servers/1c-naparnik.ts'],
                        env: { 'ONEC_AI_TOKEN': naparnikToken }
                    } as any);
                }
            }

            const newSettings = {
                ...currentSettings,
                onboarding_completed: true,
                mcp_servers: updatedServers,
                active_llm_profile: 'onboarding-profile',
                bsl_server: {
                    ...currentSettings.bsl_server,
                    enabled: bslEnvOk ? true : currentSettings.bsl_server.enabled,
                    jar_path: downloadedJarPath || currentSettings.bsl_server.jar_path || "",
                    java_path: currentSettings.bsl_server.java_path || "java"
                }
            };

            await updateSettings(newSettings);
            onComplete();
        } catch (e) {
            console.error("Failed to complete onboarding:", e);
            // Принудительно закрываем окно в любом случае
            onComplete();
        }
    };

    const handleSkip = async () => {
        finishingRef.current = true;
        setSpotlightRect(null);
        // При пропуске нам всё равно нужно пометить онбординг завершенным
        try {
            if (settings) {
                await updateSettings({ ...settings, onboarding_completed: true });
            }
        } catch (e) { }
        onComplete();
    };

    const handleSaveProfile = async () => {
        if (!selectedProvider) {
            setStep('mcp-setup');
            return;
        }

        const providerMap: Record<string, string> = {
            'openai': 'OpenAI',
            'anthropic': 'Anthropic',
            'google': 'Google',
            'deepseek': 'DeepSeek',
            'ollama': 'Ollama',
            'z.ai': 'ZAI',
            'openrouter': 'OpenRouter',
            'custom': 'Custom'
        };

        const mappedProvider = providerMap[selectedProvider] || 'OpenAI';

        let profile: any = {
            id: 'onboarding-profile',
            provider: mappedProvider,
            name: selectedProvider === 'z.ai' ? 'Z.AI' : selectedProvider === 'ollama' ? 'Ollama' : 'Custom AI',
            model: modelName || (selectedProvider === 'z.ai' ? 'glm-5' : selectedProvider === 'ollama' ? 'llama3' : ''),
            base_url: baseUrl || (selectedProvider === 'z.ai' ? 'https://api.z.ai/api/coding/paas/v4' : selectedProvider === 'ollama' ? 'http://localhost:11434/v1' : null),
            api_key_encrypted: '',
            max_tokens: 4096,
            temperature: 0.7
        };

        try {
            await saveProfile(profile, apiKey);
            await setActiveProfile('onboarding-profile');

            // Удаляем дефолтный профиль OpenAI, если он есть, так как пользователь настроил свой
            try {
                await deleteProfile('default');
            } catch (de) {
                // Игнорируем ошибку удаления, если профиля 'default' уже нет
            }

            console.log("Onboarding profile saved and activated, default profile removed");
        } catch (e) {
            console.error("Failed to save profile during onboarding:", e);
        }

        setStep('mcp-setup');
    };

    const handleSaveMCP = async () => {
        // Мы сохраним всё в handleFinish, чтобы гарантировать применение настроек 
        // даже если пользователь пропустит шаги или закроет визард на туре.
        setStep('tour');
    };

    // --- Render Steps ---

    const renderWelcome = () => (
        <div className="text-center space-y-6 max-w-md mx-auto animate-in fade-in zoom-in duration-300">
            <div className="flex justify-center mb-6">
                <div className="w-20 h-20 bg-blue-500 rounded-2xl flex items-center justify-center shadow-lg shadow-blue-500/30">
                    <Brain className="w-10 h-10 text-white" />
                </div>
            </div>
            <h1 className="text-3xl font-bold text-white">Mini AI 1C</h1>
            <p className="text-zinc-400 text-lg">
                Ваш умный напарник для разработки в 1С:Предприятие.<br />
                Давайте настроим всё за пару минут.
            </p>
            <div className="flex items-center gap-3">
                <button
                    onClick={handleSkip}
                    className="px-6 py-2 border border-zinc-700 hover:bg-white/5 text-zinc-400 font-medium rounded transition-colors flex items-center justify-center"
                >
                    Пропустить всё
                </button>
                <button
                    onClick={() => setStep('environment')}
                    className="flex-1 px-6 py-3 bg-blue-600 hover:bg-blue-500 text-white font-medium rounded transition-colors shadow-lg shadow-blue-900/20 flex items-center justify-center"
                >
                    Начать работу <ArrowRight className="w-5 h-5 ml-2" />
                </button>
            </div>
        </div>
    );

    const renderEnvironment = () => (
        <div className="space-y-6 max-w-lg mx-auto animate-in slide-in-from-right-10 duration-300">
            <h2 className="text-2xl font-bold text-white mb-2">Проверка окружения</h2>
            <p className="text-zinc-400">Для работы анализатора кода BSL нам нужны Java и Language Server.</p>

            <div className="space-y-4">
                {/* Java Check */}
                <div className="bg-zinc-800/50 p-4 rounded-xl border border-zinc-700 flex items-center justify-between">
                    <div className="flex items-center gap-3">
                        <div className="p-2 bg-orange-500/10 rounded-lg">
                            <Server className="w-6 h-6 text-orange-400" />
                        </div>
                        <div>
                            <h3 className="font-medium text-zinc-100">Java Runtime</h3>
                            <p className="text-xs text-zinc-500">Для запуска BSL LS</p>
                        </div>
                    </div>
                    <div>
                        {javaStatus === 'checking' && <span className="text-zinc-500 animate-pulse">Проверка...</span>}
                        {javaStatus === 'ok' && <Check className="w-6 h-6 text-green-500" />}
                        {javaStatus === 'missing' && (
                            <button
                                onClick={() => window.open('https://www.java.com/download/', '_blank')}
                                className="px-3 py-1.5 text-xs border border-orange-500/30 text-orange-400 hover:bg-orange-500/10 rounded flex items-center transition-colors"
                            >
                                <Download className="w-3 h-3 mr-1" /> Скачать
                            </button>
                        )}
                    </div>
                </div>

                {/* BSL LS Check */}
                <div className="relative overflow-hidden bg-zinc-800/50 rounded-xl border border-zinc-700 transition-all">
                    <div className="p-4 flex items-center justify-between">
                        <div className="flex items-center gap-3">
                            <div className="p-2 bg-blue-500/10 rounded-lg">
                                <Terminal className="w-6 h-6 text-blue-400" />
                            </div>
                            <div>
                                <h3 className="font-medium text-zinc-100">BSL Language Server</h3>
                                <p className="text-xs text-zinc-500">Анализ кода и форматирование</p>
                            </div>
                        </div>
                        <div>
                            {bslStatus === 'checking' && <span className="text-zinc-500 animate-pulse">Проверка...</span>}
                            {bslStatus === 'ok' && <Check className="w-6 h-6 text-green-500" />}
                            {bslStatus === 'missing' && (
                                <button
                                    onClick={handleDownloadBsl}
                                    disabled={isDownloadingBsl}
                                    className="px-3 py-1.5 text-xs border border-blue-500/30 text-blue-400 hover:bg-blue-500/10 rounded flex items-center transition-colors disabled:opacity-50"
                                >
                                    {isDownloadingBsl ? (
                                        <span className="flex items-center">
                                            <Bot className="w-3 h-3 mr-1 animate-bounce" />
                                            {bslProgress > 0 ? `${bslProgress}%` : 'Загрузка...'}
                                        </span>
                                    ) : (
                                        <>
                                            <Download className="w-3 h-3 mr-1" /> Скачать
                                        </>
                                    )}
                                </button>
                            )}
                        </div>
                    </div>

                    {/* Integrated Progress Bar */}
                    {isDownloadingBsl && (
                        <div className="absolute bottom-0 left-0 right-0 h-[3px] bg-zinc-800">
                            <div
                                className="h-full bg-gradient-to-r from-blue-600 via-blue-400 to-blue-500 shadow-[0_0_10px_rgba(59,130,246,0.6)] transition-all duration-300 ease-out"
                                style={{ width: `${bslProgress}%` }}
                            />
                        </div>
                    )}
                </div>
            </div>

            <div className="flex gap-3 pt-4">
                <button
                    className="px-6 py-2 border border-zinc-700 hover:bg-white/5 text-zinc-400 font-medium rounded transition-colors"
                    onClick={() => {
                        if (isDownloadingBsl) {
                            setShowAbortConfirm(true);
                        } else {
                            setStep('llm-setup');
                        }
                    }}
                >
                    Пропустить
                </button>
                <button
                    className="flex-1 px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white font-medium rounded transition-colors disabled:opacity-50 shadow-lg shadow-blue-900/20"
                    onClick={() => setStep('llm-setup')}
                    disabled={isDownloadingBsl}
                >
                    Продолжить
                </button>
            </div>

            {/* Abort Confirmation Modal */}
            {showAbortConfirm && (
                <div className="fixed inset-0 z-[300] flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
                    <div className="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 max-w-sm w-full shadow-2xl animate-in zoom-in duration-200">
                        <h3 className="text-xl font-bold text-white mb-2">Остановить скачивание?</h3>
                        <p className="text-zinc-400 mb-6">Это прервет установку BSL Language Server. Вы сможете настроить его позже в параметрах.</p>
                        <div className="flex gap-3">
                            <button
                                onClick={() => setShowAbortConfirm(false)}
                                className="flex-1 py-2 bg-zinc-800 hover:bg-zinc-700 text-white rounded-lg transition-colors"
                            >
                                Нет, продолжить
                            </button>
                            <button
                                onClick={() => {
                                    setShowAbortConfirm(false);
                                    setIsDownloadingBsl(false); // Останавливаем состояние в UI
                                    setStep('llm-setup');
                                }}
                                className="flex-1 py-2 bg-red-600 hover:bg-red-500 text-white font-bold rounded-lg transition-colors"
                            >
                                Да, остановить
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );

    const renderLLMSetup = () => (
        <div className="space-y-6 max-w-3xl mx-auto animate-in slide-in-from-right-10 duration-300">
            <h2 className="text-2xl font-bold text-white mb-2">Настройка LLM</h2>
            <p className="text-zinc-400">Выберите основного AI-провайдера и укажите токены.</p>

            <div className="grid grid-cols-3 gap-4">
                {/* z.ai */}
                <div
                    onClick={() => {
                        setSelectedProvider('z.ai');
                        setBaseUrl('https://api.z.ai/api/coding/paas/v4');
                        setModelName('glm-5');
                    }}
                    className={`p-4 rounded-xl border cursor-pointer transition-all ${selectedProvider === 'z.ai'
                        ? 'bg-blue-600/20 border-blue-500 ring-1 ring-blue-500'
                        : 'bg-zinc-800/50 border-zinc-700 hover:bg-zinc-800'
                        }`}
                >
                    <div className="flex items-center gap-3 mb-2">
                        <Cloud className="w-6 h-6 text-blue-400" />
                        <span className="font-bold text-white">z.ai</span>
                    </div>
                    <p className="text-xs text-zinc-400">Мощный облачный AI. Требуется ключ.</p>
                </div>

                {/* Ollama */}
                <div
                    onClick={() => {
                        setSelectedProvider('ollama');
                        setBaseUrl('http://localhost:11434/v1');
                        setModelName('llama3');
                    }}
                    className={`p-4 rounded-xl border cursor-pointer transition-all ${selectedProvider === 'ollama'
                        ? 'bg-orange-600/20 border-orange-500 ring-1 ring-orange-500'
                        : 'bg-zinc-800/50 border-zinc-700 hover:bg-zinc-800'
                        }`}
                >
                    <div className="flex items-center gap-3 mb-2">
                        <Terminal className="w-6 h-6 text-orange-400" />
                        <span className="font-bold text-white">Ollama</span>
                    </div>
                    <p className="text-xs text-zinc-400">Локальный, бесплатный, приватный.</p>
                </div>

                {/* Custom / OpenAI */}
                <div
                    onClick={() => {
                        setSelectedProvider('openai');
                        setBaseUrl('https://api.openai.com/v1');
                        setModelName('gpt-4o');
                    }}
                    className={`p-4 rounded-xl border cursor-pointer transition-all ${selectedProvider === 'openai'
                        ? 'bg-purple-600/20 border-purple-500 ring-1 ring-purple-500'
                        : 'bg-zinc-800/50 border-zinc-700 hover:bg-zinc-800'
                        }`}
                >
                    <div className="flex items-center gap-3 mb-2">
                        <Bot className="w-6 h-6 text-purple-400" />
                        <span className="font-bold text-white">Custom</span>
                    </div>
                    <p className="text-xs text-zinc-400">Любой OpenAI-совместимый API.</p>
                </div>
            </div>

            {/* Config Fields */}
            {selectedProvider && (
                <div className="bg-zinc-900/50 p-5 rounded-lg border border-zinc-800 animate-in fade-in slide-in-from-top-2 duration-200 space-y-4">

                    {selectedProvider === 'z.ai' && (
                        <>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">API Key</label>
                                <input
                                    type="password"
                                    value={apiKey}
                                    onChange={(e) => setApiKey(e.target.value)}
                                    placeholder="sk-..."
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-blue-500 focus:outline-none"
                                />
                            </div>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Base URL</label>
                                <input
                                    type="text"
                                    value={baseUrl}
                                    onChange={(e) => setBaseUrl(e.target.value)}
                                    placeholder="https://api.z.ai/api/coding/paas/v4"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-blue-500 focus:outline-none"
                                />
                            </div>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Model Name</label>
                                <input
                                    type="text"
                                    value={modelName}
                                    onChange={(e) => setModelName(e.target.value)}
                                    placeholder="glm-5"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-blue-500 focus:outline-none"
                                />
                            </div>
                        </>
                    )}

                    {selectedProvider === 'ollama' && (
                        <div className="space-y-4">
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Ollama URL</label>
                                <input
                                    type="text"
                                    value={baseUrl}
                                    onChange={(e) => setBaseUrl(e.target.value)}
                                    placeholder="http://localhost:11434/v1"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-orange-500 focus:outline-none"
                                />
                            </div>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Model Name</label>
                                <input
                                    type="text"
                                    value={modelName}
                                    onChange={(e) => setModelName(e.target.value)}
                                    placeholder="llama3"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-orange-500 focus:outline-none"
                                />
                            </div>
                        </div>
                    )}

                    {selectedProvider === 'openai' && (
                        <>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Base URL</label>
                                <input
                                    type="text"
                                    value={baseUrl}
                                    onChange={(e) => setBaseUrl(e.target.value)}
                                    placeholder="https://api.openai.com/v1"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-purple-500 focus:outline-none"
                                />
                            </div>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">API Key</label>
                                <input
                                    type="password"
                                    value={apiKey}
                                    onChange={(e) => setApiKey(e.target.value)}
                                    placeholder="sk-..."
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-purple-500 focus:outline-none"
                                />
                            </div>
                            <div>
                                <label className="block text-sm font-medium text-zinc-300 mb-1">Model Name</label>
                                <input
                                    type="text"
                                    value={modelName}
                                    onChange={(e) => setModelName(e.target.value)}
                                    placeholder="gpt-4o"
                                    className="w-full px-3 py-2 bg-zinc-800 rounded-md border border-zinc-700 text-white focus:border-purple-500 focus:outline-none"
                                />
                            </div>
                        </>
                    )}
                </div>
            )}

            <div className="flex gap-3 pt-6">
                <button
                    className="px-6 py-2 border border-zinc-700 hover:bg-white/5 text-zinc-400 font-medium rounded transition-colors"
                    onClick={() => setStep('mcp-setup')}
                >
                    Пропустить
                </button>
                <button
                    className="flex-1 px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white font-medium rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    disabled={!selectedProvider}
                    onClick={handleSaveProfile}
                >
                    Сохранить настройки
                </button>
            </div>
        </div>
    );

    const renderMCPSetup = () => (
        <div className="space-y-6 max-w-2xl mx-auto animate-in slide-in-from-right-10 duration-300">
            <h2 className="text-2xl font-bold text-white mb-2">Настройка MCP</h2>
            <p className="text-zinc-400 text-sm">
                Model Context Protocol (MCP) позволяет AI взаимодействовать с инструментами разработки.
                Для 1C это получение метаданных, структуры модулей и работа с 1C:Напарник.
            </p>

            <div className="bg-zinc-800/40 p-6 rounded-2xl border border-zinc-700/50 space-y-4">
                <div className="flex items-start gap-4 mb-4">
                    <div className="p-3 bg-yellow-500/10 rounded-xl">
                        <Bot className="w-8 h-8 text-yellow-500" />
                    </div>
                    <div>
                        <h3 className="text-lg font-semibold text-white">1C:Напарник</h3>
                        <p className="text-xs text-zinc-500 italic">Умный поиск и анализ кода 1С</p>
                    </div>
                </div>

                <div className="space-y-2">
                    <label className="block text-sm font-medium text-zinc-300">Токен доступа</label>
                    <input
                        type="password"
                        value={naparnikToken}
                        onChange={(e) => setNaparnikToken(e.target.value)}
                        placeholder="Введите ваш токен..."
                        className="w-full px-4 py-3 bg-zinc-900 rounded-xl border border-zinc-700 text-white focus:border-yellow-500 focus:outline-none placeholder-zinc-700 transition-all font-mono"
                    />
                    <div className="flex justify-between items-center text-[10px]">
                        <span className="text-zinc-600">Опционально. Можно добавить позже в настройках.</span>
                        <a href="https://code.1c.ai/tokens/" target="_blank" className="text-blue-400 hover:underline">Получить токен</a>
                    </div>
                </div>
            </div>

            <div className="flex gap-3 pt-6">
                <button
                    className="px-6 py-2 border border-zinc-700 hover:bg-white/5 text-zinc-400 font-medium rounded transition-colors"
                    onClick={() => setStep('tour')}
                >
                    Пропустить
                </button>
                <button
                    className="flex-1 px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white font-medium rounded transition-colors"
                    onClick={handleSaveMCP}
                >
                    Сохранить и продолжить
                </button>
            </div>
        </div>
    );

    const tourSteps = [
        {
            title: "Выбор Конфигуратора",
            desc: "Укажите активное окно Конфигуратора. Это позволит AI понимать, над какой базой вы сейчас работаете.",
            icon: <Monitor className="w-10 h-10 text-blue-400" />,
            targetId: 'configurator-selector'
        },
        {
            title: "Интеллектуальный Чат",
            desc: "Задавайте вопросы по коду, просите исправить ошибки или написать новые функции.",
            icon: <Brain className="w-10 h-10 text-purple-400" />,
            targetId: 'chat-area'
        },
        {
            title: "Боковая Панель",
            desc: "Управляйте редактором кода для детального просмотра и применения изменений.",
            icon: <PanelRight className="w-10 h-10 text-orange-400" />,
            targetId: 'code-side-panel'
        },
        {
            title: "Умный Редактор",
            desc: "Просматривайте Diff и редактируйте BSL код с подсветкой и подсказками.",
            icon: <FileText className="w-10 h-10 text-green-400" />,
            targetId: 'tour-editor'
        },
        {
            title: "Применение изменений",
            desc: "По завершении работы нажмите 'Apply Changes', чтобы автоматически вставить код в 1С.",
            icon: <Check className="w-10 h-10 text-blue-500" />,
            targetId: 'tour-apply'
        }
    ];

    const [spotlightRect, setSpotlightRect] = useState<DOMRect | null>(null);

    useEffect(() => {
        let interval: any;
        if (step === 'tour') {
            const updateRect = () => {
                if (finishingRef.current) return;
                const targetId = tourSteps[tourStep].targetId;
                if (targetId) {
                    const element = document.getElementById(targetId);
                    if (element) {
                        setSpotlightRect(element.getBoundingClientRect());
                    } else {
                        setSpotlightRect(null);
                    }
                } else {
                    setSpotlightRect(null);
                }
            };

            updateRect();
            // Поллинг для обновления координат при анимациях и рендеринге
            interval = setInterval(updateRect, 100);

            // Авто-открытие боковой панели
            if (tourSteps[tourStep].targetId === 'code-side-panel') {
                const btn = document.getElementById('tour-side-panel');
                // Открываем, если еще не открыта (проверяем наличие самой панели)
                if (btn && !document.getElementById('code-side-panel')) {
                    btn.click();
                }
            }
        }
        return () => {
            if (interval) clearInterval(interval);
        };
    }, [step, tourStep]);

    const renderTour = () => {
        const getClampedStyles = (rect: DOMRect) => {
            const pad = (rect.width > 300 || rect.height > 200) ? 4 : 8;

            let parentRect = { top: 0, left: 0, width: window.innerWidth, height: window.innerHeight };
            if (wizardRef.current) {
                parentRect = wizardRef.current.getBoundingClientRect();
            }

            // Исходные координаты с отступами
            let targetTop = rect.top - pad - parentRect.top;
            let targetLeft = rect.left - pad - parentRect.left;
            let targetWidth = rect.width + pad * 2;
            let targetHeight = rect.height + pad * 2;

            // Clamp к границам родителя
            const top = Math.max(0, targetTop);
            const left = Math.max(0, targetLeft);

            // Если элемент + отступ выходит за экран справа/снизу, уменьшаем размер
            const width = Math.min(targetWidth, parentRect.width - left - 1);
            const height = Math.min(targetHeight, parentRect.height - top - 1);

            return { top, left, width, height };
        };

        const spotlightStyles = spotlightRect ? getClampedStyles(spotlightRect) : null;

        return (
            <div className="absolute inset-0 z-[200] pointer-events-none">
                {/* Spotlight Overlay */}
                {spotlightStyles && (
                    <div className="absolute inset-0 z-[201]">
                        {/* Spotlight with Shadow Hole - Reduced opacity for better context visibility */}
                        <div
                            className="absolute rounded-lg transition-all duration-500 shadow-[0_0_0_9999px_rgba(0,0,0,0.3)] border border-white/5"
                            style={{
                                ...spotlightStyles,
                                pointerEvents: 'none'
                            }}
                        ></div>
                        {/* Pulsing Border - More intense with glow and contrast */}
                        <div
                            className="absolute border-[3px] border-blue-400 shadow-[0_0_15px_rgba(96,165,250,0.5)] ring-1 ring-white/30 rounded-lg animate-pulse transition-all duration-500"
                            style={spotlightStyles}
                        ></div>
                    </div>
                )}

                <div className="absolute inset-0 flex items-center justify-center p-8 bg-black/40 pointer-events-auto z-[210]">
                    <div className="max-w-md w-full p-8 text-center relative bg-zinc-900 border border-zinc-800 rounded-3xl shadow-2xl">
                        <div className="mb-8 flex justify-center animate-in zoom-in duration-300" key={tourStep}>
                            <div className="p-6 bg-zinc-800 rounded-full shadow-2xl shadow-blue-500/10 border border-zinc-700">
                                {tourSteps[tourStep].icon}
                            </div>
                        </div>

                        <h2 className="text-3xl font-bold text-white mb-4 animate-in slide-in-from-bottom-5 duration-300" key={`h-${tourStep}`}>
                            {tourSteps[tourStep].title}
                        </h2>
                        <p className="text-zinc-400 text-lg mb-10 h-16 animate-in slide-in-from-bottom-5 duration-300 delay-75" key={`p-${tourStep}`}>
                            {tourSteps[tourStep].desc}
                        </p>

                        <div className="mt-12 flex flex-col items-center gap-6">
                            {/* Pagination dots on top - safer from overlapping */}
                            <div className="flex gap-2.5">
                                {tourSteps.map((_, idx) => (
                                    <div
                                        key={idx}
                                        className={`w-2 h-2 rounded-full transition-all duration-300 ${idx === tourStep ? 'bg-blue-500 scale-125' : 'bg-zinc-700'}`}
                                    />
                                ))}
                            </div>

                            {/* Action buttons below */}
                            <div className="flex items-center justify-between w-full">
                                <button
                                    onClick={() => setTourStep(prev => Math.max(0, prev - 1))}
                                    disabled={tourStep === 0}
                                    className="px-4 py-2 text-zinc-400 hover:text-white disabled:opacity-30 disabled:hover:text-zinc-400 transition-colors flex items-center"
                                >
                                    <ChevronLeft className="w-5 h-5 mr-1" /> Назад
                                </button>

                                {tourStep < tourSteps.length - 1 ? (
                                    <button
                                        onClick={() => setTourStep(prev => prev + 1)}
                                        className="px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-colors flex items-center shadow-lg shadow-blue-900/20"
                                    >
                                        Далее <ChevronRight className="w-5 h-5 ml-1" />
                                    </button>
                                ) : (
                                    <button
                                        onClick={handleFinish}
                                        className="px-6 py-2 bg-green-600 hover:bg-green-500 text-white font-bold rounded-lg transition-colors flex items-center shadow-lg shadow-green-900/20 animate-pulse whitespace-nowrap"
                                    >
                                        Начать работу! <Check className="w-5 h-5 ml-2" />
                                    </button>
                                )}
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        );
    };

    return (
        <div
            ref={wizardRef}
            className={`absolute inset-0 z-[100] ${step === 'tour' ? 'bg-transparent pointer-events-none' : 'bg-[#1e1e1e] flex items-center justify-center p-6'} text-white font-sans transition-colors duration-700`}>
            <div className={`w-full ${step === 'tour' ? 'h-full' : 'max-w-4xl'} relative transition-all duration-500`}>
                {step === 'welcome' && renderWelcome()}
                {step === 'environment' && renderEnvironment()}
                {step === 'llm-setup' && renderLLMSetup()}
                {step === 'mcp-setup' && renderMCPSetup()}
                {step === 'tour' && renderTour()}
            </div>
        </div>
    );
};
