#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct NoteEvent {
    pub event: String,
    pub note_id: String,
    #[field(min = 1)]
    pub revision: i64,
}

#[framework_core::contract::schema(rename_all = "camelCase", unknown_fields = "deny")]
#[derive(Debug, Clone)]
pub struct PresenceInput {
    pub note_id: Option<String>,
}

#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct PresenceEvent {
    pub user_id: String,
    pub note_id: Option<String>,
}

#[framework_core::contract::schema]
pub struct WatchHeaders {
    #[field(rename = "last-event-id", min = 0)]
    pub last_event_id: Option<i64>,
}
