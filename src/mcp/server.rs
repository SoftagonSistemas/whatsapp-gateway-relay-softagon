use std::time::Instant;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};

use crate::core::service::RelayService;

#[derive(Clone)]
pub struct RelayMcpServer {
    service: RelayService,
    transport: &'static str,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionIdInput {
    /// WhatsApp session ID (e.g., sess_019a...).
    pub session_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateSessionInput {
    /// Human-readable session name or phone number (e.g., support-sales).
    pub name: String,
    /// Optional ISO language code for local Whisper transcription (e.g., pt, en, id) or auto.
    pub transcription_language: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreatePairingLinkInput {
    /// WhatsApp session ID to pair.
    pub session_id: String,
    /// Link validity in seconds (60..1800, default 600 = 10 minutes).
    pub ttl_seconds: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RequestPairCodeInput {
    /// WhatsApp session ID to pair.
    pub session_id: String,
    /// WhatsApp phone number with country code (digits only or E.164, e.g., 5511999990001).
    pub phone_number: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListChatsInput {
    /// WhatsApp session ID.
    pub session_id: String,
    /// Optional search query matching chat JID, contact phone, message text, caption, or transcription.
    pub query: Option<String>,
    /// Maximum number of recent chats to return (1..200, default 20).
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchMessagesInput {
    /// WhatsApp session ID.
    pub session_id: String,
    /// Case-insensitive search query (contact name/number in text/metadata, message text, or audio transcription).
    pub query: String,
    /// Maximum number of matching messages to return (1..200, default 20).
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetChatMessagesInput {
    /// WhatsApp session ID.
    pub session_id: String,
    /// Full chat JID (e.g., 5511999990001@s.whatsapp.net or group@g.us).
    pub chat_jid: String,
    /// Maximum number of recent messages to return (1..200, default 30).
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MessageIdInput {
    /// Relay message ID (e.g., msg_019a...).
    pub message_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendTextMessageInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Recipient phone number (with country code, e.g., 5511999990001) or full JID.
    pub to: String,
    /// Message text body to send.
    pub text: String,
    /// Optional external WhatsApp message ID to quote/reply to.
    pub reply_to_message_id: Option<String>,
    /// Optional idempotency key to prevent duplicate sends on retries.
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendMediaMessageInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Recipient phone number (with country code, e.g., 5511999990001) or full JID.
    pub to: String,
    /// Media category: "image", "video", "audio", "document", or "sticker".
    pub media_type: String,
    /// Public HTTP/HTTPS URL to download the media from (alternative to data_base64).
    pub media_url: Option<String>,
    /// Base64-encoded media bytes (alternative to media_url).
    pub data_base64: Option<String>,
    /// Optional MIME type (inferred automatically from HTTP Content-Type or filename if omitted).
    pub mime_type: Option<String>,
    /// Optional filename (especially useful for documents).
    pub filename: Option<String>,
    /// Optional caption text.
    pub caption: Option<String>,
    /// When media_type is "audio", set true to send as a WhatsApp voice note (PTT).
    #[serde(default)]
    pub as_voice_note: bool,
    /// Optional external WhatsApp message ID to quote/reply to.
    pub reply_to_message_id: Option<String>,
    /// Optional idempotency key to prevent duplicate sends on retries.
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendReactionInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Chat JID where the target message lives.
    pub chat_jid: String,
    /// External WhatsApp message ID to react to.
    pub message_id: String,
    /// Emoji to react with (e.g., "👍", "❤️"), or empty string "" to remove an existing reaction.
    pub emoji: String,
    /// True if reacting to a message sent by this session itself.
    #[serde(default)]
    pub from_me: bool,
    /// Optional sender JID when reacting inside a group chat.
    pub participant: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendChatPresenceInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Chat JID or phone number.
    pub chat_jid: String,
    /// Presence state: "composing" (typing), "recording" (recording audio), or "paused".
    pub state: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendLocationInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Recipient phone number or chat JID.
    pub to: String,
    /// Latitude in decimal degrees (e.g., -23.55052).
    pub latitude: f64,
    /// Longitude in decimal degrees (e.g., -46.63331).
    pub longitude: f64,
    /// Optional place name.
    pub name: Option<String>,
    /// Optional street address.
    pub address: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SendContactInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Recipient phone number or chat JID.
    pub to: String,
    /// Contact display name.
    pub display_name: String,
    /// Contact phone number with country code (used to build a vCard if vcard is omitted).
    pub phone_number: Option<String>,
    /// Optional raw vCard 3.0 string.
    pub vcard: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct EditMessageInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Chat JID where the message was sent.
    pub chat_jid: String,
    /// External WhatsApp message ID of a previously sent text message.
    pub message_id: String,
    /// Updated text body.
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RevokeMessageInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Chat JID where the message was sent.
    pub chat_jid: String,
    /// External WhatsApp message ID to delete for everyone.
    pub message_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckWhatsAppInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// List of phone numbers with country code (e.g., ["5511999990001"]).
    pub phones: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetProfilePictureInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Contact JID, @lid, or phone number.
    pub jid_or_phone: String,
    /// When true, request the smaller preview thumbnail instead of full resolution.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetGroupInfoInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// WhatsApp group JID (e.g., 120363...@g.us).
    pub group_jid: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetContactInput {
    /// WhatsApp session ID.
    pub session_id: String,
    /// Contact @lid, full JID, or phone number to resolve.
    pub jid_or_phone: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MarkReadInput {
    /// Connected WhatsApp session ID.
    pub session_id: String,
    /// Chat JID to mark as read.
    pub chat_jid: String,
    /// Optional sender JID (for group chats).
    pub sender_jid: Option<String>,
    /// Optional list of external WhatsApp message IDs; when omitted, marks the latest inbound messages in the chat as read.
    #[serde(default)]
    pub message_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DestructiveSessionInput {
    /// WhatsApp session ID.
    pub session_id: String,
    /// Explicit confirmation flag required for destructive operations.
    pub confirm: bool,
}

#[tool_router]
impl RelayMcpServer {
    pub fn new(service: RelayService, transport: &'static str) -> Self {
        Self {
            service,
            transport,
            tool_router: Self::tool_router(),
        }
    }

    fn json_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string_pretty(value)
            .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    async fn audit_and_wrap<T, F, Fut>(
        &self,
        tool_name: &str,
        session_id: Option<&str>,
        args_value: serde_json::Value,
        op: F,
    ) -> Result<CallToolResult, McpError>
    where
        T: Serialize,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let start = Instant::now();
        let args_json = serde_json::to_string(&args_value).unwrap_or_else(|_| "{}".to_string());
        match op().await {
            Ok(data) => {
                let elapsed = start.elapsed().as_millis() as i64;
                self.service
                    .record_mcp_audit(
                        self.transport,
                        tool_name,
                        session_id,
                        &args_json,
                        "ok",
                        None,
                        elapsed,
                    )
                    .await;
                Self::json_result(&data)
            }
            Err(err_msg) => {
                let elapsed = start.elapsed().as_millis() as i64;
                self.service
                    .record_mcp_audit(
                        self.transport,
                        tool_name,
                        session_id,
                        &args_json,
                        "error",
                        Some(&err_msg),
                        elapsed,
                    )
                    .await;
                Ok(CallToolResult::error(vec![ContentBlock::text(err_msg)]))
            }
        }
    }

    /// List all WhatsApp sessions configured in this local Softagon Relay instance.
    #[tool(
        description = "List all WhatsApp sessions with their ID, name, connection status, phone number, and JID."
    )]
    async fn list_sessions(&self) -> Result<CallToolResult, McpError> {
        self.audit_and_wrap("list_sessions", None, serde_json::json!({}), || async {
            let sessions = self
                .service
                .list_sessions()
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "sessions": sessions }))
        })
        .await
    }

    /// Create and start a new WhatsApp session ready for QR or 8-digit code pairing.
    #[tool(
        description = "Create a new WhatsApp session and start its pairing cycle. Follow up with create_pairing_link (recommended QR flow) or request_pair_code."
    )]
    async fn create_session(
        &self,
        Parameters(input): Parameters<CreateSessionInput>,
    ) -> Result<CallToolResult, McpError> {
        let args = serde_json::json!({
            "name": input.name,
            "transcription_language": input.transcription_language,
        });
        self.audit_and_wrap("create_session", None, args, || async {
            let rec = self
                .service
                .create_session(&input.name, input.transcription_language.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            Ok(rec)
        })
        .await
    }

    /// Create a zero-login, short-lived pairing link (/pair/{token}) with an auto-renewing QR code.
    #[tool(
        description = "Create a temporary, zero-login pairing page URL (/pair/{token}) for a WhatsApp session. Recommended for opening in a browser (e.g. Grok Bot Chrome) without exposing RELAY_API_KEY: displays a large auto-renewing QR code for up to 10 minutes, plus a PNG image endpoint (/pair/{token}/qr.png) and an optional 8-digit code tab."
    )]
    async fn create_pairing_link(
        &self,
        Parameters(input): Parameters<CreatePairingLinkInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "ttl_seconds": input.ttl_seconds,
        });
        self.audit_and_wrap("create_pairing_link", Some(&sid), args, || async {
            let link = self
                .service
                .create_pairing_link(&input.session_id, input.ttl_seconds)
                .await
                .map_err(|e| e.to_string())?;
            let host = if self.service.config.app_host == "0.0.0.0" {
                "127.0.0.1"
            } else {
                self.service.config.app_host.as_str()
            };
            let base = format!("http://{}:{}", host, self.service.config.app_port);
            Ok(serde_json::json!({
                "session_id": link.session_id,
                "token": link.token,
                "pair_url": format!("{base}/pair/{}", link.token),
                "qr_svg_url": format!("{base}/pair/{}/qr.svg", link.token),
                "qr_png_url": format!("{base}/pair/{}/qr.png", link.token),
                "status_url": format!("{base}/pair/{}/status", link.token),
                "expires_at": link.expires_at,
                "note": "Open pair_url directly in Chrome (no login or RELAY_API_KEY needed). The QR code auto-renews until connected or link expiry."
            }))
        })
        .await
    }

    /// Request an 8-digit WhatsApp pairing code as a camera-free alternative to scanning a QR code.
    #[tool(
        description = "Request an 8-character WhatsApp phone pairing code (XXXX-XXXX, valid ~3 minutes) as an alternative when the user cannot scan a QR code with a camera."
    )]
    async fn request_pair_code(
        &self,
        Parameters(input): Parameters<RequestPairCodeInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "phone_number": input.phone_number,
        });
        self.audit_and_wrap("request_pair_code", Some(&sid), args, || async {
            let payload = self
                .service
                .request_pair_code(&input.session_id, &input.phone_number)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "code": payload.code,
                "formatted_code": payload.formatted_code,
                "expires_at": payload.expires_at,
                "instructions": "In WhatsApp on your phone: Linked devices -> Link a device -> Link with phone number instead -> enter formatted_code."
            }))
        })
        .await
    }

    /// Get live connection status for a specific WhatsApp session.
    #[tool(
        description = "Get the live connection status, phone number, and JID of a WhatsApp session."
    )]
    async fn get_session_status(
        &self,
        Parameters(input): Parameters<SessionIdInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        self.audit_and_wrap(
            "get_session_status",
            Some(&sid),
            serde_json::json!({ "session_id": sid }),
            || async {
                let (record, live) = self
                    .service
                    .get_session_status(&input.session_id)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({
                    "session_id": record.id,
                    "name": record.name,
                    "status": live,
                    "status_reason": record.status_reason,
                    "phone_number": record.phone_number,
                    "jid": record.jid,
                }))
            },
        )
        .await
    }

    /// List recent WhatsApp chats/conversations for a session, with optional filter.
    #[tool(
        description = "List recent WhatsApp conversations for a session, optionally filtered by a query matching contact JID, phone number, message text, or transcription."
    )]
    async fn list_chats(
        &self,
        Parameters(input): Parameters<ListChatsInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let limit = input.limit.unwrap_or(20);
        let args = serde_json::json!({
            "session_id": sid,
            "query": input.query,
            "limit": limit,
        });
        self.audit_and_wrap("list_chats", Some(&sid), args, || async {
            let chats = self
                .service
                .list_chats(&input.session_id, input.query.as_deref(), limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "session_id": input.session_id, "chats": chats }))
        })
        .await
    }

    /// Search messages across a WhatsApp session by text, caption, transcription, or participant.
    #[tool(
        description = "Search WhatsApp messages in a session by text, caption, audio transcription, sender, or chat JID."
    )]
    async fn search_messages(
        &self,
        Parameters(input): Parameters<SearchMessagesInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let limit = input.limit.unwrap_or(20);
        let args = serde_json::json!({
            "session_id": sid,
            "query": input.query,
            "limit": limit,
        });
        self.audit_and_wrap("search_messages", Some(&sid), args, || async {
            let messages = self
                .service
                .search_messages(&input.session_id, &input.query, limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "session_id": input.session_id, "messages": messages }))
        })
        .await
    }

    /// Retrieve recent messages (including voice-note transcriptions) from a specific chat JID.
    #[tool(
        description = "Get recent messages and voice-note transcriptions from a specific WhatsApp chat JID."
    )]
    async fn get_chat_messages(
        &self,
        Parameters(input): Parameters<GetChatMessagesInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let limit = input.limit.unwrap_or(30);
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "limit": limit,
        });
        self.audit_and_wrap("get_chat_messages", Some(&sid), args, || async {
            let messages = self
                .service
                .get_chat_messages(&input.session_id, &input.chat_jid, limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "messages": messages,
            }))
        })
        .await
    }

    /// Retrieve the local Whisper transcription for a voice note message.
    #[tool(
        description = "Get the offline Whisper transcription for a specific WhatsApp audio message ID."
    )]
    async fn get_message_transcription(
        &self,
        Parameters(input): Parameters<MessageIdInput>,
    ) -> Result<CallToolResult, McpError> {
        let args = serde_json::json!({ "message_id": input.message_id });
        self.audit_and_wrap("get_message_transcription", None, args, || async {
            let transcription = self
                .service
                .get_message_transcription(&input.message_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "message_id": input.message_id,
                "transcription": transcription,
            }))
        })
        .await
    }

    /// Send a WhatsApp text message from a connected session.
    #[tool(
        description = "Send a WhatsApp text message to a phone number or chat JID using a connected session. Supports quoting/replying via reply_to_message_id and idempotency_key. Audited in SQLite."
    )]
    async fn send_text_message(
        &self,
        Parameters(input): Parameters<SendTextMessageInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let preview: String = input.text.chars().take(120).collect();
        let args = serde_json::json!({
            "session_id": sid,
            "to": input.to,
            "text_preview": preview,
            "reply_to_message_id": input.reply_to_message_id,
            "idempotency_key": input.idempotency_key,
        });
        self.audit_and_wrap("send_text_message", Some(&sid), args, || async {
            let msg = self
                .service
                .send_text_message_with_reply(
                    &input.session_id,
                    &input.to,
                    &input.text,
                    input.reply_to_message_id.as_deref(),
                    input.idempotency_key.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "message_id": msg.id,
                "external_id": msg.external_id,
                "session_id": msg.session_id,
                "chat_jid": msg.chat_jid,
                "status": msg.status,
                "created_at": msg.created_at,
            }))
        })
        .await
    }

    /// Send an image, video, audio (or voice note), document, or sticker via HTTP/HTTPS URL or base64.
    #[tool(
        description = "Send a WhatsApp media message (image, video, audio, document, or sticker) using either a public media_url or data_base64. Set as_voice_note=true for audio PTT voice notes and reply_to_message_id to quote a message. Audited in SQLite."
    )]
    async fn send_media_message(
        &self,
        Parameters(input): Parameters<SendMediaMessageInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "to": input.to,
            "media_type": input.media_type,
            "media_url": input.media_url,
            "has_base64": input.data_base64.as_ref().is_some_and(|s| !s.is_empty()),
            "mime_type": input.mime_type,
            "filename": input.filename,
            "as_voice_note": input.as_voice_note,
            "reply_to_message_id": input.reply_to_message_id,
            "idempotency_key": input.idempotency_key,
        });
        self.audit_and_wrap("send_media_message", Some(&sid), args, || async {
            let kind = match input.media_type.trim().to_ascii_lowercase().as_str() {
                "image" => crate::engines::whatsapp::MediaKind::Image,
                "video" => crate::engines::whatsapp::MediaKind::Video,
                "audio" => crate::engines::whatsapp::MediaKind::Audio,
                "document" => crate::engines::whatsapp::MediaKind::Document,
                "sticker" => crate::engines::whatsapp::MediaKind::Sticker,
                other => {
                    return Err(format!(
                        "unsupported media_type '{other}': expected image, video, audio, document, or sticker"
                    ));
                }
            };
            let msg = self
                .service
                .send_media_message_with_options(
                    &input.session_id,
                    &input.to,
                    kind,
                    input.mime_type.as_deref(),
                    input.filename.as_deref(),
                    input.caption.as_deref(),
                    input.data_base64.as_deref(),
                    input.media_url.as_deref(),
                    input.as_voice_note,
                    input.reply_to_message_id.as_deref(),
                    input.idempotency_key.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "message_id": msg.id,
                "external_id": msg.external_id,
                "session_id": msg.session_id,
                "chat_jid": msg.chat_jid,
                "media_id": msg.media_id,
                "message_type": msg.message_type,
                "status": msg.status,
                "created_at": msg.created_at,
            }))
        })
        .await
    }

    /// React to a WhatsApp message with an emoji (or remove a reaction by passing an empty emoji string).
    #[tool(
        description = "Send an emoji reaction to a WhatsApp message (pass empty string '' to remove an existing reaction)."
    )]
    async fn send_reaction(
        &self,
        Parameters(input): Parameters<SendReactionInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "message_id": input.message_id,
            "emoji": input.emoji,
            "from_me": input.from_me,
        });
        self.audit_and_wrap("send_reaction", Some(&sid), args, || async {
            let ext_id = self
                .service
                .send_reaction(
                    &input.session_id,
                    &input.chat_jid,
                    &input.message_id,
                    &input.emoji,
                    input.from_me,
                    input.participant.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "target_message_id": input.message_id,
                "emoji": input.emoji,
                "external_id": ext_id,
            }))
        })
        .await
    }

    /// Send typing ("composing"), recording audio ("recording"), or "paused" indicator to a chat.
    #[tool(
        description = "Send a chat presence indicator ('composing', 'recording', or 'paused') to a WhatsApp chat."
    )]
    async fn send_chat_presence(
        &self,
        Parameters(input): Parameters<SendChatPresenceInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "state": input.state,
        });
        self.audit_and_wrap("send_chat_presence", Some(&sid), args, || async {
            let state = match input.state.trim().to_ascii_lowercase().as_str() {
                "composing" | "typing" => crate::engines::whatsapp::ChatPresenceState::Composing,
                "recording" => crate::engines::whatsapp::ChatPresenceState::Recording,
                "paused" => crate::engines::whatsapp::ChatPresenceState::Paused,
                other => {
                    return Err(format!(
                        "invalid state '{other}': expected composing, recording, or paused"
                    ));
                }
            };
            self.service
                .send_chat_presence(&input.session_id, &input.chat_jid, state)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "state": state,
            }))
        })
        .await
    }

    /// Send a geographic location pin (latitude/longitude + optional name/address) to a WhatsApp chat.
    #[tool(
        description = "Send a geographic location pin (latitude, longitude, optional name and address) to a WhatsApp chat."
    )]
    async fn send_location(
        &self,
        Parameters(input): Parameters<SendLocationInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "to": input.to,
            "latitude": input.latitude,
            "longitude": input.longitude,
            "name": input.name,
        });
        self.audit_and_wrap("send_location", Some(&sid), args, || async {
            let msg = self
                .service
                .send_location(
                    &input.session_id,
                    &input.to,
                    input.latitude,
                    input.longitude,
                    input.name.as_deref(),
                    input.address.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(msg)
        })
        .await
    }

    /// Send a contact card (vCard) to a WhatsApp chat.
    #[tool(
        description = "Send a WhatsApp contact card (vCard) using display_name + phone_number (or raw vcard)."
    )]
    async fn send_contact(
        &self,
        Parameters(input): Parameters<SendContactInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "to": input.to,
            "display_name": input.display_name,
        });
        self.audit_and_wrap("send_contact", Some(&sid), args, || async {
            let msg = self
                .service
                .send_contact(
                    &input.session_id,
                    &input.to,
                    &input.display_name,
                    input.phone_number.as_deref(),
                    input.vcard.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(msg)
        })
        .await
    }

    /// Edit a previously sent WhatsApp text message.
    #[tool(
        description = "Edit the text of a previously sent WhatsApp message by its external message_id."
    )]
    async fn edit_message(
        &self,
        Parameters(input): Parameters<EditMessageInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "message_id": input.message_id,
        });
        self.audit_and_wrap("edit_message", Some(&sid), args, || async {
            let edit_id = self
                .service
                .edit_message(
                    &input.session_id,
                    &input.chat_jid,
                    &input.message_id,
                    &input.text,
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "message_id": input.message_id,
                "edit_id": edit_id,
                "status": "edited",
            }))
        })
        .await
    }

    /// Delete (revoke for everyone) a previously sent WhatsApp message.
    #[tool(
        description = "Delete/revoke a previously sent WhatsApp message for everyone by its external message_id."
    )]
    async fn revoke_message(
        &self,
        Parameters(input): Parameters<RevokeMessageInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "message_id": input.message_id,
        });
        self.audit_and_wrap("revoke_message", Some(&sid), args, || async {
            self.service
                .revoke_message(&input.session_id, &input.chat_jid, &input.message_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "message_id": input.message_id,
                "status": "revoked",
            }))
        })
        .await
    }

    /// Check whether one or more phone numbers are registered on WhatsApp.
    #[tool(
        description = "Check whether a list of phone numbers (with country code) are registered on WhatsApp."
    )]
    async fn check_whatsapp(
        &self,
        Parameters(input): Parameters<CheckWhatsAppInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "phones_count": input.phones.len(),
        });
        self.audit_and_wrap("check_whatsapp", Some(&sid), args, || async {
            let results = self
                .service
                .is_on_whatsapp(&input.session_id, &input.phones)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "results": results,
            }))
        })
        .await
    }

    /// Fetch the profile picture URL for a WhatsApp contact or group.
    #[tool(
        description = "Get the profile picture URL for a WhatsApp contact JID, @lid, phone number, or group."
    )]
    async fn get_profile_picture(
        &self,
        Parameters(input): Parameters<GetProfilePictureInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "jid_or_phone": input.jid_or_phone,
            "preview": input.preview,
        });
        self.audit_and_wrap("get_profile_picture", Some(&sid), args, || async {
            let picture = self
                .service
                .get_profile_picture(&input.session_id, &input.jid_or_phone, input.preview)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "jid_or_phone": input.jid_or_phone,
                "picture": picture,
            }))
        })
        .await
    }

    /// List all WhatsApp groups this session participates in.
    #[tool(
        description = "List all WhatsApp groups the session participates in, including subject and participant lists."
    )]
    async fn list_groups(
        &self,
        Parameters(input): Parameters<SessionIdInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        self.audit_and_wrap(
            "list_groups",
            Some(&sid),
            serde_json::json!({ "session_id": sid }),
            || async {
                let groups = self
                    .service
                    .list_groups(&input.session_id)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({
                    "session_id": input.session_id,
                    "groups": groups,
                }))
            },
        )
        .await
    }

    /// Fetch metadata and participants for a specific WhatsApp group JID.
    #[tool(
        description = "Get metadata (subject, description, participants, admins) for a specific WhatsApp group JID."
    )]
    async fn get_group_info(
        &self,
        Parameters(input): Parameters<GetGroupInfoInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "group_jid": input.group_jid,
        });
        self.audit_and_wrap("get_group_info", Some(&sid), args, || async {
            self.service
                .get_group_info(&input.session_id, &input.group_jid)
                .await
                .map_err(|e| e.to_string())
        })
        .await
    }

    /// Resolve a WhatsApp contact (@lid, JID, or phone number) to its phone number, canonical JID, and display name.
    #[tool(
        description = "Resolve a WhatsApp @lid, JID, or phone number into canonical phone_number, jid, lid, and push_name using WhatsApp's LID-to-PN mapping and message history."
    )]
    async fn get_contact(
        &self,
        Parameters(input): Parameters<GetContactInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "jid_or_phone": input.jid_or_phone,
        });
        self.audit_and_wrap("get_contact", Some(&sid), args, || async {
            self.service
                .get_contact(&input.session_id, &input.jid_or_phone)
                .await
                .map_err(|e| e.to_string())
        })
        .await
    }

    /// Mark WhatsApp messages in a chat as read (sends read receipts so the phone doesn't accumulate unread badges).
    #[tool(
        description = "Mark WhatsApp messages in a chat as read (sends read receipts). When message_ids is omitted, automatically marks the latest inbound messages in that chat as read."
    )]
    async fn mark_read(
        &self,
        Parameters(input): Parameters<MarkReadInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({
            "session_id": sid,
            "chat_jid": input.chat_jid,
            "sender_jid": input.sender_jid,
            "message_ids_count": input.message_ids.len(),
        });
        self.audit_and_wrap("mark_read", Some(&sid), args, || async {
            let marked_count = self
                .service
                .mark_read(
                    &input.session_id,
                    &input.chat_jid,
                    input.sender_jid.as_deref(),
                    input.message_ids.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "session_id": input.session_id,
                "chat_jid": input.chat_jid,
                "marked_count": marked_count,
            }))
        })
        .await
    }

    /// Disconnect and unlink a WhatsApp session (destructive; disabled unless MCP_ALLOW_DESTRUCTIVE=true and confirm=true).
    #[tool(
        description = "Destructive: disconnect and unlink a WhatsApp session. Disabled by default; requires MCP_ALLOW_DESTRUCTIVE=true and confirm=true."
    )]
    async fn logout_session(
        &self,
        Parameters(input): Parameters<DestructiveSessionInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({ "session_id": sid, "confirm": input.confirm });
        let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());

        if !self.service.config.mcp.allow_destructive {
            let msg = "destructive MCP operations are disabled (set MCP_ALLOW_DESTRUCTIVE=true to enable)".to_string();
            self.service
                .record_mcp_audit(
                    self.transport,
                    "logout_session",
                    Some(&sid),
                    &args_str,
                    "blocked",
                    Some(&msg),
                    0,
                )
                .await;
            return Ok(CallToolResult::error(vec![ContentBlock::text(msg)]));
        }
        if !input.confirm {
            let msg = "logout_session requires confirm=true".to_string();
            self.service
                .record_mcp_audit(
                    self.transport,
                    "logout_session",
                    Some(&sid),
                    &args_str,
                    "blocked",
                    Some(&msg),
                    0,
                )
                .await;
            return Ok(CallToolResult::error(vec![ContentBlock::text(msg)]));
        }

        self.audit_and_wrap("logout_session", Some(&sid), args, || async {
            self.service
                .logout_session(&input.session_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "session_id": input.session_id, "status": "logged_out" }))
        })
        .await
    }

    /// Delete a disconnected WhatsApp session (destructive; disabled unless MCP_ALLOW_DESTRUCTIVE=true and confirm=true).
    #[tool(
        description = "Destructive: permanently delete a disconnected WhatsApp session. Disabled by default; requires MCP_ALLOW_DESTRUCTIVE=true and confirm=true."
    )]
    async fn delete_session(
        &self,
        Parameters(input): Parameters<DestructiveSessionInput>,
    ) -> Result<CallToolResult, McpError> {
        let sid = input.session_id.clone();
        let args = serde_json::json!({ "session_id": sid, "confirm": input.confirm });
        let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());

        if !self.service.config.mcp.allow_destructive {
            let msg = "destructive MCP operations are disabled (set MCP_ALLOW_DESTRUCTIVE=true to enable)".to_string();
            self.service
                .record_mcp_audit(
                    self.transport,
                    "delete_session",
                    Some(&sid),
                    &args_str,
                    "blocked",
                    Some(&msg),
                    0,
                )
                .await;
            return Ok(CallToolResult::error(vec![ContentBlock::text(msg)]));
        }
        if !input.confirm {
            let msg = "delete_session requires confirm=true".to_string();
            self.service
                .record_mcp_audit(
                    self.transport,
                    "delete_session",
                    Some(&sid),
                    &args_str,
                    "blocked",
                    Some(&msg),
                    0,
                )
                .await;
            return Ok(CallToolResult::error(vec![ContentBlock::text(msg)]));
        }

        self.audit_and_wrap("delete_session", Some(&sid), args, || async {
            self.service
                .delete_session(&input.session_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "session_id": input.session_id, "deleted": true }))
        })
        .await
    }
}

#[tool_handler]
impl ServerHandler for RelayMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "softagon-relay",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Softagon Relay local WhatsApp MCP server. Manage sessions with list_sessions/create_session/get_session_status, pair without login via create_pairing_link (auto-renewing QR) or request_pair_code (8-digit code), inspect conversations with list_chats/search_messages/get_chat_messages/get_message_transcription, resolve contacts/groups with get_contact/check_whatsapp/get_profile_picture/list_groups/get_group_info, send read receipts and typing indicators with mark_read/send_chat_presence, and interact with send_text_message/send_media_message (with reply_to_message_id, as_voice_note, sticker, and idempotency_key), send_reaction, send_location, send_contact, edit_message, and revoke_message.",
            )
    }
}
