import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { Settings, Loader2, Minus, Square, X, ArrowUp, ChevronDown, Trash2, Monitor, RefreshCw, PanelRight, FileText, MousePointerClick } from 'lucide-react';
import { SettingsPanel } from './components/SettingsPanel';
import { MarkdownRenderer } from './components/MarkdownRenderer';
import { CodeSidePanel } from './components/CodeSidePanel';
import logo from './assets/logo.png';

interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

interface LLMProfile {
  id: string;
  name: string;
  provider: string;
  model: string;
}

interface ProfileStore {
  profiles: LLMProfile[];
  active_profile_id: string;
}

interface WindowInfo {
  hwnd: number;
  title: string;
}

interface AppSettings {
  configurator: {
    window_title_pattern: string;
    selected_window_hwnd: number | null;
  };
  bsl_server: {
    jar_path: string;
    websocket_port: number;
    enabled: boolean;
    java_path: string;
    auto_download: boolean;
  };
  ui: {
    theme: string;
    minimize_to_tray: boolean;
    start_minimized: boolean;
  };
}

interface BslStatus {
  installed: boolean;
  java_info: string;
  connected: boolean;
}

interface BslDiagnostic {
  line: number;
  message: string;
  severity: string;
}

function App() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [profiles, setProfiles] = useState<ProfileStore | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [bslStatus, setBslStatus] = useState<BslStatus | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showModelDropdown, setShowModelDropdown] = useState(false);

  // Side Panel State
  const [showSidePanel, setShowSidePanel] = useState(false);
  const [originalCode, setOriginalCode] = useState('');
  const [modifiedCode, setModifiedCode] = useState('');
  const [diagnostics, setDiagnostics] = useState<BslDiagnostic[]>([]);
  const [isApplying, setIsApplying] = useState(false);
  const [contextMode, setContextMode] = useState<'module' | 'selection'>('selection');

  // Configurator Selection State
  const [showConfigDropdown, setShowConfigDropdown] = useState(false);
  const [showGetCodeDropdown, setShowGetCodeDropdown] = useState(false);
  const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);

  // Command Menu State
  const [commandMenu, setCommandMenu] = useState<{
    isOpen: boolean;
    type: '/' | '@' | null;
    filter: string;
    selectedIndex: number;
  }>({ isOpen: false, type: null, filter: '', selectedIndex: 0 });

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Auto-scroll to bottom
  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages]);

  // Handle automatic window resizing when side panel is opened
  useEffect(() => {
    if (showSidePanel) {
      const ensureWidth = async () => {
        try {
          const appWindow = getCurrentWindow();
          const factor = await appWindow.scaleFactor();
          const innerSize = await appWindow.innerSize();

          const logicalWidth = innerSize.width / factor;
          const logicalHeight = innerSize.height / factor;

          // Minimum width to fit Chat (min 400px) + Side Panel (500px) + some buffer
          const minWidthRequired = 950;

          if (logicalWidth < minWidthRequired) {
            await appWindow.setSize(new LogicalSize(minWidthRequired, logicalHeight));
          }
        } catch (error) {
          console.error('Failed to resize window:', error);
        }
      };
      ensureWidth();
    }
  }, [showSidePanel]);

  // Validate Code Effect
  useEffect(() => {
    if (!modifiedCode) {
      setDiagnostics([]);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const diags = await invoke<BslDiagnostic[]>('analyze_bsl', { code: modifiedCode });
        setDiagnostics(diags);
      } catch (e) {
        console.warn("Analysis check failed", e);
      }
    }, 800);
    return () => clearTimeout(timer);
  }, [modifiedCode]);

  // Load profiles and settings on mount (Chat starts empty)
  useEffect(() => {
    invoke<ProfileStore>('get_profiles').then(setProfiles);
    invoke<AppSettings>('get_settings').then(set => {
      setSettings(set);
      // If we have a selected window, verify it still exists or refresh list
      if (set) {
        refreshConfigurators(set.configurator.window_title_pattern);
      }
    });

    // Initial status check
    invoke<BslStatus>('check_bsl_status_cmd').then(setBslStatus).catch(console.error);

    // Periodic status check
    const interval = setInterval(() => {
      invoke<BslStatus>('check_bsl_status_cmd').then(setBslStatus).catch(console.error);
    }, 5000);

    return () => clearInterval(interval);
  }, []);

  // Disable context menu
  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };
    document.addEventListener('contextmenu', handleContextMenu);
    return () => document.removeEventListener('contextmenu', handleContextMenu);
  }, []);

  // Listen for chat events
  useEffect(() => {
    const unlistenChunk = listen<string>('chat-chunk', (event) => {
      setMessages(prev => {
        const last = prev[prev.length - 1];
        if (last && last.role === 'assistant') {
          return [...prev.slice(0, -1), { ...last, content: last.content + event.payload }];
        }
        return [...prev, { role: 'assistant', content: event.payload }];
      });
    });

    const unlistenDone = listen('chat-done', () => {
      setIsLoading(false);
      // Save assistant message and update editor code
      setMessages(prev => {
        const last = prev[prev.length - 1];
        if (last && last.role === 'assistant') {
          invoke('save_chat_message', { role: 'assistant', content: last.content });

          // Auto-update removed in favor of "Load Code" button
          // const codeBlockMatch = [...last.content.matchAll(/```(?:bsl|1c)\s*([\s\S]*?)```/gi)].pop();
          // if (codeBlockMatch && codeBlockMatch[1]) {
          //   const newCode = codeBlockMatch[1].trim();
          //   console.log("Auto-updating modified code from AI response");
          //   setModifiedCode(newCode);
          //   // If panel is closed, maybe open it? activeContextMode is already set if we had context
          //   // But if user just asked a question without context, we might not want to clobber.
          //   // Only update if we are already in a "session" (i.e. we have originalCode)
          //   // or if we want to support generating code from scratch.
          //   // For now, let's update it unconditionally so the user sees the result in "Modified".
          //   if (!showSidePanel) setShowSidePanel(true);
          // }
        }
        return prev;
      });
    });

    return () => {
      unlistenChunk.then(fn => fn());
      unlistenDone.then(fn => fn());
    };
  }, []);

  const refreshConfigurators = async (pattern: string = 'Конфигуратор') => {
    try {
      const windows = await invoke<WindowInfo[]>('find_configurator_windows_cmd', { pattern });
      setDetectedWindows(windows);

      // Auto-deselect if stale (saved window no longer exists)
      if (settings?.configurator.selected_window_hwnd) {
        const stillExists = windows.some(w => w.hwnd === settings.configurator.selected_window_hwnd);
        if (!stillExists) {
          // Window is gone, reset selection
          const newSet = { ...settings, configurator: { ...settings.configurator, selected_window_hwnd: null } };
          setSettings(newSet);
          invoke('save_settings', { newSettings: newSet });
        }
      }
    } catch (e) {
      console.error("Failed to find windows", e);
    }
  };

  const selectConfigurator = async (hwnd: number) => {
    if (!settings) return;
    const newSettings = {
      ...settings,
      configurator: { ...settings.configurator, selected_window_hwnd: hwnd }
    };
    setSettings(newSettings);
    setShowConfigDropdown(false);
    await invoke('save_settings', { newSettings });
  };

  const getActiveConfiguratorTitle = () => {
    if (!settings?.configurator.selected_window_hwnd) return "Configurator";
    const win = detectedWindows.find(w => w.hwnd === settings.configurator.selected_window_hwnd);
    return win ? win.title : `ID: ${settings.configurator.selected_window_hwnd} (Not Found)`;
  };

  const fetchCodeForContext = async (useSelectAll: boolean) => {
    let targetHwnd = settings?.configurator.selected_window_hwnd;

    if (!targetHwnd) {
      const windows = await invoke<Array<{ hwnd: number; title: string }>>('find_configurator_windows_cmd', { pattern: 'Конфигуратор' });
      if (windows.length > 0) targetHwnd = windows[0].hwnd;
    }

    if (targetHwnd) {
      try {
        const code = await invoke<string>('get_code_from_configurator', {
          hwnd: targetHwnd,
          useSelectAll: useSelectAll
        });

        if (code) {
          setContextMode(useSelectAll ? 'module' : 'selection');
          setOriginalCode(code);
          setModifiedCode(code); // Initially modified is same as original
          setDiagnostics([]);
          setShowSidePanel(true);
        }
      } catch (e) {
        console.error("Failed to fetch code", e);
      }
    }
  };



  const handleApplyToConfigurator = async () => {
    setIsApplying(true);
    let targetHwnd = settings?.configurator.selected_window_hwnd;
    console.log("[Apply] Target HWND from settings:", targetHwnd);

    if (!targetHwnd) {
      console.log("[Apply] No specific window selected, searching...");
      const windows = await invoke<Array<{ hwnd: number; title: string }>>('find_configurator_windows_cmd', { pattern: 'Конфигуратор' });
      if (windows.length > 0) {
        targetHwnd = windows[0].hwnd;
        console.log("[Apply] Auto-selected window:", targetHwnd, windows[0].title);
      } else {
        console.error("[Apply] No Configurator window found.");
        alert("No 1C Configurator window found! Please open Configurator.");
        setIsApplying(false);
        return;
      }
    }

    if (targetHwnd && modifiedCode) {
      console.log("[Apply] Applying code to HWND:", targetHwnd, "Mode:", contextMode);
      try {
        await invoke('paste_code_to_configurator', {
          hwnd: targetHwnd,
          code: modifiedCode,
          useSelectAll: contextMode === 'module'
        });
        console.log("[Apply] Success");
      } catch (e) {
        console.error("Failed to apply code", e);
        alert("Failed to apply code: " + e);
      }
    } else {
      console.warn("[Apply] Missing HWND or Code");
    }
    setIsApplying(false);
  };

  const sendMessage = async () => {
    if (!input.trim() || isLoading) return;

    // 1. UI: Show clean user message
    const cleanContent = input;
    const userMessage: ChatMessage = { role: 'user', content: cleanContent };

    setMessages(prev => [...prev, userMessage]);
    setInput('');
    setIsLoading(true);

    // Save clean message to history
    await invoke('save_chat_message', { role: 'user', content: cleanContent });

    // 2. Backend: Prepare payload with context
    let contextPayload = cleanContent;

    // Auto-attach code context if active
    if (modifiedCode?.trim()) {
      contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${modifiedCode}\n\`\`\`\n`;

      // Attach diagnostics if any
      if (diagnostics.length > 0) {
        const diagStr = diagnostics.map(d => `- Line ${d.line + 1}: ${d.message} (${d.severity})`).join('\n');
        contextPayload += `\n=== DETECTED ERRORS ===\n${diagStr}\n`;
        contextPayload += `\nPlease fix these errors in the code.`;
      }
    }

    try {
      // Construct message history for AI, ensuring the last message has the full context
      const payloadMessages = messages.map(m => ({ role: m.role, content: m.content }));
      payloadMessages.push({ role: 'user', content: contextPayload });

      await invoke('stream_chat', {
        messages: payloadMessages,
      });

    } catch (err) {
      const errorMsg = `❌ Ошибка: ${err} `;
      setMessages(prev => [...prev, { role: 'assistant', content: errorMsg }]);
      await invoke('save_chat_message', { role: 'assistant', content: errorMsg });
      setIsLoading(false);
    }
  };

  // Helper to find last code block
  const getLastCodeBlock = () => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const match = messages[i].content.match(/```(?: bsl | 1c) ?\n([\s\S] *?)```/);
      if (match) return match[1];
    }
    return null;
  };

  const SLASH_COMMANDS = [
    { id: 'clear', label: 'Clear Chat', desc: 'Clear current messages', action: () => setMessages([]) },
    { id: 'new', label: 'New Chat', desc: 'Start a new session', action: () => setMessages([]) },
    { id: 'settings', label: 'Settings', desc: 'Open settings', action: () => setShowSettings(true) },
    { id: 'help', label: 'Help', desc: 'List commands', action: () => alert('Available commands: /clear, /new, /settings, /check, /analyze, /format') },
    {
      id: 'check', label: 'Check Configurator', desc: 'Find 1C windows', action: async () => {
        const windows = await invoke<Array<{ hwnd: number; title: string }>>('find_configurator_windows_cmd', { pattern: 'Конфигуратор' });
        alert(windows.length ? `Found: ${windows.map(w => w.title).join(', ')} ` : 'No windows found');
      }
    },
    {
      id: 'analyze', label: 'Analyze BSL', desc: 'Analyze last code block', action: async () => {
        const code = getLastCodeBlock();
        if (!code) { alert('No BSL code found in chat history'); return; }
        try {
          alert("Analysis started (backend impl pending full integration)...");
        } catch (e) { alert('Analysis failed: ' + e); }
      }
    },
    { id: 'format', label: 'Format BSL', desc: 'Format last code block', action: () => alert('Format command triggered') }
  ];

  const CONTEXT_COMMANDS = [
    { id: 'configurator', label: 'Configurator (Module)', desc: 'Capture full module logic (Ctrl+A)', type: 'context' },
    { id: 'selection', label: 'Configurator (Selection)', desc: 'Capture selected code', type: 'context' },
  ];

  const handleInputChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newVal = e.target.value;
    setInput(newVal);

    // Simple trigger detection (last word)
    const lastWord = newVal.split(/\s/).pop() || '';

    if (lastWord.startsWith('/')) {
      setCommandMenu({
        isOpen: true,
        type: '/',
        filter: lastWord.slice(1).toLowerCase(),
        selectedIndex: 0,
      });
    } else if (lastWord.startsWith('@')) {
      setCommandMenu({
        isOpen: true,
        type: '@',
        filter: lastWord.slice(1).toLowerCase(),
        selectedIndex: 0,
      });
    } else {
      setCommandMenu(prev => prev.isOpen ? { ...prev, isOpen: false } : prev);
    }
  };

  const executeCommand = (item: any) => {
    if (commandMenu.type === '/') {
      item.action();
      setInput('');
    } else if (commandMenu.type === '@') {
      if (item.type === 'context') {
        const parts = input.split(/\s/);
        parts.pop(); // remove partial trigger
        setInput(parts.join(' ') + (parts.length ? ' ' : '') + '@' + item.id + ' ');
      }
    }
    setCommandMenu({ isOpen: false, type: null, filter: '', selectedIndex: 0 });
    inputRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (commandMenu.isOpen) {
      let items: any[] = [];
      if (commandMenu.type === '/') {
        items = SLASH_COMMANDS.filter(c => c.id.includes(commandMenu.filter) || c.label.toLowerCase().includes(commandMenu.filter));
      } else if (commandMenu.type === '@') {
        items = CONTEXT_COMMANDS.filter(c => c.id.includes(commandMenu.filter));
      }

      if (items.length > 0) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setCommandMenu(prev => ({ ...prev, selectedIndex: (prev.selectedIndex + 1) % items.length }));
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          setCommandMenu(prev => ({ ...prev, selectedIndex: (prev.selectedIndex - 1 + items.length) % items.length }));
        } else if (e.key === 'Enter' || e.key === 'Tab') {
          e.preventDefault();
          executeCommand(items[commandMenu.selectedIndex]);
          return;
        } else if (e.key === 'Escape') {
          setCommandMenu({ ...commandMenu, isOpen: false });
          return;
        }
      }
    } else {
      if (e.key === 'Enter' && !e.shiftKey) {
        const trimmed = input.trim();
        if (trimmed.startsWith('/')) {
          const cmdName = trimmed.substring(1).toLowerCase();
          const exactCmd = SLASH_COMMANDS.find(c => c.id === cmdName);
          if (exactCmd) {
            e.preventDefault();
            exactCmd.action();
            setInput('');
            return;
          }
        }
      }
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  // Window controls
  const appWindow = getCurrentWindow();
  const minimize = () => {
    appWindow.minimize().catch(e => console.error('Minimize error:', e));
  };
  const maximize = async () => {
    try {
      const isMaximized = await appWindow.isMaximized();
      if (isMaximized) {
        await appWindow.unmaximize();
      } else {
        await appWindow.maximize();
      }
    } catch (e) {
      console.error('Maximize error:', e);
    }
  };
  const close = () => {
    appWindow.close().catch(e => console.error('Close error:', e));
  };

  return (
    <div className="flex flex-col h-screen bg-transparent">
      {/* Custom Title Bar */}
      <div className="relative h-10 bg-[#09090b] flex items-center justify-between px-4 border-b border-[#27272a] select-none z-50">
        <div
          data-tauri-drag-region
          className="absolute inset-0 z-0"
          onMouseDown={() => appWindow.startDragging()}
        />

        <div className="relative z-10 flex items-center gap-2 pointer-events-none">
          <img src={logo} alt="Logo" className="w-5 h-5" />
          <span className="text-sm font-medium text-zinc-300">Mini AI 1C</span>
        </div>

        <div className="relative z-50 flex items-center gap-1 pointer-events-auto">
          <button onClick={minimize} className="p-1.5 hover:bg-zinc-800 rounded transition-colors text-zinc-400 hover:text-white">
            <Minus className="w-4 h-4" />
          </button>
          <button onClick={maximize} className="p-1.5 hover:bg-zinc-800 rounded transition-colors text-zinc-400 hover:text-white">
            <Square className="w-3 h-3" />
          </button>
          <button onClick={close} className="p-1.5 hover:bg-red-900/50 hover:text-red-200 rounded transition-colors text-zinc-400">
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Header Actions - MOVED UP */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-[#27272a] bg-[#09090b]">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 px-2 py-1 rounded-md bg-zinc-900/50 border border-zinc-800/50">
            <div className={`w-1.5 h-1.5 rounded-full ${!bslStatus ? 'bg-zinc-600 animate-pulse' : bslStatus.connected ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.4)]' : 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.4)]'}`} />
            <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest hidden md:inline">BSL LS</span>
            <span className="text-[10px] text-zinc-600 font-medium hidden md:inline">
              {!bslStatus ? 'Initializing...' : bslStatus.connected ? 'Connected' : 'Offline'}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowSidePanel(!showSidePanel)}
            className={`p-2 hover:bg-[#27272a] rounded-lg transition-colors ${showSidePanel ? 'text-blue-400 bg-blue-500/10' : 'text-zinc-400'}`}
            title="Toggle Code Panel"
          >
            <PanelRight className="w-4 h-4" />
          </button>
          <div className="w-px h-4 bg-[#27272a] mx-1" />
          <button
            onClick={() => setMessages([])}
            className="p-2 hover:bg-[#27272a] rounded-lg transition-colors group"
            title="Clear Chat"
          >
            <Trash2 className="w-4 h-4 text-zinc-400 group-hover:text-red-400 transition-colors" />
          </button>
          <button
            onClick={() => setShowSettings(true)}
            className="p-2 hover:bg-[#27272a] rounded-lg transition-colors"
            title="Settings"
          >
            <Settings className="w-4 h-4 text-zinc-400" />
          </button>
        </div>
      </div>

      <div className="flex flex-1 overflow-hidden bg-[#09090b] relative">
        {/* Main Content Area (Messages + Input) - ENFORCE MIN WIDTH */}
        <div className="flex flex-col flex-1 min-w-[400px] transition-all duration-300">

          {/* Messages */}
          <div className="flex-1 overflow-y-auto bg-[#09090b]">
            {messages.length === 0 && (
              <div className="flex flex-col items-center justify-center h-full text-zinc-500 opacity-50">
                <img src={logo} alt="Logo" className="w-20 h-20 mb-6 grayscale opacity-20" />
                <p className="text-xl font-medium text-zinc-400">Mini AI 1C</p>
                <p className="text-sm text-zinc-600 mt-2">Ready to assist with BSL and Configuration</p>
              </div>
            )}

            <div className="flex flex-col pb-4">
              {messages.map((msg, i) => (
                <div key={i} className="w-full mb-4 px-4">
                  <div className={`p-6 rounded-xl border border-[#27272a] ${msg.role === 'user' ? 'bg-[#18181b] mr-12' : 'bg-transparent'}`}>
                    <div className="flex-1 min-w-0 overflow-hidden">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-[13px] font-semibold text-zinc-300">
                          {msg.role === 'user' ? 'User' : 'Mini AI 1C'}
                        </span>
                        <span className="text-[11px] text-zinc-600">
                          {new Date().toLocaleTimeString()}
                        </span>
                      </div>
                      <div className="text-zinc-300 leading-relaxed text-[13px]">
                        {msg.role === 'assistant' ? (
                          <div className="flex flex-col gap-2">
                            <MarkdownRenderer content={msg.content} />
                            {(() => {
                              const codeBlockMatch = [...msg.content.matchAll(/```(?:bsl|1c)\s*([\s\S]*?)```/gi)].pop();
                              if (codeBlockMatch && codeBlockMatch[1]) {
                                return (
                                  <button
                                    onClick={() => {
                                      const newCode = codeBlockMatch[1].trim();
                                      setModifiedCode(newCode);
                                      setShowSidePanel(true);
                                    }}
                                    className="self-start flex items-center gap-2 px-3 py-1.5 mt-2 text-[11px] font-medium text-blue-400 bg-blue-500/10 hover:bg-blue-500/20 border border-blue-500/20 rounded-md transition-all group"
                                  >
                                    <PanelRight className="w-3 h-3 group-hover:scale-110 transition-transform" />
                                    Review & Apply
                                  </button>
                                );
                              }
                              return null;
                            })()}
                          </div>
                        ) : (
                          <pre className="whitespace-pre-wrap font-sans text-[13px] text-zinc-300" style={{ fontFamily: 'Inter, sans-serif' }}>{msg.content}</pre>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
              {isLoading && messages[messages.length - 1]?.role === 'user' && (
                <div className="w-full px-4 mb-4">
                  <div className="p-6 rounded-xl border border-[#27272a] bg-transparent flex items-center gap-3">
                    <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                    <span className="text-zinc-400 text-sm">Thinking...</span>
                  </div>
                </div>
              )}
            </div>
            <div ref={messagesEndRef} />
          </div>

          {/* Input Area (now inside the left column) */}
          <div className="p-4 bg-[#09090b] border-t border-[#27272a]">
            <div className="relative bg-[#18181b] border border-[#27272a] rounded-xl focus-within:ring-1 focus-within:ring-blue-500/50 transition-all min-h-[120px] flex flex-col">
              {commandMenu.isOpen && (
                <div className="absolute bottom-full left-0 mb-2 w-72 bg-[#18181b] border border-[#27272a] rounded-xl shadow-2xl overflow-hidden z-20">
                  <div className="max-h-60 overflow-y-auto p-1">
                    {(() => {
                      let items: any[] = [];
                      if (commandMenu.type === '/') {
                        items = SLASH_COMMANDS.filter(c => c.id.includes(commandMenu.filter) || c.label.toLowerCase().includes(commandMenu.filter));
                      } else if (commandMenu.type === '@') {
                        items = CONTEXT_COMMANDS.filter(c => c.id.includes(commandMenu.filter));
                      }
                      return items.map((item, i) => (
                        <button
                          key={item.id}
                          onClick={() => executeCommand(item)}
                          className={`w-full text-left px-3 py-2 rounded-lg text-sm flex items-center justify-between ${i === commandMenu.selectedIndex ? 'bg-blue-600 text-white' : 'text-zinc-400 hover:bg-[#27272a]'}`}
                        >
                          <span className="flex items-center gap-2">
                            <span className="font-medium">{commandMenu.type}{item.name || item.id}</span>
                          </span>
                          <span className="text-xs opacity-50">{item.desc || item.label}</span>
                        </button>
                      ));
                    })()}
                  </div>
                </div>
              )}

              <textarea
                ref={inputRef}
                value={input}
                onChange={handleInputChange}
                onKeyDown={handleKeyDown}
                onFocus={() => {
                  setShowModelDropdown(false);
                }}
                placeholder="Plan, @ for context, / for commands"
                className="w-full h-full bg-transparent text-zinc-300 px-4 py-3 resize-none focus:outline-none placeholder-zinc-600 text-[13px] font-sans leading-relaxed flex-1"
                style={{ fontFamily: 'Inter, sans-serif' }}
              />

              <div className="px-3 pb-2 pt-0 flex items-end gap-2 pointer-events-auto flex-nowrap w-full overflow-hidden">
                {profiles && (
                  <div className="flex items-center gap-1 flex-1 flex-shrink-0">
                    <div className="flex items-center gap-1 flex-nowrap flex-shrink-0">
                      {showModelDropdown && (
                        <div className="absolute bottom-full left-0 mb-2 w-56 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl overflow-hidden z-30 ring-1 ring-black/20">
                          <div className="p-1">
                            <div className="px-3 py-2 text-[11px] font-bold text-zinc-500 uppercase tracking-wider">Select Model</div>
                            {profiles.profiles.map(p => (
                              <button
                                key={p.id}
                                onClick={() => {
                                  invoke('set_active_profile', { profileId: p.id }).then(() => {
                                    invoke<ProfileStore>('get_profiles').then(setProfiles);
                                    setShowModelDropdown(false);
                                  });
                                }}
                                className={`w-full text-left px-3 py-2 rounded-md text-[13px] flex items-center justify-between ${p.id === profiles.active_profile_id ? 'bg-blue-500/10 text-blue-400' : 'text-zinc-400 hover:bg-[#27272a] hover:text-zinc-200'}`}
                              >
                                <span>{p.name}</span>
                                {p.id === profiles.active_profile_id && <div className="w-1.5 h-1.5 rounded-full bg-blue-500" />}
                              </button>
                            ))}
                            <div className="h-px bg-[#27272a] my-1" />
                            <button
                              onClick={() => {
                                setShowSettings(true);
                                setShowModelDropdown(false);
                              }}
                              className="w-full text-left px-3 py-2 rounded-md text-[13px] flex items-center gap-2 text-zinc-500 hover:bg-[#27272a] hover:text-zinc-300"
                            >
                              <Settings className="w-3 h-3" />
                              Manage Profiles...
                            </button>
                          </div>
                        </div>
                      )}
                      <div className="flex items-center gap-1 flex-shrink-0">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setShowModelDropdown(!showModelDropdown);
                          }}
                          className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showModelDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                        >
                          <ChevronDown className={`w-3 h-3 text-zinc-500 transition-transform ${showModelDropdown ? 'rotate-180' : ''}`} />
                          <span className="hidden sm:inline whitespace-nowrap">Agent</span>
                        </button>

                        {/* CONFIGURATOR SELECTOR */}
                        <div className="relative flex-shrink-0">
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              if (!showConfigDropdown) refreshConfigurators(settings?.configurator.window_title_pattern);
                              setShowConfigDropdown(!showConfigDropdown);
                            }}
                            className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showConfigDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'} max-w-[140px]`}
                            title="Select Configurator Window"
                          >
                            <Monitor className="w-3.5 h-3.5 text-zinc-500" />
                            <span className="truncate max-w-[80px] hidden sm:inline whitespace-nowrap">{getActiveConfiguratorTitle()}</span>
                            <ChevronDown className={`w-3 h-3 text-zinc-500 transition-transform ${showConfigDropdown ? 'rotate-180' : ''}`} />
                          </button>
                          {showConfigDropdown && (
                            <div className="absolute bottom-full left-0 mb-2 w-64 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl overflow-hidden z-30 ring-1 ring-black/20">
                              <div className="p-2 border-b border-[#27272a] flex justify-between items-center text-xs text-zinc-500 font-bold uppercase tracking-wider">
                                <span>Detect 1C Configurator</span>
                                <button onClick={() => refreshConfigurators(settings?.configurator.window_title_pattern)} className="hover:text-zinc-300">
                                  <RefreshCw className="w-3 h-3" />
                                </button>
                              </div>
                              <div className="max-h-48 overflow-y-auto p-1">
                                {detectedWindows.length === 0 ? (
                                  <div className="p-3 text-xs text-zinc-600 text-center italic">No 1C Configurator found</div>
                                ) : (
                                  detectedWindows.map(win => (
                                    <button
                                      key={win.hwnd}
                                      onClick={() => selectConfigurator(win.hwnd)}
                                      className={`w-full text-left px-3 py-2 rounded-md text-xs truncate flex items-center justify-between ${settings?.configurator.selected_window_hwnd === win.hwnd ? 'bg-blue-500/10 text-blue-400' : 'text-zinc-400 hover:bg-[#27272a] hover:text-zinc-200'}`}
                                    >
                                      <span className="truncate">{win.title}</span>
                                      {settings?.configurator.selected_window_hwnd === win.hwnd && <div className="w-1.5 h-1.5 rounded-full bg-blue-500" />}
                                    </button>
                                  ))
                                )}
                              </div>
                            </div>
                          )}
                        </div>

                        <div className="h-4 w-px bg-zinc-700/50 mx-1 hidden sm:block flex-shrink-0" />

                        {/* CONTEXT ACTIONS (DROPDOWN) */}
                        <div className="relative flex-shrink-0">
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              setShowGetCodeDropdown(!showGetCodeDropdown);
                            }}
                            className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showGetCodeDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                            title="Get Code Options"
                          >
                            <FileText className="w-3.5 h-3.5" />
                            <span className="hidden sm:inline whitespace-nowrap">Get Code</span>
                            <ChevronDown className={`w-3 h-3 text-zinc-500 transition-transform ${showGetCodeDropdown ? 'rotate-180' : ''}`} />
                          </button>
                          {showGetCodeDropdown && (
                            <div className="absolute bottom-full right-0 mb-2 w-40 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl overflow-hidden z-30 ring-1 ring-black/20 flex flex-col p-1">
                              <button
                                onClick={() => { fetchCodeForContext(true); setShowGetCodeDropdown(false); }}
                                className="flex items-center gap-2 px-3 py-2 rounded-md text-xs text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left"
                              >
                                <FileText className="w-3.5 h-3.5" />
                                <span>Module (All)</span>
                              </button>
                              <button
                                onClick={() => { fetchCodeForContext(false); setShowGetCodeDropdown(false); }}
                                className="flex items-center gap-2 px-3 py-2 rounded-md text-xs text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left"
                              >
                                <MousePointerClick className="w-3.5 h-3.5" />
                                <span>Selection</span>
                              </button>
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  </div>
                )}

                <button
                  onClick={sendMessage}
                  disabled={isLoading || !input.trim()}
                  className={`p-2 rounded-lg transition-colors flex-shrink-0 ${input.trim() ? 'bg-blue-600 hover:bg-blue-500 text-white shadow-lg shadow-blue-500/20' : 'bg-[#27272a] text-zinc-600'}`}
                >
                  <ArrowUp className="w-4 h-4" strokeWidth={2.5} />
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* Side Panel: Full Height Sibling (RIGHT) */}
        <div className={`z-40 h-full border-l border-[#27272a] transition-all duration-300 ${showSidePanel ? 'flex' : 'hidden'}`}>
          <CodeSidePanel
            isOpen={showSidePanel}
            onClose={() => setShowSidePanel(false)}
            originalCode={originalCode}
            modifiedCode={modifiedCode}
            onModifiedCodeChange={setModifiedCode}
            diagnostics={diagnostics}
            onApply={handleApplyToConfigurator}
            isApplying={isApplying}
          />
        </div>
      </div>

      <SettingsPanel
        isOpen={showSettings}
        onClose={() => {
          setShowSettings(false);
          invoke<ProfileStore>('get_profiles').then(setProfiles);
          invoke<AppSettings>('get_settings').then(setSettings);
        }}
      />
    </div>
  );
}

export default App;
