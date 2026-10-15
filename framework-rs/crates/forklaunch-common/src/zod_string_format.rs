//! Pure Zod v3 string predicates shared by schema and runtime consumers.

/// Match the exact ASCII UUID grammar used by Zod v3.
#[must_use]
pub fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Match the exact ASCII email grammar used by Zod v3.
#[must_use]
pub fn email(value: &str) -> bool {
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
