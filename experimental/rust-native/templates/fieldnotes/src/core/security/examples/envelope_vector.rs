use fieldnotes_security::EnvelopeKey;
use uuid::Uuid;
fn main() {
    let key = if std::env::args().any(|arg| arg == "--v2") {
        EnvelopeKey::with_rotation("primary".into(), [7; 32], vec![], None).unwrap()
    } else {
        EnvelopeKey::new([7; 32])
    };
    let tenant = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let resource = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    println!(
        "{}",
        key.seal(tenant, resource, "note-body", "hello 📝".as_bytes())
            .unwrap()
    );
}
