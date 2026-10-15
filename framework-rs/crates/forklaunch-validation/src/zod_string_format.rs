//! Exact bounded string grammars from the repository's Zod v3 factories.

/// Runtime formats matching Zod v3, not generic JSON Schema format semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodStringFormat {
    /// Hyphenated hex groups; no UUID version or variant restriction.
    Uuid,
    /// Zod's ASCII email grammar, without DNS, delivery or length validation.
    Email,
    /// Node 24.2.0 / Ada 3.2.4 URL acceptance; never normalizes output.
    /// Requires the opt-in `zod-uri` feature and a C++20 build toolchain.
    #[cfg(feature = "zod-uri")]
    Uri,
}

impl ZodStringFormat {
    pub(super) fn rejection(self, value: &str) -> Option<String> {
        match self {
            Self::Uuid if !uuid(value) => Some("Invalid uuid".to_owned()),
            Self::Email if !email(value) => Some("Invalid email".to_owned()),
            #[cfg(feature = "zod-uri")]
            Self::Uri if !ada_url::Url::can_parse(value, None) => Some("Invalid url".to_owned()),
            _ => None,
        }
    }
}

use forklaunch_common::zod_string_format::{email, uuid};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_grammar_preserves_nonstandard_acceptance_and_rejection() {
        assert!(uuid("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"));
        assert!(!uuid("urn:uuid:00000000-0000-0000-0000-000000000000"));
        assert!(email("a'b@b-.co"));
        for invalid in [
            "a%b@b.co",
            "a'@b.co",
            "a..b@b.co",
            "a@-b.co",
            "é@b.co",
            "a@b.co\n",
        ] {
            assert!(!email(invalid), "{invalid:?}");
        }
    }
}

#[cfg(all(test, feature = "zod-uri"))]
mod uri_tests {
    use super::ZodStringFormat;
    use crate::typescript_coerce::{
        ZodStringCheck, ZodStringRule, ZodStringStep, zod_string_checks,
    };
    use serde_json::json;
    #[test]
    fn node_pinned_unicode_and_original_string_are_preserved() {
        for value in [
            " https://example.com/a b ",
            "http://a\u{061d}b.test",
            "http://xn--ab-hsd.test",
            "http://a%D8%9Db.test",
            "file:///C:/a",
            "mailto:a@b",
        ] {
            let input = json!(value);
            assert_eq!(
                zod_string_checks(
                    Some(&input),
                    &[ZodStringCheck {
                        rule: ZodStringRule::Format(ZodStringFormat::Uri),
                        message: None
                    }]
                ),
                Ok(value)
            );
        }
        for value in [
            "relative/path",
            "http://a\u{0c5c}b.test",
            "http://a%E0%B1%9Cb.test",
            "http://[::1",
            "http://host:65536",
        ] {
            assert_eq!(
                ZodStringFormat::Uri.rejection(value).as_deref(),
                Some("Invalid url")
            );
        }
    }
    #[test]
    fn ordered_uri_errors_and_literal_override_preserve_existing_string_semantics() {
        let input = json!(" invalid ");
        let checks = [
            ZodStringCheck {
                rule: ZodStringRule::Format(ZodStringFormat::Uri),
                message: Some("\0custom\n💡"),
            },
            ZodStringCheck {
                rule: ZodStringRule::Step(ZodStringStep::Trim),
                message: None,
            },
            ZodStringCheck {
                rule: ZodStringRule::EndsWith("z"),
                message: None,
            },
        ];
        let errors = zod_string_checks(Some(&input), &checks).unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].message, "\0custom\n💡");
        assert_eq!(input, " invalid ");
        assert_eq!(
            zod_string_checks(None, &checks).unwrap_err()[0].message,
            "Required"
        );
    }
}
