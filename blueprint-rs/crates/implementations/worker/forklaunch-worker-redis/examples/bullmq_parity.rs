use forklaunch_worker_redis::bullmq::{BullMqConsumer, BullMqOptions, BullMqProducer, protocol};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn options(input: &Value) -> Result<BullMqOptions, Box<dyn std::error::Error>> {
    let mut options = BullMqOptions::new(&std::env::var("FORKLAUNCH_TEST_REDIS_URL")?);
    options.queue.prefix = "caller-override".into();
    options.retries = u32::try_from(input["retries"].as_u64().unwrap_or(3))?;
    let interval = input["interval"].as_u64().unwrap_or(20);
    options.backoff = if input["backoff"] == "fixed" {
        protocol::types::BackoffStrategy::Fixed(interval)
    } else {
        protocol::types::BackoffStrategy::Exponential(interval)
    };
    if input["removeOnFail"] == true {
        options.worker.remove_on_fail = Some(protocol::types::RemoveOnFinish::Bool(true));
    }
    if let Some(count) = input["keepCount"].as_u64() {
        options.queue.default_job_options = protocol::JobOptions::default()
            .remove_on_complete(protocol::types::RemoveOnFinish::Count(usize::try_from(
                count,
            )?))
            .remove_on_fail(protocol::types::RemoveOnFinish::Count(usize::try_from(
                count,
            )?));
    }
    options.close_timeout = Duration::from_secs(5);
    Ok(options)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input: Value = serde_json::from_reader(std::io::stdin())?;
    let name = input["queue"].as_str().ok_or("Queue required")?;
    if input["mode"] == "consumeDenied" {
        let worker = BullMqConsumer::connect(
            name,
            |_: protocol::Job, _token| async { Ok(Value::Null) },
            options(&input)?,
        )
        .await;
        let error = match worker {
            Ok(worker) => worker.run_until(std::future::pending()).await.unwrap_err(),
            Err(error) => error,
        };
        println!("{}", json!({"error":error.code}));
        return Ok(());
    }
    let producer = BullMqProducer::connect(name, options(&input)?).await?;
    let output = match input["mode"].as_str().ok_or("Mode required")? {
        "produce" => {
            let events = input["events"].as_array().ok_or("Events required")?;
            let mut ids = Vec::new();
            if let Some(first) = events.first() {
                ids.push(producer.enqueue_job(first.clone()).await?);
            }
            ids.extend(
                producer
                    .enqueue_batch_jobs(events.iter().skip(1).cloned().collect())
                    .await?,
            );
            json!({"ids":ids,"events":producer.peek_events().await?})
        }
        "peek" => json!({"events":producer.peek_events().await?}),
        "consume" => consume(name, &input, &producer).await?,
        _ => return Err("Unknown mode".into()),
    };
    producer.close().await;
    println!("{output}");
    Ok(())
}

async fn consume(
    name: &str,
    input: &Value,
    producer: &BullMqProducer,
) -> Result<Value, Box<dyn std::error::Error>> {
    let failures = std::sync::Mutex::new(Vec::new());
    let attempts = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let captured = attempts.clone();
    let worker = BullMqConsumer::connect(
        name,
        move |job: protocol::Job, _token| {
            let captured = captured.clone();
            async move {
                captured
                    .lock()
                    .await
                    .push(json!({"event":job.data(),"attempt":job.attempts_made()}));
                if let Some(delay) = job.data()["sleepMs"].as_u64() {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                if u64::from(job.attempts_made()) < job.data()["failAttempts"].as_u64().unwrap_or(0)
                {
                    return Err(protocol::Error::ProcessingError("fixture_failure".into()));
                }
                Ok(json!({"native":true,"id":job.data()["id"]}))
            }
        },
        options(input)?,
    )
    .await?;
    let peek = worker.peek_events().await?;
    let expected = input["expected"]
        .as_u64()
        .ok_or("Expected count required")?;
    worker
        .run_until_with_failure_handler(
            async {
                loop {
                    if let Some(expected) = input["expectedAttempts"].as_u64()
                        && u64::try_from(attempts.lock().await.len()).unwrap() >= expected
                    {
                        break;
                    }
                    if input["shutdownAfterStart"] == true && !attempts.lock().await.is_empty() {
                        break;
                    }
                    if input["removeOnFail"] == true
                        && u64::try_from(failures.lock().unwrap().len()).unwrap() >= expected
                    {
                        break;
                    }
                    let counts = producer
                        .inner()
                        .get_job_counts()
                        .await
                        .expect("Counts available");
                    if counts.completed + counts.failed >= expected {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            },
            |event, error| {
                failures
                    .lock()
                    .unwrap()
                    .push(json!({"event":event,"error":error.code}));
            },
        )
        .await?;
    let counts = producer.inner().get_job_counts().await?;
    Ok(
        json!({"peek":peek,"attempts":*attempts.lock().await,"failures":*failures.lock().unwrap(),"completed":counts.completed,"failed":counts.failed}),
    )
}
