use forklaunch_worker_redis::bullmq::{BullMqOptions, BullMqProducer, protocol};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input: Value = serde_json::from_reader(std::io::stdin())?;
    let name = input["queue"].as_str().ok_or("Queue required")?;
    let scheduler = input["scheduler"].as_str().ok_or("Scheduler required")?;
    let mut options = BullMqOptions::new(&std::env::var("FORKLAUNCH_TEST_REDIS_URL")?);
    options.queue.prefix = "caller-override".into();
    let job_options: protocol::JobOptions =
        serde_json::from_value(input.get("jobOptions").cloned().unwrap_or(json!({})))?;
    options.queue.default_job_options = job_options.clone();
    let producer = BullMqProducer::connect(name, options).await?;
    let queue = producer.inner();
    let output = match input["mode"].as_str().ok_or("Mode required")? {
        "upsert" => {
            let job = queue
                .upsert_job_scheduler(
                    scheduler,
                    serde_json::from_value(input["repeat"].clone())?,
                    Some(input["event"]["id"].as_str().ok_or("Event ID required")?),
                    Some(input["event"].clone()),
                    Some(job_options),
                )
                .await?;
            json!({"id":job.map(|job| job.id().to_owned()),"scheduler":queue.get_job_scheduler(scheduler).await?})
        }
        "inspect" => json!({"scheduler":queue.get_job_scheduler(scheduler).await?}),
        "remove" => json!({"removed":queue.remove_job_scheduler(scheduler).await?}),
        _ => return Err("Unknown mode".into()),
    };
    producer.close().await;
    println!("{output}");
    Ok(())
}
