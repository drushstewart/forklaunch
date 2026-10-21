use crate::{api::context::NoteContext, registrations::AppState};
use axum::extract::State;
use fieldnotes_contracts::{PresenceEvent, PresenceInput};
use framework::native::SocketSession;
use uuid::Uuid;

pub(crate) async fn presence(
    State(state): State<AppState>,
    initial: NoteContext,
    (): (),
    mut session: SocketSession<NoteContext, PresenceInput, PresenceEvent>,
) {
    let workspace = initial.tenant.tenant_id();
    let mut events = state.presence.subscribe();
    loop {
        tokio::select! {
            message=session.recv()=>{
                let Some((context,input))=message else{break;};
                if let Some(note_id)=&input.note_id {
                    let Ok(note_id)=Uuid::parse_str(note_id) else{break;};
                    if session.operation(state.notes.get(&context.tenant,note_id)).await.is_err(){break;}
                }
                let _=state.presence.send((workspace,PresenceEvent{user_id:context.tenant.user_id().to_owned(),note_id:input.note_id}));
            }
            event=events.recv()=>match event {
                Ok((tenant,event)) if tenant==workspace=>if session.send(event).await.is_err(){break;},
                Ok(_)=>{},
                Err(_)=>break,
            }
        }
    }
}
