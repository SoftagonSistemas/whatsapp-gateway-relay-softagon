<script setup lang="ts">
import { ref } from 'vue';
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
    <ShortcutsModal v-if="showShortcuts" @close="showShortcuts = false" />
  </div>
</template>
