use crate::{api::context::NoteContext, registrations::AppState};
use axum::extract::State;
use fieldnotes_contracts::{NoteEvent, WatchInput};
use framework::native::{EndpointError, ServerEvent, StreamContext};
use std::time::Duration;

pub(crate) async fn watch_notes(
    State(state): State<AppState>,
    context: StreamContext<NoteContext>,
    request: WatchInput,
) -> Result<
    impl futures_util::Stream<Item = Result<ServerEvent<NoteEvent>, EndpointError>> + Send,
    EndpointError,
> {
    let mut cursor = request.headers.last_event_id.unwrap_or(0);
    Ok(async_stream::stream! {
        let mut interval=tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let identity=match context.refresh().await{Ok(context)=>context.tenant,Err(error)=>{yield Err(error);break;}};
            let events=match context.operation(state.notes.events(&identity,cursor)).await{Ok(events)=>events,Err(_)=>{yield Err(EndpointError::Unavailable);break;}};
            for event in events {
                cursor=event.id;
                let payload=NoteEvent{event:event.kind,note_id:event.note_id.to_string(),revision:event.revision};
                yield Ok(ServerEvent::new(payload).with_id(cursor.to_string()).with_event("note"));
            }
        }
    })
}
