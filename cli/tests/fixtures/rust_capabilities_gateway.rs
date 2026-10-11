use messages::capabilities::*;
use forklaunch_blueprint_core::{email_client::{EmailAddresses,EmailSendRequest},sms_client::SmsSendRequest,voice_client::StartOutboundCallRequest};
#[tokio::main]
async fn main() -> Result<(),Box<dyn std::error::Error>> {
    let email=email_client()?;
    assert!(!email.suppressed("fixture@example.com").await?);
    email.send(&EmailSendRequest {to:EmailAddresses::One("fixture@example.com".into()),subject:"Native Docker delivery".into(),text:Some("Exact Unicode 💬 bytes".into()),html:None,reply_to:None,tags:None}).await?;
    sms_client()?.send(&SmsSendRequest{to:"+15551234567".into(),body:"Native Docker delivery".into(),purpose:None}).await?;
    whatsapp_client()?.templates().await?;
    let voice=voice_client()?;
    let call=voice.start_outbound_call(&StartOutboundCallRequest{to:"+15551234567".into(),flow:"appointment_reminder".into(),attributes:None}).await?;
    voice.call(&call.call_id).await?;
    voice.end_call(&call.call_id).await?;
    payments_client()?.status().await?;
    println!("PASS all five native capability factories reach the signed gateway through Docker DNS");
    Ok(())
}
