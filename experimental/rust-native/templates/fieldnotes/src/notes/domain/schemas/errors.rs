#[framework_core::contract::schema]
#[derive(Debug, Clone)]
pub struct ErrorBody {
    pub error: String,
}
