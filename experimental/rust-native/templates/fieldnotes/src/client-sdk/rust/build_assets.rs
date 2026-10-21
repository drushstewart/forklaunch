pub fn emit(out:&std::path::Path)->Result<(),Box<dyn std::error::Error>> {
    forklaunch_core::build::rust_websocket_module(
        &fieldnotes_contracts::presence_contract(),
        "fieldnotes-client",
        "NotesPresenceClient",
        out.join("presence.rs"),
    )?;
    Ok(())
}
