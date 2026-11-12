import {
  type DiffToken,
  computeDiff,
  countChanges,
  escapeHtml,
  renderDiffHtml,
  tokenize,
} from '../diff';
import { describe, expect, it } from 'vitest';

const joined = (tokens: DiffToken[]): string =>
  tokens.map((t) => t.text).join('');

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

  it('puts exactly one space around a deleted word', () => {
    expect(joined(computeDiff('foo bar baz', 'foo baz'))).toBe('foo bar baz');
    expect(joined(computeDiff('foo bar', 'foo baz'))).toBe('foo bar baz');
  });

  it('separates a leading deleted word from the next word', () => {
    const result = computeDiff('the cat', 'cat');
    expect(joined(result)).toBe('the cat');
    expect(result[0]).toEqual({ text: 'the', changed: false, deleted: true });
  });

  it('separates consecutive deleted words', () => {
    expect(joined(computeDiff('a b c', 'c'))).toBe('a b c');
  });

  it('keeps line breaks and leading whitespace of the corrected text', () => {
    expect(joined(computeDiff(' a\nb', ' a\nb'))).toBe(' a\nb');
    // The deleted word comes first, struck through, then one space and the fix.
    expect(joined(computeDiff(' helo\nworld', ' hello\nworld'))).toBe(
      'helo hello\nworld',
    );
  });
});

describe('countChanges', () => {
  it('counts nothing for identical texts', () => {
    expect(countChanges(computeDiff('same text', 'same text'))).toBe(0);
  });

  it('counts a replaced word once', () => {
    expect(countChanges(computeDiff('she go home', 'she goes home'))).toBe(1);
  });

  it('counts adjacent edits as one and separate edits apart', () => {
    expect(countChanges(computeDiff('a b c d', 'a x y d'))).toBe(1);
    expect(countChanges(computeDiff('a b c d e', 'x b c d y'))).toBe(2);
  });
});

describe('escapeHtml', () => {
  it('escapes markup characters', () => {
    expect(escapeHtml('<b a="1">&</b>')).toBe(
      '&lt;b a=&quot;1&quot;&gt;&amp;&lt;/b&gt;',
    );
  });
});

describe('renderDiffHtml', () => {
  it('leaves unchanged text plain', () => {
    expect(renderDiffHtml(computeDiff('a b', 'a b'))).toBe('a b');
  });

  it('wraps deleted and inserted words in separate spans', () => {
    const html = renderDiffHtml(computeDiff('she go home', 'she goes home'));
    expect(html).toMatch(
      /^she <span class="[^"]*line-through[^"]*">go<\/span>/,
    );
    expect(html).toContain('<span class="text-purple-400');
    expect(html).toContain('>goes</span> home');
  });

  it('groups consecutive edits of the same kind into one span', () => {
    const html = renderDiffHtml(computeDiff('a b c d', 'a x y d'));
    expect(html).toContain('>b c</span>');
    expect(html).toContain('>x y</span>');
  });

  it('escapes the text it renders', () => {
    const html = renderDiffHtml(computeDiff('<i>', '<b>'));
    expect(html).not.toContain('<i>');
    expect(html).toContain('&lt;b&gt;');
  });

  it('closes a group at the end of the text', () => {
    expect(renderDiffHtml(computeDiff('a', 'b'))).toMatch(/<\/span>$/);
  });
});
