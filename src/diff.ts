export interface DiffToken {
  text: string;
  changed: boolean;
  deleted?: boolean;
}

/** Splits text into alternating word and whitespace tokens. */
export function tokenize(text: string): string[] {
  return text.split(/(\s+)/);
}

/**
 * LCS-based sequence diff.
 * Returns tokens for the corrected text (insertions marked changed)
 * plus deleted tokens from the original interleaved at the right positions.
 */
export function computeDiff(original: string, corrected: string): DiffToken[] {
  const origTokens = tokenize(original);
  const corrTokens = tokenize(corrected);

  // Build LCS table on word tokens only (skip whitespace for matching)
  const origWords = origTokens.filter((t) => t.trim() !== '');
  const corrWords = corrTokens.filter((t) => t.trim() !== '');

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

  // Build output tokens, re-inserting whitespace between words
  const result: DiffToken[] = [];
  let corrIdx = 0; // position in corrTokens (includes whitespace)

  for (const op of ops) {
    if (op.type === 'delete') {
      if (result.length > 0 && result[result.length - 1].text !== ' ') {
        result.push({ text: ' ', changed: false });
      }
      result.push({ text: op.text, changed: false, deleted: true });
      result.push({ text: ' ', changed: false });
    } else {
      // Advance corrIdx to find this word in corrTokens
      while (corrIdx < corrTokens.length && corrTokens[corrIdx].trim() === '') {
        result.push({ text: corrTokens[corrIdx], changed: false });
        corrIdx++;
      }
      // corrIdx always points to a word token here: ops are derived from corrWords
      // (the non-whitespace subset of corrTokens), so a matching token always exists.
      result.push({
        text: corrTokens[corrIdx],
        changed: op.type === 'insert',
      });
      corrIdx++;
    }
  }

  // Flush remaining whitespace
  while (corrIdx < corrTokens.length) {
    result.push({ text: corrTokens[corrIdx], changed: false });
    corrIdx++;
  }

  return result;
}
