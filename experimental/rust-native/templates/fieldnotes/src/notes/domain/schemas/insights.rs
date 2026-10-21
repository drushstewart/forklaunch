#[framework_core::contract::schema(rename_all = "camelCase", unknown_fields = "deny")]
#[derive(Debug, Clone)]
pub struct InternalInsightInput {
    pub operation_id: String,
    #[field(min = 1)]
    pub revision: i64,
    pub encrypted_payload: String,
}

#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct InsightResult {
    pub operation_id: String,
    #[field(min = 1)]
    pub revision: i64,
    pub status: InsightStatus,
}

#[framework_core::contract::schema(rename_all = "lowercase")]
#[derive(Debug, Clone)]
pub enum InsightStatus {
    Applied,
    Duplicate,
    Stale,
}

#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct InsightRequested {
    pub operation_id: String,
    #[field(min = 1)]
    pub revision: i64,
}
