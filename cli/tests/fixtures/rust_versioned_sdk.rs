use versions_sdk::*;
#[tokio::main]
async fn main() {
    let client=VersionsClient::new(std::env::args().nth(1).unwrap());
    let VersionsVersionsWriteVersionV1Responses::Status201(one)=client.versions_write_version_v_1(VersionsVersionsWriteVersionV1Request{body:Some(r#"{"kind":"v1","value":"水 🦀"}"#.into()),..Default::default()}).await.unwrap() else {panic!("version 1 status")};
    assert_eq!(one.value,"水 🦀");
    let VersionsVersionsWriteVersionV2Responses::Status202(two)=client.versions_write_version_v_2(VersionsVersionsWriteVersionV2Request{body:Some(r#"{"kind":"v2","value":42}"#.into()),..Default::default()}).await.unwrap() else {panic!("version 2 status")};
    assert_eq!(two.value,42);
    println!("PASS typed Rust versioned SDK");
}
