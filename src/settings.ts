// ── Pure helpers (exported for unit tests) ────────────────────────────────────

export type Provider = 'ollama' | 'claude' | 'openai';

export interface ModelInfo {
  defaultValue: string;
}

export function getModelInfo(provider: Provider): ModelInfo {
  switch (provider) {
    case 'ollama':
      return { defaultValue: '' };
    case 'claude':
      return { defaultValue: 'claude-haiku-4-5-20251001' };
    case 'openai':
      return { defaultValue: 'gpt-4o-mini' };
  }
}

export function requiresApiKey(provider: Provider): boolean {
  return provider === 'claude' || provider === 'openai';
}

// ── DOM glue (runs only in browser) ──────────────────────────────────────────

async function initSettings(): Promise<void> {
  const { invoke, convertFileSrc } = await import('@tauri-apps/api/core');
  const { resolveResource } = await import('@tauri-apps/api/path');

  const appIcon = document.getElementById('app-icon') as HTMLImageElement;
  resolveResource('icon.png')
    .then((p) => {
      appIcon.src = convertFileSrc(p);
    })
    .catch(() => {});

  const DEFAULT_SHORTCUT = 'Ctrl+Shift+Space';

  interface SavedSettings {
    provider: Provider;
    model: string;
    launchAtLogin: boolean;
    customPrompt?: string | null;
    shortcut: string;
  }

  // Elements
  const providerSelect = document.getElementById(
    'provider-select',
  ) as HTMLSelectElement;
  const sectionOllama = document.getElementById('section-ollama')!;
  const sectionClaude = document.getElementById('section-claude')!;
  const sectionOpenai = document.getElementById('section-openai')!;
  const ollamaModelSelect = document.getElementById(
    'ollama-model-select',
  ) as HTMLSelectElement;
  const ollamaStatus = document.getElementById('ollama-status')!;
  const btnOllamaRefresh = document.getElementById('btn-ollama-refresh')!;
  const claudeModelSelect = document.getElementById(
    'claude-model-select',
  ) as HTMLSelectElement;
  const claudeKeyInput = document.getElementById(
    'claude-key-input',
  ) as HTMLInputElement;
  const btnToggleClaudeKey = document.getElementById('btn-toggle-claude-key')!;
  const openaiModelSelect = document.getElementById(
    'openai-model-select',
  ) as HTMLSelectElement;
  const openaiKeyInput = document.getElementById(
    'openai-key-input',
  ) as HTMLInputElement;
  const btnToggleOpenaiKey = document.getElementById('btn-toggle-openai-key')!;
  const promptTextarea = document.getElementById(
    'prompt-textarea',
  ) as HTMLTextAreaElement;
  const btnResetPrompt = document.getElementById('btn-reset-prompt')!;
  const launchAtLogin = document.getElementById(
    'launch-at-login',
  ) as HTMLInputElement;
  const btnSave = document.getElementById('btn-save') as HTMLButtonElement;
  const btnUpdate = document.getElementById('btn-update') as HTMLButtonElement;
  const appVersion = document.getElementById('app-version')!;
  const shortcutRecorder = document.getElementById(
    'shortcut-recorder',
  ) as HTMLButtonElement;
  const btnResetShortcut = document.getElementById('btn-reset-shortcut')!;
  const shortcutHint = document.getElementById('shortcut-hint')!;

  let defaultPrompt = '';
  let savedClaudeKey = '';
  let savedOpenaiKey = '';
  let currentShortcut = DEFAULT_SHORTCUT;
  let isRecording = false;

  const EYE_SVG =
    '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/></svg>';
  const EYE_OFF_SVG =
    '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-10-7-10-7a18.06 18.06 0 0 1 5.06-5.94"/><path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 10 7 10 7a18.1 18.1 0 0 1-2.16 3.19"/><line x1="1" y1="1" x2="23" y2="23"/></svg>';

  function setupEyeToggle(input: HTMLInputElement, btn: HTMLElement): void {
    btn.innerHTML = EYE_SVG;
    btn.addEventListener('click', () => {
      const isHidden = input.type === 'password';
      input.type = isHidden ? 'text' : 'password';
      btn.innerHTML = isHidden ? EYE_OFF_SVG : EYE_SVG;
    });
  }

  function setShortcut(value: string): void {
    currentShortcut = value;
    shortcutRecorder.textContent = value;
  }

  function startRecording(): void {
    isRecording = true;
    shortcutRecorder.textContent = 'Press keys…';
    shortcutRecorder.classList.add('border-purple-500/50', 'text-white');
    shortcutRecorder.classList.remove('text-white/70');
    shortcutHint.textContent = 'Press Esc to cancel';
  }

  function stopRecording(): void {
    isRecording = false;
    clearHeldModifiers();
    shortcutRecorder.textContent = currentShortcut;
    shortcutRecorder.classList.remove('border-purple-500/50', 'text-white');
    shortcutRecorder.classList.add('text-white/70');
    shortcutHint.textContent = '';
  }

  async function loadKeyIntoInput(
    provider: 'claude' | 'openai',
    input: HTMLInputElement,
    btn: HTMLElement,
  ): Promise<void> {
    const key = await invoke<string>('get_api_key', { provider });
    input.value = key;
    input.type = 'password';
    btn.innerHTML = EYE_SVG;
    if (provider === 'claude') savedClaudeKey = key;
    else savedOpenaiKey = key;
  }

  function getSelectedProvider(): Provider {
    return providerSelect.value as Provider;
  }

  function populateSelect(
    select: HTMLSelectElement,
    models: string[],
    current: string,
  ): void {
    select.innerHTML = '';
    for (const m of models) {
      const opt = document.createElement('option');
      opt.value = m;
      opt.textContent = m;
      if (m === current) opt.selected = true;
      select.appendChild(opt);
    }
  }

  async function loadOllamaModels(currentModel: string): Promise<void> {
    ollamaStatus.textContent = 'Loading…';
    try {
      const models = await invoke<string[]>('get_ollama_models');
      if (models.length === 0) {
        ollamaStatus.textContent = 'Ollama is running but has no models';
      } else {
        ollamaStatus.textContent = '';
      }
      populateSelect(ollamaModelSelect, models, currentModel);
    } catch {
      ollamaStatus.textContent = 'Ollama is not running';
      ollamaModelSelect.innerHTML = '';
    }
  }

  async function loadClaudeModels(currentModel: string): Promise<void> {
    try {
      const models = await invoke<string[]>('get_claude_models');
      populateSelect(
        claudeModelSelect,
        models,
        currentModel || getModelInfo('claude').defaultValue,
      );
    } catch {
      populateSelect(
        claudeModelSelect,
        [getModelInfo('claude').defaultValue],
        currentModel,
      );
    }
  }

  async function loadOpenaiModels(currentModel: string): Promise<void> {
    try {
      const models = await invoke<string[]>('get_openai_models');
      populateSelect(
        openaiModelSelect,
        models,
        currentModel || getModelInfo('openai').defaultValue,
      );
    } catch {
      populateSelect(
        openaiModelSelect,
        [getModelInfo('openai').defaultValue],
        currentModel,
      );
    }
  }

  async function switchProvider(
    provider: Provider,
    currentModel = '',
  ): Promise<void> {
    sectionOllama.classList.toggle('hidden', provider !== 'ollama');
    sectionClaude.classList.toggle('hidden', provider !== 'claude');
    sectionOpenai.classList.toggle('hidden', provider !== 'openai');

    if (provider === 'ollama') {
      await loadOllamaModels(currentModel);
    } else if (provider === 'claude') {
      await Promise.all([
        loadClaudeModels(currentModel),
        loadKeyIntoInput('claude', claudeKeyInput, btnToggleClaudeKey),
      ]);
    } else {
      await Promise.all([
        loadOpenaiModels(currentModel),
        loadKeyIntoInput('openai', openaiKeyInput, btnToggleOpenaiKey),
      ]);
    }
  }

  function getSelectedModel(): string {
    const provider = getSelectedProvider();
    if (provider === 'ollama') return ollamaModelSelect.value;
    if (provider === 'claude') return claudeModelSelect.value;
    return openaiModelSelect.value;
  }

  async function saveSettings(): Promise<void> {
    const provider = getSelectedProvider();
    const model = getSelectedModel();
    const promptValue = promptTextarea.value.trim();
    const customPrompt = promptValue === defaultPrompt ? null : promptValue;

    // Save API key only if it was changed
    const claudeKey = claudeKeyInput.value;
    if (provider === 'claude' && claudeKey && claudeKey !== savedClaudeKey) {
      await invoke('save_api_key', { key: claudeKey, provider: 'claude' });
      savedClaudeKey = claudeKey;
      await loadClaudeModels(claudeModelSelect.value);
    }
    const openaiKey = openaiKeyInput.value;
    if (provider === 'openai' && openaiKey && openaiKey !== savedOpenaiKey) {
      await invoke('save_api_key', { key: openaiKey, provider: 'openai' });
      savedOpenaiKey = openaiKey;
      await loadOpenaiModels(openaiModelSelect.value);
    }

    await invoke('save_settings', {
      settings: {
        provider,
        model,
        launchAtLogin: launchAtLogin.checked,
        customPrompt,
        shortcut: currentShortcut,
      },
    });

    await invoke('update_shortcut', { shortcut: currentShortcut });
  }

  // ── Event wiring ────────────────────────────────────────────────────────────

  providerSelect.addEventListener('change', () => {
    void switchProvider(getSelectedProvider());
  });

  btnOllamaRefresh.addEventListener('click', () => {
    void loadOllamaModels(ollamaModelSelect.value);
  });

  btnResetPrompt.addEventListener('click', () => {
    promptTextarea.value = defaultPrompt;
  });

  shortcutRecorder.addEventListener('click', () => {
    if (isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  });

  btnResetShortcut.addEventListener('click', () => {
    stopRecording();
    setShortcut(DEFAULT_SHORTCUT);
  });

  const CODE_MAP: Record<string, string> = {
    // Modifier keys
    ControlLeft: 'Ctrl',
    ControlRight: 'Ctrl',
    ShiftLeft: 'Shift',
    ShiftRight: 'Shift',
    AltLeft: 'Alt',
    AltRight: 'Alt',
    MetaLeft: 'Meta',
    MetaRight: 'Meta',
    // Special keys
    Space: 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    // Punctuation
    Minus: '-',
    Equal: '=',
    BracketLeft: '[',
    BracketRight: ']',
    Backslash: '\\',
    Semicolon: ';',
    Quote: "'",
    Backquote: '`',
    Comma: ',',
    Period: '.',
    Slash: '/',
  };

  const MODIFIER_KEYS = new Set(['Ctrl', 'Shift', 'Alt', 'Meta']);

  function codeToKey(code: string): string | null {
    if (code in CODE_MAP) return CODE_MAP[code];
    if (code.startsWith('Key')) return code.slice(3); // KeyA → A
    if (code.startsWith('Digit')) return code.slice(5); // Digit1 → 1
    if (/^F\d+$/.test(code)) return code; // F1–F12
    return null;
  }

  // Tracks modifiers in the order the user pressed them
  const heldModifiers: string[] = [];

  function clearHeldModifiers(): void {
    heldModifiers.length = 0;
  }

  document.addEventListener('keydown', (e) => {
    if (!isRecording) return;
    e.preventDefault();

    if (e.code === 'Escape') {
      stopRecording();
      clearHeldModifiers();
      return;
    }

    const keyName = codeToKey(e.code);
    const isModifier = keyName !== null && MODIFIER_KEYS.has(keyName);

    // Track and preview modifier keys in press order
    if (isModifier) {
      if (!heldModifiers.includes(keyName!)) heldModifiers.push(keyName!);
      shortcutRecorder.textContent = heldModifiers.join('+') + '+';
      return;
    }

    // Require at least one modifier before the final key
    if (heldModifiers.length === 0) return;

    if (!keyName) return; // unrecognised physical key — keep recording

    setShortcut([...heldModifiers, keyName].join('+'));
    stopRecording();
    clearHeldModifiers();
  });

  document.addEventListener('keyup', (e) => {
    if (!isRecording) return;
    const keyName = codeToKey(e.code);
    if (!keyName) return;
    if (!MODIFIER_KEYS.has(keyName)) {
      if (heldModifiers.length === 0) return;
      setShortcut([...heldModifiers, keyName].join('+'));
      stopRecording();
      return;
    }

    const idx = heldModifiers.indexOf(keyName);
    if (idx !== -1) heldModifiers.splice(idx, 1);
  });

  btnSave.addEventListener('click', () => {
    void (async () => {
      btnSave.disabled = true;
      try {
        await saveSettings();
        btnSave.textContent = 'Saved ✓';
        setTimeout(() => {
          btnSave.textContent = 'Save Settings';
          btnSave.disabled = false;
        }, 1500);
      } catch (e) {
        btnSave.textContent = `Error: ${String(e).replace(/^Error:\s*/, '')}`;
        btnSave.disabled = false;
        setTimeout(() => {
          btnSave.textContent = 'Save Settings';
        }, 3000);
      }
    })();
  });

  const { listen } = await import('@tauri-apps/api/event');

  await listen<string>('shortcut-triggered', ({ payload }) => {
    if (isRecording) {
      setShortcut(payload);
      stopRecording();
    }
  });

  await listen('settings-opened', () => {
    void (async () => {
      if (isRecording) stopRecording();
      const saved = await invoke<SavedSettings>('get_settings');
      providerSelect.value = saved.provider;
      launchAtLogin.checked = saved.launchAtLogin;
      promptTextarea.value = saved.customPrompt ?? defaultPrompt;
      setShortcut(saved.shortcut ?? DEFAULT_SHORTCUT);
      await switchProvider(saved.provider, saved.model);
    })();
  });
  await listen<{
    state: string;
    version?: string;
    message?: string;
  }>('update-status', ({ payload }) => {
    switch (payload.state) {
      case 'checking':
        btnUpdate.textContent = 'Checking…';
        btnUpdate.disabled = true;
        break;
      case 'up-to-date':
        btnUpdate.textContent = 'Up to date ✓';
        setTimeout(() => {
          btnUpdate.textContent = 'Check for Updates';
          btnUpdate.disabled = false;
        }, 2000);
        break;
      case 'downloading':
        btnUpdate.textContent = `Downloading v${payload.version ?? ''}…`;
        btnUpdate.disabled = true;
        break;
      case 'installing':
        btnUpdate.textContent = 'Installing…';
        btnUpdate.disabled = true;
        break;
      case 'error':
        btnUpdate.textContent = 'Check for Updates';
        btnUpdate.disabled = false;
        break;
    }
  });

  btnUpdate.addEventListener('click', () => {
    void invoke('check_update');
  });

  // ── Initial load ────────────────────────────────────────────────────────────

  const [settings, version, prompt] = await Promise.all([
    invoke<SavedSettings>('get_settings'), // returns Result on Rust side, unwrapped by Tauri
    invoke<string>('get_app_version'),
    invoke<string>('get_default_prompt'),
  ]);

  defaultPrompt = prompt;
  appVersion.textContent = `v${version}`;
  providerSelect.value = settings.provider;
  launchAtLogin.checked = settings.launchAtLogin;
  promptTextarea.value = settings.customPrompt ?? defaultPrompt;
  setShortcut(settings.shortcut ?? DEFAULT_SHORTCUT);

  setupEyeToggle(claudeKeyInput, btnToggleClaudeKey);
  setupEyeToggle(openaiKeyInput, btnToggleOpenaiKey);

  await switchProvider(settings.provider, settings.model);
}

if (typeof document !== 'undefined') {
  void initSettings();
}
