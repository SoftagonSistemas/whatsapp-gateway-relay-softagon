use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use thiserror::Error;

const DISALLOWED_API_KEYS: &[&str] = &[
    "change-this-before-starting",
    "change-me-to-a-strong-secret",
    "replace-with-a-strong-random-key",
    "replace-with-a-strong-secret",
    "troque-por-uma-chave-forte",
    "cambia-por-una-clave-segura",
    "remplacez-par-une-cle-forte",
    "your-strong-secret",
    "your-generated-64-char-hex-key",
    "sua-chave-gerada-com-openssl",
    "sua-chave-aqui",
    "changeme",
    "secret",
    "password",
    "admin",
    "test",
];
const MIN_API_KEY_LEN: usize = 16;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(&'static str),
    #[error("invalid value for {key}: {reason}")]
    Invalid { key: &'static str, reason: String },
    #[error("failed to create runtime directory {path}: {source}")]
    DirCreate {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_host: String,
    pub app_port: u16,
    pub api_key: String,
    pub data_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub docs_enabled: bool,
    pub docs_public: bool,
    pub media_max_upload_bytes: usize,
    pub session_rate_limit_per_minute: u32,
    pub sqlite: SqliteConfig,
    pub media: MediaConfig,
    pub transcription: TranscriptionConfig,
    pub webhooks: WebhookConfig,
    pub mcp: McpConfig,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub http_enabled: bool,
    pub http_host: String,
    pub http_port: u16,
    pub allow_container_bind: bool,
    pub allow_destructive: bool,
    pub allowed_origins: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            http_enabled: false,
            http_host: "127.0.0.1".to_string(),
            http_port: 8091,
            allow_container_bind: false,
            allow_destructive: false,
            allowed_origins: Vec::new(),
        }
    }
}

impl McpConfig {
    pub fn bind_addr(&self) -> Result<SocketAddr, ConfigError> {
        let addr: SocketAddr = format!("{}:{}", self.http_host, self.http_port)
            .parse()
            .map_err(|e| ConfigError::Invalid {
                key: "MCP_HTTP_HOST",
                reason: format!("invalid socket address: {e}"),
            })?;
        if !addr.ip().is_loopback() && !self.allow_container_bind {
            return Err(ConfigError::Invalid {
                key: "MCP_HTTP_HOST",
                reason: format!(
                    "refusing to bind MCP HTTP server to non-loopback address '{}'; use 127.0.0.1 or ::1 (or set MCP_ALLOW_CONTAINER_BIND=true when publishing -p 127.0.0.1:8091:8091 in Docker)",
                    self.http_host
                ),
            });
        }
        Ok(addr)
    }
}

#[derive(Debug, Clone)]
pub struct SqliteConfig {
    pub relay_database_path: PathBuf,
    pub whatsapp_database_path: PathBuf,
    pub busy_timeout_ms: u64,
    pub journal_mode: JournalMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalMode {
    Wal,
    Delete,
    Truncate,
    Memory,
}

impl JournalMode {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "wal" => Ok(Self::Wal),
            "delete" => Ok(Self::Delete),
            "truncate" => Ok(Self::Truncate),
            "memory" => Ok(Self::Memory),
            other => Err(ConfigError::Invalid {
                key: "SQLITE_JOURNAL_MODE",
                reason: format!(
                    "unsupported journal mode '{other}', expected wal|delete|truncate|memory"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MediaBackendConfig {
    Local {
        path: PathBuf,
    },
    S3 {
        endpoint_url: Option<String>,
        region: String,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
        prefix: String,
    },
}

#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub backend: MediaBackendConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperModel {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

impl WhisperModel {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tiny" => Ok(Self::Tiny),
            "base" => Ok(Self::Base),
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            "large" => Ok(Self::Large),
            other => Err(ConfigError::Invalid {
                key: "TRANSCRIPTION_MODEL",
                reason: format!(
                    "unsupported model '{other}', expected tiny|base|small|medium|large"
                ),
            }),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn ggml_filename(&self) -> &'static str {
        match self {
            Self::Tiny => "ggml-tiny.bin",
            Self::Base => "ggml-base.bin",
            Self::Small => "ggml-small.bin",
            Self::Medium => "ggml-medium.bin",
            Self::Large => "ggml-large-v3.bin",
        }
    }

    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.ggml_filename()
        )
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub enabled: bool,
    pub engine: String,
    pub model: WhisperModel,
    pub language: Option<String>,
    pub model_dir: PathBuf,
    pub preload: bool,
    pub idle_unload_seconds: u64,
    pub threads: usize,
    pub workers: usize,
    pub max_queue: usize,
    pub timeout_seconds: u64,
    pub delete_temp_audio: bool,
}

#[derive(Debug, Clone)]
pub struct WebhookConfig {
    pub delivery_timeout_seconds: u64,
    pub max_attempts: u32,
    pub backoff_base_seconds: u64,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let app_host = lookup_or(&lookup, "APP_HOST", "0.0.0.0");
        let app_port = parse_u16(&lookup, "APP_PORT", 8080)?;

        let api_key = lookup("RELAY_API_KEY")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or(ConfigError::Missing("RELAY_API_KEY"))?;

        let normalized_key = api_key.to_ascii_lowercase();
        if DISALLOWED_API_KEYS.contains(&normalized_key.as_str()) {
            return Err(ConfigError::Invalid {
                key: "RELAY_API_KEY",
                reason: format!(
                    "refusing to start with known example/placeholder key '{api_key}'; generate a random key with `openssl rand -hex 32`"
                ),
            });
        }
        if (api_key.starts_with("${") && api_key.ends_with('}'))
            || (api_key.starts_with('<') && api_key.ends_with('>'))
        {
            return Err(ConfigError::Invalid {
                key: "RELAY_API_KEY",
                reason: format!(
                    "refusing to start with unexpanded placeholder '{api_key}'; generate a real key with `openssl rand -hex 32`"
                ),
            });
        }
        if api_key.len() < MIN_API_KEY_LEN {
            return Err(ConfigError::Invalid {
                key: "RELAY_API_KEY",
                reason: format!(
                    "key is too short ({} chars, minimum {MIN_API_KEY_LEN}); generate a random key with `openssl rand -hex 32`",
                    api_key.len()
                ),
            });
        }

        let data_dir = PathBuf::from(lookup_or(&lookup, "DATA_DIR", "/data"));
        let tmp_dir = data_dir.join("tmp");
        let docs_enabled = parse_bool(&lookup, "DOCS_ENABLED", true)?;
        let docs_public = parse_bool(&lookup, "DOCS_PUBLIC", false)?;
        let media_max_upload_bytes =
            parse_usize(&lookup, "MEDIA_MAX_UPLOAD_BYTES", 25 * 1024 * 1024)?;
        if media_max_upload_bytes == 0 {
            return Err(ConfigError::Invalid {
                key: "MEDIA_MAX_UPLOAD_BYTES",
                reason: "must be greater than 0".into(),
            });
        }
        let session_rate_limit_per_minute =
            parse_u32(&lookup, "SESSION_RATE_LIMIT_PER_MINUTE", 60)?;

        let relay_database_path = PathBuf::from(lookup_or(
            &lookup,
            "RELAY_DATABASE_PATH",
            &data_dir.join("relay.db").to_string_lossy(),
        ));
        let whatsapp_database_path = PathBuf::from(lookup_or(
            &lookup,
            "WHATSAPP_DATABASE_PATH",
            &data_dir.join("whatsapp-sessions.db").to_string_lossy(),
        ));
        let busy_timeout_ms = parse_u64(&lookup, "SQLITE_BUSY_TIMEOUT_MS", 5000)?;
        let journal_mode = JournalMode::parse(&lookup_or(&lookup, "SQLITE_JOURNAL_MODE", "wal"))?;

        let media_storage = lookup_or(&lookup, "MEDIA_STORAGE", "local").to_ascii_lowercase();
        let media = match media_storage.as_str() {
            "local" => {
                let path = PathBuf::from(lookup_or(
                    &lookup,
                    "MEDIA_LOCAL_PATH",
                    &data_dir.join("media").to_string_lossy(),
                ));
                MediaConfig {
                    backend: MediaBackendConfig::Local { path },
                }
            }
            "s3" => {
                let bucket = required_non_empty(&lookup, "S3_BUCKET")?;
                let access_key_id = required_non_empty(&lookup, "S3_ACCESS_KEY_ID")?;
                let secret_access_key = required_non_empty(&lookup, "S3_SECRET_ACCESS_KEY")?;
                let region = lookup_or(&lookup, "S3_REGION", "auto");
                let endpoint_url = lookup("S3_ENDPOINT_URL")
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty());
                let prefix = lookup_or(&lookup, "S3_PREFIX", "softagon-relay/");
                MediaConfig {
                    backend: MediaBackendConfig::S3 {
                        endpoint_url,
                        region,
                        bucket,
                        access_key_id,
                        secret_access_key,
                        prefix,
                    },
                }
            }
            other => {
                return Err(ConfigError::Invalid {
                    key: "MEDIA_STORAGE",
                    reason: format!("unsupported backend '{other}', expected local|s3"),
                });
            }
        };

        let tx_enabled = parse_bool(&lookup, "TRANSCRIPTION_ENABLED", true)?;
        let tx_engine = lookup_or(&lookup, "TRANSCRIPTION_ENGINE", "whisper").to_ascii_lowercase();
        if tx_engine != "whisper" {
            return Err(ConfigError::Invalid {
                key: "TRANSCRIPTION_ENGINE",
                reason: format!("unsupported engine '{tx_engine}', expected whisper"),
            });
        }
        let tx_model = WhisperModel::parse(&lookup_or(&lookup, "TRANSCRIPTION_MODEL", "base"))?;
        let tx_language = {
            let raw = lookup_or(&lookup, "TRANSCRIPTION_LANGUAGE", "auto");
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
                None
            } else {
                Some(trimmed.to_string())
            }
        };
        let tx_model_dir = PathBuf::from(lookup_or(
            &lookup,
            "TRANSCRIPTION_MODEL_DIR",
            &data_dir.join("models").to_string_lossy(),
        ));
        let tx_preload = parse_bool(&lookup, "TRANSCRIPTION_PRELOAD", false)?;
        let tx_idle_unload = parse_u64(&lookup, "TRANSCRIPTION_IDLE_UNLOAD_SECONDS", 180)?;
        let tx_threads = parse_usize(&lookup, "TRANSCRIPTION_THREADS", 1)?;
        let tx_workers = parse_usize(&lookup, "TRANSCRIPTION_WORKERS", 1)?;
        let tx_max_queue = parse_usize(&lookup, "TRANSCRIPTION_MAX_QUEUE", 20)?;
        let tx_timeout = parse_u64(&lookup, "TRANSCRIPTION_TIMEOUT_SECONDS", 180)?;
        let tx_delete_temp = parse_bool(&lookup, "TRANSCRIPTION_DELETE_TEMP_AUDIO", true)?;

        if tx_threads == 0 {
            return Err(ConfigError::Invalid {
                key: "TRANSCRIPTION_THREADS",
                reason: "must be >= 1".into(),
            });
        }
        if tx_workers == 0 {
            return Err(ConfigError::Invalid {
                key: "TRANSCRIPTION_WORKERS",
                reason: "must be >= 1".into(),
            });
        }
        if tx_max_queue == 0 {
            return Err(ConfigError::Invalid {
                key: "TRANSCRIPTION_MAX_QUEUE",
                reason: "must be >= 1".into(),
            });
        }

        let webhook_timeout = parse_u64(&lookup, "WEBHOOK_DELIVERY_TIMEOUT_SECONDS", 10)?;
        let webhook_max_attempts = parse_u32(&lookup, "WEBHOOK_MAX_ATTEMPTS", 8)?;
        let webhook_backoff_base = parse_u64(&lookup, "WEBHOOK_BACKOFF_BASE_SECONDS", 2)?;

        if webhook_timeout == 0 {
            return Err(ConfigError::Invalid {
                key: "WEBHOOK_DELIVERY_TIMEOUT_SECONDS",
                reason: "must be >= 1".into(),
            });
        }
        if webhook_max_attempts == 0 {
            return Err(ConfigError::Invalid {
                key: "WEBHOOK_MAX_ATTEMPTS",
                reason: "must be >= 1".into(),
            });
        }

        let mcp_http_enabled = parse_bool(&lookup, "MCP_HTTP_ENABLED", false)?;
        let mcp_http_host = lookup_or(&lookup, "MCP_HTTP_HOST", "127.0.0.1");
        let mcp_http_port = parse_u16(&lookup, "MCP_HTTP_PORT", 8091)?;
        let mcp_allow_container_bind = parse_bool(&lookup, "MCP_ALLOW_CONTAINER_BIND", false)?;
        let mcp_allow_destructive = parse_bool(&lookup, "MCP_ALLOW_DESTRUCTIVE", false)?;
        let mcp_allowed_origins = lookup("MCP_ALLOWED_ORIGINS")
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mcp = McpConfig {
            http_enabled: mcp_http_enabled,
            http_host: mcp_http_host,
            http_port: mcp_http_port,
            allow_container_bind: mcp_allow_container_bind,
            allow_destructive: mcp_allow_destructive,
            allowed_origins: mcp_allowed_origins,
        };
        if mcp.http_enabled {
            mcp.bind_addr()?;
        }

        Ok(Self {
            app_host,
            app_port,
            api_key,
            data_dir,
            tmp_dir,
            docs_enabled,
            docs_public,
            media_max_upload_bytes,
            session_rate_limit_per_minute,
            sqlite: SqliteConfig {
                relay_database_path,
                whatsapp_database_path,
                busy_timeout_ms,
                journal_mode,
            },
            media,
            transcription: TranscriptionConfig {
                enabled: tx_enabled,
                engine: tx_engine,
                model: tx_model,
                language: tx_language,
                model_dir: tx_model_dir,
                preload: tx_preload,
                idle_unload_seconds: tx_idle_unload,
                threads: tx_threads,
                workers: tx_workers,
                max_queue: tx_max_queue,
                timeout_seconds: tx_timeout,
                delete_temp_audio: tx_delete_temp,
            },
            webhooks: WebhookConfig {
                delivery_timeout_seconds: webhook_timeout,
                max_attempts: webhook_max_attempts,
                backoff_base_seconds: webhook_backoff_base,
            },
            mcp,
        })
    }

    pub fn bind_addr(&self) -> Result<SocketAddr, ConfigError> {
        format!("{}:{}", self.app_host, self.app_port)
            .parse()
            .map_err(|e| ConfigError::Invalid {
                key: "APP_HOST",
                reason: format!("invalid socket address: {e}"),
            })
    }

    pub fn ensure_directories(&self) -> Result<(), ConfigError> {
        ensure_dir(&self.data_dir)?;
        ensure_dir(&self.tmp_dir)?;
        ensure_dir(&self.transcription.model_dir)?;
        if let Some(parent) = self.sqlite.relay_database_path.parent()
            && !parent.as_os_str().is_empty()
        {
            ensure_dir(parent)?;
        }
        if let Some(parent) = self.sqlite.whatsapp_database_path.parent()
            && !parent.as_os_str().is_empty()
        {
            ensure_dir(parent)?;
        }
        if let MediaBackendConfig::Local { path } = &self.media.backend {
            ensure_dir(path)?;
        }
        Ok(())
    }
}

fn ensure_dir(path: &Path) -> Result<(), ConfigError> {
    std::fs::create_dir_all(path).map_err(|source| ConfigError::DirCreate {
        path: path.to_path_buf(),
        source,
    })
}

fn lookup_or<F: Fn(&str) -> Option<String>>(lookup: &F, key: &str, default: &str) -> String {
    lookup(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn required_non_empty<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
) -> Result<String, ConfigError> {
    lookup(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or(ConfigError::Missing(key))
}

fn parse_bool<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match lookup(key).map(|v| v.trim().to_ascii_lowercase()) {
        None => Ok(default),
        Some(v) if v.is_empty() => Ok(default),
        Some(v) if matches!(v.as_str(), "true" | "1" | "yes") => Ok(true),
        Some(v) if matches!(v.as_str(), "false" | "0" | "no") => Ok(false),
        Some(other) => Err(ConfigError::Invalid {
            key,
            reason: format!("invalid boolean '{other}'"),
        }),
    }
}

fn parse_u16<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
    default: u16,
) -> Result<u16, ConfigError> {
    match lookup(key).map(|v| v.trim().to_string()) {
        None => Ok(default),
        Some(v) if v.is_empty() => Ok(default),
        Some(v) => v.parse().map_err(|e| ConfigError::Invalid {
            key,
            reason: format!("invalid u16 '{v}': {e}"),
        }),
    }
}

fn parse_u32<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
    match lookup(key).map(|v| v.trim().to_string()) {
        None => Ok(default),
        Some(v) if v.is_empty() => Ok(default),
        Some(v) => v.parse().map_err(|e| ConfigError::Invalid {
            key,
            reason: format!("invalid u32 '{v}': {e}"),
        }),
    }
}

fn parse_u64<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
    default: u64,
) -> Result<u64, ConfigError> {
    match lookup(key).map(|v| v.trim().to_string()) {
        None => Ok(default),
        Some(v) if v.is_empty() => Ok(default),
        Some(v) => v.parse().map_err(|e| ConfigError::Invalid {
            key,
            reason: format!("invalid u64 '{v}': {e}"),
        }),
    }
}

fn parse_usize<F: Fn(&str) -> Option<String>>(
    lookup: &F,
    key: &'static str,
    default: usize,
) -> Result<usize, ConfigError> {
    match lookup(key).map(|v| v.trim().to_string()) {
        None => Ok(default),
        Some(v) if v.is_empty() => Ok(default),
        Some(v) => v.parse().map_err(|e| ConfigError::Invalid {
            key,
            reason: format!("invalid usize '{v}': {e}"),
        }),
    }
}
