<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Check, ChevronDown, Globe } from 'lucide-vue-next';
import { LOCALE_OPTIONS, setAppLocale, type SupportedLocale } from '../i18n';

const { t, locale } = useI18n();

const open = ref(false);
const rootRef = ref<HTMLElement | null>(null);
const triggerRef = ref<HTMLButtonElement | null>(null);
const optionRefs = ref<HTMLButtonElement[]>([]);

const current = computed(
  () => LOCALE_OPTIONS.find((l) => l.code === locale.value) ?? LOCALE_OPTIONS[0],
);

function close(restoreFocus = false) {
  if (!open.value) return;
  open.value = false;
  if (restoreFocus) {
    nextTick(() => triggerRef.value?.focus());
  }
}

function toggle() {
  open.value = !open.value;
  if (open.value) {
    nextTick(() => {
      const idx = LOCALE_OPTIONS.findIndex((l) => l.code === locale.value);
      optionRefs.value[idx >= 0 ? idx : 0]?.focus();
    });
  }
}

function select(code: SupportedLocale) {
  setAppLocale(code);
  close(true);
}

function onOptionKeydown(e: KeyboardEvent, idx: number) {
  if (e.key === 'Escape') {
    e.preventDefault();
    close(true);
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    optionRefs.value[(idx + 1) % LOCALE_OPTIONS.length]?.focus();
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    optionRefs.value[(idx - 1 + LOCALE_OPTIONS.length) % LOCALE_OPTIONS.length]?.focus();
  } else if (e.key === 'Home') {
    e.preventDefault();
    optionRefs.value[0]?.focus();
  } else if (e.key === 'End') {
    e.preventDefault();
    optionRefs.value[LOCALE_OPTIONS.length - 1]?.focus();
  } else if (e.key === 'Tab') {
    close(false);
  }
}

function onOutsidePointer(e: PointerEvent) {
  if (!open.value) return;
  if (rootRef.value && !rootRef.value.contains(e.target as Node)) {
    close(false);
  }
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (open.value && e.key === 'Escape') {
    e.preventDefault();
    close(true);
  }
}

onMounted(() => {
  document.addEventListener('pointerdown', onOutsidePointer);
  document.addEventListener('keydown', onGlobalKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onOutsidePointer);
  document.removeEventListener('keydown', onGlobalKeydown);
});
</script>

<template>
  <div ref="rootRef" class="lang-switcher">
    <button
      ref="triggerRef"
      type="button"
      class="lang-trigger hov"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-label="`${t('common.language')}: ${current.name}`"
      :title="`${t('common.language')} · ${current.name}`"
      @click="toggle"
    >
      <Globe :size="15" aria-hidden="true" class="lang-globe" />
      <span class="lang-code">{{ current.short }}</span>
      <ChevronDown
        :size="14"
        aria-hidden="true"
        class="lang-caret"
        :class="{ open }"
      />
    </button>

    <div
      v-if="open"
      role="listbox"
      :aria-label="t('common.language')"
      class="lang-menu"
    >
      <button
        v-for="(l, idx) in LOCALE_OPTIONS"
        :key="l.code"
        :ref="(el) => { if (el) optionRefs[idx] = el as HTMLButtonElement; }"
        type="button"
        role="option"
        :aria-selected="locale === l.code"
        :lang="l.code"
        class="lang-option"
        :class="{ active: locale === l.code }"
        @click="select(l.code)"
        @keydown="onOptionKeydown($event, idx)"
      >
        <span class="opt-short">{{ l.short }}</span>
        <span class="opt-name">{{ l.name }}</span>
        <Check v-if="locale === l.code" :size="14" aria-hidden="true" class="opt-check" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.lang-switcher {
  position: relative;
}

.lang-trigger {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 38px;
  padding: 0 10px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font: 700 12px 'Montserrat', sans-serif;
  letter-spacing: 0.04em;
  cursor: pointer;
}

.lang-globe {
  color: var(--muted);
  flex: none;
}

.lang-code {
  min-width: 24px;
  text-align: center;
}

.lang-caret {
  color: var(--muted);
  flex: none;
  transition: transform 0.14s ease;
}

.lang-caret.open {
  transform: rotate(180deg);
}

.lang-menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 50;
  min-width: 224px;
  max-height: 440px;
  overflow-y: auto;
  padding: 5px;
  border-radius: 12px;
  border: 1px solid var(--border-strong);
  background: var(--surface-2);
  box-shadow: var(--shadow);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.lang-option {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 8px 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-2);
  font: 500 13px 'Montserrat', sans-serif;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s ease;
}

.lang-option:hover,
.lang-option:focus-visible {
  background: var(--hover);
  color: var(--text);
}

.lang-option.active {
  background: var(--sel);
  color: var(--text);
  font-weight: 600;
}

.opt-short {
  flex: none;
  min-width: 34px;
  padding: 2px 6px;
  border-radius: 5px;
  background: var(--kbd-bg);
  border: 1px solid var(--border);
  font: 700 10.5px 'JetBrains Mono', ui-monospace, monospace;
  letter-spacing: 0.04em;
  text-align: center;
  color: var(--text-2);
}

.lang-option.active .opt-short {
  border-color: var(--sel-border);
  color: var(--accent);
}

.opt-name {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.opt-check {
  flex: none;
  color: var(--accent);
}
</style>
