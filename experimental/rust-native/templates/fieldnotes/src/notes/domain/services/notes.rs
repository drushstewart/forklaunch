//! Application operations: expose contract DTOs while keeping storage records private to persistence.
use crate::domain::{mappers::NotesMappers, types::*};
use crate::persistence::{AttachmentDownload, NotesRepository};
use fieldnotes_contracts::{Attachment, Note};
use fieldnotes_security::{SecurityError, VerifiedTenant};
use uuid::Uuid;

pub struct NotesService {
    repository: NotesRepository,
    mappers: NotesMappers,
}
impl NotesService {
    pub fn new(repository: NotesRepository) -> Result<Self, SecurityError> {
        Ok(Self {
            repository,
            mappers: NotesMappers::new()?,
        })
    }
    pub async fn list(&self, scope: &VerifiedTenant) -> Result<Vec<Note>, SecurityError> {
        let records = self.repository.list(scope).await?;
        let mut notes = Vec::with_capacity(records.len());
        for record in records {
            notes.push(self.mappers.note(record).await?);
        }
        Ok(notes)
    }
    pub async fn get(&self, scope: &VerifiedTenant, id: Uuid) -> Result<Note, SecurityError> {
        self.mappers
            .note(self.repository.get(scope, id).await?)
            .await
    }
    pub async fn create(
        &self,
        scope: &VerifiedTenant,
        title: &str,
        body: &str,
    ) -> Result<Note, SecurityError> {
        self.mappers
            .note(self.repository.create(scope, title, body).await?)
            .await
    }
    pub async fn update(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
        expected_revision: i64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<Note, SecurityError> {
        self.mappers
            .note(
                self.repository
                    .update(scope, id, expected_revision, title, body)
                    .await?,
            )
            .await
    }
    pub async fn delete(&self, scope: &VerifiedTenant, id: Uuid) -> Result<(), SecurityError> {
        self.repository.delete(scope, id).await
    }
    pub async fn request_insights(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
    ) -> Result<InsightRequest, SecurityError> {
        self.repository.request_insights(scope, id).await
    }
    pub async fn events(
        &self,
        scope: &VerifiedTenant,
        after_id: i64,
    ) -> Result<Vec<NoteEventRecord>, SecurityError> {
        self.repository.events(scope, after_id).await
    }
    pub async fn claim_internal_nonce(
        &self,
        nonce: String,
        expiry: i64,
    ) -> Result<bool, SecurityError> {
        self.repository.claim_internal_nonce(nonce, expiry).await
    }
    pub async fn apply_encrypted_insight(
        &self,
        tenant: Uuid,
        note: Uuid,
        input: &fieldnotes_contracts::InternalInsightInput,
    ) -> Result<InsightApplyResult, SecurityError> {
        let decoded = self.repository.decode_insight(tenant, note, input).await?;
        self.repository.apply_insight(tenant, note, decoded).await
    }
    pub async fn apply_insight(
        &self,
        tenant: Uuid,
        id: Uuid,
        input: InsightInput,
    ) -> Result<InsightApplyResult, SecurityError> {
        self.repository.apply_insight(tenant, id, input).await
    }
    pub async fn upload_attachment(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
        filename: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<Attachment, SecurityError> {
        self.mappers
            .attachment(
                self.repository
                    .upload_attachment(scope, note, filename, content_type, body)
                    .await?,
            )
            .await
    }
    pub async fn list_attachments(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
    ) -> Result<Vec<Attachment>, SecurityError> {
        let records = self.repository.list_attachments(scope, note).await?;
        let mut attachments = Vec::with_capacity(records.len());
        for record in records {
            attachments.push(self.mappers.attachment(record).await?);
        }
        Ok(attachments)
    }
    pub async fn download_attachment(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
        id: Uuid,
    ) -> Result<AttachmentDownload, SecurityError> {
        self.repository.download_attachment(scope, note, id).await
    }
}
