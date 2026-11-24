use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Notify;

use crate::storage::database::transcriptions::TranscriptionJobRecord;
use crate::storage::database::{Database, DbError};

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("transcription queue is full (max {0})")]
    QueueFull(usize),
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

#[derive(Clone)]
pub struct TranscriptionQueue {
    db: Database,
    max_queue: usize,
    notify: Arc<Notify>,
}

impl TranscriptionQueue {
    pub fn new(db: Database, max_queue: usize, notify: Arc<Notify>) -> Self {
        Self {
            db,
            max_queue,
            notify,
        }
    }

    pub async fn recover_stale_jobs(&self) -> Result<u64, DbError> {
        let count = self.db.recover_stale_transcription_jobs().await?;
        if count > 0 {
            self.notify.notify_waiters();
        }
        Ok(count)
    }

    pub async fn enqueue(
        &self,
        message_id: &str,
        session_id: &str,
        media_id: &str,
        language_override: Option<&str>,
    ) -> Result<TranscriptionJobRecord, QueueError> {
        let pending = self.db.count_pending_transcription_jobs().await? as usize;
        if pending >= self.max_queue {
            return Err(QueueError::QueueFull(self.max_queue));
        }
        let job_id = format!("txj_{}", uuid::Uuid::now_v7());
        let job = self
            .db
            .enqueue_transcription_job(&job_id, message_id, session_id, media_id, language_override)
            .await?;
        self.notify.notify_one();
        Ok(job)
    }

    pub async fn claim_next(&self) -> Result<Option<TranscriptionJobRecord>, DbError> {
        self.db.claim_next_transcription_job().await
    }

    pub async fn mark_completed(&self, job_id: &str) -> Result<(), DbError> {
        self.db
            .finish_transcription_job(job_id, "completed", None)
            .await
    }

    pub async fn mark_failed(&self, job_id: &str, error: &str) -> Result<(), DbError> {
        self.db
            .finish_transcription_job(job_id, "failed", Some(error))
            .await
    }

    pub fn notify(&self) -> &Arc<Notify> {
        &self.notify
    }
}
