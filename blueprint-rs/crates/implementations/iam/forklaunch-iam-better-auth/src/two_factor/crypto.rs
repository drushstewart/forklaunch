//! Fixed default TOTP profile; no configurable algorithm or untrusted key parameters.
use super::{BlueprintResult, error};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use zeroize::Zeroizing;

pub(super) fn random(length: usize) -> BlueprintResult<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(vec![0; length]);
    SystemRandom::new().fill(&mut bytes).map_err(|_| {
        error(
            "TWO_FACTOR_INTERNAL",
            "Unable to generate authentication secret",
        )
    })?;
    Ok(bytes)
}
pub(super) fn opaque(length: usize) -> BlueprintResult<String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut result = String::with_capacity(length);
    while result.len() < length {
        for byte in random(32)?.iter().copied().filter(|byte| *byte < 248) {
            result.push(char::from(ALPHABET[usize::from(byte % 62)]));
            if result.len() == length {
                break;
            }
        }
    }
    Ok(result)
}
pub(super) fn backup_codes() -> BlueprintResult<Vec<String>> {
    (0..10)
        .map(|_| opaque(10).map(|code| format!("{}-{}", &code[..5], &code[5..])))
        .collect()
}
pub(super) fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::new();
    let mut buffer = 0u16;
    let mut bits = 0u8;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(char::from(ALPHABET[usize::from((buffer >> bits) & 31)]));
        }
    }
    if bits != 0 {
        output.push(char::from(
            ALPHABET[usize::from((buffer << (5 - bits)) & 31)],
        ));
    }
    output
}
pub(super) fn code(secret: &[u8], counter: u64, digits: u32) -> String {
    // SHA-1 is the interoperable RFC 6238 / Better Auth default, not a password hash.
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let signature = hmac::sign(&key, &counter.to_be_bytes());
    let bytes = signature.as_ref();
    let offset = usize::from(bytes[19] & 15);
    let value = ((u32::from(bytes[offset]) & 127) << 24)
        | (u32::from(bytes[offset + 1]) << 16)
        | (u32::from(bytes[offset + 2]) << 8)
        | u32::from(bytes[offset + 3]);
    format!(
        "{:0width$}",
        value % 10u32.pow(digits),
        width = digits as usize
    )
}
#[cfg(test)]
pub(super) fn matches(secret: &[u8], supplied: &str, seconds: i64) -> bool {
    seconds
        .checked_mul(1000)
        .is_some_and(|ms| matches_with_options(secret, supplied, ms, super::TotpOptions::default()))
}
pub(super) fn matches_with_options(
    secret: &[u8],
    supplied: &str,
    millis: i64,
    options: super::TotpOptions,
) -> bool {
    if supplied.len() != options.digits().count() as usize
        || !supplied.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    let Some(counter) = options.counter(millis) else {
        return false;
    };
    let mut matched = false;
    for offset in [-1i64, 0, 1] {
        if let Some(step) = counter.checked_add_signed(offset) {
            let expected = code(secret, step, options.digits().count());
            let difference = expected
                .bytes()
                .zip(supplied.bytes())
                .fold(0u8, |value, (a, b)| value | (a ^ b));
            matched |= difference == 0;
        }
    }
    matched
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6238_sha1_vectors_and_base32() {
        let secret = b"12345678901234567890";
        for (seconds, expected) in [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(code(secret, seconds / 30, 8), expected);
        }
        for (input, expected) in [
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(input.as_bytes()), expected);
        }
        let current = code(secret, 1_800_000_000 / 30, 6);
        assert!(matches(secret, &current, 1_800_000_000));
        assert!(matches(secret, &current, 1_800_000_030));
        assert!(matches(secret, &current, 1_799_999_970));
        assert!(!matches(secret, &current, 1_800_000_060));
        assert!(!matches(secret, "0000000", 1_800_000_000));
    }
}
