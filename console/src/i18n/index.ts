import { createI18n } from 'vue-i18n';
import ptBR from './pt-BR.json';
import en from './en.json';
import es from './es.json';
import fr from './fr.json';

export type SupportedLocale = 'pt-BR' | 'en' | 'es' | 'fr';

export const LOCALE_OPTIONS: Array<{
  code: SupportedLocale;
  short: string;
  name: string;
}> = [
  { code: 'pt-BR', short: 'PT', name: 'Português' },
  { code: 'en', short: 'EN', name: 'English' },
  { code: 'es', short: 'ES', name: 'Español' },
  { code: 'fr', short: 'FR', name: 'Français' },
];

const STORAGE_KEY = 'relay.lang';

function normalizeLocale(raw: string | null | undefined): SupportedLocale | null {
  if (!raw) return null;
  const low = raw.toLowerCase();
  if (low.startsWith('pt')) return 'pt-BR';
  if (low.startsWith('en')) return 'en';
  if (low.startsWith('es')) return 'es';
  if (low.startsWith('fr')) return 'fr';
  return null;
}

export function resolveInitialLocale(): SupportedLocale {
  try {
    const saved = normalizeLocale(window.localStorage.getItem(STORAGE_KEY));
    if (saved) return saved;
  } catch {
    // ignore storage errors
  }
  const nav = normalizeLocale(typeof navigator !== 'undefined' ? navigator.language : null);
  return nav ?? 'pt-BR';
}

const initialLocale = resolveInitialLocale();

export const i18n = createI18n({
  legacy: false,
  locale: initialLocale,
  fallbackLocale: 'pt-BR',
  messages: {
    'pt-BR': ptBR,
    en,
    es,
    fr,
  },
});

export function setAppLocale(locale: SupportedLocale) {
  i18n.global.locale.value = locale;
  try {
    window.localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // ignore
  }
  if (typeof document !== 'undefined') {
    document.documentElement.lang = locale;
  }
}

if (typeof document !== 'undefined') {
  document.documentElement.lang = initialLocale;
}
