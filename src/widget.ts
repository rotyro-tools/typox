import {
  type DiffToken,
  computeDiff,
  countChanges,
  renderDiffHtml,
} from './diff';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface CorrectionPayload {
  original: string;
  corrected: string;
  noChanges: boolean;
  /** Set when the correction still looks off (another language, a rewrite…). */
  warning?: string | null;
}

interface ErrorPayload {
  message: string;
  /** False when retrying can't help (nothing was selected, no permission…). */
  retryable?: boolean;
}

const byId = <T extends HTMLElement = HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

const beforeText = byId('before-text');
const afterText = byId('after-text');
const btnApply = byId('btn-apply');
const btnEdit = byId('btn-edit');
const btnRetry = byId('btn-retry');
const skeletonView = byId('skeleton-view');
const diffView = byId('diff-view');
const noChangesView = byId('no-changes-view');
const btnSkeleton = byId('btn-skeleton');
const btnReal = byId('btn-real');
const btnErrorBar = byId('btn-error');
const btnErrorRetry = byId('btn-error-retry');
const changesCount = byId('changes-count');
const errorView = byId('error-view');
const errorMessage = byId('error-message');

let isEditing = false;
let isBusy = false; // an apply, dismiss or retry is on its way
let lastTokens: DiffToken[] | null = null;
let lastOriginal = '';
let lastCorrected = '';

const DEFAULT_HEIGHT = 160;
// Fixed chrome: title bar + label row + paddings + footer
const CHROME_HEIGHT = 28 + 20 + 32 + 44;

const show = (el: HTMLElement, visible: boolean): void => {
  el.classList.toggle('hidden', !visible);
};

function getColumnWidth(): number {
  // window width - px-4 (32px) - 1px divider, split across 2 cols, minus col padding (12px each)
  return Math.floor((document.documentElement.clientWidth - 32 - 1) / 2) - 12;
}

function measureColumnHeight(html: string): number {
  const probe = document.createElement('div');
  probe.style.cssText = `position:fixed;visibility:hidden;top:0;left:0;width:${getColumnWidth()}px;font-size:0.875rem;line-height:1.625;word-break:break-word;white-space:pre-wrap;`;
  probe.innerHTML = html;
  document.body.appendChild(probe);
  const height = probe.offsetHeight;
  probe.remove();
  return height;
}

/** Grows the window to fit both columns; Rust caps it and keeps it centered. */
async function resizeToContent(): Promise<void> {
  const contentHeight = Math.max(
    measureColumnHeight(beforeText.innerHTML),
    measureColumnHeight(afterText.innerHTML),
  );
  const height = Math.max(CHROME_HEIGHT + contentHeight, DEFAULT_HEIGHT);
  try {
    await invoke('fit_widget', { height });
  } catch {
    // Keeping the default size is fine.
  }
}

const COPY_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';
const CHECK_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
const copyTimers = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();

function copyText(btn: HTMLElement, text: string): void {
  void navigator.clipboard.writeText(text).then(() => {
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

byId('btn-copy-before').addEventListener('click', (e) => {
  copyText(e.currentTarget as HTMLElement, lastOriginal);
});
byId('btn-copy-after').addEventListener('click', (e) => {
  copyText(e.currentTarget as HTMLElement, lastCorrected);
});

type ViewName = 'skeleton' | 'diff' | 'noChanges' | 'error';

function switchView(view: ViewName): void {
  const toggle = (el: HTMLElement, visible: boolean, display: string) => {
    show(el, visible);
    el.classList.toggle(display, visible);
  };
  toggle(skeletonView, view === 'skeleton', 'grid');
  toggle(diffView, view === 'diff', 'grid');
  toggle(noChangesView, view === 'noChanges', 'flex');
  toggle(errorView, view === 'error', 'flex');
  toggle(btnSkeleton, view === 'skeleton', 'flex');
  toggle(btnReal, view === 'diff' || view === 'noChanges', 'flex');
  toggle(btnErrorBar, view === 'error', 'flex');
}

function showErrorView(message: string, retryable = true): void {
  errorMessage.textContent = message;
  show(btnErrorRetry, retryable);
  switchView('error');
}

/** Shows the edit count, or the warning in its place. */
function showStatus(count: number, warning?: string | null): void {
  changesCount.classList.toggle('text-amber-300/80', Boolean(warning));
  changesCount.classList.toggle('text-white/30', !warning);
  if (warning) {
    changesCount.textContent = `⚠ ${warning}`;
    changesCount.title = warning;
  } else {
    changesCount.textContent = `${count} change${count === 1 ? '' : 's'}`;
    changesCount.title = '';
  }
  show(changesCount, Boolean(warning) || count > 0);
}

function clearContent(): void {
  isEditing = false;
  afterText.contentEditable = 'false';
  beforeText.textContent = '';
  afterText.textContent = '';
  lastTokens = null;
  [btnApply, btnEdit, btnRetry, changesCount].forEach((el) => show(el, false));
  switchView('skeleton');
}

const EDIT_CLASSES = ['outline-none', 'ring-1', 'ring-white/20', 'rounded'];

function setEditMode(active: boolean, keepEdits = false): void {
  isEditing = active;
  if (active) {
    afterText.textContent = lastCorrected;
    afterText.contentEditable = 'plaintext-only';
    afterText.classList.add(...EDIT_CLASSES);
    afterText.focus();
    show(btnRetry, false);
    show(btnEdit, false);
    btnApply.innerHTML =
      'Done <kbd class="text-[10px] opacity-60 font-sans ml-1">↵</kbd>';
    show(btnApply, true);
    return;
  }
  afterText.contentEditable = 'false';
  afterText.classList.remove(...EDIT_CLASSES);
  show(btnRetry, true);
  show(btnEdit, true);
  btnApply.innerHTML =
    'Apply <kbd class="text-[10px] opacity-60 font-sans ml-1">↵</kbd>';
  if (keepEdits) {
    // innerText keeps the line breaks typed in the editor.
    lastCorrected = afterText.innerText;
    lastTokens = computeDiff(lastOriginal, lastCorrected);
    showStatus(countChanges(lastTokens));
  }
  if (lastTokens !== null) {
    afterText.innerHTML = renderDiffHtml(lastTokens);
  }
}

/** Runs one widget action at a time, showing its error if it fails. */
async function runAction(
  action: () => Promise<unknown>,
  clear = true,
): Promise<void> {
  if (isBusy) return;
  isBusy = true;
  if (clear) clearContent();
  try {
    await action();
  } catch (e) {
    showErrorView(String(e));
  } finally {
    isBusy = false;
  }
}

function applyCorrection(): Promise<void> {
  const overrideText = isEditing ? afterText.innerText : lastCorrected;
  return runAction(() => invoke('apply_correction', { overrideText }));
}

/** Not gated by isBusy: Esc must work even while a retry is waiting on the AI. */
async function dismissCorrection(): Promise<void> {
  clearContent();
  try {
    await invoke('dismiss_correction');
  } catch (e) {
    showErrorView(String(e));
  }
}

function retryCorrection(): Promise<void> {
  // The outcome arrives as a correction-ready or correction-error event.
  return runAction(() => invoke('retry_correction'));
}

btnApply.addEventListener('click', () => void applyCorrection());
byId('btn-dismiss').addEventListener('click', () => void dismissCorrection());
btnEdit.addEventListener('click', () => setEditMode(!isEditing));
btnRetry.addEventListener('click', () => void retryCorrection());
btnErrorRetry.addEventListener('click', () => void retryCorrection());
byId('btn-error-dismiss').addEventListener(
  'click',
  () => void dismissCorrection(),
);

const visible = (el: HTMLElement): boolean => !el.classList.contains('hidden');

document.addEventListener('keydown', (e: KeyboardEvent) => {
  if (e.repeat || e.isComposing) return;
  if (isEditing) {
    if (e.key === 'Escape') {
      e.preventDefault();
      setEditMode(false);
    } else if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      setEditMode(false, true);
    }
    return;
  }
  // Leave Cmd/Ctrl/Alt combinations (copy, select all…) alone.
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const key = e.key.toLowerCase();
  if (e.key === 'Escape') {
    void dismissCorrection();
  } else if (e.key === 'Enter' && visible(btnApply)) {
    e.preventDefault();
    void applyCorrection();
  } else if (key === 'e' && visible(btnEdit)) {
    e.preventDefault();
    setEditMode(true);
  } else if (
    key === 'r' &&
    (visible(btnRetry) || (visible(btnErrorBar) && visible(btnErrorRetry)))
  ) {
    void retryCorrection();
  }
});

async function initListeners(): Promise<void> {
  await listen<ErrorPayload>('correction-error', ({ payload }) => {
    showErrorView(payload.message, payload.retryable !== false);
  });

  await listen('correction-started', () => {
    clearContent();
  });

  await listen<CorrectionPayload>('correction-ready', ({ payload }) => {
    const { original, corrected, noChanges, warning } = payload;
    lastOriginal = original;
    lastCorrected = corrected;
    isEditing = false;

    if (noChanges) {
      lastTokens = null;
      switchView('noChanges');
      show(btnApply, false);
      show(btnEdit, false);
      show(btnRetry, true);
      show(changesCount, false);
      return;
    }

    lastTokens = computeDiff(original, corrected);
    switchView('diff');
    beforeText.textContent = original;
    afterText.innerHTML = renderDiffHtml(lastTokens);
    show(btnApply, true);
    show(btnEdit, true);
    show(btnRetry, true);
    showStatus(countChanges(lastTokens), warning);
    void resizeToContent();
  });
}

void initListeners();
