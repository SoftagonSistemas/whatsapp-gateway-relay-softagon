import { ref } from 'vue';
import { useApi, type HealthResponse, type SessionRecord } from './useApi';

const sessions = ref<SessionRecord[]>([]);
const sessionsLoading = ref(false);
const sessionsError = ref<string | null>(null);
const health = ref<HealthResponse | null>(null);
const healthOnline = ref<boolean | null>(null);

export function useSessions() {
  const api = useApi();

  async function fetchHealth() {
    try {
      health.value = await api.getHealth();
      healthOnline.value = health.value.status === 'ok';
    } catch {
      healthOnline.value = false;
    }
    return health.value;
  }

  async function fetchSessions() {
    sessionsLoading.value = true;
    sessionsError.value = null;
    try {
      const res = await api.listSessions();
      sessions.value = res.sessions ?? [];
    } catch (err) {
      sessionsError.value = err instanceof Error ? err.message : 'Failed to load sessions';
    } finally {
      sessionsLoading.value = false;
    }
    return sessions.value;
  }

  return {
    sessions,
    sessionsLoading,
    sessionsError,
    health,
    healthOnline,
    fetchHealth,
    fetchSessions,
  };
}
