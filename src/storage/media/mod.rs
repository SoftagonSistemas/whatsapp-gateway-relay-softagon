use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::signer::Signer;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{MediaBackendConfig, MediaConfig};

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("object store error: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("invalid object path: {0}")]
    InvalidPath(#[from] object_store::path::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("media backend configuration error: {0}")]
    Config(String),
}

#[derive(Debug, Clone)]
pub struct StoredObjectMeta {
    pub object_key: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub backend: &'static str,
}

#[async_trait]
pub trait MediaStorage: Send + Sync {
    async fn put(&self, key: &str, data: Bytes) -> Result<StoredObjectMeta, MediaError>;
    async fn get(&self, key: &str) -> Result<Bytes, MediaError>;
    async fn delete(&self, key: &str) -> Result<(), MediaError>;
    fn backend_name(&self) -> &'static str;
    async fn presigned_get_url(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<String>, MediaError>;
}

pub struct ObjectStoreMediaStorage {
    store: Arc<dyn ObjectStore>,
    s3_signer: Option<Arc<AmazonS3>>,
    prefix: String,
    backend: &'static str,
}

impl ObjectStoreMediaStorage {
    pub fn from_config(cfg: &MediaConfig) -> Result<Self, MediaError> {
        match &cfg.backend {
            MediaBackendConfig::Local { path } => {
                std::fs::create_dir_all(path)?;
                let store = LocalFileSystem::new_with_prefix(path)?;
                Ok(Self {
                    store: Arc::new(store),
                    s3_signer: None,
                    prefix: String::new(),
                    backend: "local",
                })
            }
            MediaBackendConfig::S3 {
                endpoint_url,
                region,
                bucket,
                access_key_id,
                secret_access_key,
                prefix,
            } => {
                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(bucket)
                    .with_region(region)
                    .with_access_key_id(access_key_id)
                    .with_secret_access_key(secret_access_key);

                if let Some(ep) = endpoint_url {
                    let allow_http = ep.starts_with("http://");
                    builder = builder.with_endpoint(ep).with_allow_http(allow_http);
                }

                let s3 = Arc::new(builder.build()?);
                let normalized_prefix = prefix.trim_matches('/').to_string();

                Ok(Self {
                    store: s3.clone(),
                    s3_signer: Some(s3),
                    prefix: normalized_prefix,
                    backend: "s3",
                })
            }
        }
    }

    fn full_key(&self, key: &str) -> String {
        let trimmed = key.trim_start_matches('/');
        if self.prefix.is_empty() {
            trimmed.to_string()
        } else {
            format!("{}/{}", self.prefix, trimmed)
        }
    }
}

#[async_trait]
impl MediaStorage for ObjectStoreMediaStorage {
    async fn put(&self, key: &str, data: Bytes) -> Result<StoredObjectMeta, MediaError> {
        let full_key = self.full_key(key);
        let path = ObjectPath::parse(&full_key)?;
        let size_bytes = data.len() as i64;
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let sha256 = hex::encode(hasher.finalize());

        self.store.put(&path, PutPayload::from_bytes(data)).await?;

        Ok(StoredObjectMeta {
            object_key: full_key,
            size_bytes,
            sha256,
            backend: self.backend,
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, MediaError> {
        let path = ObjectPath::parse(key)?;
        let res = self.store.get(&path).await?;
        Ok(res.bytes().await?)
    }

    async fn delete(&self, key: &str) -> Result<(), MediaError> {
        let path = ObjectPath::parse(key)?;
        self.store.delete(&path).await?;
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        self.backend
    }

    async fn presigned_get_url(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<String>, MediaError> {
        let Some(signer) = &self.s3_signer else {
            return Ok(None);
        };
        let path = ObjectPath::parse(key)?;
        let url = signer.signed_url(http::Method::GET, &path, ttl).await?;
        Ok(Some(url.to_string()))
    }
}

pub fn build_media_object_key(session_id: &str, media_id: &str, extension: Option<&str>) -> String {
    match extension.filter(|e| !e.is_empty()) {
        Some(ext) => format!("{session_id}/{media_id}.{}", ext.trim_start_matches('.')),
        None => format!("{session_id}/{media_id}"),
    }
}
