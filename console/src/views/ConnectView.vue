<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { Check, ChevronLeft, ChevronRight, RefreshCw } from 'lucide-vue-next';
import TopBar from '../components/TopBar.vue';
import iconImg from '../assets/softagon-icon.png';
import { useApi, type SessionRecord } from '../composables/useApi';
import { useSessions } from '../composables/useSessions';
import { formatPhoneDigits, parsePhoneInput } from '../composables/usePhone';
import { isInputFocused } from '../composables/useHotkeys';

const route = useRoute();
const router = useRouter();
const { t, tm } = useI18n();
const api = useApi();
const { sessions, fetchSessions } = useSessions();

const phoneInput = ref(typeof route.query.phone === 'string' ? route.query.phone : '');
const session = ref<SessionRecord | null>(null);
const qrSvg = ref<string | null>(null);
const expiresAt = ref<number | null>(null);
const secondsLeft = ref<number>(60);
const liveStatus = ref<string>('created');
const loading = ref(false);
const errorMsg = ref<string | null>(null);
const isMobile = ref(false);

let pollTimer: number | null = null;
let tickTimer: number | null = null;

const parsedPhone = computed(() => parsePhoneInput(phoneInput.value));
const displayPhone = computed(() => {
  if (session.value) {
    const p = parsePhoneInput(session.value.name);
    return p.isPhone ? p.formatted : session.value.name;
  }
  return parsedPhone.value.isPhone ? parsedPhone.value.formatted : phoneInput.value;
});

const qrSteps = computed(() => tm('connect.sQr') as Array<{ a: string; b: string; c: string }>);

const progressPct = computed(() => Math.max(0, Math.min(100, (secondsLeft.value / 60) * 100)));

async function startConnectFlow(rawTarget?: string) {
  const existingId = typeof route.query.session_id === 'string' ? route.query.session_id : null;
  loading.value = true;
  errorMsg.value = null;

  try {
    await fetchSessions();
    if (existingId) {
      session.value = await api.getSession(existingId);
    } else {
      const target = (rawTarget ?? phoneInput.value).trim();
      const parsed = parsePhoneInput(target);
      const name = parsed.isPhone ? parsed.digits : target || `whatsapp-${Date.now().toString().slice(-4)}`;
      const existing = sessions.value.find(
        (s) => s.name === name || (s.phone_number && s.phone_number === name),
      );
      if (existing) {
        session.value = existing;
      } else {
        session.value = await api.createSession(name, 'pt');
      }
    }

    if (session.value) {
      await refreshStatusAndQr();
      startTimers();
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to create session';
  } finally {
    loading.value = false;
  }
}

async function refreshStatusAndQr() {
  if (!session.value) return;
  try {
    const st = await api.getSessionStatus(session.value.id);
    liveStatus.value = st.status;
    if (st.status === 'connected') {
      stopTimers();
      await fetchSessions();
      router.replace({ name: 'session', params: { id: session.value.id } });
      return;
    }

    try {
      const [qrMeta, svg] = await Promise.all([
        api.getSessionQr(session.value.id),
        api.getSessionQrSvg(session.value.id),
      ]);
      qrSvg.value = svg;
      if (qrMeta.qr?.expires_at) {
        expiresAt.value = new Date(qrMeta.qr.expires_at).getTime();
        updateCountdown();
      }
    } catch {
      qrSvg.value = null;
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to poll status';
  }
}

function updateCountdown() {
  if (!expiresAt.value) return;
  const diff = Math.max(0, Math.ceil((expiresAt.value - Date.now()) / 1000));
  secondsLeft.value = diff;
}

async function regenerateQr() {
  if (!session.value) return;
  loading.value = true;
  errorMsg.value = null;
  qrSvg.value = null;
  try {
    await api.reconnectSession(session.value.id);
    window.setTimeout(refreshStatusAndQr, 900);
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Reconnect failed';
  } finally {
    loading.value = false;
  }
}

function startTimers() {
  stopTimers();
  pollTimer = window.setInterval(refreshStatusAndQr, 2000);
  tickTimer = window.setInterval(updateCountdown, 1000);
}

function stopTimers() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
  if (tickTimer !== null) {
    window.clearInterval(tickTimer);
    tickTimer = null;
  }
}

function onKeyDown(e: KeyboardEvent) {
  if (isInputFocused(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key.toLowerCase() === 'r' && session.value) {
    e.preventDefault();
    regenerateQr();
  }
}

onMounted(() => {
  isMobile.value =
    window.innerWidth < 640 || window.matchMedia?.('(pointer: coarse)').matches === true;
  window.addEventListener('keydown', onKeyDown);

  if (route.query.phone || route.query.session_id) {
    startConnectFlow();
  }
});

onUnmounted(() => {
  stopTimers();
  window.removeEventListener('keydown', onKeyDown);
});
</script>

<template>
  <TopBar />

  <main class="connect-main">
    <section aria-labelledby="cx-h" class="connect-card">
      <div class="connect-head">
        <router-link to="/" :aria-label="t('connect.backAria')" class="back-btn hov">
          <ChevronLeft :size="16" aria-hidden="true" />
        </router-link>
        <span class="crumb">{{ t('connect.crumb') }}</span>
        <ChevronRight :size="14" aria-hidden="true" style="color: var(--muted)" />
        <span v-if="displayPhone" class="phone-chip mono">{{ displayPhone }}</span>
        <span style="flex: 1"></span>
        <span v-if="session" role="status" class="waiting-badge">
          <span class="pulse waiting-dot" aria-hidden="true"></span>
          {{ liveStatus === 'connected' ? t('home.connected') : t('connect.waiting') }}
        </span>
      </div>

      <!-- Step 1: Enter phone number if none provided in URL -->
      <form
        v-if="!session && !loading"
        class="phone-form pad-sm"
        @submit.prevent="startConnectFlow()"
      >
        <h1 id="cx-h" class="cx-h1">{{ t('connectMobile.h1') }}</h1>
        <p class="cx-sub">{{ t('connectMobile.sub') }}</p>
        <label for="phone-in" class="phone-label">{{ t('connectMobile.label') }}</label>
        <div class="field phone-field">
          <input
            id="phone-in"
            v-model="phoneInput"
            type="tel"
            class="inp mono phone-input"
            placeholder="11 99999-0001"
            autofocus
          />
        </div>
        <p v-if="parsedPhone.isPhone" class="phone-hint ok">
          {{ t('connectMobile.ok') }} <span class="mono">{{ formatPhoneDigits(parsedPhone.digits) }}</span>
        </p>
        <p v-else-if="phoneInput.trim()" class="phone-hint">{{ t('connectMobile.bad') }}</p>
        <button type="submit" class="btn btn-primary" style="min-height: 48px">
          {{ t('home.generate') }} <span class="kbd">↵</span>
        </button>
      </form>

      <!-- Step 2: Live QR Code + Progress -->
      <div v-else class="connect-grid">
        <div class="qr-col pad-sm">
          <!-- Mobile notice: pair-code endpoint is listed in section 9 API gaps -->
          <p v-if="isMobile" role="status" class="mobile-note">
            {{ t('connectMobile.desk') }}
          </p>

          <div class="qr-tile" :aria-busy="loading">
            <div
              v-if="qrSvg"
              class="qr-svg-wrap"
              role="img"
              :aria-label="t('connect.qrAlt')"
              v-html="qrSvg"
            ></div>
            <div v-else class="qr-placeholder">
              <span v-if="errorMsg" role="alert" style="color: var(--danger)">{{ errorMsg }}</span>
              <span v-else>{{ t('connect.waiting') }}…</span>
            </div>
            <span v-if="qrSvg" class="qr-center-logo" aria-hidden="true">
              <img :src="iconImg" alt="" style="width: 36px; height: 36px; display: block" />
            </span>
          </div>

          <div v-if="qrSvg" class="countdown-wrap">
            <div class="countdown-labels">
              <span>{{ t('connect.newQrIn') }}</span>
              <span class="mono" style="color: var(--text)">{{ secondsLeft }} s</span>
            </div>
            <div
              role="progressbar"
              :aria-label="t('connect.newQrIn')"
              aria-valuemin="0"
              aria-valuemax="60"
              :aria-valuenow="secondsLeft"
              class="countdown-track"
            >
              <div class="countdown-bar" :style="{ width: `${progressPct}%` }"></div>
            </div>
          </div>

          <button
            type="button"
            class="btn hov"
            :disabled="loading"
            @click="regenerateQr"
          >
            <RefreshCw :size="15" aria-hidden="true" />
            <span>{{ t('connect.regen') }}</span>
            <span class="kbd hide-sm">r</span>
          </button>
        </div>

        <div class="steps-col pad-sm">
          <div>
            <h1 id="cx-h" class="cx-h1">{{ t('connect.h1Qr') }}</h1>
            <p v-if="session" class="cx-sub">
              {{ t('connect.lead1') }}
              <span class="mono" style="color: var(--text-2)">{{ session.name }}</span>
              {{ t('connect.lead2') }}
            </p>
          </div>

          <ol class="steps-list">
            <li v-for="(s, idx) in qrSteps" :key="idx" class="step-item">
              <span aria-hidden="true" class="step-num mono">{{ idx + 1 }}</span>
              <span class="step-text">
                {{ s.a }}<b style="color: var(--text)">{{ s.b }}</b>{{ s.c }}
              </span>
            </li>
          </ol>

          <div class="progress-card" role="status">
            <span class="progress-title">{{ t('connect.progress') }}</span>
            <div class="progress-row done">
              <Check :size="16" aria-hidden="true" style="color: var(--accent)" />
              <span>{{ t('connect.created') }}</span>
              <span style="flex: 1"></span>
              <span class="mono muted-sm">POST /v1/sessions</span>
            </div>
            <div class="progress-row" :class="{ done: !!qrSvg }">
              <Check v-if="qrSvg" :size="16" aria-hidden="true" style="color: var(--accent)" />
              <span v-else class="dot-pending" aria-hidden="true"></span>
              <span>{{ t('connect.qrGen') }}</span>
              <span style="flex: 1"></span>
              <span class="mono muted-sm">GET /qr.svg</span>
            </div>
            <div class="progress-row active">
              <span class="pulse waiting-dot" aria-hidden="true" style="margin: 0 4px"></span>
              <span>{{ t('connect.waitingPhone') }}</span>
              <span style="flex: 1"></span>
              <span class="mono muted-sm">GET /status</span>
            </div>
            <div class="progress-row pending">
              <span class="dot-pending" aria-hidden="true"></span>
              <span>{{ t('connect.ready') }}</span>
            </div>
          </div>

          <p class="cx-after">{{ t('connect.after') }}</p>
        </div>
      </div>

      <footer class="connect-foot">
        <span>{{ t('connect.foot') }}</span>
        <router-link to="/" class="btn">{{ t('connect.back') }}</router-link>
      </footer>
    </section>
  </main>
</template>

<style scoped>
.connect-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 36px 16px 64px;
}

.connect-card {
  width: 100%;
  max-width: 980px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow);
  overflow: hidden;
}

.connect-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  min-height: 60px;
  flex-wrap: wrap;
  border-bottom: 1px solid var(--border);
}

.back-btn {
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  border-radius: 8px;
  background: var(--tint-neutral);
  color: var(--text-2);
}

.crumb {
  font-size: 14px;
  font-weight: 500;
  color: var(--muted);
}

.phone-chip {
  font-size: 13.5px;
  font-weight: 600;
  background: var(--tint-orange);
  color: var(--orange);
  border: 1px solid var(--primary);
  border-radius: 7px;
  padding: 4px 10px;
}

.waiting-badge {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--amber);
}

.waiting-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #FFA800;
}

.phone-form {
  max-width: 480px;
  margin: 0 auto;
  padding: 40px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.phone-label {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-2);
}

.phone-field {
  height: 50px;
  padding: 0 14px;
  border-radius: 12px;
  border: 1px solid var(--border-strong);
  background: var(--sunken);
  display: flex;
  align-items: center;
}

.phone-input {
  width: 100%;
  background: transparent;
  border: 0;
  color: var(--text);
  font-size: 17px;
}

.phone-hint {
  margin: 0;
  font-size: 13px;
  color: var(--muted);
}

.phone-hint.ok {
  color: var(--accent);
}

.connect-grid {
  display: flex;
  flex-wrap: wrap;
}

.qr-col {
  flex: 1 1 360px;
  min-width: 0;
  padding: 36px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 20px;
  border-right: 1px solid var(--border);
}

.mobile-note {
  margin: 0;
  padding: 10px 14px;
  border-radius: 10px;
  background: var(--tint-amber);
  color: var(--amber);
  font-size: 12.5px;
  text-align: center;
}

.qr-tile {
  width: 290px;
  height: 290px;
  background: #FFFFFF;
  border-radius: 18px;
  padding: 16px;
  position: relative;
  border: 1px solid var(--border);
  box-shadow: 0 0 0 6px var(--qr-ring);
  display: grid;
  place-items: center;
}

.qr-svg-wrap {
  width: 100%;
  height: 100%;
}

.qr-svg-wrap :deep(svg) {
  width: 100%;
  height: 100%;
  display: block;
}

.qr-placeholder {
  color: #53607A;
  font-size: 13px;
  text-align: center;
  padding: 12px;
}

.qr-center-logo {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 46px;
  height: 46px;
  border-radius: 12px;
  background: #FFFFFF;
  display: grid;
  place-items: center;
}

.countdown-wrap {
  width: 290px;
  max-width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.countdown-labels {
  display: flex;
  justify-content: space-between;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-2);
}

.countdown-track {
  height: 6px;
  background: var(--tint-neutral);
  border-radius: 6px;
  overflow: hidden;
}

.countdown-bar {
  height: 100%;
  background: var(--accent-fill);
  transition: width 0.5s linear;
}

.steps-col {
  flex: 1 1 380px;
  min-width: 0;
  padding: 36px;
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.cx-h1 {
  margin: 0 0 8px;
  font-size: 24px;
  font-weight: 800;
  letter-spacing: -0.02em;
}

.cx-sub {
  margin: 0;
  font-size: 14px;
  color: var(--muted);
  line-height: 1.55;
}

.steps-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.step-item {
  display: flex;
  gap: 14px;
  align-items: flex-start;
}

.step-num {
  width: 28px;
  height: 28px;
  flex: none;
  border-radius: 50%;
  display: grid;
  place-items: center;
  background: var(--tint-cyan);
  color: var(--accent);
  font-size: 13px;
  font-weight: 700;
}

.step-text {
  font-size: 14.5px;
  line-height: 1.55;
  padding-top: 3px;
  color: var(--text-2);
}

.progress-card {
  background: var(--sunken);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.progress-title {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
}

.progress-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--muted);
  flex-wrap: wrap;
}

.progress-row.done {
  color: var(--text-2);
}

.progress-row.active {
  color: var(--amber);
  font-weight: 600;
}

.muted-sm {
  font-size: 12px;
  color: var(--muted);
  font-weight: 400;
}

.dot-pending {
  width: 14px;
  height: 14px;
  margin: 0 1px;
  border-radius: 50%;
  border: 1.5px dashed currentColor;
}

.cx-after {
  margin: 0;
  font-size: 13px;
  color: var(--muted);
  line-height: 1.55;
}

.connect-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 18px;
  flex-wrap: wrap;
  background: var(--sunken);
  border-top: 1px solid var(--border);
  font-size: 12px;
  color: var(--muted);
}

@media (max-width: 780px) {
  .qr-col {
    border-right: 0;
    border-bottom: 1px solid var(--border);
  }
}
</style>
