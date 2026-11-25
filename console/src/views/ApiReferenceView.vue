<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import yaml from 'js-yaml';
import { Check, Copy, ExternalLink, Play, Shield, Unlock } from 'lucide-vue-next';
import TopBar from '../components/TopBar.vue';
import { useApi } from '../composables/useApi';
import { useAuth } from '../composables/useAuth';
import { MOD_KEY, isMacOS } from '../composables/useHotkeys';
import { normalizeText } from '../composables/usePhone';

interface OpenApiParam {
  name: string;
  in: string;
  required?: boolean;
  description?: string;
}

interface EndpointItem {
  id: string;
  tag: string;
  method: string;
  path: string;
  summary: string;
  description?: string;
  isPublic: boolean;
  isWebhook?: boolean;
  params: OpenApiParam[];
  hasBody: boolean;
  sampleBody: string;
  responses: Array<{ code: string; description: string }>;
}

const route = useRoute();
const { t } = useI18n();
const api = useApi();
const { apiKey } = useAuth();

const endpoints = ref<EndpointItem[]>([]);
const loading = ref(true);
const loadError = ref<string | null>(null);
const filter = ref(typeof route.query.tag === 'string' ? route.query.tag : '');
const selectedId = ref<string>('');
const codeLang = ref<'curl' | 'js' | 'python' | 'php'>('curl');
const copiedPath = ref(false);
const copiedSpecUrl = ref(false);

async function copyOpenApiJsonUrl() {
  try {
    await navigator.clipboard.writeText(`${baseUrl.value}/openapi.json`);
    copiedSpecUrl.value = true;
    setTimeout(() => (copiedSpecUrl.value = false), 1800);
  } catch {
    // ignore clipboard errors
  }
}

// Live execution state
const paramValues = ref<Record<string, string>>({});
const bodyText = ref('');
const running = ref(false);
const liveResp = ref<{ status: number; ms: number; body: string } | null>(null);

const baseUrl = computed(() =>
  typeof window !== 'undefined' ? window.location.origin : 'http://localhost:8080',
);

const filteredEndpoints = computed(() => {
  const q = normalizeText(filter.value.trim());
  if (!q) return endpoints.value;
  return endpoints.value.filter((e) =>
    normalizeText(`${e.method} ${e.path} ${e.summary} ${e.tag}`).includes(q),
  );
});

const groupedEndpoints = computed(() => {
  const map = new Map<string, EndpointItem[]>();
  for (const ep of filteredEndpoints.value) {
    const arr = map.get(ep.tag) ?? [];
    arr.push(ep);
    map.set(ep.tag, arr);
  }
  return Array.from(map.entries()).map(([tag, items]) => ({ tag, items }));
});

const selected = computed(
  () => endpoints.value.find((e) => e.id === selectedId.value) ?? filteredEndpoints.value[0] ?? null,
);

watch(selected, (ep) => {
  liveResp.value = null;
  if (!ep) return;
  const nextParams: Record<string, string> = {};
  for (const p of ep.params) {
    nextParams[p.name] = paramValues.value[p.name] ?? '';
  }
  paramValues.value = nextParams;
  bodyText.value = ep.sampleBody;
});

function resolvePath(ep: EndpointItem): string {
  let out = ep.path;
  for (const p of ep.params) {
    if (p.in === 'path') {
      const val = paramValues.value[p.name]?.trim() || `{${p.name}}`;
      out = out.replace(`{${p.name}}`, encodeURIComponent(val));
    }
  }
  return out;
}

const codeSnippet = computed(() => {
  const ep = selected.value;
  if (!ep) return '';
  if (ep.isWebhook) {
    return `// Verify X-Relay-Signature: sha256=HMAC_SHA256(secret, \`\${timestamp}.\${rawBody}\`)
import crypto from 'node:crypto';

const expected = 'sha256=' + crypto
  .createHmac('sha256', process.env.WEBHOOK_SECRET)
  .update(\`\${req.headers['x-relay-timestamp']}.\${rawBody}\`)
  .digest('hex');

const valid = crypto.timingSafeEqual(
  Buffer.from(req.headers['x-relay-signature']),
  Buffer.from(expected),
);`;
  }

  const url = `${baseUrl.value}${resolvePath(ep)}`;
  const authHeader = ep.isPublic ? '' : `Authorization: Bearer $RELAY_API_KEY`;

  if (codeLang.value === 'curl') {
    const lines = [`curl -X ${ep.method} "${url}"`];
    if (authHeader) lines.push(`  -H "${authHeader}"`);
    if (ep.hasBody) {
      lines.push(`  -H "Content-Type: application/json"`);
      lines.push(`  -d '${bodyText.value.replace(/\n/g, ' ')}'`);
    }
    return lines.join(' \\\n');
  }

  if (codeLang.value === 'js') {
    return `const res = await fetch("${url}", {
  method: "${ep.method}",${
      ep.isPublic && !ep.hasBody
        ? ''
        : `
  headers: {${ep.isPublic ? '' : '\n    Authorization: `Bearer ${process.env.RELAY_API_KEY}`,'}${ep.hasBody ? '\n    "Content-Type": "application/json",' : ''}
  },`
    }${ep.hasBody ? `\n  body: JSON.stringify(${bodyText.value}),` : ''}
});
console.log(res.status, await res.text());`;
  }

  if (codeLang.value === 'python') {
    return `import os, requests

res = requests.request(
    "${ep.method}",
    "${url}",${ep.isPublic ? '' : '\n    headers={"Authorization": f"Bearer {os.environ[\'RELAY_API_KEY\']}"},'}${ep.hasBody ? `\n    json=${bodyText.value},` : ''}
)
print(res.status_code, res.text)`;
  }

  return `<?php
$ch = curl_init("${url}");
curl_setopt($ch, CURLOPT_CUSTOMREQUEST, "${ep.method}");
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);${
    ep.isPublic
      ? ''
      : `\ncurl_setopt($ch, CURLOPT_HTTPHEADER, [\n    "Authorization: Bearer " . getenv("RELAY_API_KEY"),${ep.hasBody ? '\n    "Content-Type: application/json",' : ''}\n]);`
  }${ep.hasBody ? `\ncurl_setopt($ch, CURLOPT_POSTFIELDS, '${bodyText.value.replace(/'/g, "\\'")}');` : ''}
$resp = curl_exec($ch);
echo $resp;`;
});

async function runEndpoint() {
  const ep = selected.value;
  if (!ep || ep.isWebhook) return;
  if (['POST', 'PATCH', 'DELETE', 'PUT'].includes(ep.method)) {
    if (!window.confirm(`${ep.method} ${resolvePath(ep)} — execute against this server?`)) {
      return;
    }
  }

  running.value = true;
  liveResp.value = null;
  const started = performance.now();
  try {
    const headers: Record<string, string> = {};
    if (!ep.isPublic && apiKey.value) {
      headers['Authorization'] = `Bearer ${apiKey.value}`;
    }
    let body: string | undefined;
    if (ep.hasBody && bodyText.value.trim()) {
      headers['Content-Type'] = 'application/json';
      body = bodyText.value;
    }
    const res = await fetch(resolvePath(ep), {
      method: ep.method,
      headers,
      body,
    });
    const text = await res.text();
    let pretty = text;
    try {
      pretty = JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      // keep raw text (e.g., SVG)
    }
    liveResp.value = {
      status: res.status,
      ms: Math.round(performance.now() - started),
      body: pretty || '(empty)',
    };
  } catch (err) {
    liveResp.value = {
      status: 0,
      ms: Math.round(performance.now() - started),
      body: err instanceof Error ? err.message : 'Request failed',
    };
  } finally {
    running.value = false;
  }
}

async function copyEndpointPath() {
  if (!selected.value) return;
  try {
    await navigator.clipboard.writeText(selected.value.path);
    copiedPath.value = true;
    window.setTimeout(() => (copiedPath.value = false), 1600);
  } catch {
    // ignore
  }
}

function buildSampleFromSchema(schema: Record<string, unknown> | undefined, root: Record<string, unknown>): unknown {
  if (!schema) return {};
  if (typeof schema.$ref === 'string') {
    const name = schema.$ref.split('/').pop() || '';
    const schemas = (root.components as Record<string, Record<string, Record<string, unknown>>> | undefined)?.schemas;
    return buildSampleFromSchema(schemas?.[name], root);
  }
  if (schema.type === 'object' && schema.properties && typeof schema.properties === 'object') {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(schema.properties as Record<string, Record<string, unknown>>)) {
      if (v.example !== undefined) out[k] = v.example;
      else if (v.type === 'boolean') out[k] = true;
      else if (v.type === 'integer') out[k] = 0;
      else if (v.type === 'array') out[k] = [];
      else out[k] = k === 'to' ? '5511999999999' : k === 'text' ? t('apiReference.hello') : '';
    }
    return out;
  }
  return {};
}

async function loadSpec() {
  loading.value = true;
  loadError.value = null;
  try {
    const raw = await api.getOpenApiYaml();
    const doc = yaml.load(raw) as Record<string, unknown>;
    const paths = (doc.paths as Record<string, Record<string, unknown>>) || {};
    const compParams =
      ((doc.components as Record<string, unknown> | undefined)?.parameters as Record<
        string,
        OpenApiParam
      >) || {};

    const list: EndpointItem[] = [];
    for (const [pathStr, pathItem] of Object.entries(paths)) {
      const inheritedParams = ((pathItem.parameters as Array<Record<string, unknown>>) || []).map(
        (p) => {
          if (typeof p.$ref === 'string') {
            return compParams[p.$ref.split('/').pop() || ''] ?? { name: 'id', in: 'path' };
          }
          return p as unknown as OpenApiParam;
        },
      );

      for (const method of ['get', 'post', 'patch', 'put', 'delete']) {
        const op = pathItem[method] as Record<string, unknown> | undefined;
        if (!op) continue;
        const tags = (op.tags as string[]) || ['General'];
        const isPublic = Array.isArray(op.security) && op.security.length === 0;
        const reqBody = op.requestBody as
          | { content?: { 'application/json'?: { schema?: Record<string, unknown> } } }
          | undefined;
        const hasBody = !!reqBody?.content?.['application/json'];
        const sample = hasBody
          ? JSON.stringify(
              buildSampleFromSchema(reqBody?.content?.['application/json']?.schema, doc),
              null,
              2,
            )
          : '';
        const responsesObj = (op.responses as Record<string, { description?: string }>) || {};
        const responses = Object.entries(responsesObj).map(([code, r]) => ({
          code,
          description: r?.description || '',
        }));

        list.push({
          id: `${method}-${pathStr}`,
          tag: tags[0],
          method: method.toUpperCase(),
          path: pathStr,
          summary: String(op.summary || pathStr),
          description: op.description ? String(op.description) : undefined,
          isPublic,
          params: inheritedParams,
          hasBody,
          sampleBody: sample,
          responses,
        });
      }
    }

    // Add webhooks.relayEvent entry from OpenAPI 3.2.1 spec
    const webhooksObj = doc.webhooks as Record<string, Record<string, Record<string, unknown>>> | undefined;
    if (webhooksObj?.relayEvent?.post) {
      const wh = webhooksObj.relayEvent.post;
      list.push({
        id: 'webhook-relayEvent',
        tag: 'Events',
        method: 'POST',
        path: t('apiReference.yourUrl'),
        summary: String(wh.summary || 'Webhook delivery'),
        description: wh.description ? String(wh.description) : undefined,
        isPublic: false,
        isWebhook: true,
        params: [],
        hasBody: false,
        sampleBody: '',
        responses: [{ code: '2xx', description: t('apiReference.r.2xx') }],
      });
    }

    endpoints.value = list;
    if (list.length > 0) {
      selectedId.value = filteredEndpoints.value[0]?.id ?? list[0].id;
    }
  } catch (err) {
    loadError.value = err instanceof Error ? err.message : 'Failed to load /openapi.yaml';
  } finally {
    loading.value = false;
  }
}

function onKeyDown(e: KeyboardEvent) {
  const mod = isMacOS() ? e.metaKey : e.ctrlKey;
  if (mod && e.key === 'Enter') {
    e.preventDefault();
    runEndpoint();
  }
}

onMounted(() => {
  loadSpec();
  window.addEventListener('keydown', onKeyDown);
});

onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown);
});
</script>

<template>
  <TopBar />

  <div class="api-shell">
    <aside class="api-side" :aria-label="t('apiReference.endpoints')">
      <label for="api-filter" class="sr">{{ t('apiReference.filter') }}</label>
      <input
        id="api-filter"
        v-model="filter"
        type="text"
        class="inp side-filter"
        :placeholder="t('apiReference.filter')"
      />

      <div class="spec-box">
        <div class="spec-head">
          <span class="spec-title">{{ t('apiReference.specTitle') }}</span>
          <button
            type="button"
            class="spec-copy"
            :title="t('apiReference.copySpecUrl')"
            @click="copyOpenApiJsonUrl"
          >
            <Check v-if="copiedSpecUrl" :size="12" aria-hidden="true" />
            <Copy v-else :size="12" aria-hidden="true" />
            <span>{{ copiedSpecUrl ? t('connect.copied') : t('apiReference.copySpecUrl') }}</span>
          </button>
        </div>
        <div class="spec-links">
          <a href="/openapi.json" target="_blank" rel="noopener" class="spec-pill mono">
            <span>openapi.json</span>
            <ExternalLink :size="11" aria-hidden="true" />
          </a>
          <a href="/openapi.yaml" target="_blank" rel="noopener" class="spec-pill mono">
            <span>openapi.yaml</span>
            <ExternalLink :size="11" aria-hidden="true" />
          </a>
        </div>
      </div>

      <div v-if="loading" class="side-msg">…</div>
      <div v-else-if="loadError" role="alert" class="side-msg err">{{ loadError }}</div>
      <div v-else-if="groupedEndpoints.length === 0" class="side-msg">
        {{ t('apiReference.noMatch') }}
      </div>
      <div v-else class="side-groups">
        <div v-for="g in groupedEndpoints" :key="g.tag" class="side-group">
          <div class="group-title">{{ g.tag }}</div>
          <button
            v-for="ep in g.items"
            :key="ep.id"
            type="button"
            class="ep-btn hov"
            :class="{ active: selected?.id === ep.id }"
            @click="selectedId = ep.id"
          >
            <span class="m-tag mono" :class="`m-${ep.method.toLowerCase()}`">{{ ep.method }}</span>
            <span class="ep-path mono">{{ ep.path }}</span>
          </button>
        </div>
      </div>
    </aside>

    <main class="api-detail">
      <div v-if="selected" class="api-content">
        <div class="ep-head">
          <div class="ep-title-row">
            <span class="m-tag lg mono" :class="`m-${selected.method.toLowerCase()}`">
              {{ selected.method }}
            </span>
            <code class="mono ep-full-path">{{ selected.path }}</code>
            <button type="button" class="btn" @click="copyEndpointPath">
              <Check v-if="copiedPath" :size="14" aria-hidden="true" />
              <Copy v-else :size="14" aria-hidden="true" />
              <span>{{ copiedPath ? t('connect.copied') : t('apiReference.copy') }}</span>
            </button>
          </div>
          <h1 class="ep-h1">{{ selected.summary }}</h1>
          <p v-if="selected.description" class="ep-desc">{{ selected.description }}</p>
          <span class="badge-pill" :class="selected.isPublic ? 'cyan' : ''">
            <Unlock v-if="selected.isPublic" :size="13" aria-hidden="true" />
            <Shield v-else :size="13" aria-hidden="true" />
            <span>{{ selected.isPublic ? t('apiReference.public') : t('apiReference.auth') }}</span>
          </span>
        </div>

        <!-- Path / Header Parameters -->
        <section v-if="selected.params.length > 0" class="api-card">
          <h2 class="sec-h2">{{ t('apiReference.params') }}</h2>
          <div v-for="p in selected.params" :key="p.name" class="param-row">
            <label :for="`p-${p.name}`" class="param-label mono">
              {{ p.name }}
              <span v-if="p.required" class="req">*</span>
              <span class="in-tag">({{ p.in }})</span>
            </label>
            <input
              :id="`p-${p.name}`"
              v-model="paramValues[p.name]"
              class="inp param-input mono"
              :placeholder="p.name"
            />
          </div>
        </section>

        <!-- Request Body -->
        <section v-if="selected.hasBody" class="api-card">
          <h2 class="sec-h2">{{ t('apiReference.body') }}</h2>
          <label for="req-body" class="sr">{{ t('apiReference.body') }}</label>
          <textarea
            id="req-body"
            v-model="bodyText"
            rows="6"
            class="code-block body-editor"
            spellcheck="false"
          ></textarea>
        </section>

        <!-- Code Example + Run -->
        <section class="api-card">
          <div class="code-head">
            <div role="tablist" :aria-label="t('apiReference.language2')" class="lang-tabs">
              <button
                v-for="l in (['curl', 'js', 'python', 'php'] as const)"
                :key="l"
                type="button"
                role="tab"
                :aria-selected="codeLang === l"
                class="lang-tab"
                :class="{ active: codeLang === l }"
                @click="codeLang = l"
              >
                {{ l === 'js' ? 'JavaScript' : l.toUpperCase() }}
              </button>
            </div>

            <button
              v-if="!selected.isWebhook"
              type="button"
              class="btn btn-primary"
              :disabled="running"
              @click="runEndpoint"
            >
              <Play :size="14" aria-hidden="true" />
              <span>{{ t('apiReference.run') }}</span>
              <span class="kbd hide-sm">{{ MOD_KEY }}↵</span>
            </button>
          </div>
          <pre class="code-block"><code>{{ codeSnippet }}</code></pre>

          <div v-if="liveResp" class="live-resp" role="status">
            <div class="live-resp-head">
              <strong>{{ t('apiReference.liveRes') }}</strong>
              <span class="badge-pill" :class="liveResp.status >= 200 && liveResp.status < 300 ? 'cyan' : 'danger'">
                HTTP {{ liveResp.status }} · {{ liveResp.ms }} ms
              </span>
            </div>
            <pre class="code-block"><code>{{ liveResp.body }}</code></pre>
          </div>
        </section>

        <!-- Responses -->
        <section class="api-card">
          <h2 class="sec-h2">{{ t('apiReference.responses') }}</h2>
          <div v-for="r in selected.responses" :key="r.code" class="resp-row">
            <span class="kbd mono">{{ r.code }}</span>
            <span class="resp-desc">{{ r.description }}</span>
          </div>
        </section>
      </div>
    </main>
  </div>
</template>

<style scoped>
.api-shell {
  flex: 1;
  display: grid;
  grid-template-columns: 300px 1fr;
  min-height: 0;
}

.api-side {
  border-right: 1px solid var(--border);
  background: var(--sunken);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

.side-filter {
  height: 38px;
  padding: 0 12px;
  border-radius: 10px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  color: var(--text);
  font-size: 13px;
}

.spec-box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--surface);
}

.spec-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}

.spec-title {
  font-size: 11px;
  font-weight: 700;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.spec-copy {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 7px;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--accent);
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
}

.spec-copy:hover {
  background: var(--tint-cyan);
}

.spec-links {
  display: flex;
  gap: 6px;
}

.spec-pill {
  flex: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  padding: 5px 8px;
  border-radius: 7px;
  border: 1px solid var(--border);
  background: var(--sunken);
  color: var(--text-2);
  font-size: 11px;
  text-decoration: none;
}

.spec-pill:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.side-groups {
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}

.group-title {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--muted);
  padding: 4px 8px;
}

.ep-btn {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  text-align: left;
  cursor: pointer;
}

.ep-btn.active {
  background: var(--sel);
  border-color: var(--sel-border);
  color: var(--text);
}

.m-tag {
  font-size: 10px;
  font-weight: 700;
  padding: 3px 6px;
  border-radius: 5px;
  min-width: 44px;
  text-align: center;
  flex: none;
}

.m-tag.lg {
  font-size: 12px;
  padding: 5px 10px;
}

.m-get {
  background: var(--tint-cyan);
  color: var(--accent);
}

.m-post {
  background: var(--tint-orange);
  color: var(--orange);
}

.m-patch,
.m-put {
  background: var(--tint-amber);
  color: var(--amber);
}

.m-delete {
  background: rgba(239, 68, 68, 0.16);
  color: var(--danger);
}

.ep-path {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.side-msg {
  font-size: 13px;
  color: var(--muted);
  padding: 12px 6px;
}

.side-msg.err {
  color: var(--danger);
}

.api-detail {
  padding: 28px 32px 64px;
  min-width: 0;
}

.api-content {
  max-width: 820px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.ep-head {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
}

.ep-title-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.ep-full-path {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
  word-break: break-all;
}

.ep-h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 800;
}

.ep-desc {
  margin: 0;
  font-size: 14px;
  color: var(--muted);
  line-height: 1.55;
}

.api-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

.sec-h2 {
  margin: 0;
  font-size: 12px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--muted);
}

.param-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.param-label {
  min-width: 150px;
  font-size: 13px;
  font-weight: 600;
}

.req {
  color: var(--orange);
}

.in-tag {
  font-size: 11px;
  color: var(--muted);
}

.param-input {
  flex: 1;
  min-width: 180px;
  height: 38px;
  padding: 0 12px;
  border-radius: 8px;
  border: 1px solid var(--border-strong);
  background: var(--sunken);
  color: var(--text);
  font-size: 13px;
}

.body-editor {
  width: 100%;
  resize: vertical;
}

.code-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.lang-tabs {
  display: flex;
  gap: 4px;
  padding: 3px;
  border-radius: 10px;
  background: var(--sunken);
  border: 1px solid var(--border);
}

.lang-tab {
  border: 0;
  background: transparent;
  color: var(--muted);
  font: 700 12px 'Montserrat', sans-serif;
  padding: 6px 10px;
  border-radius: 7px;
  cursor: pointer;
}

.lang-tab.active {
  background: var(--sel);
  color: var(--text);
}

.live-resp {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
}

.live-resp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
}

.resp-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-2);
}

@media (max-width: 820px) {
  .api-shell {
    grid-template-columns: 1fr;
  }
  .api-side {
    border-right: 0;
    border-bottom: 1px solid var(--border);
  }
  .api-detail {
    padding: 20px 14px 48px;
  }
}
</style>
