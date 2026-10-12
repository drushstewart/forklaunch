use native_api_sdk::{NativeApiClient, NativeApiHealthGetHealthResponses};
use typescript_api_sdk::{
    TypescriptApiClient, TypescriptApiProfilesCreateProfileRequest,
    TypescriptApiProfilesCreateProfileResponses, TypescriptApiProfilesGetProfileRequest,
    TypescriptApiProfilesGetProfileResponses,
};

#[tokio::main]
async fn main() {
    tokio::time::timeout(std::time::Duration::from_secs(10), verify())
        .await
        .expect("mixed SDK request deadline");
    println!("PASS Rust SDK clients call native and TypeScript HTTP services");
}

async fn verify() {
    let args: Vec<_> = std::env::args().collect();
    let native = NativeApiClient::new(&args[1]);
    let NativeApiHealthGetHealthResponses::Status200(health) =
        native.health_get_health(Default::default()).await.unwrap()
    else {
        panic!("Native service health failed");
    };
    assert_eq!(health.status, "ok");
    let typescript =
        TypescriptApiClient::new(&args[2]).with_default_header("x-sdk-consumer", "rust");
    let id = "Ada / 🦀 & backend";
    let mut request = TypescriptApiProfilesGetProfileRequest::default();
    request.params.insert("id".into(), id.into());
    let TypescriptApiProfilesGetProfileResponses::Status200(profile) =
        typescript.profiles_get_profile(request).await.unwrap()
    else {
        panic!("TypeScript service profile failed");
    };
    assert_eq!(profile.id, id);
    assert_eq!(profile.profile_name, "Ada");
    let mut request = TypescriptApiProfilesGetProfileRequest::default();
    request.params.insert("id".into(), "missing".into());
    let TypescriptApiProfilesGetProfileResponses::Status404(error) =
        typescript.profiles_get_profile(request).await.unwrap()
    else {
        panic!("TypeScript service should return a typed 404");
    };
    assert_eq!(error.error_code, "not_found");
    let request = TypescriptApiProfilesCreateProfileRequest {
        body: Some(r#"{"profileName":"Grace"}"#.into()),
        ..Default::default()
    };
    let TypescriptApiProfilesCreateProfileResponses::Status201(created) =
        typescript.profiles_create_profile(request).await.unwrap()
    else {
        panic!("TypeScript service create failed");
    };
    assert_eq!(created.id, "created");
    assert_eq!(created.profile_name, "Grace");
}
