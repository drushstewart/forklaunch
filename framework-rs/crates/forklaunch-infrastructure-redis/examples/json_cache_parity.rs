use forklaunch_core::persistence::{
    TenantContext,
    encryption::{CiphertextFormat, FieldEncryptor},
    json::JsonRecordCodec,
};
use forklaunch_infrastructure_redis::{
    RedisTtlCacheConfig, client::RedisTtlCache, json::JsonCache,
};
use serde_json::{Value, json};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input: Value = serde_json::from_reader(std::io::stdin())?;
    let previous: Vec<String> = serde_json::from_value(input["previous"].clone())?;
    let encryptor = FieldEncryptor::new(input["master"].as_str().ok_or("Master required")?)?
        .with_previous_keys(&previous)?
        .with_format(if input["format"] == "v3" {
            CiphertextFormat::V3
        } else {
            CiphertextFormat::V2
        });
    let mut cache = JsonCache::new(
        RedisTtlCache::connect(RedisTtlCacheConfig::new(
            std::env::var("FORKLAUNCH_TEST_REDIS_URL")?,
            Duration::from_secs(60),
        ))?,
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
        let key = operation["key"].as_str().unwrap_or("");
        let result = match operation["op"].as_str().ok_or("Operation required")? {
            "put" => cache
                .put_record(
                    key,
                    &operation["value"],
                    operation["ttl"].as_u64().map(Duration::from_millis),
                    tenant.as_ref(),
                )
                .map(|()| Value::Null),
            "get" => cache
                .read_record::<Value>(key, tenant.as_ref())
                .map(|value| json!({"value":value,"found":value.is_some()})),
            "batch" => {
                let keys: Vec<String> = serde_json::from_value(operation["keys"].clone())?;
                cache
                    .read_batch_records::<Value>(&keys, tenant.as_ref())
                    .map(|values| json!(values))
            }
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
