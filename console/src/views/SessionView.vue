<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import {
  AlertTriangle,
  Check,
  Command,
  Copy,
  HardDrive,
  LogOut,
  Mic,
  Plus,
  QrCode,
  RefreshCw,
  Send,
  Trash2,
  Webhook,
  X,
} from 'lucide-vue-next';
import TopBar from '../components/TopBar.vue';
import { useApi, type SessionRecord } from '../composables/useApi';
import { useSessions } from '../composables/useSessions';
import { MOD_KEY, isInputFocused, isMacOS } from '../composables/useHotkeys';
import { normalizeText, parsePhoneInput } from '../composables/usePhone';

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const api = useApi();
const { sessions, sessionsLoading, fetchSessions, health, fetchHealth } = useSessions();

const current = ref<SessionRecord | null>(null);
const loadingDetail = ref(false);
const detailError = ref<string | null>(null);
const copiedField = ref<string | null>(null);

// Action Panel (Raycast-style)
const actionsOpen = ref(false);
const actionQuery = ref('');
const actionInput = ref<HTMLInputElement | null>(null);
const actionTrigger = ref<HTMLButtonElement | null>(null);

// Inline Send Test Message
const showSendForm = ref(route.query.action === 'send');
const sendTo = ref('');
const sendText = ref('');
const sending = ref(false);
const sendStatus = ref<{ ok: boolean; msg: string } | null>(null);
const parsedRecipient = computed(() => parsePhoneInput(sendTo.value));
const recipientValid = computed(() => parsedRecipient.value.isPhone || parsedRecipient.value.isJid);

// Logout / Delete confirmation
const confirmLogout = ref(false);
const confirmDelete = ref(false);
const busyAction = ref(false);

const canDelete = computed(() => !!current.value && current.value.status !== 'connected');

interface SessionAction {
  id: string;
  group: 'sess' | 'conn' | 'int' | 'danger';
  label: string;
  kbd?: string;
  run: () => void;
}

const sessionId = computed(() => String(route.params.id || ''));

async function loadCurrent() {
  if (!sessionId.value) return;
  loadingDetail.value = true;
  detailError.value = null;
  try {
    current.value = await api.getSession(sessionId.value);
  } catch (err) {
    detailError.value = err instanceof Error ? err.message : 'Failed to load session';
    current.value = null;
  } finally {
    loadingDetail.value = false;
  }
}

async function copyText(val: string, field: string) {
  try {
    await navigator.clipboard.writeText(val);
    copiedField.value = field;
    window.setTimeout(() => {
      if (copiedField.value === field) copiedField.value = null;
    }, 1800);
  } catch {
    // ignore
  }
}

function openActions() {
  actionsOpen.value = true;
  actionQuery.value = '';
  window.setTimeout(() => actionInput.value?.focus(), 20);
}

function closeActions() {
  actionsOpen.value = false;
  actionTrigger.value?.focus();
}

async function handleReconnect() {
  if (!current.value) return;
  busyAction.value = true;
  closeActions();
  try {
    await api.reconnectSession(current.value.id);
    await router.push({ name: 'connect', query: { session_id: current.value.id } });
  } finally {
    busyAction.value = false;
  }
}

async function handleLogout() {
  if (!current.value) return;
  busyAction.value = true;
  try {
    await api.logoutSession(current.value.id);
    confirmLogout.value = false;
    closeActions();
    await Promise.all([loadCurrent(), fetchSessions(), fetchHealth()]);
  } finally {
    busyAction.value = false;
  }
}

async function handleDelete() {
  if (!current.value || !canDelete.value) return;
  busyAction.value = true;
  try {
    await api.deleteSession(current.value.id);
    confirmDelete.value = false;
    closeActions();
    await Promise.all([fetchSessions(), fetchHealth()]);
    if (sessions.value.length > 0) {
      await router.replace({ name: 'session', params: { id: sessions.value[0].id } });
    } else {
      await router.replace({ name: 'home' });
    }
  } catch (err) {
    detailError.value = err instanceof Error ? err.message : 'Failed to delete session';
  } finally {
    busyAction.value = false;
  }
}

async function submitTestMessage() {
  if (!current.value || !recipientValid.value || !sendText.value.trim()) return;
  sending.value = true;
  sendStatus.value = null;
  try {
    const res = await api.sendTextMessage(
      current.value.id,
      parsedRecipient.value.digits,
      sendText.value.trim(),
    );
    sendStatus.value = { ok: true, msg: `${res.status} · ${res.id}` };
    sendText.value = '';
  } catch (err) {
    sendStatus.value = {
      ok: false,
      msg: err instanceof Error ? err.message : 'Send failed',
    };
  } finally {
    sending.value = false;
  }
}

const actionsList = computed<SessionAction[]>(() => {
  if (!current.value) return [];
  const s = current.value;
  return [
    {
      id: 'send',
      group: 'sess',
      label: t('session.aSend'),
      run: () => {
        closeActions();
        showSendForm.value = true;
      },
    },
    {
      id: 'copy-id',
      group: 'sess',
      label: `${t('session.copy')} ID`,
      kbd: 'c',
      run: () => {
        copyText(s.id, 'id');
        closeActions();
      },
    },
    ...(s.jid
      ? [
          {
            id: 'copy-jid',
            group: 'sess' as const,
            label: t('session.aJid'),
            run: () => {
              copyText(s.jid!, 'jid');
              closeActions();
            },
          },
        ]
      : []),
    {
      id: 'qr',
      group: 'conn',
      label: t('session.aQr'),
      run: () => {
        closeActions();
        router.push({ name: 'connect', query: { session_id: s.id } });
      },
    },
    {
      id: 'restart',
      group: 'conn',
      label: t('session.aRst'),
      run: handleReconnect,
    },
    {
      id: 'hook',
      group: 'int',
      label: t('session.aHook'),
      run: () => {
        closeActions();
        router.push({ name: 'events', query: { session_id: s.id } });
      },
    },
    ...(s.status === 'connected'
      ? [
          {
            id: 'logout',
            group: 'danger' as const,
            label: t('session.aOut'),
            run: () => {
              confirmLogout.value = true;
            },
          },
        ]
      : [
          {
            id: 'delete',
            group: 'danger' as const,
            label: 'Excluir sessão',
            run: () => {
              confirmDelete.value = true;
            },
          },
        ]),
  ];
});

const filteredActions = computed(() => {
  const q = normalizeText(actionQuery.value.trim());
  if (!q) return actionsList.value;
  return actionsList.value.filter((a) => normalizeText(a.label).includes(q));
});

const groupedActions = computed(() => {
  const groups = [
    { key: 'sess', title: t('session.gSess'), items: [] as SessionAction[] },
    { key: 'conn', title: t('session.gConn'), items: [] as SessionAction[] },
    { key: 'int', title: t('session.gInt'), items: [] as SessionAction[] },
    { key: 'danger', title: t('session.gDanger'), items: [] as SessionAction[] },
  ];
  for (const a of filteredActions.value) {
    const g = groups.find((x) => x.key === a.group);
    if (g) g.items.push(a);
  }
  return groups.filter((g) => g.items.length > 0);
});

function onKeyDown(e: KeyboardEvent) {
  const mod = isMacOS() ? e.metaKey : e.ctrlKey;
  if (mod && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    e.stopImmediatePropagation();
    if (actionsOpen.value) closeActions();
    else openActions();
    return;
  }
  if (actionsOpen.value && e.key === 'Escape') {
    e.preventDefault();
    closeActions();
    return;
  }
  if (actionsOpen.value && !isInputFocused(e.target) && e.key.toLowerCase() === 'c' && current.value) {
    e.preventDefault();
    copyText(current.value.id, 'id');
    closeActions();
  }
}

watch(sessionId, loadCurrent);

onMounted(async () => {
  await fetchSessions();
  fetchHealth();
  if (!sessionId.value && sessions.value.length > 0) {
    router.replace({ name: 'session', params: { id: sessions.value[0].id } });
  } else {
    loadCurrent();
  }
  window.addEventListener('keydown', onKeyDown, true);
});

onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown, true);
});
</script>

<template>
  <TopBar />

  <div class="split-shell">
    <aside class="sidebar" :aria-label="t('session.sessions')">
      <div class="side-head">
        <span class="side-title">{{ t('session.sessions') }}</span>
        <router-link to="/connect" class="btn btn-icon" :aria-label="t('home.newNum')">
          <Plus :size="15" aria-hidden="true" />
        </router-link>
      </div>

      <div v-if="sessionsLoading" class="side-empty">…</div>
      <div v-else-if="sessions.length === 0" class="side-empty">
        {{ t('home.newNumSub') }}
      </div>
      <div v-else class="side-list">
        <router-link
          v-for="s in sessions"
          :key="s.id"
          :to="`/sessions/${s.id}`"
          class="side-item hov"
          :class="{ active: s.id === sessionId }"
        >
          <span class="side-item-name">{{ s.name }}</span>
          <span
            class="side-dot"
            :class="{
              on: s.status === 'connected',
              warn: s.status === 'pairing_required',
            }"
            :title="s.status"
          ></span>
        </router-link>
      </div>

      <div class="side-foot">
        <router-link to="/events" class="side-link hov">
          <Webhook :size="15" aria-hidden="true" />
          <span>{{ t('session.events') }}</span>
        </router-link>
      </div>
    </aside>

    <main class="detail-pane">
      <div v-if="loadingDetail" class="state-box">…</div>
      <div v-else-if="detailError" role="alert" class="state-box err">{{ detailError }}</div>
      <div v-else-if="!current && !sessionsLoading" class="state-box">
        <p style="margin: 0 0 14px">{{ t('home.newNumSub') }}</p>
        <router-link to="/connect" class="btn btn-primary">{{ t('home.newNum') }}</router-link>
      </div>
      <div v-else-if="current" class="detail-content">
        <div class="detail-head">
          <div>
            <div class="detail-title-row">
              <h1 class="detail-h1">{{ current.name }}</h1>
              <span
                class="badge-pill"
                :class="current.status === 'connected' ? 'cyan' : current.status === 'pairing_required' ? 'amber' : ''"
              >
                {{ current.status === 'connected' ? t('session.connected') : current.status }}
              </span>
            </div>
            <p v-if="current.phone_number" class="mono muted-line">{{ current.phone_number }}</p>
          </div>

          <div class="detail-btns">
            <button
              type="button"
              class="btn btn-primary"
              @click="showSendForm = !showSendForm"
            >
              <Send :size="15" aria-hidden="true" />
              <span>{{ t('session.sendMsg') }}</span>
            </button>
            <button
              ref="actionTrigger"
              type="button"
              class="btn"
              @click="openActions"
            >
              <Command :size="15" aria-hidden="true" />
              <span>{{ t('session.actions') }}</span>
              <span class="kbd hide-sm">{{ MOD_KEY }}K</span>
            </button>
          </div>
        </div>

        <!-- Friendly Disconnection / Account Restriction Banner -->
        <section
          v-if="current.status === 'logged_out'"
          role="alert"
          class="restriction-banner"
        >
          <AlertTriangle :size="18" class="restriction-icon" aria-hidden="true" />
          <div class="restriction-body">
            <strong class="restriction-title">{{ t('session.loggedOutTitle') }}</strong>
            <p class="restriction-desc">
              {{
                current.status_reason === 'account_restricted'
                  ? t('session.restrictedDesc')
                  : current.status_reason === 'logout'
                    ? t('session.manualLogoutDesc')
                    : t('session.deviceRemovedDesc')
              }}
            </p>
            <span v-if="current.status_reason" class="mono restriction-code">
              {{ current.status_reason }}
            </span>
          </div>
        </section>

        <!-- Inline Test Message Form -->
        <section v-if="showSendForm" class="card-block">
          <div class="card-block-head">
            <h2 class="card-h2">{{ t('session.aSend') }}</h2>
            <button type="button" class="btn btn-icon" :aria-label="t('session.close')" @click="showSendForm = false">
              <X :size="15" aria-hidden="true" />
            </button>
          </div>
          <form class="send-form" @submit.prevent="submitTestMessage">
            <div class="form-row">
              <label for="send-to" class="sr">{{ t('session.number') }}</label>
              <input
                id="send-to"
                v-model="sendTo"
                type="tel"
                class="inp form-input mono"
                placeholder="+55 87 99200-5656"
                required
              />
              <label for="send-msg" class="sr">{{ t('session.sendMsg') }}</label>
              <input
                id="send-msg"
                v-model="sendText"
                class="inp form-input"
                :placeholder="t('apiReference.hello')"
                required
              />
              <button type="submit" class="btn btn-primary" :disabled="sending || !recipientValid">
                {{ t('session.sendMsg') }}
              </button>
            </div>
            <p v-if="sendTo.trim() && recipientValid" class="send-msg ok mono">
              {{ t('connectMobile.ok') }} {{ parsedRecipient.formatted }}
            </p>
            <p v-else-if="sendTo.trim()" class="send-msg" style="color: var(--muted)">
              {{ t('connectMobile.bad') }}
            </p>
            <p
              v-if="sendStatus"
              :role="sendStatus.ok ? 'status' : 'alert'"
              class="send-msg"
              :class="{ ok: sendStatus.ok, err: !sendStatus.ok }"
            >
              {{ sendStatus.msg }}
            </p>
          </form>
        </section>

        <!-- Session Identity (ID + JID — no invented session token or stats) -->
        <section class="card-block">
          <h2 class="card-h2">{{ t('session.gSess') }}</h2>
          <div class="kv-row">
            <span class="kv-label">ID</span>
            <code class="mono kv-val">{{ current.id }}</code>
            <button
              type="button"
              class="btn"
              @click="copyText(current.id, 'id')"
            >
              <Check v-if="copiedField === 'id'" :size="14" aria-hidden="true" />
              <Copy v-else :size="14" aria-hidden="true" />
              <span>{{ copiedField === 'id' ? t('connect.copied') : t('session.copy') }}</span>
            </button>
          </div>
          <div class="kv-row">
            <span class="kv-label">JID</span>
            <code class="mono kv-val">{{ current.jid || '—' }}</code>
            <button
              v-if="current.jid"
              type="button"
              class="btn"
              @click="copyText(current.jid, 'jid')"
            >
              <Check v-if="copiedField === 'jid'" :size="14" aria-hidden="true" />
              <Copy v-else :size="14" aria-hidden="true" />
              <span>{{ copiedField === 'jid' ? t('connect.copied') : t('session.copy') }}</span>
            </button>
          </div>
        </section>

        <!-- Integrations (real server data from /v1/health + session.transcription_language) -->
        <section class="card-block">
          <h2 class="card-h2">{{ t('session.integrations') }}</h2>
          <div class="int-row">
            <Mic :size="18" aria-hidden="true" style="color: var(--accent)" />
            <div class="int-copy">
              <div class="int-title">{{ t('session.trTitle') }}</div>
              <div class="int-sub">
                {{ current.transcription_language || 'auto' }} ·
                {{ health?.transcription_available ? 'Whisper ready' : 'Whisper feature off' }}
              </div>
            </div>
            <span class="badge-pill cyan">
              {{ health?.transcription_enabled ? t('session.activeF') : 'off' }}
            </span>
          </div>
          <div class="int-row">
            <HardDrive :size="18" aria-hidden="true" style="color: var(--amber)" />
            <div class="int-copy">
              <div class="int-title">{{ t('session.storage') }}</div>
              <div class="int-sub">{{ t('session.storageSub') }}</div>
            </div>
            <span class="badge-pill">{{ health?.media_storage || t('session.local') }}</span>
          </div>
          <div class="int-row">
            <Webhook :size="18" aria-hidden="true" style="color: var(--orange)" />
            <div class="int-copy">
              <div class="int-title">{{ t('session.events') }}</div>
              <div class="int-sub">{{ t('home.hooksSub') }}</div>
            </div>
            <router-link :to="`/events?session_id=${current.id}`" class="btn">
              {{ t('home.open') }}
            </router-link>
          </div>
        </section>

        <!-- Quick Connection Actions -->
        <section class="card-block">
          <h2 class="card-h2">{{ t('session.gConn') }}</h2>
          <div class="quick-conn">
            <router-link :to="`/connect?session_id=${current.id}`" class="btn">
              <QrCode :size="15" aria-hidden="true" />
              <span>{{ t('session.aQr') }}</span>
            </router-link>
            <button type="button" class="btn" :disabled="busyAction" @click="handleReconnect">
              <RefreshCw :size="15" aria-hidden="true" />
              <span>{{ t('session.aRst') }}</span>
            </button>
            <button
              v-if="current.status === 'connected'"
              type="button"
              class="btn btn-danger"
              @click="confirmLogout = true"
            >
              <LogOut :size="15" aria-hidden="true" />
              <span>{{ t('session.aOut') }}</span>
            </button>
            <button
              v-else-if="canDelete"
              type="button"
              class="btn btn-danger"
              @click="confirmDelete = true"
            >
              <Trash2 :size="15" aria-hidden="true" />
              <span>Excluir sessão</span>
            </button>
          </div>
        </section>
      </div>
    </main>
  </div>

  <!-- Floating Raycast Action Panel -->
  <div v-if="actionsOpen" class="ap-backdrop" @click.self="closeActions">
    <div
      role="dialog"
      aria-modal="true"
      :aria-label="t('session.actions')"
      class="ap"
    >
      <div class="ap-head">
        <input
          ref="actionInput"
          v-model="actionQuery"
          type="text"
          class="inp ap-input"
          :placeholder="t('session.searchActions')"
        />
        <button type="button" class="kbd" style="cursor: pointer" @click="closeActions">esc</button>
      </div>
      <div class="ap-body">
        <template v-for="g in groupedActions" :key="g.key">
          <div class="ap-group" :class="{ danger: g.key === 'danger' }">{{ g.title }}</div>
          <button
            v-for="a in g.items"
            :key="a.id"
            type="button"
            class="ap-row hov"
            :class="{ danger: a.group === 'danger' }"
            @click="a.run()"
          >
            <span>{{ a.label }}</span>
            <span v-if="a.kbd" class="kbd">{{ a.kbd }}</span>
          </button>
        </template>
        <div v-if="groupedActions.length === 0" class="ap-empty">{{ t('session.noActions') }}</div>
      </div>
    </div>
  </div>

  <!-- Logout Confirmation Dialog -->
  <div v-if="confirmLogout" class="ap-backdrop" @click.self="confirmLogout = false">
    <div role="dialog" aria-modal="true" class="confirm-card">
      <h2 class="card-h2" style="color: var(--danger)">{{ t('session.aOut') }}</h2>
      <p style="margin: 8px 0 18px; font-size: 13.5px; color: var(--text-2)">
        {{ current?.name }} ({{ current?.id }})
      </p>
      <div style="display: flex; justify-content: flex-end; gap: 10px">
        <button type="button" class="btn" @click="confirmLogout = false">
          {{ t('session.close') }}
        </button>
        <button type="button" class="btn btn-danger" :disabled="busyAction" @click="handleLogout">
          {{ t('session.aOut') }}
        </button>
      </div>
    </div>
  </div>
  <!-- Delete Confirmation Dialog -->
  <div v-if="confirmDelete" class="ap-backdrop" @click.self="confirmDelete = false">
    <div role="dialog" aria-modal="true" class="confirm-card">
      <h2 class="card-h2" style="color: var(--danger)">Excluir sessão</h2>
      <p style="margin: 8px 0 18px; font-size: 13.5px; color: var(--text-2)">
        {{ current?.name }} ({{ current?.id }})
      </p>
      <div style="display: flex; justify-content: flex-end; gap: 10px">
        <button type="button" class="btn" @click="confirmDelete = false">
          {{ t('session.close') }}
        </button>
        <button type="button" class="btn btn-danger" :disabled="busyAction" @click="handleDelete">
          Excluir sessão
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.split-shell {
  flex: 1;
  display: grid;
  grid-template-columns: 260px 1fr;
  min-height: 0;
}

.sidebar {
  border-right: 1px solid var(--border);
  background: var(--sunken);
  display: flex;
  flex-direction: column;
  padding: 14px;
  gap: 10px;
}

.side-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.side-title {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
}

.side-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
}

.side-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid transparent;
  color: var(--text-2);
  text-decoration: none;
  font-size: 13.5px;
  font-weight: 600;
}

.side-item.active {
  background: var(--sel);
  border-color: var(--sel-border);
  color: var(--text);
}

.side-item-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.side-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--track-off);
  flex: none;
}

.side-dot.on {
  background: var(--dot);
}

.side-dot.warn {
  background: #FFA800;
}

.side-empty {
  font-size: 13px;
  color: var(--muted);
  padding: 8px 4px;
}

.side-foot {
  border-top: 1px solid var(--border);
  padding-top: 10px;
}

.side-link {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  color: var(--text-2);
  text-decoration: none;
  font-size: 13px;
  font-weight: 600;
}

.detail-pane {
  padding: 28px 32px 64px;
  min-width: 0;
}

.detail-content {
  max-width: 820px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}

.detail-title-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.detail-h1 {
  margin: 0;
  font-size: 24px;
  font-weight: 800;
}

.muted-line {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--muted);
}

.detail-btns {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.card-block {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.card-block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card-h2 {
  margin: 0;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
}

.kv-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.kv-label {
  width: 40px;
  font-size: 12px;
  font-weight: 700;
  color: var(--muted);
}

.kv-val {
  flex: 1;
  min-width: 160px;
  font-size: 13px;
  color: var(--text);
  background: var(--sunken);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 10px;
  overflow-x: auto;
}

.int-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  border-top: 1px solid var(--border);
}

.int-row:first-of-type {
  border-top: 0;
  padding-top: 0;
}

.int-copy {
  flex: 1;
  min-width: 0;
}

.int-title {
  font-size: 14px;
  font-weight: 600;
}

.int-sub {
  font-size: 12.5px;
  color: var(--muted);
}

.quick-conn {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.send-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.form-row {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.form-input {
  flex: 1 1 200px;
  height: 40px;
  padding: 0 12px;
  border-radius: 10px;
  border: 1px solid var(--border-strong);
  background: var(--sunken);
  color: var(--text);
  font-size: 13.5px;
}

.send-msg {
  margin: 0;
  font-size: 13px;
}

.send-msg.ok {
  color: var(--accent);
}

.send-msg.err {
  color: var(--danger);
}

.state-box {
  padding: 36px;
  text-align: center;
  color: var(--muted);
}

.state-box.err {
  color: var(--danger);
}

.ap-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  z-index: 60;
}

.ap {
  position: fixed;
  right: 24px;
  bottom: 24px;
  width: 340px;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  box-shadow: var(--shadow);
  overflow: hidden;
}

.ap-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
}

.ap-input {
  flex: 1;
  background: transparent;
  border: 0;
  color: var(--text);
  font-size: 13.5px;
}

.ap-body {
  padding: 8px;
  max-height: 360px;
  overflow-y: auto;
}

.ap-group {
  padding: 8px 10px 4px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--muted);
}

.ap-group.danger {
  color: var(--danger);
}

.ap-row {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  font: 500 13.5px 'Montserrat', sans-serif;
  cursor: pointer;
}

.ap-row.danger {
  color: var(--danger);
}

.ap-empty {
  padding: 16px;
  text-align: center;
  font-size: 13px;
  color: var(--muted);
}

.confirm-card {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: calc(100% - 32px);
  max-width: 400px;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  padding: 20px;
  box-shadow: var(--shadow);
}

.restriction-banner {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 14px 16px;
  border-radius: 12px;
  border: 1px solid rgba(245, 158, 11, 0.45);
  background: rgba(245, 158, 11, 0.1);
  color: var(--text);
}

.restriction-icon {
  color: var(--amber);
  flex: none;
  margin-top: 2px;
}

.restriction-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.restriction-title {
  font-size: 13px;
  font-weight: 700;
}

.restriction-desc {
  margin: 0;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-2);
}

.restriction-code {
  font-size: 11px;
  color: var(--amber);
}

@media (max-width: 780px) {
  .split-shell {
    grid-template-columns: 1fr;
  }
  .sidebar {
    border-right: 0;
    border-bottom: 1px solid var(--border);
  }
  .detail-pane {
    padding: 20px 14px 48px;
  }
}
</style>
