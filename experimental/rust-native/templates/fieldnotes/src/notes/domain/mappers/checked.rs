//! Persistence-to-domain boundaries also protect callers that do not use HTTP.
use fieldnotes_contracts::{Attachment, Note};
use fieldnotes_security::SecurityError;
use framework_core::mappers::CheckedMapper;

pub(crate) struct NotesMappers {
    notes: CheckedMapper<Note>,
    attachments: CheckedMapper<Attachment>,
}
impl NotesMappers {
    pub(crate) fn new() -> Result<Self, SecurityError> {
        Ok(Self {
            notes: CheckedMapper::new().map_err(|_| SecurityError::Storage)?,
            attachments: CheckedMapper::new().map_err(|_| SecurityError::Storage)?,
        })
    }
    pub(crate) async fn note(&self, record: crate::NoteRecord) -> Result<Note, SecurityError> {
        self.notes
            .to_dto(record, |record| {
                std::future::ready(Ok(super::notes::note(record)))
            })
            .await
            .map_err(|_| SecurityError::Storage)
    }
    pub(crate) async fn attachment(
        &self,
        record: crate::AttachmentRecord,
    ) -> Result<Attachment, SecurityError> {
        self.attachments
            .to_dto(record, |record| {
                std::future::ready(Ok(super::attachments::attachment(record)))
            })
            .await
            .map_err(|_| SecurityError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    fn record() -> crate::NoteRecord {
        let now = "2026-10-17T12:00:00.000Z".parse().unwrap();
        crate::NoteRecord {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            title: "valid note".into(),
            body: "body".into(),
            revision: 1,
            created_at: now,
            updated_at: now,
            insight_excerpt: None,
            insight_word_count: None,
        }
    }
    #[tokio::test]
    async fn invalid_storage_records_do_not_escape_domain_boundary() {
        let mapper = NotesMappers::new().unwrap();
        let good = mapper.note(record()).await.unwrap();
        assert_eq!(good.title, "valid note");
        let mut bad = record();
        bad.revision = 0;
        assert!(matches!(
            mapper.note(bad).await,
            Err(SecurityError::Storage)
        ));
        let mut bad = record();
        bad.title.clear();
        assert!(matches!(
            mapper.note(bad).await,
            Err(SecurityError::Storage)
        ));
        let mut bad = record();
        bad.updated_at = "2026-10-17T12:00:00.000001Z".parse().unwrap();
        assert!(matches!(
            mapper.note(bad).await,
            Err(SecurityError::Storage)
        ));
        let bad = crate::AttachmentRecord {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            note_id: Uuid::nil(),
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            size: -1,
            created_at: record().created_at,
        };
        assert!(matches!(
            mapper.attachment(bad).await,
            Err(SecurityError::Storage)
        ));
    }
}
