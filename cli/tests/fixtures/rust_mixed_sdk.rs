use native_api_sdk::{Client as NativeClient, GetHealthResponses};
use typescript_api_sdk::{
    Client as TypescriptClient, CreateProfileRequest, CreateProfileRequestBody,
    CreateProfileResponses, GetProfileRequest, GetProfileResponses,
};
#[tokio::main]
async fn main() {
    tokio::time::timeout(std::time::Duration::from_secs(10), verify())
        .await
        .expect("request deadline");
    println!(
        "PASS local Rust clients call native and TypeScript HTTP services with typed bodies/statuses"
    );
}
async fn verify() {
    let args: Vec<_> = std::env::args().collect();
    let GetHealthResponses::Status200(health) =
        NativeClient::new(&args[1]).get_health().await.unwrap()
    else {
        panic!("native health failed")
    };
    assert_eq!(health.status, "ok");
    let client = TypescriptClient::new(&args[2]).with_default_header("x-sdk-consumer", "rust");
    let id = "Ada / 🦀 & backend";
    let mut input = GetProfileRequest::default();
    input.params.id = id.into();
    let GetProfileResponses::Status200(profile) = client.get_profile(input).await.unwrap() else {
        panic!("profile failed")
    };
    assert_eq!(profile.id, id);
    assert_eq!(profile.profile_name, "Ada");
    let mut input = GetProfileRequest::default();
    input.params.id = "missing".into();
    let GetProfileResponses::Status404(error) = client.get_profile(input).await.unwrap() else {
        panic!("missing typed 404")
    };
    assert_eq!(error.error_code, "not_found");
    let input = CreateProfileRequest {
        body: Some(CreateProfileRequestBody {
            profile_name: "Grace".into(),
        }),
        ..Default::default()
    };
    let CreateProfileResponses::Status201(created) = client.create_profile(input).await.unwrap();
    assert_eq!(created.id, "created");
    assert_eq!(created.profile_name, "Grace");
}
