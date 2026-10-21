mod build_assets;
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../notes/domain");
    println!("cargo:rerun-if-changed=../../notes/api/contracts");
    println!("cargo:rerun-if-changed=../../notes/sdk.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    forklaunch_core::build::rust_http_module(
        &fieldnotes_contracts::registry(),
        "fieldnotes-client",
        "NotesClient",
        out.join("client.rs"),
    )?;
    println!("cargo:rerun-if-changed=build_assets.rs");
    build_assets::emit(&out)?;
    Ok(())
}
