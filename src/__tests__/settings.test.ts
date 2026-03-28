import { getModelInfo, requiresApiKey } from '../settings';
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

  it('claude: returns the correct default model identifier', () => {
    expect(getModelInfo('claude').defaultValue).toBe(
      'claude-haiku-4-5-20251001',
    );
  });

  it('openai: returns the correct default model identifier', () => {
    expect(getModelInfo('openai').defaultValue).toBe('gpt-4o-mini');
  });
});
