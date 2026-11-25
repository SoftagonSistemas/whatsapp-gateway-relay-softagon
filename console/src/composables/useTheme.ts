import { computed, ref } from 'vue';

export type ThemeMode = 'dark' | 'light';

const STORAGE_KEY = 'relay.theme';

function detectInitialTheme(): ThemeMode {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === 'dark' || saved === 'light') return saved;
  } catch {
    // ignore
  }
  if (typeof window !== 'undefined' && window.matchMedia) {
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  }
  return 'dark';
}

const theme = ref<ThemeMode>(detectInitialTheme());

export function useTheme() {
  const isDark = computed(() => theme.value === 'dark');
  const isLight = computed(() => theme.value === 'light');

  function setTheme(next: ThemeMode) {
    theme.value = next;
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // ignore
    }
  }

  function toggleTheme() {
    setTheme(theme.value === 'dark' ? 'light' : 'dark');
  }

  return {
    theme,
    isDark,
    isLight,
    setTheme,
    toggleTheme,
  };
}
