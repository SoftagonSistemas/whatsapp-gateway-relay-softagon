use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use chrono::Utc;
use qrcode::{Color, QrCode};
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::api::error::ApiError;
use crate::engines::whatsapp::SessionConnectionState;

/// Pure-Rust zero-dependency PNG encoder for crisp black-on-white QR codes.
/// Uses 8-bit grayscale with uncompressed DEFLATE stored blocks (RFC 1951 type 0).
pub fn render_qr_png(data: &str) -> Result<Vec<u8>, ApiError> {
    let qr = QrCode::new(data.as_bytes())
        .map_err(|e| ApiError::Internal(format!("qr encode error: {e}")))?;
    let modules = qr.width();
    let quiet = 4usize;
    let scale = 8usize;
    let total_modules = modules + quiet * 2;
    let img_size = total_modules * scale;

    // Build raw scanlines: 1 filter byte (0 = None) + img_size grayscale bytes per row
    let row_bytes = 1 + img_size;
    let mut raw = Vec::with_capacity(row_bytes * img_size);
    for y in 0..img_size {
        raw.push(0u8); // filter type 0
        let my = y / scale;
        let in_y = my >= quiet && my < quiet + modules;
        for x in 0..img_size {
            let mx = x / scale;
            let dark = in_y
                && mx >= quiet
                && mx < quiet + modules
                && qr[(mx - quiet, my - quiet)] == Color::Dark;
            raw.push(if dark { 0x0f } else { 0xff });
        }
    }

    // Wrap raw scanlines in a valid zlib stream using uncompressed DEFLATE stored blocks
    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 1) * 5 + 6);
    zlib.push(0x78); // CMF: deflate, 32K window
    zlib.push(0x01); // FLG: no preset dict, check bits (0x7801 % 31 == 0)

    let mut offset = 0usize;
    while offset < raw.len() {
        let remaining = raw.len() - offset;
        let chunk_len = remaining.min(65535);
        let is_final = offset + chunk_len == raw.len();
        zlib.push(if is_final { 0x01 } else { 0x00 });
        let len_u16 = chunk_len as u16;
        zlib.extend_from_slice(&len_u16.to_le_bytes());
        zlib.extend_from_slice(&(!len_u16).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + chunk_len]);
        offset += chunk_len;
    }

    let adler = adler32(&raw);
    zlib.extend_from_slice(&adler.to_be_bytes());

    // Assemble PNG file
    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(img_size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img_size as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(0); // color type: grayscale
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_png_chunk(&mut png, b"IHDR", &ihdr);
    write_png_chunk(&mut png, b"IDAT", &zlib);
    write_png_chunk(&mut png, b"IEND", &[]);

    Ok(png)
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

#[derive(Debug, Serialize)]
pub struct PairTokenStatusResponse {
    pub session_name: String,
    pub status: String,
    pub status_reason: Option<String>,
    pub qr_available: bool,
    pub qr_expires_at: Option<chrono::DateTime<Utc>>,
    pub pair_code: Option<String>,
    pub pair_code_expires_at: Option<chrono::DateTime<Utc>>,
    pub link_expires_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PublicPairCodeRequest {
    pub phone_number: String,
}

#[derive(Debug, Serialize)]
pub struct PublicPairCodeResponse {
    pub formatted_code: String,
    pub expires_at: chrono::DateTime<Utc>,
}

async fn resolve_valid_token(
    state: &AppState,
    token: &str,
) -> Result<crate::core::pairing::PairingLinkRecord, ApiError> {
    let Some(link) = state.pairing_links.get_valid(token).await else {
        return Err(ApiError::NotFound(
            "pairing link is invalid or expired; generate a new pairing link".into(),
        ));
    };
    Ok(link)
}

pub async fn pair_page(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let session = state.sessions.get_session(&link.session_id).await?;
    let safe_name = html_escape(&session.name);
    let html = build_pair_page_html(&token, &safe_name);
    Ok((
        StatusCode::OK,
        [(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, max-age=0"),
        )],
        Html(html),
    )
        .into_response())
}

pub async fn pair_status(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<PairTokenStatusResponse>, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let (rec, live_status) = state.sessions.get_status(&link.session_id).await?;

    if live_status == SessionConnectionState::Connected.as_str() {
        // Auto-invalidate token once connected so it cannot be reused
        state.pairing_links.invalidate(&token).await;
    }

    let qr = state.sessions.get_qr(&link.session_id).await?;
    let pair_code = state.sessions.get_pair_code(&link.session_id).await?;

    Ok(Json(PairTokenStatusResponse {
        session_name: rec.name,
        status: live_status,
        status_reason: rec.status_reason,
        qr_available: qr.is_some(),
        qr_expires_at: qr.map(|q| q.expires_at),
        pair_code: pair_code.as_ref().map(|p| p.formatted_code.clone()),
        pair_code_expires_at: pair_code.map(|p| p.expires_at),
        link_expires_at: link.expires_at,
    }))
}

pub async fn pair_qr_svg(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let Some(qr) = state.sessions.get_qr(&link.session_id).await? else {
        return Err(ApiError::NotFound(
            "no active QR code right now (connecting, refreshing, or already connected)".into(),
        ));
    };
    let code = QrCode::new(qr.code.as_bytes())
        .map_err(|e| ApiError::Internal(format!("qr render error: {e}")))?;
    let svg_str = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(320, 320)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#0f172a"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("image/svg+xml; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, max-age=0"),
            ),
        ],
        svg_str,
    )
        .into_response())
}

pub async fn pair_qr_png(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let Some(qr) = state.sessions.get_qr(&link.session_id).await? else {
        return Err(ApiError::NotFound(
            "no active QR code right now (connecting, refreshing, or already connected)".into(),
        ));
    };
    let bytes = render_qr_png(&qr.code)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, max-age=0"),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn pair_reconnect(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let (_, live_status) = state.sessions.get_status(&link.session_id).await?;
    if live_status == SessionConnectionState::Connected.as_str() {
        state.pairing_links.invalidate(&token).await;
        return Ok(Json(serde_json::json!({ "status": "connected" })));
    }
    match state.pairing_links.try_mark_reconnect(&token).await {
        Ok(true) => {
            let _ = state.sessions.reconnect_session(&link.session_id).await?;
            Ok(Json(serde_json::json!({ "status": "reconnecting" })))
        }
        Ok(false) => Ok(Json(serde_json::json!({ "status": "cooldown" }))),
        Err(()) => Err(ApiError::NotFound(
            "pairing link is invalid or expired".into(),
        )),
    }
}

pub async fn pair_request_code(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<PublicPairCodeRequest>,
) -> Result<Json<PublicPairCodeResponse>, ApiError> {
    let link = resolve_valid_token(&state, &token).await?;
    let payload = state
        .sessions
        .request_pair_code(&link.session_id, &body.phone_number)
        .await?;
    Ok(Json(PublicPairCodeResponse {
        formatted_code: payload.formatted_code,
        expires_at: payload.expires_at,
    }))
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build_pair_page_html(token: &str, session_name: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Softagon Relay · Connect WhatsApp ({session_name})</title>
  <style>
    :root {{
      color-scheme: dark;
      --bg: #0b1220; --surface: #111a2e; --sunken: #0e1628;
      --border: rgba(255,255,255,.12); --text: #e8eef7; --muted: #97a3ba;
      --accent: #4fcbee; --primary: #fe682d; --ok: #22c55e; --warn: #ffc24d;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 20px;
      background: var(--bg); color: var(--text);
      font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
    }}
    .card {{
      width: 100%; max-width: 520px; background: var(--surface);
      border: 1px solid var(--border); border-radius: 18px; padding: 28px;
      box-shadow: 0 24px 70px rgba(0,0,0,.55);
    }}
    .top {{ display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 18px; }}
    .brand {{ font-weight: 800; font-size: 15px; letter-spacing: .02em; }}
    .pill {{
      font-size: 12px; font-weight: 600; padding: 4px 10px; border-radius: 999px;
      background: rgba(79,203,238,.15); color: var(--accent); border: 1px solid rgba(79,203,238,.35);
    }}
    .pill.ok {{ background: rgba(34,197,94,.16); color: var(--ok); border-color: rgba(34,197,94,.4); }}
    h1 {{ margin: 0 0 6px; font-size: 22px; font-weight: 800; }}
    .sub {{ margin: 0 0 18px; color: var(--muted); font-size: 14px; line-height: 1.5; }}
    .tabs {{ display: flex; gap: 6px; background: var(--sunken); padding: 4px; border-radius: 10px; margin-bottom: 20px; border: 1px solid var(--border); }}
    .tab {{
      flex: 1; padding: 9px 12px; border: 0; border-radius: 8px; cursor: pointer;
      background: transparent; color: var(--muted); font-weight: 700; font-size: 13px;
    }}
    .tab.active {{ background: rgba(79,203,238,.18); color: var(--text); }}
    .qr-box {{
      display: grid; place-items: center; background: #fff; border-radius: 16px;
      padding: 16px; width: 296px; height: 296px; margin: 0 auto 14px;
    }}
    .qr-box img {{ width: 100%; height: 100%; object-fit: contain; display: block; }}
    .qr-loading {{ color: #0f172a; font-weight: 700; font-size: 14px; text-align: center; padding: 12px; }}
    .meta-row {{ display: flex; justify-content: space-between; align-items: center; font-size: 12.5px; color: var(--muted); margin-bottom: 14px; }}
    .steps {{ margin: 0; padding-left: 18px; color: var(--muted); font-size: 13.5px; line-height: 1.65; }}
    .steps strong {{ color: var(--text); }}
    .btn {{
      display: inline-flex; align-items: center; justify-content: center; gap: 8px;
      padding: 10px 16px; border-radius: 10px; border: 1px solid var(--border);
      background: var(--sunken); color: var(--text); font-weight: 700; font-size: 13px; cursor: pointer;
    }}
    .btn.primary {{ background: var(--primary); color: #0b1220; border-color: transparent; width: 100%; }}
    .inp {{
      width: 100%; padding: 12px 14px; border-radius: 10px; border: 1px solid var(--border);
      background: var(--sunken); color: var(--text); font-size: 15px; font-family: ui-monospace, monospace;
      margin-bottom: 12px;
    }}
    .code-box {{
      text-align: center; font-family: ui-monospace, monospace; font-size: 32px; font-weight: 800;
      letter-spacing: .16em; padding: 18px; border-radius: 14px; background: var(--sunken);
      border: 1px solid rgba(79,203,238,.4); margin: 14px 0; color: var(--accent);
    }}
    .done {{ text-align: center; padding: 28px 12px; }}
    .done h2 {{ color: var(--ok); margin: 0 0 8px; }}
    .foot {{ margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--border); font-size: 12px; color: var(--muted); text-align: center; }}
    .hide {{ display: none !important; }}
    .err {{ color: #ff8f87; font-size: 13px; margin: 8px 0; }}
  </style>
</head>
<body>
  <main class="card">
    <div class="top">
      <div class="brand">Softagon Relay · {session_name}</div>
      <span id="status-pill" class="pill">Waiting for scan</span>
    </div>

    <div id="connected-view" class="done hide">
      <h2 id="t-connected-h">WhatsApp Connected!</h2>
      <p id="t-connected-p" class="sub">Session <strong>{session_name}</strong> is paired and ready. You can close this tab and return to your assistant.</p>
      <a id="t-open-console" href="/console" class="btn" style="margin-top:16px;text-decoration:none">Open Web Console →</a>
    </div>

    <div id="pairing-view">
      <h1 id="t-h1">Scan with WhatsApp</h1>
      <p id="t-sub" class="sub">Zero-login temporary pairing page. The QR code refreshes automatically while this link is active.</p>

      <div class="tabs" role="tablist">
        <button id="tab-qr" class="tab active" type="button" onclick="switchTab('qr')">QR Code (Recommended)</button>
        <button id="tab-code" class="tab" type="button" onclick="switchTab('code')">8-Digit Code</button>
      </div>

      <section id="panel-qr">
        <div class="qr-box">
          <img id="qr-img" alt="WhatsApp QR code" class="hide" />
          <div id="qr-loading" class="qr-loading">Generating fresh QR code…</div>
        </div>
        <div class="meta-row">
          <span id="qr-timer">Auto-refresh enabled</span>
          <button id="t-new-qr" type="button" class="btn" onclick="manualRefresh()">New QR</button>
        </div>
        <ol id="t-qr-steps" class="steps">
          <li>Open <strong>WhatsApp</strong> on your phone.</li>
          <li>Go to <strong>Linked devices → Link a device</strong>.</li>
          <li>Point your camera at the <strong>QR code</strong> above.</li>
        </ol>
      </section>

      <section id="panel-code" class="hide">
        <p id="t-code-sub" class="sub">Prefer connecting without a camera? Enter your WhatsApp number with country code (e.g. <code>5511999990001</code>).</p>
        <form onsubmit="requestCode(event)">
          <input id="phone-input" class="inp" type="tel" placeholder="5511999990001" required />
          <button id="code-submit" type="submit" class="btn primary">Get 8-Digit Code</button>
        </form>
        <div id="code-error" class="err hide"></div>
        <div id="code-result" class="hide">
          <div id="code-value" class="code-box">---- ----</div>
          <button id="t-copy-code" type="button" class="btn primary" onclick="copyCode()">Copy Code</button>
          <ol id="t-code-steps" class="steps" style="margin-top:14px">
            <li>Open <strong>WhatsApp → Linked devices → Link a device</strong>.</li>
            <li>Tap <strong>Link with phone number instead</strong>.</li>
            <li>Type the 8-character code shown above.</li>
          </ol>
        </div>
      </section>
    </div>

    <div id="t-foot" class="foot">Scoped temporary link · No API key exposed · Auto-expires after pairing</div>
  </main>

  <script>
    const TOKEN = {token_json};
    const I18N = {{
      en: {{
        waiting: 'Waiting for scan', connected: 'Connected', expired: 'Link expired',
        connectedH: 'WhatsApp Connected!',
        connectedP: 'Session is paired and ready. You can close this tab and return to your assistant.',
        openConsole: 'Open Web Console →',
        h1: 'Scan with WhatsApp',
        sub: 'Zero-login temporary pairing page. The QR code refreshes automatically while this link is active.',
        tabQr: 'QR Code (Recommended)', tabCode: '8-Digit Code',
        loading: 'Generating fresh QR code…', autoOn: 'Auto-refresh enabled', newQr: 'New QR',
        refreshing: 'Refreshing QR cycle…', autoIn: 'Auto-refreshes in {{s}}s', autoNow: 'Refreshing QR automatically…',
        renewing: 'QR cycle expired · auto-renewing…',
        expiredMsg: 'This temporary pairing link expired. Ask your assistant for a new link.',
        qrSteps: ['Open <strong>WhatsApp</strong> on your phone.', 'Go to <strong>Linked devices → Link a device</strong>.', 'Point your camera at the <strong>QR code</strong> above.'],
        codeSub: 'Prefer connecting without a camera? Enter your WhatsApp number with country code (e.g. <code>5511999990001</code>).',
        getCode: 'Get 8-Digit Code', copyCode: 'Copy Code', copied: 'Copied!',
        codeSteps: ['Open <strong>WhatsApp → Linked devices → Link a device</strong>.', 'Tap <strong>Link with phone number instead</strong>.', 'Type the 8-character code shown above.'],
        foot: 'Scoped temporary link · No API key exposed · Auto-expires after pairing',
      }},
      pt: {{
        waiting: 'Aguardando leitura', connected: 'Conectado', expired: 'Link expirado',
        connectedH: 'WhatsApp Conectado!',
        connectedP: 'A sessão está pareada e pronta. Você pode fechar esta aba e voltar para o seu assistente.',
        openConsole: 'Abrir Console Web →',
        h1: 'Escaneie com o WhatsApp',
        sub: 'Página temporária sem login. O QR Code se renova automaticamente enquanto este link estiver ativo.',
        tabQr: 'QR Code (Recomendado)', tabCode: 'Código de 8 dígitos',
        loading: 'Gerando novo QR Code…', autoOn: 'Renovação automática ativa', newQr: 'Novo QR',
        refreshing: 'Renovando ciclo do QR…', autoIn: 'Novo QR em {{s}}s', autoNow: 'Renovando QR automaticamente…',
        renewing: 'QR expirado · gerando outro…',
        expiredMsg: 'Este link temporário expirou. Peça um novo link ao seu assistente.',
        qrSteps: ['Abra o <strong>WhatsApp</strong> no celular.', 'Toque em <strong>Aparelhos conectados → Conectar aparelho</strong>.', 'Aponte a câmera para o <strong>QR Code</strong> acima.'],
        codeSub: 'Prefere conectar sem câmera? Digite seu número do WhatsApp com DDI e DDD (ex.: <code>5511999990001</code>).',
        getCode: 'Gerar código de 8 dígitos', copyCode: 'Copiar código', copied: 'Copiado!',
        codeSteps: ['Abra <strong>WhatsApp → Aparelhos conectados → Conectar aparelho</strong>.', 'Toque em <strong>Conectar com número de telefone</strong>.', 'Digite o código de 8 caracteres exibido acima.'],
        foot: 'Link temporário restrito · Nenhuma chave exposta · Expira ao conectar',
      }},
      es: {{
        waiting: 'Esperando escaneo', connected: 'Conectado', expired: 'Enlace expirado',
        connectedH: '¡WhatsApp conectado!',
        connectedP: 'La sesión está vinculada y lista. Puedes cerrar esta pestaña y volver a tu asistente.',
        openConsole: 'Abrir Consola Web →',
        h1: 'Escanea con WhatsApp',
        sub: 'Página temporal sin inicio de sesión. El código QR se renueva automáticamente mientras este enlace esté activo.',
        tabQr: 'Código QR (Recomendado)', tabCode: 'Código de 8 dígitos',
        loading: 'Generando nuevo código QR…', autoOn: 'Actualización automática activa', newQr: 'Nuevo QR',
        refreshing: 'Renovando ciclo QR…', autoIn: 'Nuevo QR en {{s}}s', autoNow: 'Renovando QR automáticamente…',
        renewing: 'QR expirado · renovando…',
        expiredMsg: 'Este enlace temporal expiró. Pide un nuevo enlace a tu asistente.',
        qrSteps: ['Abre <strong>WhatsApp</strong> en tu teléfono.', 'Ve a <strong>Dispositivos vinculados → Vincular dispositivo</strong>.', 'Apunta la cámara al <strong>código QR</strong> de arriba.'],
        codeSub: '¿Prefieres conectar sin cámara? Escribe tu número con código de país (ej.: <code>5511999990001</code>).',
        getCode: 'Obtener código de 8 dígitos', copyCode: 'Copiar código', copied: '¡Copiado!',
        codeSteps: ['Abre <strong>WhatsApp → Dispositivos vinculados → Vincular dispositivo</strong>.', 'Toca <strong>Vincular con el número de teléfono</strong>.', 'Escribe el código de 8 caracteres mostrado arriba.'],
        foot: 'Enlace temporal limitado · Sin clave API expuesta · Expira al conectar',
      }},
      fr: {{
        waiting: 'En attente de scan', connected: 'Connecté', expired: 'Lien expiré',
        connectedH: 'WhatsApp connecté !',
        connectedP: 'La session est jumelée et prête. Vous pouvez fermer cet onglet et revenir à votre assistant.',
        openConsole: 'Ouvrir la Console Web →',
        h1: 'Scannez avec WhatsApp',
        sub: 'Page temporaire sans connexion. Le QR Code se renouvelle automatiquement tant que ce lien est actif.',
        tabQr: 'QR Code (Recommandé)', tabCode: 'Code à 8 chiffres',
        loading: 'Génération d’un nouveau QR Code…', autoOn: 'Renouvellement automatique actif', newQr: 'Nouveau QR',
        refreshing: 'Renouvellement du QR…', autoIn: 'Nouveau QR dans {{s}}s', autoNow: 'Renouvellement automatique…',
        renewing: 'QR expiré · renouvellement…',
        expiredMsg: 'Ce lien temporaire a expiré. Demandez un nouveau lien à votre assistant.',
        qrSteps: ['Ouvrez <strong>WhatsApp</strong> sur votre téléphone.', 'Allez dans <strong>Appareils connectés → Connecter un appareil</strong>.', 'Pointez la caméra vers le <strong>QR Code</strong> ci-dessus.'],
        codeSub: 'Vous préférez sans caméra ? Saisissez votre numéro avec l’indicatif pays (ex. <code>5511999990001</code>).',
        getCode: 'Obtenir le code à 8 chiffres', copyCode: 'Copier le code', copied: 'Copié !',
        codeSteps: ['Ouvrez <strong>WhatsApp → Appareils connectés → Connecter un appareil</strong>.', 'Touchez <strong>Connecter avec le numéro de téléphone</strong>.', 'Saisissez le code à 8 caractères affiché ci-dessus.'],
        foot: 'Lien temporaire restreint · Aucune clé API exposée · Expire après connexion',
      }},
      id: {{
        waiting: 'Menunggu pemindaian', connected: 'Terhubung', expired: 'Tautan kedaluwarsa',
        connectedH: 'WhatsApp Terhubung!',
        connectedP: 'Sesi sudah tersambung dan siap. Anda dapat menutup tab ini dan kembali ke asisten Anda.',
        openConsole: 'Buka Konsol Web →',
        h1: 'Pindai dengan WhatsApp',
        sub: 'Halaman pairing sementara tanpa login. Kode QR diperbarui otomatis selama tautan ini aktif.',
        tabQr: 'Kode QR (Disarankan)', tabCode: 'Kode 8 Digit',
        loading: 'Membuat kode QR baru…', autoOn: 'Pembaruan otomatis aktif', newQr: 'QR Baru',
        refreshing: 'Memperbarui siklus QR…', autoIn: 'QR baru dalam {{s}} dtk', autoNow: 'Memperbarui QR otomatis…',
        renewing: 'QR kedaluwarsa · memperbarui…',
        expiredMsg: 'Tautan sementara ini sudah kedaluwarsa. Minta tautan baru ke asisten Anda.',
        qrSteps: ['Buka <strong>WhatsApp</strong> di ponsel Anda.', 'Buka <strong>Perangkat tertaut → Tautkan perangkat</strong>.', 'Arahkan kamera ke <strong>kode QR</strong> di atas.'],
        codeSub: 'Lebih suka tanpa kamera? Masukkan nomor WhatsApp dengan kode negara (mis. <code>6281234567890</code>).',
        getCode: 'Dapatkan Kode 8 Digit', copyCode: 'Salin Kode', copied: 'Tersalin!',
        codeSteps: ['Buka <strong>WhatsApp → Perangkat tertaut → Tautkan perangkat</strong>.', 'Ketuk <strong>Tautkan dengan nomor telepon</strong>.', 'Masukkan kode 8 karakter di atas.'],
        foot: 'Tautan sementara · Tanpa kunci API · Otomatis kedaluwarsa setelah terhubung',
      }},
      ms: {{
        waiting: 'Menunggu imbasan', connected: 'Disambungkan', expired: 'Pautan tamat tempoh',
        connectedH: 'WhatsApp Disambungkan!',
        connectedP: 'Sesi telah digandingkan dan sedia. Anda boleh menutup tab ini dan kembali ke pembantu anda.',
        openConsole: 'Buka Konsol Web →',
        h1: 'Imbas dengan WhatsApp',
        sub: 'Halaman gandingan sementara tanpa log masuk. Kod QR diperbaharui secara automatik semasa pautan aktif.',
        tabQr: 'Kod QR (Disyorkan)', tabCode: 'Kod 8 Digit',
        loading: 'Menjana kod QR baharu…', autoOn: 'Pembaharuan automatik aktif', newQr: 'QR Baharu',
        refreshing: 'Memperbaharui kitaran QR…', autoIn: 'QR baharu dalam {{s}}s', autoNow: 'Memperbaharui QR automatik…',
        renewing: 'QR tamat tempoh · memperbaharui…',
        expiredMsg: 'Pautan sementara ini telah tamat tempoh. Minta pautan baharu daripada pembantu anda.',
        qrSteps: ['Buka <strong>WhatsApp</strong> pada telefon anda.', 'Pergi ke <strong>Peranti dipautkan → Pautkan peranti</strong>.', 'Halakan kamera ke <strong>kod QR</strong> di atas.'],
        codeSub: 'Lebih suka tanpa kamera? Masukkan nombor WhatsApp dengan kod negara (cth. <code>60123456789</code>).',
        getCode: 'Dapatkan Kod 8 Digit', copyCode: 'Salin Kod', copied: 'Disalin!',
        codeSteps: ['Buka <strong>WhatsApp → Peranti dipautkan → Pautkan peranti</strong>.', 'Ketik <strong>Pautkan dengan nombor telefon</strong>.', 'Masukkan kod 8 aksara yang dipaparkan di atas.'],
        foot: 'Pautan sementara terhad · Tiada kunci API didedahkan · Tamat selepas bersambung',
      }},
    }};

    function detectLang() {{
      const langs = (navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language || 'en']);
      for (const raw of langs) {{
        const base = String(raw || '').toLowerCase().split(/[-_]/)[0];
        if (base === 'in') return 'id';
        if (I18N[base]) return base;
      }}
      return 'en';
    }}

    const T = I18N[detectLang()] || I18N.en;
    document.documentElement.lang = detectLang();
    document.getElementById('status-pill').textContent = T.waiting;
    document.getElementById('t-connected-h').textContent = T.connectedH;
    document.getElementById('t-connected-p').textContent = T.connectedP;
    document.getElementById('t-open-console').textContent = T.openConsole;
    document.getElementById('t-h1').textContent = T.h1;
    document.getElementById('t-sub').textContent = T.sub;
    document.getElementById('tab-qr').textContent = T.tabQr;
    document.getElementById('tab-code').textContent = T.tabCode;
    document.getElementById('qr-loading').textContent = T.loading;
    document.getElementById('qr-timer').textContent = T.autoOn;
    document.getElementById('t-new-qr').textContent = T.newQr;
    document.getElementById('t-qr-steps').innerHTML = T.qrSteps.map(s => `<li>${{s}}</li>`).join('');
    document.getElementById('t-code-sub').innerHTML = T.codeSub;
    document.getElementById('code-submit').textContent = T.getCode;
    document.getElementById('t-copy-code').textContent = T.copyCode;
    document.getElementById('t-code-steps').innerHTML = T.codeSteps.map(s => `<li>${{s}}</li>`).join('');
    document.getElementById('t-foot').textContent = T.foot;

    let lastQrKey = '';
    let autoReconnectInFlight = false;
    let pollHandle = null;

    function switchTab(name) {{
      document.getElementById('tab-qr').classList.toggle('active', name === 'qr');
      document.getElementById('tab-code').classList.toggle('active', name === 'code');
      document.getElementById('panel-qr').classList.toggle('hide', name !== 'qr');
      document.getElementById('panel-code').classList.toggle('hide', name !== 'code');
    }}

    async function manualRefresh() {{
      document.getElementById('qr-timer').textContent = T.refreshing;
      await fetch(`/pair/${{TOKEN}}/reconnect`, {{ method: 'POST' }});
      setTimeout(poll, 900);
    }}

    async function requestCode(ev) {{
      ev.preventDefault();
      const errEl = document.getElementById('code-error');
      const btn = document.getElementById('code-submit');
      errEl.classList.add('hide');
      btn.disabled = true;
      try {{
        const phone = document.getElementById('phone-input').value.trim();
        const res = await fetch(`/pair/${{TOKEN}}/pair-code`, {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify({{ phone_number: phone }}),
        }});
        const data = await res.json();
        if (!res.ok) throw new Error(data.message || 'Failed to generate code');
        document.getElementById('code-value').textContent = data.formatted_code;
        document.getElementById('code-result').classList.remove('hide');
      }} catch (e) {{
        errEl.textContent = e.message;
        errEl.classList.remove('hide');
      }} finally {{
        btn.disabled = false;
      }}
    }}

    async function copyCode() {{
      const txt = document.getElementById('code-value').textContent.trim();
      try {{
        await navigator.clipboard.writeText(txt);
        const b = document.getElementById('t-copy-code');
        b.textContent = T.copied;
        setTimeout(() => {{ b.textContent = T.copyCode; }}, 1800);
      }} catch {{}}
    }}

    async function poll() {{
      try {{
        const res = await fetch(`/pair/${{TOKEN}}/status`, {{ cache: 'no-store' }});
        if (!res.ok) {{
          document.getElementById('status-pill').textContent = T.expired;
          document.getElementById('qr-loading').textContent = T.expiredMsg;
          document.getElementById('qr-loading').classList.remove('hide');
          document.getElementById('qr-img').classList.add('hide');
          clearInterval(pollHandle);
          return;
        }}
        const st = await res.json();
        if (st.status === 'connected') {{
          document.getElementById('status-pill').textContent = T.connected;
          document.getElementById('status-pill').classList.add('ok');
          document.getElementById('pairing-view').classList.add('hide');
          document.getElementById('connected-view').classList.remove('hide');
          clearInterval(pollHandle);
          return;
        }}
        if (st.qr_available) {{
          const key = st.qr_expires_at || String(Date.now());
          if (key !== lastQrKey) {{
            lastQrKey = key;
            const img = document.getElementById('qr-img');
            img.src = `/pair/${{TOKEN}}/qr.svg?t=${{Date.now()}}`;
            img.classList.remove('hide');
            document.getElementById('qr-loading').classList.add('hide');
          }}
          if (st.qr_expires_at) {{
            const secs = Math.max(0, Math.round((new Date(st.qr_expires_at).getTime() - Date.now()) / 1000));
            document.getElementById('qr-timer').textContent = secs > 0 ? T.autoIn.replace('{{s}}', String(secs)) : T.autoNow;
          }}
        }} else if ((st.status === 'disconnected' || st.status === 'logged_out' || st.status_reason === 'qr_expired') && !autoReconnectInFlight) {{
          autoReconnectInFlight = true;
          document.getElementById('qr-timer').textContent = T.renewing;
          try {{
            await fetch(`/pair/${{TOKEN}}/reconnect`, {{ method: 'POST' }});
          }} finally {{
            setTimeout(() => {{ autoReconnectInFlight = false; }}, 4000);
          }}
        }}
      }} catch {{}}
    }}

    poll();
    pollHandle = setInterval(poll, 2000);
  </script>
</body>
</html>"#,
        session_name = session_name,
        token_json = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".to_string()),
    )
}
