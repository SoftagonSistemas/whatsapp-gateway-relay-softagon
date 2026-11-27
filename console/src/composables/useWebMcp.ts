import { onBeforeUnmount, watch } from 'vue';
import type { Router } from 'vue-router';
import {
  LOCALE_OPTIONS,
  setAppLocale,
  type SupportedLocale,
} from '../i18n';
import { useAuth } from './useAuth';
import { useApi } from './useApi';
import { useTheme } from './useTheme';
import { useSessions } from './useSessions';

function getModelContext(): ModelContext | null {
  if (typeof document !== 'undefined' && document.modelContext) {
    return document.modelContext;
  }
  if (typeof navigator !== 'undefined' && navigator.modelContext) {
    return navigator.modelContext;
  }
  return null;
}

/**
 * Registers Softagon Relay Web Console client-side tools with the browser's
 * WebMCP (`document.modelContext` / `navigator.modelContext`) API.
 *
 * Lifecycle follows Chrome Modern Web Guidance:
 * - Tools are registered with an `AbortSignal` and torn down automatically via `AbortController`.
 * - Authenticated tools are registered only while `isAuthenticated` is true and unregistered on logout.
 * - Destructive actions (logout/delete) and API keys are intentionally never exposed.
 */
export function useWebMcp(router: Router) {
  const { isAuthenticated } = useAuth();
  const api = useApi();
  const { setTheme } = useTheme();
  const { fetchSessions, fetchHealth } = useSessions();

  let baseController: AbortController | null = null;
  let authController: AbortController | null = null;

  const supportedCodes = LOCALE_OPTIONS.map((l) => l.code);

  function registerSafe(mc: ModelContext, tool: WebMcpToolDefinition, signal: AbortSignal) {
    try {
      const maybePromise = mc.registerTool(tool, { signal });
      if (maybePromise && typeof (maybePromise as Promise<void>).catch === 'function') {
        (maybePromise as Promise<void>).catch(() => {
          // Ignore duplicate/unsupported registration errors in experimental browsers
        });
      }
    } catch {
      // Gracefully ignore if browser WebMCP signature differs
    }
  }

  function registerPublicTools() {
    const mc = getModelContext();
    if (!mc) return;

    baseController?.abort();
    baseController = new AbortController();
    const { signal } = baseController;

    registerSafe(
      mc,
      {
        name: 'relay_get_health',
        title: 'Get Softagon Relay Server Health',
        description:
          'Check live health status of the Softagon Relay WhatsApp gateway, including SQLite status, media storage backend, Whisper availability, and active session count.',
        inputSchema: {
          type: 'object',
          properties: {},
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: async () => {
          const health = await api.getHealth();
          await fetchHealth();
          return health;
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_set_language',
        title: 'Switch Console Language',
        description:
          'Change the Softagon Relay Web Console UI language immediately among the 10 supported locales (pt-BR, en, es, fr, id, bew, su, jv, bbc, ms).',
        inputSchema: {
          type: 'object',
          properties: {
            locale: {
              type: 'string',
              enum: supportedCodes,
              description: 'Target BCP 47 locale code.',
            },
          },
          required: ['locale'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: (input) => {
          const code = String(input.locale ?? '') as SupportedLocale;
          if (!supportedCodes.includes(code)) {
            return { ok: false, error: `Unsupported locale '${code}'. Supported: ${supportedCodes.join(', ')}` };
          }
          setAppLocale(code);
          const meta = LOCALE_OPTIONS.find((l) => l.code === code);
          return { ok: true, locale: code, name: meta?.name ?? code };
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_set_theme',
        title: 'Switch Console Theme',
        description: 'Switch the Softagon Relay Web Console between dark and light mode.',
        inputSchema: {
          type: 'object',
          properties: {
            theme: {
              type: 'string',
              enum: ['dark', 'light'],
              description: 'Desired color theme.',
            },
          },
          required: ['theme'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: (input) => {
          const next = input.theme === 'light' ? 'light' : 'dark';
          setTheme(next);
          return { ok: true, theme: next };
        },
      },
      signal,
    );
  }

  function registerAuthTools() {
    authController?.abort();
    authController = null;

    if (!isAuthenticated.value) return;
    const mc = getModelContext();
    if (!mc) return;

    authController = new AbortController();
    const { signal } = authController;

    registerSafe(
      mc,
      {
        name: 'relay_navigate',
        title: 'Navigate Web Console',
        description:
          'Navigate the Softagon Relay Web Console to a specific view: home, connect, sessions, events, or api.',
        inputSchema: {
          type: 'object',
          properties: {
            view: {
              type: 'string',
              enum: ['home', 'connect', 'sessions', 'events', 'api'],
              description: 'Target view name.',
            },
            session_id: {
              type: 'string',
              description: 'Optional session ID when opening the sessions or connect view.',
            },
          },
          required: ['view'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: async (input) => {
          const view = String(input.view ?? 'home');
          const sessionId = typeof input.session_id === 'string' ? input.session_id.trim() : '';
          if (view === 'sessions' && sessionId) {
            await router.push({ name: 'session', params: { id: sessionId } });
          } else if (view === 'sessions') {
            await router.push({ name: 'sessions' });
          } else if (view === 'connect' && sessionId) {
            await router.push({ name: 'connect', query: { session_id: sessionId } });
          } else if (view === 'connect') {
            await router.push({ name: 'connect' });
          } else if (view === 'events') {
            await router.push({ name: 'events' });
          } else if (view === 'api') {
            await router.push({ name: 'api' });
          } else {
            await router.push({ name: 'home' });
          }
          return { ok: true, path: router.currentRoute.value.fullPath };
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_list_sessions',
        title: 'List WhatsApp Sessions',
        description:
          'List all configured WhatsApp sessions with their ID, name, connection status, phone number, JID, and transcription language.',
        inputSchema: {
          type: 'object',
          properties: {},
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: async () => {
          const res = await api.listSessions();
          await fetchSessions();
          return res;
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_get_session_status',
        title: 'Get WhatsApp Session Status',
        description:
          'Get live connection status and any WhatsApp disconnection or restriction reason for a specific session ID.',
        inputSchema: {
          type: 'object',
          properties: {
            session_id: {
              type: 'string',
              description: 'Session ID (UUIDv7).',
            },
          },
          required: ['session_id'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: async (input) => {
          const id = String(input.session_id ?? '').trim();
          if (!id) return { ok: false, error: 'session_id is required' };
          return await api.getSessionStatus(id);
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_list_webhooks',
        title: 'List Webhook Subscriptions',
        description:
          'List configured HMAC-SHA256 webhook subscriptions (URLs, subscribed events, enabled state). Secrets are never returned.',
        inputSchema: {
          type: 'object',
          properties: {},
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: async () => {
          return await api.listWebhooks();
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_create_session',
        title: 'Create WhatsApp Session & Open QR Pairing',
        description:
          'Create a new WhatsApp session and navigate the console to the live QR pairing screen so the user can scan with their phone.',
        inputSchema: {
          type: 'object',
          properties: {
            name: {
              type: 'string',
              description: 'Human-readable session name or phone number (e.g., support-sales).',
            },
            transcription_language: {
              type: 'string',
              description: 'Optional ISO language code for local Whisper transcription (e.g., pt, en, id, jv) or auto.',
            },
          },
          required: ['name'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: async (input) => {
          const name = String(input.name ?? '').trim();
          if (!name) return { ok: false, error: 'name is required' };
          const lang =
            typeof input.transcription_language === 'string' && input.transcription_language.trim()
              ? input.transcription_language.trim()
              : null;
          const created = await api.createSession(name, lang);
          await fetchSessions();
          await router.push({ name: 'connect', query: { session_id: created.id } });
          return created;
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_create_pairing_link',
        title: 'Create Zero-Login WhatsApp Pairing Link',
        description:
          'Generate a temporary, zero-login pairing URL (/pair/{token}) with an auto-renewing QR code and optional 8-digit code tab. Recommended for opening in a browser without exposing RELAY_API_KEY.',
        inputSchema: {
          type: 'object',
          properties: {
            session_id: {
              type: 'string',
              description: 'WhatsApp session ID to pair.',
            },
            ttl_seconds: {
              type: 'number',
              description: 'Optional link lifetime in seconds (60..1800, default 600).',
            },
          },
          required: ['session_id'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: async (input) => {
          const sessionId = String(input.session_id ?? '').trim();
          if (!sessionId) return { ok: false, error: 'session_id is required' };
          const ttl = typeof input.ttl_seconds === 'number' ? input.ttl_seconds : undefined;
          const res = await api.createPairLink(sessionId, ttl);
          const origin = typeof window !== 'undefined' ? window.location.origin : '';
          return {
            ...res,
            pair_url: `${origin}${res.pair_path}`,
            qr_png_url: `${origin}${res.qr_png_path}`,
          };
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_request_pair_code',
        title: 'Request 8-Digit WhatsApp Pairing Code',
        description:
          'Request an 8-character WhatsApp pairing code (XXXX-XXXX) as a camera-free alternative to scanning a QR code.',
        inputSchema: {
          type: 'object',
          properties: {
            session_id: {
              type: 'string',
              description: 'WhatsApp session ID.',
            },
            phone_number: {
              type: 'string',
              description: 'WhatsApp phone number with country code (e.g., 5511999990001).',
            },
          },
          required: ['session_id', 'phone_number'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: async (input) => {
          const sessionId = String(input.session_id ?? '').trim();
          const phone = String(input.phone_number ?? '').trim();
          if (!sessionId || !phone) {
            return { ok: false, error: 'session_id and phone_number are required' };
          }
          return await api.requestPairCode(sessionId, phone);
        },
      },
      signal,
    );

    registerSafe(
      mc,
      {
        name: 'relay_send_text_message',
        title: 'Send WhatsApp Text Message',
        description:
          'Send a WhatsApp text message from a connected session to a recipient phone number (with country code) or WhatsApp JID.',
        inputSchema: {
          type: 'object',
          properties: {
            session_id: {
              type: 'string',
              description: 'Connected session ID.',
            },
            to: {
              type: 'string',
              description: 'Recipient phone number with country code (e.g., +6281234567890) or WhatsApp JID.',
            },
            text: {
              type: 'string',
              description: 'Text message body to send.',
            },
            idempotency_key: {
              type: 'string',
              description: 'Optional idempotency key to prevent duplicate sends.',
            },
          },
          required: ['session_id', 'to', 'text'],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: false },
        execute: async (input) => {
          const sessionId = String(input.session_id ?? '').trim();
          const to = String(input.to ?? '').trim();
          const text = String(input.text ?? '').trim();
          const idempotencyKey =
            typeof input.idempotency_key === 'string' && input.idempotency_key.trim()
              ? input.idempotency_key.trim()
              : undefined;
          if (!sessionId || !to || !text) {
            return { ok: false, error: 'session_id, to, and text are required' };
          }
          return await api.sendTextMessage(sessionId, to, text, idempotencyKey);
        },
      },
      signal,
    );
  }

  registerPublicTools();

  const stopAuthWatch = watch(
    isAuthenticated,
    () => {
      registerAuthTools();
    },
    { immediate: true },
  );

  onBeforeUnmount(() => {
    stopAuthWatch();
    baseController?.abort();
    authController?.abort();
  });
}
