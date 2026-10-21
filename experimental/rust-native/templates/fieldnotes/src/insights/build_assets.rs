pub fn generate() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../notes/domain/events/insight.rs");
    use fieldnotes_contracts::events::insight::{
        InsightEvent, PURPOSE, RESULT_PURPOSE, TracedNoteJob,
    };
    use framework_core::build::{ChannelContextFields, ChannelEvent, ChannelWire};
    let context = ChannelContextFields {
        tenant: "workspaceId",
        resource: "noteId",
        operation: "operationId",
        revision: "revision",
    };
    let traced_context = ChannelContextFields {
        tenant: "payload.workspaceId",
        resource: "payload.noteId",
        operation: "payload.operationId",
        revision: "payload.revision",
    };
    let root =
        framework_core::build::application_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))?
            .canonicalize()?;
    let output = root.join("src/client-sdk/events");
    let evidence =
        framework_core::build::provenance::ServiceBuild::capture(&[])?.include_assets(&[
            framework_core::build::provenance::GeneratedAssets::channels(&output).with_inputs([
                framework_core::build::provenance::InputScope::required("src/notes/domain/events"),
            ]),
        ])?;
    framework_core::build::channel_events(
        "note-insights",
        &[
            ChannelEvent {
                name: "NoteJob",
                purpose: PURPOSE,
                schema: framework_core::contract::schema_value_for::<TracedNoteJob>(),
                context: traced_context,
                wire: ChannelWire::Flat {
                    context,
                    ciphertext: "payload",
                },
            },
            ChannelEvent {
                name: "InsightEvent",
                purpose: RESULT_PURPOSE,
                schema: framework_core::contract::schema_value_for::<InsightEvent>(),
                context,
                wire: ChannelWire::SealedEvent,
            },
        ],
        include_str!("../notes/domain/events/insight.rs"),
        &[
            "framework_core (forklaunch-core, typed-schema)",
            "worker (forklaunch-worker-interface)",
            "uuid (serde,v4)",
            "zeroize",
        ],
        &output,
    )?;
    evidence.record_assets("insights", &output.join("provenance.json"))?;
    Ok(())
}
