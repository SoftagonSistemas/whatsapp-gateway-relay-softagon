import { computed, ref } from 'vue';

const STORAGE_KEY = 'softagon_relay_api_key';

function readStoredKey(): string {
  try {
    return (
      window.localStorage.getItem(STORAGE_KEY) ||
      window.sessionStorage.getItem(STORAGE_KEY) ||
      ''
    );
  } catch {
    return '';
  }
}

const apiKey = ref<string>(readStoredKey());

export function useAuth() {
  const isAuthenticated = computed(() => apiKey.value.trim().length > 0);

  function setApiKey(key: string, remember: boolean) {
    const clean = key.trim();
    apiKey.value = clean;
    try {
      window.localStorage.removeItem(STORAGE_KEY);
      window.sessionStorage.removeItem(STORAGE_KEY);
      if (clean) {
        if (remember) {
          window.localStorage.setItem(STORAGE_KEY, clean);
        } else {
          window.sessionStorage.setItem(STORAGE_KEY, clean);
        }
      }
    } catch {
      // ignore storage errors
    }
  }

  function clearApiKey() {
    apiKey.value = '';
    try {
      window.localStorage.removeItem(STORAGE_KEY);
      window.sessionStorage.removeItem(STORAGE_KEY);
    } catch {
      // ignore
    }
  }

  return {
    apiKey,
    isAuthenticated,
    setApiKey,
    clearApiKey,
  };
}
