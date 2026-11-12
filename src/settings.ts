// ── Pure helpers (exported for unit tests) ────────────────────────────────────

export type Provider = 'ollama' | 'claude' | 'openai';
export type KeyProvider = Exclude<Provider, 'ollama'>;

export interface ModelInfo {
  defaultValue: string;
}

/** Suggested when Ollama has no model yet: multilingual, and fits a 16 GB+ machine. */
export const RECOMMENDED_OLLAMA_MODELS = {
  /** Apple's MLX engine: about 8× faster than the regular build on Apple Silicon. */
  appleSilicon: 'gemma4:12b-mlx',
  other: 'gemma4:12b-it-qat',
};

export function getModelInfo(provider: Provider): ModelInfo {
  switch (provider) {
    case 'ollama':
      return { defaultValue: '' };
    case 'claude':
      return { defaultValue: 'claude-haiku-5-5' };
    case 'openai':
      return { defaultValue: 'gpt-6-luna' };
  }
}

export function requiresApiKey(provider: Provider): provider is KeyProvider {
  return provider === 'claude' || provider === 'openai';
}

/** The prompt to save: null (use the built-in one) when empty or unchanged. */
export function customPromptValue(
  text: string,
  defaultPrompt: string,
): string | null {
  const trimmed = text.trim();
  return trimmed === '' || trimmed === defaultPrompt.trim() ? null : trimmed;
}

// ── DOM glue (runs only in browser) ──────────────────────────────────────────

interface SavedSettings {
  provider: Provider;
  model: string;
  launchAtLogin: boolean;
  customPrompt?: string | null;
  shortcut: string;
}

interface KeyStatus {
  saved: boolean;
  keychainOk: boolean;
}

async function initSettings(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');

  const DEFAULT_SHORTCUT = 'Ctrl+Shift+Space';
  const byId = <T extends HTMLElement = HTMLElement>(id: string): T =>
    document.getElementById(id) as T;

  // Elements
  const providerSelect = byId<HTMLSelectElement>('provider-select');
  const sections: Record<Provider, HTMLElement> = {
    ollama: byId('section-ollama'),
    claude: byId('section-claude'),
    openai: byId('section-openai'),
  };
  const ollamaModelSelect = byId<HTMLSelectElement>('ollama-model-select');
  const ollamaStatus = byId('ollama-status');
  const btnOllamaRefresh = byId('btn-ollama-refresh');
  const promptTextarea = byId<HTMLTextAreaElement>('prompt-textarea');
  const btnResetPrompt = byId('btn-reset-prompt');
  const launchAtLogin = byId<HTMLInputElement>('launch-at-login');
  const btnSave = byId<HTMLButtonElement>('btn-save');
  const btnUpdate = byId<HTMLButtonElement>('btn-update');
  const appVersion = byId('app-version');
  const shortcutRecorder = byId<HTMLButtonElement>('shortcut-recorder');
  const btnResetShortcut = byId('btn-reset-shortcut');
  const shortcutHint = byId('shortcut-hint');

  interface KeyUi {
    input: HTMLInputElement;
    toggle: HTMLElement;
    hint: HTMLElement;
    remove: HTMLButtonElement;
    select: HTMLSelectElement;
    placeholder: string;
  }
  const keyUi: Record<KeyProvider, KeyUi> = {
    claude: {
      input: byId('claude-key-input'),
      toggle: byId('btn-toggle-claude-key'),
      hint: byId('claude-key-hint'),
      remove: byId('btn-remove-claude-key'),
      select: byId('claude-model-select'),
      placeholder: 'sk-ant-…',
    },
    openai: {
      input: byId('openai-key-input'),
      toggle: byId('btn-toggle-openai-key'),
      hint: byId('openai-key-hint'),
      remove: byId('btn-remove-openai-key'),
      select: byId('openai-model-select'),
      placeholder: 'sk-…',
    },
  };

  let defaultPrompt = '';
  let currentShortcut = DEFAULT_SHORTCUT;
  let isRecording = false;

  const EYE_SVG =
    '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/></svg>';
  const EYE_OFF_SVG =
    '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-10-7-10-7a18.06 18.06 0 0 1 5.06-5.94"/><path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 10 7 10 7a18.1 18.1 0 0 1-2.16 3.19"/><line x1="1" y1="1" x2="23" y2="23"/></svg>';

  function setupEyeToggle({ input, toggle }: KeyUi): void {
    toggle.innerHTML = EYE_SVG;
    toggle.addEventListener('click', () => {
      const isHidden = input.type === 'password';
      input.type = isHidden ? 'text' : 'password';
      toggle.innerHTML = isHidden ? EYE_OFF_SVG : EYE_SVG;
    });
  }

  // ── Shortcut recorder ───────────────────────────────────────────────────────

  function setShortcut(value: string): void {
    currentShortcut = value;
    shortcutRecorder.textContent = value;
  }

  // Tracks modifiers in the order the user pressed them
  const heldModifiers: string[] = [];

  function startRecording(): void {
    isRecording = true;
    shortcutRecorder.textContent = 'Press keys…';
    shortcutRecorder.classList.add('border-purple-500/50', 'text-white');
    shortcutRecorder.classList.remove('text-white/70');
    shortcutHint.textContent = 'Press Esc to cancel';
  }

  function stopRecording(): void {
    isRecording = false;
    heldModifiers.length = 0;
    shortcutRecorder.textContent = currentShortcut;
    shortcutRecorder.classList.remove('border-purple-500/50', 'text-white');
    shortcutRecorder.classList.add('text-white/70');
    shortcutHint.textContent = '';
  }

  const CODE_MAP: Record<string, string> = {
    ControlLeft: 'Ctrl',
    ControlRight: 'Ctrl',
    ShiftLeft: 'Shift',
    ShiftRight: 'Shift',
    AltLeft: 'Alt',
    AltRight: 'Alt',
    MetaLeft: 'Meta',
    MetaRight: 'Meta',
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

  function finishRecording(keyName: string): void {
    setShortcut([...heldModifiers, keyName].join('+'));
    stopRecording();
  }

  document.addEventListener('keydown', (e) => {
    if (!isRecording) return;
    e.preventDefault();
    if (e.code === 'Escape') {
      stopRecording();
      return;
    }
    const keyName = codeToKey(e.code);
    if (keyName !== null && MODIFIER_KEYS.has(keyName)) {
      if (!heldModifiers.includes(keyName)) heldModifiers.push(keyName);
      shortcutRecorder.textContent = heldModifiers.join('+') + '+';
      return;
    }
    // Require at least one modifier before the final key
    if (heldModifiers.length === 0 || keyName === null) return;
    finishRecording(keyName);
  });

  document.addEventListener('keyup', (e) => {
    if (!isRecording) return;
    const keyName = codeToKey(e.code);
    if (keyName === null) return;
    if (!MODIFIER_KEYS.has(keyName)) {
      if (heldModifiers.length > 0) finishRecording(keyName);
      return;
    }
    const idx = heldModifiers.indexOf(keyName);
    if (idx !== -1) heldModifiers.splice(idx, 1);
  });

  shortcutRecorder.addEventListener('click', () => {
    if (isRecording) stopRecording();
    else startRecording();
  });

  btnResetShortcut.addEventListener('click', () => {
    stopRecording();
    setShortcut(DEFAULT_SHORTCUT);
  });

  // ── Providers, models and keys ──────────────────────────────────────────────

  function getSelectedProvider(): Provider {
    return providerSelect.value as Provider;
  }

  /** Fills a model list. A saved model missing from it stays listed, marked, so it isn't swapped silently. */
  function populateSelect(
    select: HTMLSelectElement,
    models: string[],
    current: string,
  ): void {
    select.innerHTML = '';
    const entries = models.map((m) => ({ value: m, label: m }));
    if (current !== '' && !models.includes(current)) {
      entries.unshift({ value: current, label: `${current} (not available)` });
    }
    for (const { value, label } of entries) {
      const opt = document.createElement('option');
      opt.value = value;
      opt.textContent = label;
      opt.selected = value === current;
      select.appendChild(opt);
    }
  }

  async function loadOllamaModels(currentModel: string): Promise<void> {
    ollamaStatus.textContent = 'Loading…';
    try {
      const models = await invoke<string[]>('get_ollama_models');
      ollamaStatus.textContent =
        models.length === 0
          ? `No models installed yet. In a terminal, run: ollama pull ${RECOMMENDED_OLLAMA_MODELS.appleSilicon} (Apple Silicon Mac) or ollama pull ${RECOMMENDED_OLLAMA_MODELS.other}`
          : '';
      populateSelect(ollamaModelSelect, models, currentModel);
    } catch {
      ollamaStatus.textContent =
        "Ollama isn't running. Open the Ollama app, then refresh.";
      populateSelect(ollamaModelSelect, [], currentModel);
    }
  }

  async function loadCloudModels(
    provider: KeyProvider,
    currentModel: string,
  ): Promise<void> {
    const fallback = getModelInfo(provider).defaultValue;
    const command =
      provider === 'claude' ? 'get_claude_models' : 'get_openai_models';
    let models = [fallback];
    try {
      models = await invoke<string[]>(command);
    } catch {
      // Keep the default model.
    }
    populateSelect(keyUi[provider].select, models, currentModel || fallback);
  }

  async function loadKeyStatus(provider: KeyProvider): Promise<void> {
    const ui = keyUi[provider];
    const status = await invoke<KeyStatus>('get_api_key_status', { provider });
    ui.input.value = '';
    ui.input.type = 'password';
    ui.toggle.innerHTML = EYE_SVG;
    ui.input.placeholder = status.saved
      ? '•••••••• saved — type a new key to replace it'
      : ui.placeholder;
    ui.remove.classList.toggle('hidden', !status.saved);
    ui.hint.textContent = !status.keychainOk
      ? "The system keychain couldn't be used, so the key can't be stored safely."
      : status.saved
        ? 'Stored in your system keychain.'
        : '';
  }

  async function switchProvider(
    provider: Provider,
    currentModel = '',
  ): Promise<void> {
    for (const [name, section] of Object.entries(sections)) {
      section.classList.toggle('hidden', name !== provider);
    }
    if (requiresApiKey(provider)) {
      await Promise.all([
        loadCloudModels(provider, currentModel),
        loadKeyStatus(provider),
      ]);
    } else {
      await loadOllamaModels(currentModel);
    }
  }

  function getSelectedModel(): string {
    const provider = getSelectedProvider();
    return requiresApiKey(provider)
      ? keyUi[provider].select.value
      : ollamaModelSelect.value;
  }

  async function saveSettings(): Promise<void> {
    const provider = getSelectedProvider();

    // A typed key replaces the saved one; an empty field keeps it.
    if (requiresApiKey(provider)) {
      const key = keyUi[provider].input.value.trim();
      if (key !== '') {
        await invoke('save_api_key', { provider, key });
        await Promise.all([
          loadKeyStatus(provider),
          loadCloudModels(provider, getSelectedModel()),
        ]);
      }
    }

    // Also registers the shortcut; an unusable one is rejected and the old one kept.
    await invoke('save_settings', {
      settings: {
        provider,
        model: getSelectedModel(),
        launchAtLogin: launchAtLogin.checked,
        customPrompt: customPromptValue(promptTextarea.value, defaultPrompt),
        shortcut: currentShortcut,
      },
    });
  }

  function applySaved(saved: SavedSettings): Promise<void> {
    if (isRecording) stopRecording();
    providerSelect.value = saved.provider;
    launchAtLogin.checked = saved.launchAtLogin;
    promptTextarea.value = saved.customPrompt ?? defaultPrompt;
    setShortcut(saved.shortcut || DEFAULT_SHORTCUT);
    return switchProvider(saved.provider, saved.model);
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

  for (const provider of ['claude', 'openai'] as const) {
    setupEyeToggle(keyUi[provider]);
    keyUi[provider].remove.addEventListener('click', () => {
      void (async () => {
        try {
          await invoke('delete_api_key', { provider });
        } catch (e) {
          keyUi[provider].hint.textContent = String(e);
          return;
        }
        await Promise.all([
          loadKeyStatus(provider),
          loadCloudModels(provider, getSelectedModel()),
        ]);
      })();
    });
  }

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
        }, 4000);
      }
    })();
  });

  await listen<string>('shortcut-triggered', ({ payload }) => {
    if (isRecording) {
      setShortcut(payload);
      stopRecording();
    }
  });

  await listen('settings-opened', () => {
    void invoke<SavedSettings>('get_settings').then(applySaved);
  });

  await listen<{ state: string; version?: string; message?: string }>(
    'update-status',
    ({ payload }) => {
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
          btnUpdate.textContent = 'Update failed';
          btnUpdate.title = payload.message ?? '';
          setTimeout(() => {
            btnUpdate.textContent = 'Check for Updates';
            btnUpdate.disabled = false;
          }, 3000);
          break;
      }
    },
  );

  btnUpdate.addEventListener('click', () => {
    void invoke('check_update');
  });

  // ── Initial load ────────────────────────────────────────────────────────────

  const [settings, version, prompt] = await Promise.all([
    invoke<SavedSettings>('get_settings'),
    invoke<string>('get_app_version'),
    invoke<string>('get_default_prompt'),
  ]);
  defaultPrompt = prompt;
  appVersion.textContent = `v${version}`;
  await applySaved(settings);
}

if (typeof document !== 'undefined') {
  void initSettings();
}
