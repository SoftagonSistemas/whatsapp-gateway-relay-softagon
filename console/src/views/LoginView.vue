<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { Eye, EyeOff, Info, KeyRound, Moon, Sun } from 'lucide-vue-next';
import lockupDark from '../assets/softagon-lockup-dark.png';
import lockupLight from '../assets/softagon-lockup-light.png';
import LanguageSwitcher from '../components/LanguageSwitcher.vue';
import { useTheme } from '../composables/useTheme';
import { useAuth } from '../composables/useAuth';
import { ApiError, useApi } from '../composables/useApi';
import { useSessions } from '../composables/useSessions';

const router = useRouter();
const route = useRoute();
const { t, locale } = useI18n();
const { isDark, toggleTheme } = useTheme();
const { setApiKey } = useAuth();
const api = useApi();
const { healthOnline, fetchHealth } = useSessions();

const key = ref('');
const show = ref(false);
const remember = ref(true);
const submitting = ref(false);
const errorMsg = ref<string | null>(null);
const keyInput = ref<HTMLInputElement | null>(null);

const hostLabel = computed(() =>
  typeof window !== 'undefined' ? window.location.host : 'localhost',
);
const docsUrl = computed(
  () =>
    `https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/${locale.value}/README.md`,
);
const themeLabel = computed(() => (isDark.value ? t('common.toLight') : t('common.toDark')));
const showLabel = computed(() => (show.value ? t('login.hide') : t('login.show')));

async function onSubmit() {
  const trimmed = key.value.trim();
  if (!trimmed) {
    errorMsg.value = t('login.helpErr');
    keyInput.value?.focus();
    return;
  }

  submitting.value = true;
  errorMsg.value = null;
  try {
    await api.verifyKey(trimmed);
    setApiKey(trimmed, remember.value);
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : '/';
    await router.replace(redirect.startsWith('/') ? redirect : '/');
  } catch (err) {
    if (err instanceof ApiError && err.status === 401) {
      errorMsg.value = t('login.helpErr');
    } else {
      errorMsg.value = err instanceof Error ? err.message : 'Network error';
    }
    keyInput.value?.focus();
  } finally {
    submitting.value = false;
  }
}

onMounted(() => {
  fetchHealth();
  keyInput.value?.focus();
});
</script>

<template>
  <div class="login-page">
    <div class="login-top">
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
    </div>

    <main class="login-main">
      <div class="login-logo-wrap">
        <img
          :src="isDark ? lockupDark : lockupLight"
          alt="Softagon Sistemas"
          class="login-logo"
        />
      </div>

      <section aria-labelledby="lg-h" class="login-card">
        <form class="login-form pad-sm" @submit.prevent="onSubmit">
          <div class="login-head">
            <span class="badge-pill cyan">Relay · v0.3.4</span>
            <h1 id="lg-h" class="login-h1">{{ t('login.h1') }}</h1>
            <p class="login-sub">{{ t('login.sub') }}</p>
          </div>

          <div class="login-field-group">
            <label for="api-key" class="login-label">{{ t('login.label') }}</label>
            <div class="field login-field" :class="{ invalid: !!errorMsg }">
              <KeyRound :size="18" aria-hidden="true" class="field-icon" />
              <input
                id="api-key"
                ref="keyInput"
                v-model="key"
                class="inp login-input"
                :type="show ? 'text' : 'password'"
                name="api_key"
                autocomplete="current-password"
                spellcheck="false"
                :aria-invalid="!!errorMsg"
                aria-describedby="api-key-help"
                :placeholder="t('login.placeholder')"
                @input="errorMsg = null"
              />
              <button
                type="button"
                class="eye-btn"
                :aria-pressed="show"
                :aria-label="showLabel"
                :title="showLabel"
                @click="show = !show"
              >
                <EyeOff v-if="show" :size="18" aria-hidden="true" />
                <Eye v-else :size="18" aria-hidden="true" />
              </button>
            </div>
            <p
              id="api-key-help"
              :role="errorMsg ? 'alert' : undefined"
              class="login-help"
              :class="{ err: !!errorMsg }"
            >
              {{ errorMsg || t('login.helpIdle') }}
            </p>
          </div>

          <label class="remember-row">
            <input v-model="remember" type="checkbox" class="remember-check" />
            <span>{{ t('login.remember') }}</span>
          </label>

          <button
            type="submit"
            class="btn btn-primary login-submit"
            :disabled="submitting"
          >
            <span>{{ t('login.enter') }}</span>
            <span class="enter-kbd">↵</span>
          </button>
        </form>

        <div class="login-footer-note">
          <Info :size="18" aria-hidden="true" class="note-icon" />
          <p class="note-text">
            {{ t('login.where1') }}
            <code class="mono code-inline">RELAY_API_KEY</code>
            {{ t('login.where2') }}
          </p>
        </div>
      </section>

      <div class="login-meta">
        <span role="status" class="status-inline">
          <span
            aria-hidden="true"
            class="status-dot"
            :class="{ offline: healthOnline === false }"
          ></span>
          <span class="mono">{{ hostLabel }}</span>
          <span>·</span>
          <span>{{ healthOnline === false ? 'offline' : t('login.online') }}</span>
        </span>
        <span aria-hidden="true">·</span>
        <a
          :href="docsUrl"
          target="_blank"
          rel="noopener"
          class="docs-link"
        >
          {{ t('common.userDocs') }}
        </a>
        <span aria-hidden="true">·</span>
        <a
          href="https://softagon.com.br"
          target="_blank"
          rel="noopener"
          class="docs-link"
          title="Softagon Sistemas — Engenharia de Software, IA e Automação WhatsApp"
        >
          {{ t('common.bySoftagon') }} (softagon.com.br)
        </a>
      </div>
    </main>
  </div>
</template>

<style scoped>
.login-page {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.login-top {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  padding: 16px 24px;
  flex-wrap: wrap;
}

.login-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px 16px 64px;
}

.login-logo-wrap {
  margin-bottom: 28px;
}

.login-logo {
  height: 44px;
  width: auto;
  display: block;
}

.login-card {
  width: 100%;
  max-width: 440px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 18px;
  box-shadow: var(--shadow);
  overflow: hidden;
}

.login-form {
  padding: 32px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.login-head {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}

.login-h1 {
  margin: 0;
  font-size: 26px;
  font-weight: 800;
  letter-spacing: -0.02em;
}

.login-sub {
  margin: 0;
  font-size: 14px;
  color: var(--muted);
  line-height: 1.55;
}

.login-field-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.login-label {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-2);
}

.login-field {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 48px;
  padding: 0 8px 0 14px;
  border-radius: 12px;
  border: 1px solid var(--border-strong);
  background: var(--sunken);
}

.login-field.invalid {
  border-color: var(--danger);
}

.field-icon {
  color: var(--muted);
  flex: none;
}

.login-input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: 0;
  color: var(--text);
  font: 500 15px 'JetBrains Mono', monospace;
  height: 100%;
}

.eye-btn {
  width: 36px;
  height: 36px;
  flex: none;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-2);
  display: grid;
  place-items: center;
  cursor: pointer;
}

.login-help {
  margin: 0;
  font-size: 12.5px;
  color: var(--muted);
  line-height: 1.45;
}

.login-help.err {
  color: var(--danger);
  font-weight: 600;
}

.remember-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 32px;
  font-size: 14px;
  color: var(--text-2);
  cursor: pointer;
}

.remember-check {
  width: 18px;
  height: 18px;
  margin: 0;
  accent-color: var(--accent-fill);
}

.login-submit {
  width: 100%;
  min-height: 52px;
  border-radius: 12px;
  font-size: 15px;
}

.enter-kbd {
  font: 600 11px 'JetBrains Mono', monospace;
  background: rgba(11, 18, 32, 0.14);
  border-radius: 5px;
  padding: 3px 6px;
}

.login-footer-note {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 16px 32px;
  background: var(--sunken);
  border-top: 1px solid var(--border);
}

.note-icon {
  color: var(--muted);
  flex: none;
  margin-top: 1px;
}

.note-text {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.55;
}

.code-inline {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
  background: var(--tint-neutral);
  border-radius: 5px;
  padding: 2px 6px;
}

.login-meta {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
  justify-content: center;
  margin-top: 22px;
  font-size: 13px;
  color: var(--muted);
}

.status-inline {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--dot);
}

.status-dot.offline {
  background: var(--danger);
}

.docs-link {
  color: var(--accent);
  font-weight: 600;
  text-decoration: none;
}
</style>
