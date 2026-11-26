<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { Check, Copy, Plus, Trash2, Webhook, X } from 'lucide-vue-next';
import TopBar from '../components/TopBar.vue';
import { useApi, type WebhookRecord } from '../composables/useApi';
import { useSessions } from '../composables/useSessions';

const ALL_EVENTS = [
  'session.status',
  'message.received',
  'message.sent',
  'message.ack',
  'message.transcription.completed',
  'message.transcription.failed',
  'group.updated',
] as const;

const route = useRoute();
const { t } = useI18n();
const api = useApi();
const { sessions, fetchSessions } = useSessions();

const webhooks = ref<WebhookRecord[]>([]);
const loading = ref(false);
const errorMsg = ref<string | null>(null);
const filterSession = ref(typeof route.query.session_id === 'string' ? route.query.session_id : '');
const selectedEventFilter = ref<string>('all');

// Create modal
const showCreate = ref(false);
const formUrl = ref('');
const formSession = ref(filterSession.value);
const formDesc = ref('');
const formEvents = ref<string[]>(['*']);
const creating = ref(false);
const createdSecret = ref<string | null>(null);
const copiedSecret = ref(false);

const filteredWebhooks = computed(() => {
  return webhooks.value.filter((w) => {
    if (filterSession.value && w.session_id && w.session_id !== filterSession.value) return false;
    if (
      selectedEventFilter.value !== 'all' &&
      !w.events.includes('*') &&
      !w.events.includes(selectedEventFilter.value)
    ) {
      return false;
    }
    return true;
  });
});

async function loadWebhooks() {
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.listWebhooks();
    webhooks.value = res.webhooks ?? [];
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to load webhooks';
  } finally {
    loading.value = false;
  }
}

function toggleEventChoice(ev: string) {
  if (ev === '*') {
    formEvents.value = ['*'];
    return;
  }
  const withoutStar = formEvents.value.filter((x) => x !== '*');
  if (withoutStar.includes(ev)) {
    const next = withoutStar.filter((x) => x !== ev);
    formEvents.value = next.length ? next : ['*'];
  } else {
    formEvents.value = [...withoutStar, ev];
  }
}

async function submitWebhook() {
  if (!formUrl.value.trim()) return;
  creating.value = true;
  errorMsg.value = null;
  try {
    const created = await api.createWebhook({
      url: formUrl.value.trim(),
      session_id: formSession.value || null,
      events: formEvents.value,
      enabled: true,
      description: formDesc.value.trim() || null,
    });
    createdSecret.value = created.secret ?? null;
    formUrl.value = '';
    formDesc.value = '';
    showCreate.value = false;
    await loadWebhooks();
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to create webhook';
  } finally {
    creating.value = false;
  }
}

async function toggleEnabled(w: WebhookRecord) {
  try {
    const updated = await api.updateWebhook(w.id, { enabled: !w.enabled });
    const idx = webhooks.value.findIndex((x) => x.id === w.id);
    if (idx !== -1) webhooks.value[idx] = updated;
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to update webhook';
  }
}

async function removeWebhook(w: WebhookRecord) {
  if (!window.confirm(`Delete webhook ${w.url}?`)) return;
  try {
    await api.deleteWebhook(w.id);
    webhooks.value = webhooks.value.filter((x) => x.id !== w.id);
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : 'Failed to delete webhook';
  }
}

async function copySecret() {
  if (!createdSecret.value) return;
  try {
    await navigator.clipboard.writeText(createdSecret.value);
    copiedSecret.value = true;
    window.setTimeout(() => (copiedSecret.value = false), 1800);
  } catch {
    // ignore
  }
}

onMounted(() => {
  fetchSessions();
  loadWebhooks();
});
</script>

<template>
  <TopBar />

  <main class="ev-main">
    <div class="ev-head">
      <div>
        <div style="display: flex; align-items: center; gap: 10px; flex-wrap: wrap">
          <h1 class="ev-h1">{{ t('events.title') }}</h1>
          <span class="badge-pill cyan">
            {{ webhooks.length }} webhook{{ webhooks.length === 1 ? '' : 's' }} · HMAC-SHA256
          </span>
        </div>
        <p class="ev-sub">{{ t('home.hooksSub') }}</p>
      </div>
      <button type="button" class="btn btn-primary" @click="showCreate = true">
        <Plus :size="16" aria-hidden="true" />
        <span>{{ t('events.newHook') }}</span>
      </button>
    </div>

    <!-- One-time secret banner -->
    <div v-if="createdSecret" role="status" class="secret-banner">
      <div style="flex: 1; min-width: 0">
        <strong>HMAC Secret (shown once):</strong>
        <code class="mono" style="margin-left: 8px; word-break: break-all">{{ createdSecret }}</code>
      </div>
      <button type="button" class="btn" @click="copySecret">
        <Check v-if="copiedSecret" :size="14" aria-hidden="true" />
        <Copy v-else :size="14" aria-hidden="true" />
        <span>{{ copiedSecret ? t('connect.copied') : t('session.copy') }}</span>
      </button>
      <button type="button" class="btn btn-icon" :aria-label="t('session.close')" @click="createdSecret = null">
        <X :size="15" aria-hidden="true" />
      </button>
    </div>

    <div v-if="errorMsg" role="alert" class="err-banner">{{ errorMsg }}</div>

    <!-- Filter pills -->
    <div class="filter-bar" role="group" :aria-label="t('events.filter')">
      <button
        type="button"
        class="chip"
        :class="{ active: selectedEventFilter === 'all' }"
        @click="selectedEventFilter = 'all'"
      >
        {{ t('events.all') }}
      </button>
      <button
        v-for="ev in ALL_EVENTS"
        :key="ev"
        type="button"
        class="chip mono"
        :class="{ active: selectedEventFilter === ev }"
        @click="selectedEventFilter = ev"
      >
        {{ ev }}
      </button>
    </div>

    <div class="ev-grid">
      <!-- Left: Webhooks CRUD -->
      <section class="ev-card">
        <h2 class="sec-h2">Webhooks ({{ filteredWebhooks.length }})</h2>
        <div v-if="loading" class="empty-msg">…</div>
        <div v-else-if="filteredWebhooks.length === 0" class="empty-msg">
          {{ t('events.empty') }}
        </div>
        <div v-else class="wh-list">
          <div v-for="w in filteredWebhooks" :key="w.id" class="wh-row">
            <div class="wh-info">
              <div class="wh-url mono">{{ w.url }}</div>
              <div class="wh-meta">
                <span>{{ w.id }}</span>
                <span v-if="w.session_id">· {{ w.session_id }}</span>
                <span>· {{ w.events.join(', ') }}</span>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              :aria-checked="w.enabled"
              :aria-label="`Toggle ${w.url}`"
              class="switch"
              :class="{ on: w.enabled }"
              @click="toggleEnabled(w)"
            >
              <span class="switch-knob"></span>
            </button>
            <button
              type="button"
              class="btn btn-icon btn-danger"
              :aria-label="`Delete ${w.url}`"
              @click="removeWebhook(w)"
            >
              <Trash2 :size="15" aria-hidden="true" />
            </button>
          </div>
        </div>
      </section>

      <!-- Right: Honest empty state for Event Feed (Section 9 API gap) -->
      <section class="ev-card">
        <h2 class="sec-h2">{{ t('events.list') }}</h2>
        <div class="gap-box">
          <Webhook :size="28" aria-hidden="true" style="color: var(--muted)" />
          <p class="gap-title">{{ t('events.empty') }}</p>
          <p class="gap-note">
            <code class="mono">GET /v1/events</code> · <code class="mono">POST /v1/webhooks/deliveries/{id}/retry</code>
          </p>
        </div>
      </section>
    </div>

    <!-- Create Webhook Modal -->
    <div v-if="showCreate" class="modal-backdrop" @click.self="showCreate = false">
      <div role="dialog" aria-modal="true" class="modal-card">
        <div class="modal-head">
          <h2 style="margin: 0; font-size: 16px; font-weight: 700">{{ t('events.newHook') }}</h2>
          <button type="button" class="btn btn-icon" :aria-label="t('session.close')" @click="showCreate = false">
            <X :size="16" aria-hidden="true" />
          </button>
        </div>
        <form
          class="modal-form"
          toolname="create_webhook_subscription"
          tooldescription="Register a new HMAC-SHA256 signed webhook URL to receive WhatsApp gateway events."
          @submit.prevent="submitWebhook"
        >
          <label for="wh-url" class="f-label">URL</label>
          <input
            id="wh-url"
            v-model="formUrl"
            name="url"
            type="url"
            required
            class="inp f-input mono"
            placeholder="https://example.com/webhooks/whatsapp"
            toolparamdescription="Destination HTTPS webhook endpoint URL."
          />

          <label for="wh-sess" class="f-label">{{ t('events.session') }}</label>
          <select
            id="wh-sess"
            v-model="formSession"
            name="session_id"
            class="f-input"
            toolparamdescription="Optional WhatsApp session ID to filter events, or empty for all sessions."
          >
            <option value="">{{ t('events.all') }}</option>
            <option v-for="s in sessions" :key="s.id" :value="s.id">
              {{ s.name }} ({{ s.id }})
            </option>
          </select>

          <span class="f-label">{{ t('events.title') }}</span>
          <div class="ev-checks">
            <button
              type="button"
              class="chip"
              :class="{ active: formEvents.includes('*') }"
              @click="toggleEventChoice('*')"
            >
              * ({{ t('events.all') }})
            </button>
            <button
              v-for="ev in ALL_EVENTS"
              :key="ev"
              type="button"
              class="chip mono"
              :class="{ active: formEvents.includes(ev) }"
              @click="toggleEventChoice(ev)"
            >
              {{ ev }}
            </button>
          </div>

          <div style="display: flex; justify-content: flex-end; gap: 10px; margin-top: 8px">
            <button type="button" class="btn" @click="showCreate = false">{{ t('session.close') }}</button>
            <button type="submit" class="btn btn-primary" :disabled="creating">
              {{ t('events.newHook') }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </main>
</template>

<style scoped>
.ev-main {
  flex: 1;
  max-width: 1120px;
  width: 100%;
  margin: 0 auto;
  padding: 28px 20px 64px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.ev-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
  flex-wrap: wrap;
}

.ev-h1 {
  margin: 0;
  font-size: 26px;
  font-weight: 800;
}

.ev-sub {
  margin: 6px 0 0;
  font-size: 14px;
  color: var(--muted);
}

.secret-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 14px 16px;
  border-radius: 12px;
  background: var(--tint-amber);
  border: 1px solid rgba(255, 168, 0, 0.4);
  color: var(--text);
  font-size: 13px;
}

.err-banner {
  padding: 12px 16px;
  border-radius: 10px;
  background: rgba(239, 68, 68, 0.14);
  border: 1px solid rgba(239, 68, 68, 0.35);
  color: var(--danger);
  font-size: 13px;
}

.filter-bar {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.chip {
  padding: 6px 11px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}

.chip.active {
  background: var(--sel);
  border-color: var(--sel-border);
  color: var(--text);
}

.ev-grid {
  display: grid;
  grid-template-columns: 1.25fr 1fr;
  gap: 18px;
}

.ev-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}

.sec-h2 {
  margin: 0;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
}

.wh-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.wh-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: 10px;
  background: var(--sunken);
  border: 1px solid var(--border);
}

.wh-info {
  flex: 1;
  min-width: 0;
}

.wh-url {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.wh-meta {
  font-size: 12px;
  color: var(--muted);
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.switch {
  width: 42px;
  height: 24px;
  border-radius: 999px;
  border: 0;
  background: var(--track-off);
  position: relative;
  cursor: pointer;
  flex: none;
}

.switch.on {
  background: var(--accent-fill);
}

.switch-knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #FFFFFF;
  transition: transform 0.14s ease;
}

.switch.on .switch-knob {
  transform: translateX(18px);
}

.empty-msg {
  padding: 24px;
  text-align: center;
  color: var(--muted);
  font-size: 13.5px;
}

.gap-box {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 36px 16px;
  text-align: center;
}

.gap-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-2);
}

.gap-note {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
}

.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: grid;
  place-items: center;
  padding: 16px;
  z-index: 60;
}

.modal-card {
  width: 100%;
  max-width: 520px;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: 16px;
  box-shadow: var(--shadow);
  overflow: hidden;
}

.modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-bottom: 1px solid var(--border);
}

.modal-form {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.f-label {
  font-size: 12.5px;
  font-weight: 700;
  color: var(--text-2);
}

.f-input {
  height: 42px;
  padding: 0 12px;
  border-radius: 10px;
  border: 1px solid var(--border-strong);
  background: var(--sunken);
  color: var(--text);
  font-size: 13.5px;
}

.ev-checks {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

@media (max-width: 820px) {
  .ev-grid {
    grid-template-columns: 1fr;
  }
}
</style>
