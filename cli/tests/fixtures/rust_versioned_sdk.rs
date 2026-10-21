use versions_sdk::*;
#[tokio::main]
async fn main() {
    let client=VersionsClient::new(std::env::args().nth(1).unwrap());
    let VersionsVersionsWriteVersionV1Responses::Status201(one)=client.versions_write_version_v_1(VersionsVersionsWriteVersionV1Request{body:Some(serde_json::from_str(r#"{"kind":"v1","value":"水 🦀"}"#).unwrap()),..Default::default()}).await.unwrap() else {panic!("version 1 status")};
    assert_eq!(one.value,"水 🦀");
    let VersionsVersionsWriteVersionV2Responses::Status202(two)=client.versions_write_version_v_2(VersionsVersionsWriteVersionV2Request{body:Some(serde_json::from_str(r#"{"kind":"v2","value":42}"#).unwrap()),..Default::default()}).await.unwrap() else {panic!("version 2 status")};
    assert_eq!(two.value,42);
    let VersionsVersionedTransformWriteTransformVersionV2Responses::Status202(transformed)=client.versioned_transform_write_transform_version_v_2(VersionsVersionedTransformWriteTransformVersionV2Request{body:Some(serde_json::from_str(r#"{"value":" ab "}"#).unwrap()),..Default::default()}).await.unwrap() else {panic!("transform status")};
    assert_eq!(transformed.value,"ab!");
    let VersionsVersionedTransformWriteTransformVersionV1Responses::Status201(fallback)=client.versioned_transform_write_transform_version_v_1(VersionsVersionedTransformWriteTransformVersionV1Request{body:Some(serde_json::from_str(r#"{"value":"blocked"}"#).unwrap()),..Default::default()}).await.unwrap() else {panic!("fallback status")};
    assert_eq!(fallback.value,"BLOCKED");
    println!("PASS typed Rust versioned SDK");
}
