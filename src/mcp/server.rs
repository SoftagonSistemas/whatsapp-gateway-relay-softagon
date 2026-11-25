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
    /// Optional idempotency key to prevent duplicate sends on retries.
    pub idempotency_key: Option<String>,
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
        description = "Send a WhatsApp text message to a phone number or chat JID using a connected session. Audited in SQLite."
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
            "idempotency_key": input.idempotency_key,
        });
        self.audit_and_wrap("send_text_message", Some(&sid), args, || async {
            let msg = self
                .service
                .send_text_message(&input.session_id, &input.to, &input.text)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "message_id": msg.id,
                "session_id": msg.session_id,
                "chat_jid": msg.chat_jid,
                "status": msg.status,
                "created_at": msg.created_at,
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
                "Softagon Relay local WhatsApp MCP server. Discover sessions with list_sessions, inspect recent conversations with list_chats/search_messages/get_chat_messages, read audio transcriptions with get_message_transcription, and send WhatsApp messages with send_text_message.",
            )
    }
}
