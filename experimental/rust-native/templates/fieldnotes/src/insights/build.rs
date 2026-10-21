#[path = "persistence/compliance_schema.rs"]
mod compliance_schema;
mod build_assets;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=persistence/compliance_schema.rs");
    println!("cargo:rerun-if-changed=compliance.json");
    println!("cargo:rerun-if-changed=build_assets.rs");
    let metadata = framework_core::persistence::compliance::compliance_manifest(&compliance_schema::declarations()).to_string();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("compliance.json");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(metadata.as_str()) { std::fs::write(path, metadata)?; }
    build_assets::generate()
}
