# Documentation Officielle — Softagon Relay (Français)

> Langues : [Português (BR)](../pt-BR/README.md) · [English](../en/README.md) · [Español](../es/README.md) · **Français**

**Qu'est-ce que Softagon Relay ?**  
**Softagon Relay** est une **passerelle API HTTP WhatsApp Web open-source et auto-hébergée**, développée en **Rust** par **[Softagon Sistemas](https://softagon.com.br)**. Elle fonctionne dans un **seul conteneur Docker (~52 Mo)** avec **SQLite (WAL)** intégré, une **Console Web de style Raycast** sur `/console`, un stockage de médias **Local ou S3/R2/MinIO**, des **webhooks signés HMAC-SHA256**, **OpenAPI 3.2.1** et la **transcription locale multilingue de messages vocaux** via `whisper.cpp`.

- **Site Officiel & Contact :** <https://softagon.com.br>
- **Docker Hub :** <https://hub.docker.com/r/softagon/relay> (`softagon/relay:latest`)
- **Code source (GitHub) :** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon>
- **Licence :** Apache-2.0

---

## 1. Démarrage Rapide (Docker)

```bash
docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="remplacez-par-une-cle-forte" \
  softagon/relay:latest
```

Ouvrez la **Console Web** sur `http://localhost:8080/console` (ou `/manager`, redirigé automatiquement) et saisissez votre `RELAY_API_KEY`.

### Docker Compose sécurisé
Utilisez le fichier [compose.yaml](../../compose.yaml) du dépôt (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, utilisateur non-root UID `10001`).

---

## 2. Console Web (`/console`)

Conçue pour une productivité pilotée au clavier :
- **Connexion (`/console/login`) :** un seul champ `RELAY_API_KEY` (sans identifiant ni e-mail).
- **Barre de commandes (`/console`) :** tapez n'importe quel numéro WhatsApp (ex. `+33 6 12 34 56 78` ou `87 99200-5656`) et appuyez sur `↵` pour créer la session et afficher le QR Code SVG en direct.
- **Sessions (`/console/sessions`) :** copiez l'ID/JID, envoyez un message de test avec validation internationale, reconnectez/régénérez le QR, déconnectez ou supprimez les sessions déconnectées.
- **Webhooks (`/console/events`) :** créez, activez/désactivez et supprimez vos abonnements avec génération automatique du secret HMAC.
- **Référence de l'API (`/console/api`) :** générée en direct depuis `/openapi.yaml`, avec exemples en cURL, JavaScript, Python et PHP et exécution réelle (`⌘↵`/`Ctrl+↵`).

### Raccourcis clavier
| Raccourci | Action |
|---|---|
| `⌘K` / `Ctrl+K` | Ouvrir la barre de commandes ou le panneau d'actions |
| `/` | Focus sur la recherche |
| `n` | Connecter un nouveau numéro |
| `g` puis `s` / `e` / `a` | Aller vers Sessions / Événements / API |
| `r` | Régénérer le QR Code sur l'écran de connexion |
| `?` | Afficher la liste des raccourcis |

---

## 3. API HTTP (`/v1`) et Authentification

Toutes les routes `/v1/*` (sauf `GET /v1/health`, publique) nécessitent :

```http
Authorization: Bearer <RELAY_API_KEY>
```

| Méthode | Endpoint | Rôle |
|---|---|---|
| `GET` | `/v1/health` | État de santé du serveur (public) |
| `GET` / `POST` | `/v1/sessions` | Lister ou créer une session |
| `GET` / `DELETE` | `/v1/sessions/{id}` | Détails ou suppression d'une session déconnectée (`409` si connectée) |
| `GET` | `/v1/sessions/{id}/qr` · `/qr.svg` | QR Code d'appairage (JSON ou SVG) |
| `GET` | `/v1/sessions/{id}/status` | État de connexion en direct |
| `POST` | `/v1/sessions/{id}/reconnect` | Redémarrer et lancer un nouveau cycle QR |
| `POST` | `/v1/sessions/{id}/logout` | Déconnecter l'appareil WhatsApp |
| `POST` | `/v1/sessions/{id}/messages/text` | Envoyer un texte (supporte `Idempotency-Key` et résolution mobile BR/MX/AR) |
| `POST` | `/v1/sessions/{id}/messages/media` | Envoyer image, vidéo, audio ou document |
| `GET` | `/v1/messages/{id}` | Consulter l'état et les détails du message |
| `GET` / `POST` | `/v1/messages/{id}/transcription` | Consulter ou relancer la transcription Whisper |
| `GET` / `POST` | `/v1/webhooks` | Lister ou créer des webhooks |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{id}` | Consulter, modifier ou supprimer un webhook |

---

## 4. Webhooks Signés (HMAC-SHA256)

Événements : `session.status`, `message.received`, `message.sent`, `message.ack`, `message.transcription.completed`, `message.transcription.failed`, `group.updated`.

En-têtes envoyés à chaque livraison (**at-least-once** avec backoff exponentiel + jitter) :
- `X-Relay-Event-Id` : identifiant unique UUIDv7 pour la déduplication
- `X-Relay-Event-Type` : type d'événement
- `X-Relay-Timestamp` : horodatage Unix en secondes
- `X-Relay-Signature` : `sha256=<hex(HMAC-SHA256(secret, "{timestamp}.{raw_body}"))>`

---

## 5. Transcription Locale (Whisper) et Stockage S3

- **Messages vocaux :** décodés en Rust pur (Symphonia + `symphonia-opus` + Rubato 16 kHz mono, sans FFmpeg) et transcrits localement avec `whisper.cpp` (modèle `base` multilingue chargé à la demande et déchargé après inactivité).
- **S3 / R2 / MinIO :** définissez `MEDIA_STORAGE=s3`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` et `S3_ENDPOINT_URL`.

---

## 6. Support et Contact

Besoin d'un support dédié, d'intégrations sur mesure ou de conseil en IA et messagerie ?  
Visitez **[softagon.com.br](https://softagon.com.br)** et contactez l'équipe de **Softagon Sistemas**.
