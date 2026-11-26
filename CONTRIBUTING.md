# Contributing to Softagon Relay

Thank you for your interest in contributing to **Softagon Relay**!

## Development Setup

### Prerequisites
- **Rust** `1.94+` (`rustup`)
- **Node.js** `20+` (or **Bun** `1.1+`) for the embedded Web Console (`console/`)
- **Docker** (optional, required for full container builds with `whisper.cpp`)

> **Note on `whisper-rs`:** Local `cargo test` compiles without `cmake`/`clang` by default. The `whisper` Cargo feature is enabled in `Dockerfile` (or locally with `--features whisper` when `cmake` and `clang` are installed).

### Local Git Hooks (Required)
This repository runs all quality checks locally via `.githooks/pre-push` instead of GitHub Actions:

```bash
git config core.hooksPath .githooks
```

Before pushing, the hook automatically runs:
1. `node scripts/check-i18n-docs.mjs` (10-language key parity, version sync, PII check, and localized doc screenshots)
2. `cd console && npm run build` (`vue-tsc -b && vite build`)
3. `cargo fmt --check`
4. `cargo clippy --all-targets -- -D warnings`
5. `cargo test`

## Architecture Guidelines

- **Shared domain layer (`src/core/service.rs`):** Both the REST API (`src/api/`) and the MCP server (`src/mcp/`) must call `RelayService` directly. Do not couple MCP handlers to HTTP REST endpoints.
- **MCP safety defaults:** STDIO is the default local transport (all logs to `stderr`). Streamable HTTP binds to loopback by default; non-loopback only with the explicit `MCP_ALLOW_CONTAINER_BIND=true` Docker opt-in, always keeping `Origin` validation and Bearer authentication. Destructive tools remain disabled by default unless `MCP_ALLOW_DESTRUCTIVE=true` and `confirm: true`.
- **Single-container simplicity:** Avoid introducing mandatory external services (PostgreSQL, Redis, Chromium, FFmpeg).
- **10-language Web Console:** Keep `console/src/i18n/{pt-BR,en,es,fr,id,bew,su,jv,bbc,ms}.json` in sync when adding UI strings. In `vue-i18n` v11, escape literal braces containing spaces as `"{'{'}"` and `"{'}'}"`.

## Good Ways to Contribute

- Check open [`good first issue`](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/labels/good%20first%20issue) tickets.
- Add webhook signature verification examples, workflow templates (e.g. n8n), or new UI/documentation translations.
- Report bugs or propose features via [GitHub Issues](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues) or [Discussions](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/discussions).

## Pull Request Checklist

- [ ] Code formatted (`cargo fmt`) and passes `cargo clippy --all-targets -- -D warnings`
- [ ] All tests pass (`cargo test`) and new behavior includes unit/integration tests
- [ ] Console builds cleanly (`cd console && npm run build`) if frontend files changed
- [ ] No real phone numbers, API keys, or `.env` files included
- [ ] Updated `CHANGELOG.md` / relevant docs when adding user-facing features
