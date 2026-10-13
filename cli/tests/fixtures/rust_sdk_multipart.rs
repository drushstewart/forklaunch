use accounts_sdk::AccountsClient;
#[tokio::main]
async fn main() {
    let base=std::env::args().nth(1).expect("service URL");
    tokio::time::timeout(std::time::Duration::from_secs(10),verify(&base)).await.expect("multipart client deadline");
    println!("PASS native Rust multipart SDK");
}
async fn verify(base: &str) {
    let mut multipart_input=accounts_sdk::GetMultipartResponseRequest::default();
    multipart_input.params.insert("mode".into(),"valid".into());
    let multipart_client=AccountsClient::new(base);
    let accounts_sdk::GetMultipartResponseResponses::Status200(form)=multipart_client.get_multipart_response(multipart_input.clone()).await.unwrap() else { panic!("multipart response failed") };
    assert_eq!(form.parts.len(),4);
    assert_eq!(form.parts.iter().map(|part|part.name.as_str()).collect::<Vec<_>>(),vec!["label","label","file","file"]);
    assert_eq!(form.parts[0].bytes,"hello 水".as_bytes());
    assert_eq!(form.parts[1].bytes,b"again");
    assert_eq!(form.parts[2].bytes,vec![0,255,128,13,10]);
    assert_eq!(form.parts[2].file_name.as_deref(),Some("data.bin"));
    assert!(form.parts[3].bytes.is_empty());
    assert_eq!(form.parts[3].file_name.as_deref(),Some("empty.bin"));
    assert!(multipart_client.clone().with_multipart_response_limit(0).is_err());
    assert!(multipart_client.clone().with_multipart_response_limit(16).unwrap().get_multipart_response(multipart_input.clone()).await.is_err());
    for mode in ["wrong","missing","invalid","truncated","many"] {
        multipart_input.params.insert("mode".into(),mode.into());
        assert!(multipart_client.get_multipart_response(multipart_input.clone()).await.is_err(),"accepted {mode}");
    }
    multipart_input.params.insert("mode".into(),"empty".into());
    let accounts_sdk::GetMultipartResponseResponses::Status200(form)=multipart_client.get_multipart_response(multipart_input).await.unwrap() else { panic!("empty form response failed") };
    assert!(form.parts.is_empty());
}
