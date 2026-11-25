# Documentación Oficial — Softagon Relay (Español)

> Idiomas: [Português (BR)](../pt-BR/README.md) · [English](../en/README.md) · **Español** · [Français](../fr/README.md)

**¿Qué es Softagon Relay?**  
**Softagon Relay** es un **gateway WhatsApp Web open-source y self-hosted** desarrollado en **Rust** por **[Softagon Sistemas](https://softagon.com.br)**. Funciona en un **único contenedor Docker (~52 MB)** con **SQLite (WAL)** integrado, **Consola Web estilo Raycast** en `/console`, almacenamiento de medios **Local o S3/R2/MinIO**, **webhooks firmados con HMAC-SHA256**, **OpenAPI 3.2.1** y **transcripción offline multilingüe de notas de voz** con `whisper.cpp`.

- **Sitio Web Oficial y Contacto:** <https://softagon.com.br>
- **Docker Hub:** <https://hub.docker.com/r/softagon/relay> (`softagon/relay:latest`)
- **Código fuente (GitHub):** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon>
- **Licencia:** Apache-2.0

---

## 1. Inicio Rápido (Docker)

```bash
docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="cambia-por-una-clave-segura" \
  softagon/relay:latest
```

Abre la **Consola Web** en `http://localhost:8080/console` (o `/manager`, que redirige automáticamente) e ingresa tu `RELAY_API_KEY`.

### Docker Compose Endurecido
Usa el [compose.yaml](../../compose.yaml) del repositorio (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, usuario no-root UID `10001`).

---

## 2. Consola Web (`/console`)

Diseñada para máxima productividad con teclado:
- **Inicio de sesión (`/console/login`):** solo la clave `RELAY_API_KEY` (sin usuario ni correo).
- **Barra de comandos (`/console`):** escribe cualquier número de WhatsApp (ej.: `+34 612 345 678` o `+54 9 11 1234-5678`) y pulsa `↵` para crear la sesión y mostrar el código QR SVG en vivo.
- **Sesiones (`/console/sessions`):** copia ID/JID, envía mensajes de prueba con validación internacional, reconecta/regenera QR, cierra sesión o elimina sesiones desconectadas.
- **Webhooks (`/console/events`):** crea, activa/desactiva y elimina suscripciones con generación automática de secreto HMAC.
- **Referencia de la API (`/console/api`):** generada en vivo desde `/openapi.yaml`, con ejemplos en cURL, JavaScript, Python y PHP y ejecución real (`⌘↵`/`Ctrl+↵`).

### Atajos de teclado
| Atajo | Acción |
|---|---|
| `⌘K` / `Ctrl+K` | Abrir barra de comandos o panel de acciones |
| `/` | Enfocar búsqueda |
| `n` | Conectar nuevo número |
| `g` luego `s` / `e` / `a` | Ir a Sesiones / Eventos / API |
| `r` | Generar nuevo QR en la pantalla de conexión |
| `?` | Ver hoja de atajos |

---

## 3. API HTTP (`/v1`) y Autenticación

Todas las rutas `/v1/*` (excepto `GET /v1/health`, que es pública) requieren:

```http
Authorization: Bearer <RELAY_API_KEY>
```

| Método | Endpoint | Función |
|---|---|---|
| `GET` | `/v1/health` | Estado del servidor (público) |
| `GET` / `POST` | `/v1/sessions` | Listar o crear sesión |
| `GET` / `DELETE` | `/v1/sessions/{id}` | Detalles o eliminar sesión desconectada (`409` si está conectada) |
| `GET` | `/v1/sessions/{id}/qr` · `/qr.svg` | Código QR en JSON o SVG |
| `GET` | `/v1/sessions/{id}/status` | Estado de conexión en vivo |
| `POST` | `/v1/sessions/{id}/reconnect` | Reiniciar y generar nuevo ciclo de QR |
| `POST` | `/v1/sessions/{id}/logout` | Desvincular dispositivo de WhatsApp |
| `POST` | `/v1/sessions/{id}/messages/text` | Enviar texto (soporta `Idempotency-Key` y resolución móvil BR/MX/AR) |
| `POST` | `/v1/sessions/{id}/messages/media` | Enviar imagen, video, audio o documento |
| `GET` | `/v1/messages/{id}` | Consultar estado/detalles del mensaje |
| `GET` / `POST` | `/v1/messages/{id}/transcription` | Consultar o reintentar transcripción Whisper |
| `GET` / `POST` | `/v1/webhooks` | Listar o crear webhooks |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{id}` | Inspeccionar, actualizar o eliminar webhook |

---

## 4. Webhooks Firmados (HMAC-SHA256)

Eventos: `session.status`, `message.received`, `message.sent`, `message.ack`, `message.transcription.completed`, `message.transcription.failed`, `group.updated`.

Cabeceras en cada entrega (**at-least-once** con backoff exponencial + jitter):
- `X-Relay-Event-Id`: ID único UUIDv7 para deduplicación
- `X-Relay-Event-Type`: tipo de evento
- `X-Relay-Timestamp`: marca de tiempo Unix en segundos
- `X-Relay-Signature`: `sha256=<hex(HMAC-SHA256(secret, "{timestamp}.{raw_body}"))>`

---

## 5. Transcripción Local (Whisper) y Almacenamiento S3

- **Notas de voz:** decodificadas en Rust puro (Symphonia + `symphonia-opus` + Rubato 16 kHz mono, sin FFmpeg) y transcritas localmente con `whisper.cpp` (modelo `base` multilingüe con carga diferida y descarga automática por inactividad).
- **S3 / R2 / MinIO:** configura `MEDIA_STORAGE=s3`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` y `S3_ENDPOINT_URL`.

---

## 6. Soporte y Contacto

¿Necesitas soporte dedicado, integraciones a medida o consultoría en IA y mensajería?  
Visita **[softagon.com.br](https://softagon.com.br)** y habla con el equipo de **Softagon Sistemas**.
