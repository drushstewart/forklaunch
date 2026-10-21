use framework_core::contract::schema::Optional;

#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct Note {
    pub id: String,
    pub workspace_id: String,
    #[field(min_length = 1, max_length = 512)]
    pub title: String,
    #[field(max_length = 65536)]
    pub body: String,
    #[field(min = 1)]
    pub revision: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub insight_excerpt: Option<String>,
    #[field(min = 0)]
    pub insight_word_count: Option<i64>,
}

#[framework_core::contract::schema(rename_all = "camelCase", unknown_fields = "deny")]
#[derive(Debug, Clone)]
pub struct CreateNote {
    #[field(min_length = 1, max_length = 512)]
    pub title: String,
    #[field(max_length = 65536)]
    pub body: String,
}

#[framework_core::contract::schema(rename_all = "camelCase", unknown_fields = "deny")]
#[derive(Debug, Clone)]
pub struct PatchNote {
    #[field(min = 1)]
    pub expected_revision: i64,
    #[field(optional, min_length = 1, max_length = 512)]
    pub title: Optional<String>,
    #[field(optional, max_length = 65536)]
    pub body: Optional<String>,
}
