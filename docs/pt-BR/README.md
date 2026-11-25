# Documentação Oficial — Softagon Relay (Português)

> Idiomas: **Português (BR)** · [English](../en/README.md) · [Español](../es/README.md)

**O que é o Softagon Relay?**  
O **Softagon Relay** é um **gateway WhatsApp Web open-source e self-hosted** desenvolvido em **Rust** pela **[Softagon Sistemas](https://softagon.com.br)**. Ele roda em um **único container Docker (~52 MB)** com **SQLite (WAL)** embutido, **Console Web estilo Raycast** em `/console`, armazenamento de mídia **Local ou S3/R2/MinIO**, **webhooks assinados com HMAC-SHA256**, **OpenAPI 3.2.1** e **transcrição offline multilíngue de áudios** com `whisper.cpp`.

- **Site Oficial & Contato:** <https://softagon.com.br>
- **Docker Hub:** <https://hub.docker.com/r/softagon/relay> (`softagon/relay:latest`)
- **Código-fonte (GitHub):** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon>
- **Licença:** Apache-2.0

---

## 1. Instalação Rápida (Docker)

```bash
docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="troque-por-uma-chave-forte" \
  softagon/relay:latest
```

Acesse o **Console Web** em `http://localhost:8080/console` (ou `/manager`, que redireciona automaticamente) e informe sua `RELAY_API_KEY`.

### Perfil endurecido com Docker Compose
Use o [compose.yaml](../../compose.yaml) do repositório (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, usuário não-root UID `10001`).

---

## 2. Console Web (`/console`)

Projetado para ser usado até sem mouse:
- **Login (`/console/login`):** apenas a chave `RELAY_API_KEY` (sem usuário/senha).
- **Barra de comando (`/console`):** digite qualquer número de WhatsApp (ex.: `87 99200-5656` ou `+351 912 345 678`) e pressione `↵` para criar a sessão e abrir o QR Code SVG ao vivo.
- **Sessões (`/console/sessions`):** copie ID/JID, envie mensagem de teste com validação internacional, reinicie/reconecte, faça logout ou exclua sessões desconectadas.
- **Webhooks (`/console/events`):** crie, ative/desative e remova assinaturas de webhook com geração automática de segredo HMAC.
- **Referência da API (`/console/api`):** gerada dinamicamente do `/openapi.yaml`, com exemplos em cURL, JavaScript, Python e PHP e execução real (`⌘↵`/`Ctrl+↵`).

### Atalhos de teclado
| Atalho | Ação |
|---|---|
| `⌘K` / `Ctrl+K` | Abre a barra de comando ou o painel de ações da sessão |
| `/` | Foca a busca |
| `n` | Conectar novo número |
| `g` depois `s` / `e` / `a` | Ir para Sessões / Eventos / API |
| `r` | Gerar novo QR Code na tela de conexão |
| `?` | Exibir folha de atalhos |

---

## 3. API HTTP (`/v1`) & Autenticação

Todas as rotas `/v1/*` (exceto `GET /v1/health`) exigem:

```http
Authorization: Bearer <RELAY_API_KEY>
```

| Método | Endpoint | Função |
|---|---|---|
| `GET` | `/v1/health` | Saúde do servidor (público) |
| `GET` / `POST` | `/v1/sessions` | Listar ou criar sessão |
| `GET` / `DELETE` | `/v1/sessions/{id}` | Detalhes ou excluir sessão desconectada (`409` se conectada) |
| `GET` | `/v1/sessions/{id}/qr` · `/qr.svg` | QR Code em JSON ou SVG |
| `GET` | `/v1/sessions/{id}/status` | Estado de conexão ao vivo |
| `POST` | `/v1/sessions/{id}/reconnect` | Reiniciar e gerar novo ciclo de QR |
| `POST` | `/v1/sessions/{id}/logout` | Desconectar aparelho do WhatsApp |
| `POST` | `/v1/sessions/{id}/messages/text` | Enviar texto (aceita `Idempotency-Key` e resolve 9º dígito BR/MX/AR) |
| `POST` | `/v1/sessions/{id}/messages/media` | Enviar imagem, vídeo, áudio ou documento |
| `GET` | `/v1/messages/{id}` | Consultar status/detalhes da mensagem |
| `GET` / `POST` | `/v1/messages/{id}/transcription` | Consultar ou reprocessar transcrição Whisper |
| `GET` / `POST` | `/v1/webhooks` | Listar ou criar webhook |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{id}` | Inspecionar, atualizar ou remover webhook |

---

## 4. Webhooks Assinados (HMAC-SHA256)

Eventos emitidos: `session.status`, `message.received`, `message.sent`, `message.ack`, `message.transcription.completed`, `message.transcription.failed`, `group.updated`.

Cabeçalhos enviados em cada entrega (**at-least-once** com backoff exponencial + jitter):
- `X-Relay-Event-Id`: ID único UUIDv7 para deduplicação
- `X-Relay-Event-Type`: tipo do evento
- `X-Relay-Timestamp`: Unix timestamp em segundos
- `X-Relay-Signature`: `sha256=<hex(HMAC-SHA256(secret, "{timestamp}.{raw_body}"))>`

---

## 5. Transcrição Local (Whisper) e Storage S3

- **Áudios:** decodificados em Rust puro (Symphonia + `symphonia-opus` + Rubato 16 kHz mono, sem FFmpeg) e transcritos localmente com `whisper.cpp` (modelo `base` multilíngue carregado sob demanda e descarregado após inatividade).
- **Mídia S3/R2/MinIO:** defina `MEDIA_STORAGE=s3`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` e `S3_ENDPOINT_URL`.

---

## 6. Suporte e Contato

Precisa de suporte dedicado, integrações sob medida ou consultoria em IA e mensageria?  
Visite **[softagon.com.br](https://softagon.com.br)** e fale com a equipe da **Softagon Sistemas**.
