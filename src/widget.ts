import { type DiffToken, computeDiff } from './diff';
import { invoke } from '@tauri-apps/api/core';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

function countChanges(tokens: DiffToken[]): number {
  let count = 0;
  let inChange = false;
  for (const token of tokens) {
    if (token.changed || token.deleted === true) {
      if (!inChange) {
        count++;
        inChange = true;
      }
    } else if (token.text.trim() !== '') {
      inChange = false;
    }
    // whitespace between changes doesn't break the group
  }
  return count;
}

interface CorrectionPayload {
  original: string;
  corrected: string;
  noChanges: boolean;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function renderDiff(container: HTMLElement, tokens: DiffToken[]): void {
  // Group consecutive deleted tokens into a single span
  const parts: string[] = [];
  let i = 0;
  while (i < tokens.length) {
    const token = tokens[i];
    if (token.deleted === true) {
      let group = token.text;
      // Merge consecutive deleted tokens, skipping whitespace between them
      while (i + 1 < tokens.length) {
        let peek = i + 1;
        // Skip whitespace tokens
        while (
          peek < tokens.length &&
          tokens[peek].text.trim() === '' &&
          tokens[peek].deleted !== true
        )
          peek++;
        if (peek < tokens.length && tokens[peek].deleted === true) {
          // Include the whitespace and the next deleted token
          group += tokens
            .slice(i + 1, peek + 1)
            .map((t) => t.text)
            .join('');
          i = peek;
        } else {
          break;
        }
      }
      parts.push(
        `<span class="text-red-400 bg-red-400/10 rounded px-0.5 mx-0.5 line-through opacity-60">${escapeHtml(group)}</span>`,
      );
    } else if (token.changed) {
      let group = token.text;
      while (i + 1 < tokens.length) {
        let peek = i + 1;
        while (
          peek < tokens.length &&
          tokens[peek].text.trim() === '' &&
          !tokens[peek].changed
        )
          peek++;
        if (peek < tokens.length && tokens[peek].changed) {
          group += tokens
            .slice(i + 1, peek + 1)
            .map((t) => t.text)
            .join('');
          i = peek;
        } else {
          break;
        }
      }
      parts.push(
        `<span class="text-purple-400 bg-purple-400/10 rounded px-0.5">${escapeHtml(group)}</span>`,
      );
    } else {
      parts.push(escapeHtml(token.text));
    }
    i++;
  }
  container.innerHTML = parts.join('');
}

const beforeText = document.getElementById('before-text')!;
const afterText = document.getElementById('after-text')! as HTMLElement;
const btnApply = document.getElementById('btn-apply')!;
const btnEdit = document.getElementById('btn-edit')!;
const btnRetry = document.getElementById('btn-retry')!;
const skeletonView = document.getElementById('skeleton-view')!;
const diffView = document.getElementById('diff-view')!;
const noChangesView = document.getElementById('no-changes-view')!;
const btnSkeleton = document.getElementById('btn-skeleton')!;
const btnReal = document.getElementById('btn-real')!;
const btnErrorBar = document.getElementById('btn-error')!;
const changesCount = document.getElementById('changes-count')!;
const errorView = document.getElementById('error-view')!;
const errorMessage = document.getElementById('error-message')!;

let isEditing = false;
let lastTokens: DiffToken[] | null = null;
let lastCorrected = '';

const DEFAULT_HEIGHT = 160;
// Fixed chrome: title bar + label row + paddings + footer
const CHROME_HEIGHT = 28 + 20 + 32 + 44;

// 640px window - px-4 (32px) - 1px divider = 607px / 2 cols - pr-3/pl-3 (12px) = 291px
const COLUMN_WIDTH = 291;

function measureColumnHeight(escapedHtml: string): number {
  const probe = document.createElement('div');
  probe.style.cssText = `position:fixed;visibility:hidden;top:0;left:0;width:${COLUMN_WIDTH}px;font-size:0.875rem;line-height:1.625;word-break:break-word;white-space:normal;`;
  probe.innerHTML = escapedHtml;
  document.body.appendChild(probe);
  const h = probe.offsetHeight;
  probe.remove();
  return h;
}

async function resizeToContent(): Promise<void> {
  const maxHeight = Math.floor(window.screen.height * 0.8);
  const beforeH = measureColumnHeight(beforeText.innerHTML);
  const afterH = measureColumnHeight(afterText.innerHTML);
  const contentHeight = Math.max(beforeH, afterH);
  const desired = CHROME_HEIGHT + contentHeight;
  const clamped = Math.min(Math.max(desired, DEFAULT_HEIGHT), maxHeight);
  await getCurrentWindow().setSize(new LogicalSize(640, clamped));
}

const COPY_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';
const CHECK_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
const copyTimers = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();

function copyText(btn: HTMLElement, getText: () => string): void {
  void navigator.clipboard.writeText(getText()).then(() => {
    btn.innerHTML = CHECK_SVG;
    const existing = copyTimers.get(btn);
    if (existing !== undefined) clearTimeout(existing);
    copyTimers.set(
      btn,
      setTimeout(() => {
        btn.innerHTML = COPY_SVG;
      }, 1000),
    );
  });
}

document.getElementById('btn-copy-before')!.addEventListener('click', (e) => {
  copyText(e.currentTarget as HTMLElement, () => beforeText.textContent ?? '');
});
document.getElementById('btn-copy-after')!.addEventListener('click', (e) => {
  copyText(e.currentTarget as HTMLElement, () => lastCorrected);
});

type ViewName = 'skeleton' | 'diff' | 'noChanges' | 'error';

function switchView(view: ViewName): void {
  skeletonView.classList.toggle('hidden', view !== 'skeleton');
  skeletonView.classList.toggle('grid', view === 'skeleton');
  diffView.classList.toggle('hidden', view !== 'diff');
  diffView.classList.toggle('grid', view === 'diff');
  noChangesView.classList.toggle('hidden', view !== 'noChanges');
  noChangesView.classList.toggle('flex', view === 'noChanges');
  errorView.classList.toggle('hidden', view !== 'error');
  errorView.classList.toggle('flex', view === 'error');
  btnSkeleton.classList.toggle('hidden', view !== 'skeleton');
  btnSkeleton.classList.toggle('flex', view === 'skeleton');
  btnReal.classList.toggle('hidden', view === 'skeleton' || view === 'error');
  btnReal.classList.toggle('flex', view !== 'skeleton' && view !== 'error');
  btnErrorBar.classList.toggle('hidden', view !== 'error');
  btnErrorBar.classList.toggle('flex', view === 'error');
}

function showErrorView(message: string): void {
  errorMessage.textContent = message;
  switchView('error');
}

function clearContent(): void {
  beforeText.textContent = '';
  afterText.textContent = '';
  btnApply.classList.add('hidden');
  btnEdit.classList.add('hidden');
  btnRetry.classList.add('hidden');
  changesCount.classList.add('hidden');
  errorView.classList.remove('flex');
  errorView.classList.add('hidden');
  btnErrorBar.classList.remove('flex');
  btnErrorBar.classList.add('hidden');
  switchView('skeleton');
}

function setEditMode(active: boolean, keepEdits = false): void {
  isEditing = active;
  if (active) {
    afterText.textContent = lastCorrected;
    afterText.contentEditable = 'true';
    afterText.classList.add(
      'outline-none',
      'ring-1',
      'ring-white/20',
      'rounded',
    );
    afterText.focus();
    // In edit mode: only Dismiss + Done
    btnRetry.classList.add('hidden');
    btnEdit.classList.add('hidden');
    btnApply.innerHTML =
      'Done <kbd class="text-[10px] opacity-60 font-sans ml-1">↵</kbd>';
    btnApply.classList.remove('hidden');
  } else {
    afterText.contentEditable = 'false';
    afterText.classList.remove(
      'outline-none',
      'ring-1',
      'ring-white/20',
      'rounded',
    );
    // Restore normal buttons
    btnRetry.classList.remove('hidden');
    btnEdit.classList.remove('hidden');
    btnApply.innerHTML =
      'Apply <kbd class="text-[10px] opacity-60 font-sans ml-1">↵</kbd>';
    if (keepEdits) {
      const edited = afterText.textContent ?? '';
      lastCorrected = edited;
      const tokens = computeDiff(beforeText.textContent ?? '', edited);
      lastTokens = tokens;
      renderDiff(afterText, tokens);
      const count = countChanges(tokens);
      if (count > 0) {
        changesCount.textContent = `${count} change${count === 1 ? '' : 's'}`;
        changesCount.classList.remove('hidden');
      } else {
        changesCount.classList.add('hidden');
      }
    } else if (lastTokens !== null) {
      renderDiff(afterText, lastTokens);
    }
  }
}

async function applyCorrection(): Promise<void> {
  const overrideText = isEditing
    ? (afterText.textContent ?? lastCorrected)
    : lastCorrected;
  setEditMode(false);
  clearContent();
  await invoke('apply_correction', { overrideText });
}

async function dismissCorrection(): Promise<void> {
  setEditMode(false);
  clearContent();
  await invoke('dismiss_correction');
}

btnApply.addEventListener('click', () => {
  void applyCorrection();
});
document.getElementById('btn-dismiss')!.addEventListener('click', () => {
  void dismissCorrection();
});

btnEdit.addEventListener('click', () => {
  setEditMode(!isEditing);
});

async function retryCorrection(): Promise<void> {
  setEditMode(false);
  clearContent();
  try {
    await invoke('retry_correction');
  } catch (e) {
    showErrorView(String(e));
  }
}

btnRetry.addEventListener('click', () => {
  void retryCorrection();
});

document.addEventListener('keydown', (e: KeyboardEvent) => {
  if (isEditing) {
    if (e.key === 'Escape') {
      e.preventDefault();
      setEditMode(false);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      setEditMode(false, true);
      return;
    }
    return;
  }
  if (e.key === 'Enter' && !btnApply.classList.contains('hidden'))
    void applyCorrection();
  if (e.key === 'Escape') void dismissCorrection();
  if (e.key === 'e' && !btnEdit.classList.contains('hidden')) {
    e.preventDefault();
    setEditMode(true);
  }
  if (
    e.key === 'r' &&
    (!btnRetry.classList.contains('hidden') ||
      !btnErrorBar.classList.contains('hidden'))
  )
    void retryCorrection();
});

document.getElementById('btn-error-retry')!.addEventListener('click', () => {
  void retryCorrection();
});
document.getElementById('btn-error-dismiss')!.addEventListener('click', () => {
  void dismissCorrection();
});

async function initListeners(): Promise<void> {
  await listen<{ message: string }>('correction-error', (event) => {
    showErrorView(event.payload.message);
    void getCurrentWindow().setFocus();
  });

  await listen('correction-started', () => {
    clearContent();
  });

  await listen<CorrectionPayload>('correction-ready', (event) => {
    const { original, corrected, noChanges } = event.payload;

    if (noChanges) {
      switchView('noChanges');
      btnApply.classList.add('hidden');
      btnEdit.classList.add('hidden');
      btnRetry.classList.remove('hidden');
      changesCount.classList.add('hidden');
    } else {
      const tokens = computeDiff(original, corrected);
      const count = countChanges(tokens);
      lastTokens = tokens;
      lastCorrected = corrected;
      switchView('diff');
      beforeText.textContent = original;
      renderDiff(afterText, tokens);
      btnApply.classList.remove('hidden');
      btnEdit.classList.remove('hidden');
      btnRetry.classList.remove('hidden');
      changesCount.textContent = `${count} change${count === 1 ? '' : 's'}`;
      changesCount.classList.remove('hidden');
      void resizeToContent();
    }

    void getCurrentWindow().setFocus();
  });
}

void initListeners();
