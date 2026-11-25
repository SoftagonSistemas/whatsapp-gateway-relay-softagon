<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { useTheme } from './composables/useTheme';
import { useAuth } from './composables/useAuth';
import { useSessions } from './composables/useSessions';
import { useGlobalHotkeys } from './composables/useHotkeys';
import { setUnauthorizedHandler } from './composables/useApi';
import ShortcutsModal from './components/ShortcutsModal.vue';

const router = useRouter();
const { locale } = useI18n();
const { theme } = useTheme();
const { isAuthenticated } = useAuth();
const { sessions } = useSessions();

const showShortcuts = ref(false);
const docsUrl = computed(
  () =>
    `https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/${locale.value}/README.md`,
);

setUnauthorizedHandler(() => {
  if (router.currentRoute.value.name !== 'login') {
    router.replace({ name: 'login' });
  }
});

useGlobalHotkeys({
  onCommandK: () => {
    if (!isAuthenticated.value) return;
    router.push({ name: 'home', query: { focus: String(Date.now()) } });
  },
  onSlash: () => {
    if (!isAuthenticated.value) return;
    router.push({ name: 'home', query: { focus: String(Date.now()) } });
  },
  onNewNumber: () => {
    if (!isAuthenticated.value) return;
    router.push({ name: 'connect' });
  },
  onGoSessions: () => {
    if (!isAuthenticated.value) return;
    if (sessions.value.length > 0) {
      router.push({ name: 'session', params: { id: sessions.value[0].id } });
    } else {
      router.push({ name: 'connect' });
    }
  },
  onGoEvents: () => {
    if (!isAuthenticated.value) return;
    router.push({ name: 'events' });
  },
  onGoApi: () => {
    if (!isAuthenticated.value) return;
    router.push({ name: 'api' });
  },
  onHelp: () => {
    if (!isAuthenticated.value) return;
    showShortcuts.value = !showShortcuts.value;
  },
});
</script>

<template>
  <div class="rt" :data-theme="theme" :lang="locale">
    <router-view />
    <footer v-if="isAuthenticated" class="app-brand-footer">
      <span>{{ $t('common.bySoftagon') }}</span>
      <span aria-hidden="true">·</span>
      <a :href="docsUrl" target="_blank" rel="noopener">
        {{ $t('common.userDocs') }}
      </a>
      <span aria-hidden="true">·</span>
      <a
        href="https://softagon.com.br"
        target="_blank"
        rel="noopener"
        title="Softagon Sistemas — Soluções em Engenharia de Software, IA e WhatsApp"
      >
        {{ $t('common.talkToUs') }}
      </a>
    </footer>
    <ShortcutsModal v-if="showShortcuts" @close="showShortcuts = false" />
  </div>
</template>

<style scoped>
.app-brand-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 12px 16px;
  border-top: 1px solid var(--border);
  background: var(--sunken);
  font-size: 12px;
  color: var(--muted);
}

.app-brand-footer a {
  color: var(--accent);
  font-weight: 600;
  text-decoration: none;
}

.app-brand-footer a:hover {
  text-decoration: underline;
}
</style>
