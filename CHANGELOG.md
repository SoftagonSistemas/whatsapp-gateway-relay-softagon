# Changelog — Softagon Relay

Todas as mudanças relevantes do **Softagon Relay** são documentadas neste arquivo, seguindo o padrão [Keep a Changelog](https://keepachangelog.com/) e [Versionamento Semântico](https://semver.org/).

> **Links rápidos:** [Documentação (PT-BR)](docs/pt-BR/README.md) · [English](docs/en/README.md) · [Español](docs/es/README.md) · [Français](docs/fr/README.md) · [Docker Hub](https://hub.docker.com/r/softagon/relay) · [Softagon Sistemas](https://softagon.com.br)

---

## [0.3.2] — 2026-11-25

### Melhorias — Diagnóstico Amigável de Desconexão e Restrição de Conta
- **Persistência de `status_reason` no SQLite (`migrations/0003_session_status_reason.sql`):** registra motivos normalizados (`device_removed`, `account_restricted`, `logout`, `qr_expired`) e expõe o campo em `GET /v1/sessions`, `GET /v1/sessions/{id}/status` e na ferramenta MCP `get_session_status`.
- **Banner explicativo no Console Web (PT-BR, EN, ES, FR):** quando o WhatsApp remove o aparelho vinculado ou restringe temporariamente a conta, o Console exibe um aviso claro orientando a checar o celular e aguardar o fim da restrição antes de reconectar.
- **Erro amigável no envio de mensagens (API REST e MCP):** tentar enviar mensagens em uma sessão `logged_out` retorna `409 Conflict` explicando o motivo (`device_removed` / `account_restricted`).

---

## [0.3.1] — 2026-11-25

### Segurança & Comunidade
- **Validação endurecida de `RELAY_API_KEY`:** o servidor agora recusa iniciar com chaves fracas (< 16 caracteres) ou placeholders conhecidos de documentação (como `change-me-to-a-strong-secret`, `change-this-before-starting`, `replace-with-a-strong-random-key`, etc.).
- **Quickstart seguro:** exemplos de `docker run` atualizados para gerar chave aleatória de 256 bits (`openssl rand -hex 32`) e escutar em `127.0.0.1:8080` por padrão.
- **MCP STDIO via Docker:** exemplo pronto usando `docker run -i --rm ... softagon/relay:latest mcp --stdio` (e binário `linux-x86_64` anexado à GitHub Release).
- **Licença & Governança:** `LICENSE` verbatim oficial **Apache-2.0**, `CONTRIBUTING.md`, `SECURITY.md`, GitHub Discussions habilitado, `good first issue`s abertas e imagem de preview social (`docs/images/social-preview.png`).

---

## [0.3.0] — 2026-11-25

### Adicionado — Servidor MCP (Model Context Protocol) Nativo para Agentes de IA
- **SDK Oficial Rust (`rmcp 3.11`):** transformou o Softagon Relay em um **MCP Server local** permitindo que agentes de IA (Claude Desktop, Cursor, Antigravity, VS Code, Windsurf e clientes MCP) consultem sessões, encontrem conversas e enviem mensagens pelo WhatsApp diretamente na máquina do usuário.
- **Camada de serviços compartilhados (`RelayService`):** API REST e servidor MCP consomem os mesmos serviços de domínio (`src/core/service.rs`) — o MCP **não depende do HTTP REST**, evitando duplicação de regras, validações e rate limiting.
- **Dois transportes MCP:**
  - **STDIO (padrão local):** `softagon-relay mcp` / `softagon-relay mcp --stdio` com logs direcionados estritamente para `stderr` (mantendo `stdout` limpo para JSON-RPC).
  - **Streamable HTTP:** `softagon-relay mcp --http` ou habilitado junto com `serve` via `MCP_HTTP_ENABLED=true`, vinculado **exclusivamente a `127.0.0.1`** (`MCP_HTTP_PORT`, padrão `8091`) em `/mcp`.
- **9 Ferramentas MCP (`tools`):**
  - Leitura: `list_sessions`, `get_session_status`, `list_chats` (com filtro por contato/texto/transcrição), `search_messages`, `get_chat_messages`, `get_message_transcription`.
  - Escrita controlada: `send_text_message`.
  - Destrutivas protegidas: `logout_session` e `delete_session` (**bloqueadas por padrão**; exigem `MCP_ALLOW_DESTRUCTIVE=true` e `confirm: true`).
- **Segurança & Auditoria:**
  - Recusa iniciar MCP HTTP em endereços não-loopback (`0.0.0.0` bloqueado).
  - Validação estrita de `Origin` (proteção contra **DNS rebinding**) + autenticação `Authorization: Bearer <RELAY_API_KEY>`.
  - Nova tabela SQLite `mcp_audit_log` (`migrations/0002_mcp_audit.sql`) registrando toda chamada MCP (transporte, ferramenta, argumentos sanitizados, status `ok`/`blocked`/`error` e duração).
- **Testes automatizados (`tests/mcp_tests.rs`):** validação do fluxo real de agente de IA, bloqueio de operações destrutivas, auditoria em SQLite e proteção de loopback/`Origin`.

---

## [0.2.1] — 2026-11-25

### Adicionado — OpenAPI JSON para Integração Rápida com IA e SDKs
- **Novo endpoint `GET /openapi.json`:** converte em memória (uma única vez via `OnceLock`) o contrato OpenAPI 3.2.1 para JSON, respeitando `DOCS_ENABLED` e `DOCS_PUBLIC`.
- **Links internos no Console Web:** atalhos diretos para `openapi.json` e `openapi.yaml` no rodapé autenticado e painel dedicado na tela `/console/api` com botão de **copiar URL** em um clique (pronto para colar em GPT Actions, n8n, LangChain, Postman e Insomnia).

---

## [0.2.0] — 2026-11-25

### Adicionado — Console Web Estilo Raycast, Documentação Multilíngue e Melhorias WhatsApp
- **Console Web Embutido (`/console` e redirecionamento `/manager`):** SPA em Vue 3 + TypeScript + Vite embutida no binário Rust via `rust-embed` (bundle inicial ~91 kB gzip), com tema Claro/Escuro, navegação por teclado (`⌘K`/`Ctrl+K`, `/`, `n`, `g s/e/a`, `r`, `?`) e 4 idiomas (**PT-BR, EN, ES, FR**).
- **Identificação do dispositivo:** aparelho vinculado no WhatsApp aparece como **Softagon Relay** (Chrome) em vez de `rust`.
- **Resolução inteligente de destinatários móveis:** consulta `is_on_whatsapp` tratando variações do 9º dígito no Brasil (`55`), México (`521`/`52`) e Argentina (`549`/`54`).
- **Exclusão segura de sessões desconectadas:** `DELETE /v1/sessions/{id}` retorna `409 Conflict` se a sessão estiver conectada; botão exibido no Console apenas para sessões desconectadas.
- **Reconexão sob demanda:** `POST /v1/sessions/{id}/reconnect` e QR Code SVG ao vivo em `GET /v1/sessions/{id}/qr.svg`.
- **Documentação visual em 4 idiomas:** guias completos em `docs/{pt-BR,en,es,fr}/README.md` com **24 capturas de tela reais** (sem dados sensíveis) e links dinâmicos a partir do Login e do rodapé interno.

---

## [0.1.0] — 2026-11-24

### Adicionado — Fundação do Gateway WhatsApp em Rust (Single Container)
- **Gateway HTTP `/v1/*` em Rust (Tokio + Axum):** sessões multi-dispositivo via `whatsapp-rust`, envio de texto/mídia com `Idempotency-Key` e rate limiting por sessão.
- **Persistência zero-dependência:** SQLite em modo WAL embutido (`migrations/0001_initial.sql`).
- **Armazenamento de mídia plugável:** Local (`/data/media`) ou compatível com **S3 / Cloudflare R2 / MinIO** via `object_store`.
- **Webhooks assinados com HMAC-SHA256:** entrega *at-least-once* com outbox persistente, backoff exponencial + jitter e cabeçalhos `X-Relay-Event-Id`, `X-Relay-Event-Type`, `X-Relay-Timestamp` e `X-Relay-Signature`.
- **Transcrição offline multilíngue de áudios:** decodificação OGG/Opus em Rust puro (`symphonia` + `symphonia-opus` + `rubato`, sem FFmpeg) + inferência local com `whisper.cpp` carregado sob demanda e descarregado após inatividade.
- **Contrato OpenAPI 3.2.1 + Scalar UI (`/docs` e `/openapi.yaml`)** e imagem Docker única (~52 MB, não-root UID `10001`, `read_only`, `cap_drop: ALL`).

---

## Roadmap — Próximos Passos

### Aplicativo Desktop Nativo em Tauri (Windows, Linux e macOS)
Além da distribuição via **Docker** para servidores e nuvem, o próximo grande marco do **Softagon Relay** é uma **versão Desktop nativa construída em [Tauri](https://tauri.app/)** para **Windows, Linux e macOS** — unindo o mesmo backend Rust de baixíssimo consumo, o Console Web embarcado e o **MCP Server local** em um instalador simples com ícone na bandeja do sistema (*system tray*), pronto para conectar o WhatsApp aos seus agentes de IA sem precisar instalar Docker.

> **Você gostaria de usar o Softagon Relay Desktop (Windows / Linux / macOS) ou tem sugestões de novas ferramentas MCP?**  
> 👉 **[Abra uma Issue no GitHub](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new)** contando seu caso de uso e sistema operacional! O feedback da comunidade define diretamente a prioridade dos próximos lançamentos.
