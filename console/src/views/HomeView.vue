<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import {
  Activity,
  BookOpen,
  Contrast,
  Globe,
  KeyRound,
  Mic,
  Plus,
  QrCode,
  Search,
  Send,
  Webhook,
} from 'lucide-vue-next';
import TopBar from '../components/TopBar.vue';
import iconImg from '../assets/softagon-icon.png';
import { useSessions } from '../composables/useSessions';
import { useTheme } from '../composables/useTheme';
import { useAuth } from '../composables/useAuth';
import { APP_VERSION, formatSessionStatus, LOCALE_OPTIONS, setAppLocale } from '../i18n';
import { MOD_KEY } from '../composables/useHotkeys';
import { normalizeText, parsePhoneInput } from '../composables/usePhone';

interface PaletteItem {
  id: string;
  section: 'connect' | 'sug' | 'sess' | 'sys';
  title: string;
  sub: string;
  meta: string;
  kbd?: string;
  icon: string;
  tint: 'cyan' | 'amber' | 'orange' | 'neutral' | 'primary' | 'av1' | 'av2' | 'av3';
  initials?: string;
  status?: string;
  keywords: string;
  run: () => void;
}

const router = useRouter();
const route = useRoute();
const { t, locale } = useI18n();
const { sessions, fetchSessions, fetchHealth } = useSessions();
const { toggleTheme } = useTheme();
const { apiKey } = useAuth();

const query = ref('');
const activeIndex = ref(0);
const inputRef = ref<HTMLInputElement | null>(null);
const toast = ref<string | null>(null);

const phoneInfo = computed(() => parsePhoneInput(query.value));

function showToast(msg: string) {
  toast.value = msg;
  window.setTimeout(() => {
    if (toast.value === msg) toast.value = null;
  }, 2200);
}

function cycleLocale() {
  const idx = LOCALE_OPTIONS.findIndex((l) => l.code === locale.value);
  const next = LOCALE_OPTIONS[(idx + 1) % LOCALE_OPTIONS.length];
  setAppLocale(next.code);
  showToast(next.name);
}

async function copyGlobalKey() {
  try {
    await navigator.clipboard.writeText(apiKey.value);
    showToast(t('connect.copied'));
  } catch {
    // ignore
  }
}

const allItems = computed<PaletteItem[]>(() => {
  const items: PaletteItem[] = [];

  if (phoneInfo.value.isPhone) {
    const digits = phoneInfo.value.digits;
    items.push({
      id: `connect-${digits}`,
      section: 'connect',
      title: `${t('home.connect')} ${phoneInfo.value.formatted}`,
      sub: t('home.connectSub'),
      meta: t('home.genQr'),
      kbd: '↵',
      icon: 'qr',
      tint: 'primary',
      keywords: digits,
      run: () => router.push({ name: 'connect', query: { phone: digits } }),
    });
  }

  const firstSession = sessions.value[0];

  items.push(
    {
      id: 'sug-new',
      section: 'sug',
      title: t('home.newNum'),
      sub: t('home.newNumSub'),
      meta: t('home.mSession'),
      kbd: 'n',
      icon: 'plus',
      tint: 'orange',
      keywords: 'conectar novo numero connect new number conectar nuevo connecter nouveau qr whatsapp',
      run: () => router.push({ name: 'connect' }),
    },
    {
      id: 'sug-send',
      section: 'sug',
      title: t('home.send'),
      sub: t('home.sendSub'),
      meta: t('home.mMsg'),
      icon: 'send',
      tint: 'cyan',
      keywords: 'enviar mensagem teste send test message texto media',
      run: () => {
        if (firstSession) {
          router.push({
            name: 'session',
            params: { id: firstSession.id },
            query: { action: 'send' },
          });
        } else {
          router.push({ name: 'connect' });
        }
      },
    },
    {
      id: 'sug-hooks',
      section: 'sug',
      title: t('home.hooks'),
      sub: t('home.hooksSub'),
      meta: t('home.mInt'),
      kbd: 'g e',
      icon: 'hook',
      tint: 'amber',
      keywords: 'webhooks eventos events deliveries hmac integracoes',
      run: () => router.push({ name: 'events' }),
    },
    {
      id: 'sug-tr',
      section: 'sug',
      title: t('home.tr'),
      sub: t('home.trSub'),
      meta: t('home.mAi'),
      icon: 'mic',
      tint: 'cyan',
      keywords: 'transcricao audio whisper voice speech transcription',
      run: () => router.push({ name: 'api', query: { tag: 'Transcriptions' } }),
    },
  );

  sessions.value.forEach((s, idx) => {
    const tint = (['av1', 'av2', 'av3'] as const)[idx % 3];
    const initials = s.name
      .replace(/[^a-zA-Z0-9]/g, '')
      .slice(0, 2)
      .toUpperCase() || 'WA';
    items.push({
      id: `sess-${s.id}`,
      section: 'sess',
      title: s.name,
      sub: s.jid || s.id,
      meta: s.phone_number || t('home.mSession'),
      status: formatSessionStatus(s.status, t),
      icon: 'avatar',
      tint,
      initials,
      keywords: `${s.name} ${s.id} ${s.jid || ''} ${s.phone_number || ''} ${s.status} sessao session`,
      run: () => router.push({ name: 'session', params: { id: s.id } }),
    });
  });

  items.push(
    {
      id: 'sys-health',
      section: 'sys',
      title: t('home.hl'),
      sub: t('home.hlSub'),
      meta: t('home.mStatus'),
      icon: 'pulse',
      tint: 'neutral',
      keywords: 'saude health status sqlite whisper servidor server',
      run: () => router.push({ name: 'api', query: { tag: 'Health' } }),
    },
    {
      id: 'sys-api',
      section: 'sys',
      title: t('home.dc'),
      sub: t('home.dcSub'),
      meta: t('home.mOpen'),
      kbd: 'g a',
      icon: 'book',
      tint: 'neutral',
      keywords: 'documentacao api openapi referencia reference docs scalar',
      run: () => router.push({ name: 'api' }),
    },
    {
      id: 'sys-key',
      section: 'sys',
      title: t('home.key'),
      sub: t('home.keySub'),
      meta: t('home.mSec'),
      icon: 'key',
      tint: 'neutral',
      keywords: 'copiar chave api key token seguranca security',
      run: copyGlobalKey,
    },
    {
      id: 'sys-lang',
      section: 'sys',
      title: t('home.lg'),
      sub: t('home.lgSub'),
      meta: t('home.mUi'),
      icon: 'globe',
      tint: 'neutral',
      keywords:
        'idioma language basa hata bahasa portugues english espanol francais indonesia betawi sunda sundanes jawa javanes batak toba melayu malaio jakarta bekasi surabaya bandung medan locale',
      run: cycleLocale,
    },
    {
      id: 'sys-theme',
      section: 'sys',
      title: t('home.th'),
      sub: t('home.thSub'),
      meta: t('home.mUi'),
      icon: 'theme',
      tint: 'neutral',
      keywords: 'tema theme claro escuro light dark mode',
      run: toggleTheme,
    },
  );

  return items;
});

const filteredItems = computed(() => {
  const q = normalizeText(query.value.trim());
  if (!q) return allItems.value;
  return allItems.value.filter((it) => {
    if (it.section === 'connect') return true;
    const hay = normalizeText(`${it.title} ${it.sub} ${it.meta} ${it.keywords}`);
    return hay.includes(q);
  });
});

const groupedSections = computed(() => {
  const map: Array<{ key: PaletteItem['section']; title: string; items: PaletteItem[] }> = [
    { key: 'connect', title: t('home.sConnect'), items: [] },
    { key: 'sug', title: t('home.sSug'), items: [] },
    { key: 'sess', title: t('home.sSess'), items: [] },
    { key: 'sys', title: t('home.sSys'), items: [] },
  ];
  for (const item of filteredItems.value) {
    const sec = map.find((m) => m.key === item.section);
    if (sec) sec.items.push(item);
  }
  return map.filter((m) => m.items.length > 0);
});

const activeItem = computed(() => filteredItems.value[activeIndex.value] ?? null);

watch(filteredItems, () => {
  activeIndex.value = 0;
});

watch(
  () => route.query.focus,
  (val) => {
    if (val) inputRef.value?.focus();
  },
);

function onInputKeyDown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (filteredItems.value.length > 0) {
      activeIndex.value = (activeIndex.value + 1) % filteredItems.value.length;
    }
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (filteredItems.value.length > 0) {
      activeIndex.value =
        (activeIndex.value - 1 + filteredItems.value.length) % filteredItems.value.length;
    }
  } else if (e.key === 'Enter') {
    e.preventDefault();
    activeItem.value?.run();
  } else if (e.key === 'Escape') {
    if (query.value) {
      e.preventDefault();
      query.value = '';
    }
  }
}

function flatIndexOf(id: string): number {
  return filteredItems.value.findIndex((x) => x.id === id);
}

onMounted(() => {
  fetchHealth();
  fetchSessions();
  inputRef.value?.focus();
});
</script>

<template>
  <TopBar :show-search="false" />

  <main class="hero-pad home-main">
    <p class="eyebrow">{{ t('home.eyebrow', { version: APP_VERSION }) }}</p>
    <h1 class="hero-h1 home-h1">{{ t('home.h1') }}</h1>
    <p class="home-sub">{{ t('home.sub') }}</p>

    <section :aria-label="t('home.paletteLabel')" class="palette-card">
      <div class="palette-input-row">
        <Search :size="20" aria-hidden="true" class="palette-search-ic" />
        <label for="cmd" class="sr">{{ t('home.inputLabel') }}</label>
        <input
          id="cmd"
          ref="inputRef"
          v-model="query"
          class="inp palette-input"
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls="cmd-listbox"
          :aria-activedescendant="activeItem ? `opt-${activeItem.id}` : undefined"
          autocomplete="off"
          :placeholder="t('home.placeholder')"
          @keydown="onInputKeyDown"
        />
        <button
          v-if="query"
          type="button"
          class="kbd"
          :aria-label="t('home.clear')"
          style="cursor: pointer"
          @click="query = ''; inputRef?.focus()"
        >
          esc
        </button>
        <span class="kbd hide-sm">{{ MOD_KEY }}K</span>
      </div>

      <div
        id="cmd-listbox"
        role="listbox"
        :aria-label="t('home.results')"
        class="palette-list"
      >
        <template v-for="sec in groupedSections" :key="sec.key">
          <div class="sec-title">{{ sec.title }}</div>
          <button
            v-for="it in sec.items"
            :id="`opt-${it.id}`"
            :key="it.id"
            type="button"
            role="option"
            :aria-selected="activeItem?.id === it.id"
            class="palette-row hov"
            :class="{
              selected: activeItem?.id === it.id,
              'connect-row': it.section === 'connect',
            }"
            @mouseenter="activeIndex = flatIndexOf(it.id)"
            @click="it.run()"
          >
            <span class="row-tile" :class="`tint-${it.tint}`">
              <QrCode v-if="it.icon === 'qr'" :size="20" aria-hidden="true" />
              <Plus v-else-if="it.icon === 'plus'" :size="18" aria-hidden="true" />
              <Send v-else-if="it.icon === 'send'" :size="18" aria-hidden="true" />
              <Webhook v-else-if="it.icon === 'hook'" :size="18" aria-hidden="true" />
              <Mic v-else-if="it.icon === 'mic'" :size="18" aria-hidden="true" />
              <Activity v-else-if="it.icon === 'pulse'" :size="18" aria-hidden="true" />
              <BookOpen v-else-if="it.icon === 'book'" :size="18" aria-hidden="true" />
              <KeyRound v-else-if="it.icon === 'key'" :size="18" aria-hidden="true" />
              <Globe v-else-if="it.icon === 'globe'" :size="18" aria-hidden="true" />
              <Contrast v-else-if="it.icon === 'theme'" :size="18" aria-hidden="true" />
              <span v-else class="av-text">{{ it.initials }}</span>
            </span>

            <span class="row-copy">
              <span class="row-title">
                <span>{{ it.title }}</span>
                <span v-if="it.status" class="row-status">
                  <span aria-hidden="true" class="row-dot"></span>
                  {{ it.status }}
                </span>
              </span>
              <span class="row-sub" :class="{ mono: it.section === 'sess' }">{{ it.sub }}</span>
            </span>

            <span class="hide-sm row-meta" :class="{ orange: it.section === 'connect' }">
              {{ it.meta }}
            </span>
            <span v-if="it.kbd" class="kbd hide-sm">{{ it.kbd }}</span>
          </button>
        </template>

        <div v-if="filteredItems.length === 0" role="status" class="empty-box">
          {{ t('home.emptyA') }} “{{ query }}”. {{ t('home.emptyB') }}
        </div>
      </div>

      <footer class="palette-foot">
        <span class="foot-brand">
          <img :src="iconImg" alt="" class="foot-icon" />
          <span class="hide-sm" style="font-weight: 600">Softagon Relay</span>
        </span>
        <span class="foot-actions">
          <span v-if="toast" role="status" style="color: var(--accent); font-weight: 600">
            {{ toast }}
          </span>
          <span class="foot-primary">
            {{ phoneInfo.isPhone ? t('home.generate') : t('home.open') }}
            <span class="kbd">↵</span>
          </span>
          <span aria-hidden="true" class="foot-sep"></span>
          <span class="foot-hint">
            {{ t('home.actions') }} <span class="kbd">{{ MOD_KEY }}K</span>
          </span>
        </span>
      </footer>
    </section>

    <div v-if="sessions.length === 0 && !query" class="try-row">
      <button type="button" class="try-chip hov" @click="query = '11 99999-0001'; inputRef?.focus()">
        {{ t('home.try1') }}
      </button>
      <button type="button" class="try-chip hov" @click="query = 'webhook'; inputRef?.focus()">
        {{ t('home.try2') }}
      </button>
    </div>

    <div class="hint-bar hide-sm">
      <span><span class="kbd">↑</span> <span class="kbd">↓</span> {{ t('home.hNav') }}</span>
      <span><span class="kbd">↵</span> {{ t('home.hRun') }}</span>
      <span><span class="kbd">n</span> {{ t('home.hNew') }}</span>
      <span><span class="kbd">?</span></span>
    </div>
  </main>
</template>

<style scoped>
.home-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 64px 16px 64px;
}

.eyebrow {
  margin: 0 0 12px;
  font-size: 13px;
  font-weight: 600;
  color: var(--amber);
  letter-spacing: 0.04em;
}

.home-h1 {
  margin: 0;
  font-size: 40px;
  line-height: 1.15;
  font-weight: 800;
  letter-spacing: -0.025em;
  text-align: center;
}

.home-sub {
  margin: 12px 0 32px;
  font-size: 16px;
  color: var(--muted);
  text-align: center;
  max-width: 540px;
  line-height: 1.55;
}

.palette-card {
  width: 100%;
  max-width: 760px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow);
  overflow: hidden;
}

.palette-input-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 18px;
  height: 64px;
  border-bottom: 1px solid var(--border);
}

.palette-search-ic {
  color: var(--muted);
  flex: none;
}

.palette-input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: 0;
  color: var(--text);
  font: 500 17px 'Montserrat', sans-serif;
  height: 100%;
}

.palette-list {
  padding: 8px;
  max-height: 540px;
  overflow-y: auto;
}

.sec-title {
  padding: 12px 12px 6px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
}

.palette-row {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text);
  text-align: left;
  cursor: pointer;
}

.palette-row.selected {
  background: var(--sel);
  border-color: var(--sel-border);
}

.palette-row.connect-row {
  background: var(--tint-orange);
  border-color: var(--primary);
}

.row-tile {
  width: 38px;
  height: 38px;
  border-radius: 10px;
  display: grid;
  place-items: center;
  flex: none;
}

.tint-primary {
  background: var(--primary);
  color: var(--on-primary);
}

.tint-orange {
  background: var(--tint-orange);
  color: var(--orange);
}

.tint-cyan {
  background: var(--tint-cyan);
  color: var(--accent);
}

.tint-amber {
  background: var(--tint-amber);
  color: var(--amber);
}

.tint-neutral {
  background: var(--tint-neutral);
  color: var(--text-2);
}

.tint-av1 {
  background: var(--av1-bg);
  color: var(--av1-fg);
}

.tint-av2 {
  background: var(--av2-bg);
  color: var(--av2-fg);
}

.tint-av3 {
  background: var(--av3-bg);
  color: var(--av3-fg);
}

.av-text {
  font: 700 13px 'Montserrat', sans-serif;
}

.row-copy {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.row-title {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
}

.row-status {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 600;
  color: var(--accent);
}

.row-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--dot);
}

.row-sub {
  font-size: 12.5px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-meta {
  font-size: 12px;
  color: var(--muted);
  white-space: nowrap;
}

.row-meta.orange {
  color: var(--orange);
  font-weight: 600;
}

.empty-box {
  padding: 32px 16px;
  text-align: center;
  color: var(--muted);
  font-size: 14px;
}

.palette-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 0 14px;
  height: 46px;
  background: var(--sunken);
  border-top: 1px solid var(--border);
  font-size: 12px;
  color: var(--muted);
}

.foot-brand,
.foot-actions,
.foot-primary,
.foot-hint {
  display: flex;
  align-items: center;
  gap: 8px;
}

.foot-icon {
  width: 18px;
  height: 18px;
  display: block;
}

.foot-primary {
  color: var(--text);
  font-weight: 600;
}

.foot-sep {
  width: 1px;
  height: 16px;
  background: var(--border-strong);
}

.try-row {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: center;
  margin-top: 18px;
}

.try-chip {
  padding: 8px 14px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-2);
  font: 500 13px 'Montserrat', sans-serif;
  cursor: pointer;
}

.hint-bar {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-top: 20px;
  font-size: 12px;
  color: var(--muted);
}
</style>
