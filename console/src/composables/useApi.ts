import { useAuth } from './useAuth';

export interface HealthResponse {
  status: string;
  database: string;
  media_storage: string;
  transcription_enabled: boolean;
  transcription_available: boolean;
  active_sessions: number;
}

export interface SessionRecord {
  id: string;
  name: string;
  status: string;
  status_reason?: string | null;
  phone_number: string | null;
  jid: string | null;
  transcription_language: string | null;
  created_at: string;
  updated_at: string;
}

export interface SessionQrResponse {
  session_id: string;
  qr: {
    code: string;
    expires_at: string;
  } | null;
}

export interface SessionStatusResponse {
  session_id: string;
  status: string;
  status_reason?: string | null;
  phone_number: string | null;
  jid: string | null;
}

export interface MessageRecord {
  id: string;
  session_id: string;
  external_id: string | null;
  direction: string;
  chat_jid: string;
  sender_jid: string | null;
  message_type: string;
  text_body: string | null;
  caption: string | null;
  media_id: string | null;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface WebhookRecord {
  id: string;
  url: string;
  secret?: string | null;
  session_id: string | null;
  events: string[];
  enabled: boolean;
  description: string | null;
  created_at: string;
  updated_at: string;
}

export class ApiError extends Error {
  status: number;
  code?: string;

  constructor(status: number, message: string, code?: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

let unauthorizedHandler: (() => void) | null = null;

export function setUnauthorizedHandler(fn: () => void) {
  unauthorizedHandler = fn;
}

export function useApi() {
  const { apiKey, clearApiKey } = useAuth();

  async function request<T>(
    path: string,
    options: {
      method?: string;
      body?: unknown;
      headers?: Record<string, string>;
      auth?: boolean;
      overrideKey?: string;
      rawText?: boolean;
    } = {},
  ): Promise<T> {
    const method = options.method ?? 'GET';
    const headers: Record<string, string> = { ...(options.headers ?? {}) };

    if (options.auth !== false) {
      const token = options.overrideKey ?? apiKey.value;
      if (token) {
        headers['Authorization'] = `Bearer ${token}`;
      }
    }

    let body: string | undefined;
    if (options.body !== undefined) {
      headers['Content-Type'] = 'application/json';
      body = JSON.stringify(options.body);
    }

    const res = await fetch(path, { method, headers, body });

    if (res.status === 401 && options.auth !== false && !options.overrideKey) {
      clearApiKey();
      if (unauthorizedHandler) unauthorizedHandler();
      throw new ApiError(401, 'Unauthorized', 'unauthorized');
    }

    if (res.status === 204) {
      return undefined as T;
    }

    if (!res.ok) {
      let msg = res.statusText || `HTTP ${res.status}`;
      let code: string | undefined;
      try {
        const errJson = await res.json();
        if (errJson?.message) msg = errJson.message;
        if (errJson?.error) code = errJson.error;
      } catch {
        // ignore parse error
      }
      throw new ApiError(res.status, msg, code);
    }

    if (options.rawText) {
      return (await res.text()) as T;
    }

    return (await res.json()) as T;
  }

  return {
    request,
    getHealth: () => request<HealthResponse>('/v1/health', { auth: false }),
    verifyKey: (key: string) =>
      request<{ sessions: SessionRecord[] }>('/v1/sessions', { overrideKey: key }),
    listSessions: () => request<{ sessions: SessionRecord[] }>('/v1/sessions'),
    createSession: (name: string, transcriptionLanguage?: string | null) =>
      request<SessionRecord>('/v1/sessions', {
        method: 'POST',
        body: { name, transcription_language: transcriptionLanguage ?? null },
      }),
    getSession: (id: string) => request<SessionRecord>(`/v1/sessions/${encodeURIComponent(id)}`),
    getSessionQr: (id: string) =>
      request<SessionQrResponse>(`/v1/sessions/${encodeURIComponent(id)}/qr`),
    getSessionQrSvg: (id: string) =>
      request<string>(`/v1/sessions/${encodeURIComponent(id)}/qr.svg`, { rawText: true }),
    requestPairCode: (id: string, phoneNumber: string) =>
      request<{
        session_id: string;
        code: string;
        formatted_code: string;
        expires_at: string;
      }>(`/v1/sessions/${encodeURIComponent(id)}/pair-code`, {
        method: 'POST',
        body: { phone_number: phoneNumber },
      }),
    createPairLink: (id: string, ttlSeconds?: number) =>
      request<{
        session_id: string;
        token: string;
        pair_path: string;
        qr_svg_path: string;
        qr_png_path: string;
        status_path: string;
        expires_at: string;
      }>(`/v1/sessions/${encodeURIComponent(id)}/pair-link`, {
        method: 'POST',
        body: ttlSeconds ? { ttl_seconds: ttlSeconds } : {},
      }),
    getSessionStatus: (id: string) =>
      request<SessionStatusResponse>(`/v1/sessions/${encodeURIComponent(id)}/status`),
    reconnectSession: (id: string) =>
      request<SessionRecord>(`/v1/sessions/${encodeURIComponent(id)}/reconnect`, {
        method: 'POST',
      }),
    logoutSession: (id: string) =>
      request<{ session_id: string; status: string }>(
        `/v1/sessions/${encodeURIComponent(id)}/logout`,
        { method: 'POST' },
      ),
    deleteSession: (id: string) =>
      request<void>(`/v1/sessions/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    sendTextMessage: (sessionId: string, to: string, text: string, idempotencyKey?: string) =>
      request<MessageRecord>(`/v1/sessions/${encodeURIComponent(sessionId)}/messages/text`, {
        method: 'POST',
        body: { to, text },
        headers: idempotencyKey ? { 'Idempotency-Key': idempotencyKey } : undefined,
      }),
    listWebhooks: () => request<{ webhooks: WebhookRecord[] }>('/v1/webhooks'),
    createWebhook: (payload: {
      url: string;
      secret?: string | null;
      session_id?: string | null;
      events: string[];
      enabled?: boolean;
      description?: string | null;
    }) =>
      request<WebhookRecord>('/v1/webhooks', {
        method: 'POST',
        body: payload,
      }),
    updateWebhook: (
      id: string,
      payload: {
        url?: string;
        secret?: string;
        events?: string[];
        enabled?: boolean;
        description?: string | null;
      },
    ) =>
      request<WebhookRecord>(`/v1/webhooks/${encodeURIComponent(id)}`, {
        method: 'PATCH',
        body: payload,
      }),
    deleteWebhook: (id: string) =>
      request<void>(`/v1/webhooks/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    getOpenApiYaml: () => request<string>('/openapi.yaml', { rawText: true }),
  };
}
