export interface DiffToken {
  text: string;
  changed: boolean;
  deleted?: boolean;
}

/** Splits text into alternating word and whitespace tokens. */
export function tokenize(text: string): string[] {
  return text.split(/(\s+)/);
}

const isBlank = (text: string): boolean => text.trim() === '';

/**
 * LCS-based sequence diff.
 * Returns tokens for the corrected text (insertions marked changed)
 * plus deleted tokens from the original interleaved at the right positions.
 */
export function computeDiff(original: string, corrected: string): DiffToken[] {
  const origTokens = tokenize(original);
  const corrTokens = tokenize(corrected);

  // Build LCS table on word tokens only (skip whitespace for matching)
  const origWords = origTokens.filter((t) => !isBlank(t));
  const corrWords = corrTokens.filter((t) => !isBlank(t));

  const m = origWords.length;
  const n = corrWords.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () =>
    new Array(n + 1).fill(0),
  );

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      dp[i][j] =
        origWords[i - 1] === corrWords[j - 1]
          ? dp[i - 1][j - 1] + 1
          : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }

  // Backtrack to find edit operations on word tokens
  type Op = { type: 'keep' | 'insert' | 'delete'; text: string };
  const ops: Op[] = [];
  let i = m,
    j = n;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && origWords[i - 1] === corrWords[j - 1]) {
      ops.push({ type: 'keep', text: corrWords[j - 1] });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      ops.push({ type: 'insert', text: corrWords[j - 1] });
      j--;
    } else {
      ops.push({ type: 'delete', text: origWords[i - 1] });
      i--;
    }
  }
  ops.reverse();

  // Build output tokens, re-inserting the corrected text's whitespace between words.
  // Deleted words get exactly one space on each side, so nothing doubles up.
  const result: DiffToken[] = [];
  let corrIdx = 0; // position in corrTokens (includes whitespace)

  for (const op of ops) {
    const last = result.at(-1);
    if (op.type === 'delete') {
      if (last !== undefined && !isBlank(last.text)) {
        result.push({ text: ' ', changed: false });
      }
      result.push({ text: op.text, changed: false, deleted: true });
      continue;
    }
    let spaced = false;
    while (corrIdx < corrTokens.length && isBlank(corrTokens[corrIdx])) {
      if (corrTokens[corrIdx] !== '') {
        result.push({ text: corrTokens[corrIdx], changed: false });
        spaced = true;
      }
      corrIdx++;
    }
    if (!spaced && last?.deleted === true) {
      result.push({ text: ' ', changed: false });
    }
    // corrIdx always points to a word token here: ops are derived from corrWords
    // (the non-whitespace subset of corrTokens), so a matching token always exists.
    result.push({ text: corrTokens[corrIdx], changed: op.type === 'insert' });
    corrIdx++;
  }

  // Flush remaining whitespace
  while (corrIdx < corrTokens.length) {
    result.push({ text: corrTokens[corrIdx], changed: false });
    corrIdx++;
  }

  return result;
}

/** Number of separate edits; whitespace between changed words doesn't split an edit. */
export function countChanges(tokens: DiffToken[]): number {
  let count = 0;
  let inChange = false;
  for (const token of tokens) {
    if (token.changed || token.deleted === true) {
      if (!inChange) {
        count++;
        inChange = true;
      }
    } else if (!isBlank(token.text)) {
      inChange = false;
    }
  }
  return count;
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

const DELETED_CLASS =
  'text-red-400 bg-red-400/10 rounded px-0.5 mx-0.5 line-through opacity-60';
const INSERTED_CLASS = 'text-purple-400 bg-purple-400/10 rounded px-0.5';

/**
 * HTML for the corrected text with edits highlighted. Consecutive deleted (or inserted)
 * words, and the whitespace between them, share one span. All text is escaped.
 */
export function renderDiffHtml(tokens: DiffToken[]): string {
  const kind = (t: DiffToken): 'deleted' | 'inserted' | 'plain' =>
    t.deleted === true ? 'deleted' : t.changed ? 'inserted' : 'plain';

  const parts: string[] = [];
  let i = 0;
  while (i < tokens.length) {
    const current = kind(tokens[i]);
    if (current === 'plain') {
      parts.push(escapeHtml(tokens[i].text));
      i++;
      continue;
    }
    let group = tokens[i].text;
    let end = i + 1;
    for (;;) {
      let peek = end;
      while (
        peek < tokens.length &&
        kind(tokens[peek]) === 'plain' &&
        isBlank(tokens[peek].text)
      ) {
        peek++;
      }
      if (peek >= tokens.length || kind(tokens[peek]) !== current) break;
      group += tokens
        .slice(end, peek + 1)
        .map((t) => t.text)
        .join('');
      end = peek + 1;
    }
    const cls = current === 'deleted' ? DELETED_CLASS : INSERTED_CLASS;
    parts.push(`<span class="${cls}">${escapeHtml(group)}</span>`);
    i = end;
  }
  return parts.join('');
}
