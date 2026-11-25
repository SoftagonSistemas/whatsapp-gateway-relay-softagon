import { onMounted, onUnmounted } from 'vue';

export function isMacOS(): boolean {
  if (typeof navigator === 'undefined') return false;
  const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  const platform = uaData?.platform || navigator.platform || navigator.userAgent || '';
  return /mac|iphone|ipad|ipod/i.test(platform);
}

export const MOD_KEY = isMacOS() ? '⌘' : 'Ctrl';

export function isInputFocused(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || tag === 'select' || target.isContentEditable;
}

export interface GlobalHotkeyHandlers {
  onCommandK?: () => void;
  onSlash?: () => void;
  onNewNumber?: () => void;
  onGoSessions?: () => void;
  onGoEvents?: () => void;
  onGoApi?: () => void;
  onHelp?: () => void;
}

export function useGlobalHotkeys(handlers: GlobalHotkeyHandlers) {
  let pendingG = false;
  let gTimer: number | null = null;

  function clearG() {
    pendingG = false;
    if (gTimer !== null) {
      window.clearTimeout(gTimer);
      gTimer = null;
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    const mod = isMacOS() ? e.metaKey : e.ctrlKey;

    if (mod && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      handlers.onCommandK?.();
      return;
    }

    if (isInputFocused(e.target) || e.metaKey || e.ctrlKey || e.altKey) {
      return;
    }

    const key = e.key.toLowerCase();

    if (pendingG) {
      clearG();
      if (key === 's') {
        e.preventDefault();
        handlers.onGoSessions?.();
        return;
      }
      if (key === 'e') {
        e.preventDefault();
        handlers.onGoEvents?.();
        return;
      }
      if (key === 'a') {
        e.preventDefault();
        handlers.onGoApi?.();
        return;
      }
    }

    if (key === 'g') {
      pendingG = true;
      gTimer = window.setTimeout(clearG, 900);
      return;
    }

    if (e.key === '/') {
      e.preventDefault();
      handlers.onSlash?.();
      return;
    }

    if (key === 'n') {
      e.preventDefault();
      handlers.onNewNumber?.();
      return;
    }

    if (e.key === '?') {
      e.preventDefault();
      handlers.onHelp?.();
    }
  }

  onMounted(() => window.addEventListener('keydown', onKeyDown));
  onUnmounted(() => {
    clearG();
    window.removeEventListener('keydown', onKeyDown);
  });
}
