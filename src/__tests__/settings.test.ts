import {
  RECOMMENDED_OLLAMA_MODELS,
  customPromptValue,
  getModelInfo,
  requiresApiKey,
} from '../settings';
import { describe, expect, it } from 'vitest';

describe('requiresApiKey', () => {
  it('returns false for ollama', () => {
    expect(requiresApiKey('ollama')).toBe(false);
  });

  it('returns true for claude', () => {
    expect(requiresApiKey('claude')).toBe(true);
  });

  it('returns true for openai', () => {
    expect(requiresApiKey('openai')).toBe(true);
  });
});

describe('getModelInfo', () => {
  it('ollama: empty default value', () => {
    expect(getModelInfo('ollama').defaultValue).toBe('');
  });

  it('claude: defaults to the current Haiku', () => {
    expect(getModelInfo('claude').defaultValue).toBe('claude-haiku-5-5');
  });

  it('openai: defaults to the gpt-4o-mini replacement', () => {
    expect(getModelInfo('openai').defaultValue).toBe('gpt-6-luna');
  });
});

describe('RECOMMENDED_OLLAMA_MODELS', () => {
  it('are full Ollama tags', () => {
    for (const model of Object.values(RECOMMENDED_OLLAMA_MODELS)) {
      expect(model).toMatch(/^[a-z0-9.]+:[a-z0-9.-]+$/);
    }
  });

  it('recommends the MLX build on Apple Silicon', () => {
    expect(RECOMMENDED_OLLAMA_MODELS.appleSilicon).toMatch(/-mlx$/);
  });
});

describe('customPromptValue', () => {
  const def = 'Default prompt.';

  it('returns null for an empty prompt, so the built-in one is used', () => {
    expect(customPromptValue('   ', def)).toBeNull();
  });

  it('returns null when the prompt is the default', () => {
    expect(customPromptValue('Default prompt.\n', def)).toBeNull();
  });

  it('returns the trimmed custom prompt', () => {
    expect(customPromptValue('  Only fix typos. ', def)).toBe(
      'Only fix typos.',
    );
  });
});
