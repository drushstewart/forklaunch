//! Exact bounded string grammars from the repository's Zod v3 factories.

/// Runtime formats matching Zod v3, not generic JSON Schema format semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodStringFormat {
    /// Hyphenated hex groups; no UUID version or variant restriction.
    Uuid,
    /// Zod's ASCII email grammar, without DNS, delivery or length validation.
    Email,
}

impl ZodStringFormat {
    pub(super) fn rejection(self, value: &str) -> Option<String> {
        match self {
            Self::Uuid if !uuid(value) => Some("Invalid uuid".to_owned()),
            Self::Email if !email(value) => Some("Invalid email".to_owned()),
            _ => None,
        }
    }
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn email(value: &str) -> bool {
    if !value.is_ascii() || value.starts_with('.') || value.contains("..") {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    let terminal = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-');
    if local.is_empty()
        || !local
            .bytes()
            .all(|byte| terminal(byte) || matches!(byte, b'\'' | b'.'))
        || !local.bytes().last().is_some_and(terminal)
    {
        return false;
    }
    let Some((labels, top)) = domain.rsplit_once('.') else {
        return false;
    };
    top.len() >= 2
        && top.bytes().all(|byte| byte.is_ascii_alphabetic())
        && labels.split('.').all(|label| {
            label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

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
