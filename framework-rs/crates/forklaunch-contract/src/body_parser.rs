//! Serializable parser choices shared by all versions of a route.
use forklaunch_common::json_string;

// Independent switches mirror body-parser options, rather than mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyParserOptions {
    pub json_wire: BodyParserWireOptions,
    pub text_wire: BodyParserWireOptions,
    pub form_wire: BodyParserWireOptions,
    pub raw_wire: BodyParserWireOptions,
    pub json_strict: bool,
    pub json_default_charset: String,
    pub text_default_charset: String,
    pub form_default_charset: String,
    pub form_extended: bool,
    pub form_charset_sentinel: bool,
    pub form_interpret_numeric_entities: bool,
    /// Bounded by the runtime to 32 bracket levels.
    pub form_depth: usize,
    /// Bounded by the runtime to 100,000 parameters.
    pub form_parameter_limit: usize,
}
impl Default for BodyParserOptions {
    fn default() -> Self {
        Self {
            json_wire: BodyParserWireOptions::default(),
            text_wire: BodyParserWireOptions::default(),
            form_wire: BodyParserWireOptions::default(),
            raw_wire: BodyParserWireOptions::default(),
            json_strict: true,
            json_default_charset: "utf-8".into(),
            text_default_charset: "utf-8".into(),
            form_default_charset: "utf-8".into(),
            form_extended: true,
            form_charset_sentinel: false,
            form_interpret_numeric_entities: false,
            form_depth: 32,
            form_parameter_limit: 1000,
        }
    }
}
impl BodyParserOptions {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"jsonStrict\":{},\"jsonDefaultCharset\":{},\"textDefaultCharset\":{},\"formDefaultCharset\":{},\"formExtended\":{},\"formCharsetSentinel\":{},\"formInterpretNumericEntities\":{},\"formDepth\":{},\"formParameterLimit\":{},\"jsonWire\":{},\"textWire\":{},\"formWire\":{},\"rawWire\":{}}}",
            self.json_strict,
            json_string(&self.json_default_charset),
            json_string(&self.text_default_charset),
            json_string(&self.form_default_charset),
            self.form_extended,
            self.form_charset_sentinel,
            self.form_interpret_numeric_entities,
            self.form_depth,
            self.form_parameter_limit,
            self.json_wire.to_json(),
            self.text_wire.to_json(),
            self.form_wire.to_json(),
            self.raw_wire.to_json()
        )
    }
}

/// Byte limits apply after decompression; transport limits still bound wire bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyParserWireOptions {
    pub limit_bytes: usize,
    pub inflate: bool,
}
impl Default for BodyParserWireOptions {
    fn default() -> Self {
        Self {
            limit_bytes: 100 * 1024,
            inflate: true,
        }
    }
}
impl BodyParserWireOptions {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"limitBytes\":{},\"inflate\":{}}}",
            self.limit_bytes, self.inflate
        )
    }
}
