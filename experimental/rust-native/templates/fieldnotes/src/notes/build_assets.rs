//! Additional domain socket contracts; mandatory HTTP/compliance assets remain in build.rs.
pub fn emit(
    output: &framework_core::build::ServiceSdkPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    framework_core::build::typescript_websocket(
        &fieldnotes_contracts::presence_contract(),
        "NotesPresenceClient",
        output.typescript.join("presence.ts"),
        output.typescript.join("socket-contract.json"),
    )?;
    framework_core::build::rust_websocket_module(
        &fieldnotes_contracts::presence_contract(),
        "notes-sdk",
        "NotesPresenceClient",
        output.rust.with_file_name("notes_presence.rs"),
    )?;
    Ok(())
}

/// Own socket outputs explicitly; the ordinary service build hook records them.
pub fn declared_assets(
    output: &framework_core::build::ServiceSdkPaths,
) -> Result<Vec<framework_core::build::provenance::GeneratedAssets>, Box<dyn std::error::Error>> {
    Ok(vec![
        framework_core::build::provenance::GeneratedAssets::socket(
            output.typescript.join("presence.ts"),
            output.typescript.join("socket-contract.json"),
            output.rust.with_file_name("notes_presence.rs"),
        )?,
    ])
}

/// Complete compiler-generated API sets; source assembly is verified separately.
pub fn binding_declarations() -> Result<Option<Vec<framework_core::contract::assembly::BindingDeclaration>>, Box<dyn std::error::Error>> {
    let mut declarations = Vec::new();
    declarations.extend(fieldnotes_contracts::notes::binding_manifest()?);
    declarations.extend(fieldnotes_contracts::attachments::binding_manifest()?);
    declarations.extend(fieldnotes_contracts::insights::binding_manifest()?);
    declarations.extend(fieldnotes_contracts::streams::binding_manifest()?);
    declarations.extend(fieldnotes_contracts::sockets::binding_manifest()?);
    Ok(Some(declarations))
}
