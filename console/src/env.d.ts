/// <reference types="vite/client" />

declare const __APP_VERSION__: string;

interface WebMcpToolAnnotations {
  readOnlyHint?: boolean;
}

interface WebMcpToolDefinition {
  name: string;
  title?: string;
  description: string;
  inputSchema: Record<string, unknown>;
  annotations?: WebMcpToolAnnotations;
  execute: (input: Record<string, unknown>, client?: unknown) => Promise<unknown> | unknown;
}

interface WebMcpRegisterOptions {
  signal?: AbortSignal;
}

interface ModelContext {
  registerTool(tool: WebMcpToolDefinition, options?: WebMcpRegisterOptions): Promise<void> | void;
}

interface Document {
  readonly modelContext?: ModelContext;
}

interface Navigator {
  readonly modelContext?: ModelContext;
}

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<object, object, unknown>;
  export default component;
}
