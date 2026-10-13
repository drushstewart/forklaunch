//! Serializable parser choices shared by all versions of a route.
use forklaunch_common::json_string;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyParserOptions {
    pub json_strict: bool,
    pub json_default_charset: String,
    pub text_default_charset: String,
    pub form_default_charset: String,
    pub form_extended: bool,
    /// Bounded by the runtime to 32 bracket levels.
    pub form_depth: usize,
    /// Bounded by the runtime to 100,000 parameters.
    pub form_parameter_limit: usize,
}
impl Default for BodyParserOptions {
    fn default() -> Self {
        Self {
            json_strict: true,
            json_default_charset: "utf-8".into(),
            text_default_charset: "utf-8".into(),
            form_default_charset: "utf-8".into(),
            form_extended: true,
            form_depth: 32,
            form_parameter_limit: 1000,
        }
    }
}
impl BodyParserOptions {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"jsonStrict\":{},\"jsonDefaultCharset\":{},\"textDefaultCharset\":{},\"formDefaultCharset\":{},\"formExtended\":{},\"formDepth\":{},\"formParameterLimit\":{}}}",
            self.json_strict,
            json_string(&self.json_default_charset),
            json_string(&self.text_default_charset),
            json_string(&self.form_default_charset),
            self.form_extended,
            self.form_depth,
            self.form_parameter_limit
        )
    }
}
