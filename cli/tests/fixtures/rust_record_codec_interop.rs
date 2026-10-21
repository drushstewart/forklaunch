use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor, json::JsonRecordCodec};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let codec = JsonRecordCodec::new(FieldEncryptor::new("interop-record-key-fixture")?);
    let tenant = TenantContext::new("interop-tenant")?;
    let value = serde_json::json!({"note":"private ☃", "count":3});
    if let Some(ciphertext) = std::env::args().nth(1) {
        assert_eq!(codec.decode::<serde_json::Value>(ciphertext.as_bytes(), Some(&tenant))?, value);
    } else {
        println!("{}", String::from_utf8(codec.encode(&value, Some(&tenant))?)?);
    }
    Ok(())
}
