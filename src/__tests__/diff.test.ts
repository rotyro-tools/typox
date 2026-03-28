import { computeDiff, tokenize } from '../diff';
import { describe, expect, it } from 'vitest';

describe('tokenize', () => {
  it('splits plain text into word and space tokens', () => {
    expect(tokenize('hello world')).toEqual(['hello', ' ', 'world']);
  });

  it('returns single-element array for empty string', () => {
    expect(tokenize('')).toEqual(['']);
  });

  it('preserves multiple consecutive spaces', () => {
    expect(tokenize('a  b')).toEqual(['a', '  ', 'b']);
  });

  it('handles leading whitespace', () => {
    expect(tokenize(' hello')).toEqual(['', ' ', 'hello']);
  });

  it('handles trailing whitespace', () => {
    expect(tokenize('hello ')).toEqual(['hello', ' ', '']);
  });

  it('handles tab characters', () => {
    expect(tokenize('a\tb')).toEqual(['a', '\t', 'b']);
  });
});

describe('computeDiff', () => {
  it('marks no tokens as changed when texts are identical', () => {
    const result = computeDiff('hello world', 'hello world');
    expect(result.every((t) => !t.changed)).toBe(true);
  });

  it('marks a single inserted word as changed', () => {
    const result = computeDiff('hello world', 'hello earth');
    expect(result.find((t) => t.text === 'earth')?.changed).toBe(true);
    expect(result.find((t) => t.text === 'hello')?.changed).toBe(false);
  });

  it('marks inserted words as changed and deleted words as not changed', () => {
    // 'foo' and 'bar' are deleted (changed:false, deleted:true);
    // 'baz' and 'qux' are inserted (changed:true)
    const result = computeDiff('foo bar', 'baz qux');
    const inserted = result.filter((t) => t.text.trim() !== '' && !t.deleted);
    expect(inserted.every((t) => t.changed)).toBe(true);
    const deleted = result.filter((t) => t.deleted);
    expect(deleted.every((t) => !t.changed)).toBe(true);
  });

  it('never marks whitespace tokens as changed', () => {
    const result = computeDiff('foo bar', 'baz qux');
    const spaces = result.filter((t) => t.text.trim() === '');
    expect(spaces.length).toBeGreaterThan(0);
    expect(spaces.every((t) => !t.changed)).toBe(true);
  });

  it('handles empty input strings', () => {
    const result = computeDiff('', '');
    expect(result).toHaveLength(1);
    expect(result[0].text).toBe('');
    expect(result[0].changed).toBe(false);
  });

  it('is case-sensitive: differently-cased tokens are treated as distinct', () => {
    // 'Hello' is deleted, 'hello' is inserted — capitalization change shows as changed
    const result = computeDiff('Hello', 'hello');
    expect(result.find((t) => t.text === 'hello')?.changed).toBe(true);
  });

  it('inserts a space before a deleted token that follows a kept token', () => {
    // keep 'foo', delete 'bar', insert 'baz' — space must appear before the deleted token
    const result = computeDiff('foo bar', 'foo baz');
    const deletedIdx = result.findIndex((t) => t.deleted);
    expect(deletedIdx).toBeGreaterThan(0);
    expect(result[deletedIdx - 1].text).toBe(' ');
  });

  it('uses LCS: reordering words produces deletions and insertions', () => {
    // LCS of ['world','hello'] vs ['hello','world'] has length 1 —
    // one word is kept, the other pair becomes delete+insert
    const result = computeDiff('world hello', 'hello world');
    const inserted = result.filter((t) => t.text.trim() !== '' && t.changed);
    expect(inserted.length).toBeGreaterThan(0);
  });

  it('preserves whitespace tokens in the output', () => {
    const result = computeDiff('a b', 'x y');
    const spaces = result.filter((t) => t.text.trim() === '');
    expect(spaces.every((t) => !t.changed)).toBe(true);
  });
});
