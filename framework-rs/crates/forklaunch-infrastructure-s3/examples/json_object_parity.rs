use forklaunch_core::persistence::{
    TenantContext,
    encryption::{CiphertextFormat, FieldEncryptor},
    json::JsonRecordCodec,
};
use forklaunch_infrastructure_s3::{
    ObjectMetadata, aws::environment::S3Environment, json::JsonObjectStore,
};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input: Value = serde_json::from_reader(std::io::stdin())?;
    let previous: Vec<String> = serde_json::from_value(input["previous"].clone())?;
    let encryptor = FieldEncryptor::new(input["master"].as_str().ok_or("Master required")?)?
        .with_previous_keys(&previous)?
        .with_format(if input["format"] == "v3" {
            CiphertextFormat::V3
        } else {
            CiphertextFormat::V2
        });
    let store = JsonObjectStore::new(
        S3Environment::from_env()?.into_store().await?,
        JsonRecordCodec::new(encryptor),
    );
    let tenant = input["tenant"]
        .as_str()
        .map(TenantContext::new)
        .transpose()?;
    let mut output = Vec::new();
    for operation in input["operations"]
        .as_array()
        .ok_or("Operations required")?
    {
        let key = operation["key"].as_str().ok_or("Key required")?;
        let result =
            match operation["op"].as_str().ok_or("Operation required")? {
                "put" => store
                    .put_object(
                        key,
                        &operation["value"],
                        ObjectMetadata::new().with_content_type("application/json"),
                        tenant.as_ref(),
                    )
                    .await
                    .map(|()| Value::Null),
                "get" => store
                    .get_object::<Value>(key, tenant.as_ref())
                    .await
                    .map(|value| match value {
                        Some((value, metadata)) => {
                            json!({"found":true,"value":value,"contentType":metadata.content_type})
                        }
                        None => json!({"found":false}),
                    }),
                _ => return Err("Unknown operation".into()),
            };
        output.push(match result {
            Ok(value) => value,
            Err(error) => json!({"error":error.code()}),
        });
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
