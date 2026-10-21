mod build_assets;
#[path = "persistence/compliance_schema.rs"]
mod compliance_schema;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=domain");
    println!("cargo:rerun-if-changed=persistence/compliance_schema.rs");
    let metadata = framework_core::persistence::compliance::compliance_manifest(
        &compliance_schema::declarations(),
    );
    let compliance_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("compliance.json");
    println!("cargo:rerun-if-changed=compliance.json");
    let metadata = metadata.to_string();
    if std::fs::read_to_string(&compliance_path).ok().as_deref() != Some(metadata.as_str()) {
        std::fs::write(&compliance_path, metadata)?;
    }
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=api/contracts");
    println!("cargo:rerun-if-changed=sdk.rs");
    let evidence = framework_core::build::provenance::ServiceBuild::capture(&[])?;
    let output = framework_core::build::service_sdks(
        &fieldnotes_contracts::registry(),
        "notes",
        "NotesClient",
    )?;
    println!("cargo:rerun-if-changed=build_assets.rs");
    let evidence = evidence.include_assets(&build_assets::declared_assets(&output)?)?;
    let evidence = if let Some(bindings) = build_assets::binding_declarations()? { evidence.with_bindings(bindings) } else { evidence };
    build_assets::emit(&output)?;
    evidence.record("notes", &output)?;
    Ok(())
}
