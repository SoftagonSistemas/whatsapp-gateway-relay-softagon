<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { Code2, LogOut, Moon, Search, Sun } from 'lucide-vue-next';
import lockupDark from '../assets/softagon-lockup-dark.png';
import lockupLight from '../assets/softagon-lockup-light.png';
import LanguageSwitcher from './LanguageSwitcher.vue';
import { APP_VERSION } from '../i18n';
import { useTheme } from '../composables/useTheme';
import { useAuth } from '../composables/useAuth';
import { useSessions } from '../composables/useSessions';
import { MOD_KEY } from '../composables/useHotkeys';

const props = withDefaults(
  defineProps<{
    showSearch?: boolean;
  }>(),
  { showSearch: true },
);

const router = useRouter();
const route = useRoute();
const { t, locale } = useI18n();
const { isDark, toggleTheme } = useTheme();
const { clearApiKey } = useAuth();
const { health, healthOnline, fetchHealth } = useSessions();

const themeLabel = computed(() => (isDark.value ? t('common.toLight') : t('common.toDark')));

const healthPillText = computed(() => {
  if (healthOnline.value === false) return 'offline';
  const count = health.value?.active_sessions ?? 0;
  const formatted = new Intl.NumberFormat(locale.value).format(count);
  const label = t('common.health');
  // Replace the mockup's hardcoded "3" with the real active_sessions count from /v1/health
  return label.replace(/\b3\b/, formatted);
});

function goHomeSearch() {
  router.push({ name: 'home', query: { focus: '1' } });
}

function logout() {
  clearApiKey();
  router.push({ name: 'login' });
}

onMounted(() => {
  if (health.value === null) {
    fetchHealth();
  }
});
</script>

<template>
  <header class="topbar">
    <router-link to="/" class="brand">
      <img
        :src="isDark ? lockupDark : lockupLight"
        alt="Softagon Sistemas"
        class="brand-logo"
      />
      <span aria-hidden="true" class="brand-sep"></span>
      <span class="brand-name">Relay</span>
      <span class="brand-ver hide-sm mono">v{{ APP_VERSION }}</span>
    </router-link>

    <button
      v-if="props.showSearch && route.name !== 'home'"
      type="button"
      class="topbar-search hov"
      :aria-label="t('common.search')"
      @click="goHomeSearch"
    >
      <Search :size="15" aria-hidden="true" />
      <span class="hide-sm">{{ t('common.search') }}</span>
      <span class="kbd hide-sm">{{ MOD_KEY }}K</span>
    </button>

    <div class="topbar-actions">
      <router-link
        v-if="route.name === 'home'"
        to="/sessions"
        class="hide-sm health-pill hov"
        role="status"
      >
        <span
          aria-hidden="true"
          class="health-dot"
          :class="{ offline: healthOnline === false }"
        ></span>
        {{ healthPillText }}
      </router-link>

      <LanguageSwitcher />

      <button
        type="button"
        class="btn btn-icon"
        :aria-label="themeLabel"
        :title="themeLabel"
        @click="toggleTheme"
      >
        <Sun v-if="isDark" :size="18" aria-hidden="true" />
        <Moon v-else :size="18" aria-hidden="true" />
      </button>

      <router-link
        to="/api"
        class="btn hov"
        :aria-label="t('common.apiRef')"
        :title="t('common.apiRef')"
      >
        <Code2 :size="16" aria-hidden="true" />
        <span class="hide-sm">API</span>
      </router-link>

      <button
        type="button"
        class="btn btn-icon hov"
        :aria-label="t('common.logout')"
        :title="t('common.logout')"
        @click="logout"
      >
        <LogOut :size="17" aria-hidden="true" />
      </button>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 14px 24px;
  flex-wrap: wrap;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
}

.brand {
  display: flex;
  align-items: center;
  gap: 12px;
  text-decoration: none;
  color: var(--text);
}

.brand-logo {
  height: 30px;
  width: auto;
  display: block;
}

.brand-sep {
  width: 1px;
  height: 22px;
  background: var(--border-strong);
}

.brand-name {
  font: 700 15px 'Montserrat', sans-serif;
  letter-spacing: 0.02em;
  color: var(--text);
}

.brand-ver {
  font-size: 11px;
  font-weight: 600;
  color: var(--muted);
  background: var(--tint-neutral);
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 2px 7px;
}

.topbar-search {
  flex: 0 1 320px;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  height: 38px;
  padding: 0 12px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--muted);
  font: 500 13px 'Montserrat', sans-serif;
  cursor: pointer;
}

.topbar-search > span:nth-child(2) {
  flex: 1;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.topbar-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.health-pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-2);
  background: var(--tint-cyan);
  border: 1px solid var(--sel-border);
  border-radius: 999px;
  padding: 6px 12px;
  text-decoration: none;
  cursor: pointer;
}

.health-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--dot);
}

.health-dot.offline {
  background: var(--danger);
}

@media (max-width: 640px) {
  .topbar {
    padding: 12px 14px;
    gap: 10px;
  }
  .brand-logo {
    height: 24px;
  }
  .topbar-search {
    flex: 0 0 auto;
    width: 38px;
    padding: 0;
    justify-content: center;
  }
}
</style>
