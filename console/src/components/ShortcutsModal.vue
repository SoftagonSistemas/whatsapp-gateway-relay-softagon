<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { X } from 'lucide-vue-next';
import { MOD_KEY } from '../composables/useHotkeys';

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();
const closeBtn = ref<HTMLButtonElement | null>(null);

const shortcuts = [
  { keys: [`${MOD_KEY}K`], label: () => t('home.paletteLabel') + ' / ' + t('home.actions') },
  { keys: ['/'], label: () => t('common.search') },
  { keys: ['n'], label: () => t('home.newNum') },
  { keys: ['g', 's'], label: () => t('home.sSess') },
  { keys: ['g', 'e'], label: () => t('home.hooks') },
  { keys: ['g', 'a'], label: () => t('common.apiRef') },
  { keys: ['↑', '↓', '↵', 'esc'], label: () => `${t('home.hNav')} · ${t('home.hRun')} · ${t('home.hClear')}` },
  { keys: ['r'], label: () => t('connect.regen') },
  { keys: [`${MOD_KEY}↵`], label: () => t('apiReference.run') },
];

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault();
    emit('close');
  }
}

onMounted(() => {
  closeBtn.value?.focus();
  window.addEventListener('keydown', onKey);
});

onUnmounted(() => {
  window.removeEventListener('keydown', onKey);
});
</script>

<template>
  <div class="modal-backdrop" @click.self="emit('close')">
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-title"
      class="modal-card"
    >
      <div class="modal-head">
        <h2 id="shortcuts-title" style="margin: 0; font-size: 16px; font-weight: 700">
          Softagon Relay · ?
        </h2>
        <button
          ref="closeBtn"
          type="button"
          class="btn btn-icon"
          :aria-label="t('session.close')"
          @click="emit('close')"
        >
          <X :size="16" aria-hidden="true" />
        </button>
      </div>
      <div class="modal-body">
        <div v-for="(s, idx) in shortcuts" :key="idx" class="shortcut-row">
          <span class="shortcut-label">{{ s.label() }}</span>
          <span class="shortcut-keys">
            <span v-for="k in s.keys" :key="k" class="kbd">{{ k }}</span>
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
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
  max-width: 460px;
  background: var(--surface);
  border: 1px solid var(--border);
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

.modal-body {
  padding: 12px 18px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.shortcut-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
  color: var(--text-2);
}

.shortcut-keys {
  display: flex;
  align-items: center;
  gap: 4px;
}
</style>
