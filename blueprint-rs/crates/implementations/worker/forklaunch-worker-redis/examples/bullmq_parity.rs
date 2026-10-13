use forklaunch_worker_redis::bullmq::{BullMqConsumer, BullMqOptions, BullMqProducer, protocol};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn options(input: &Value) -> Result<BullMqOptions, Box<dyn std::error::Error>> {
    let mut options = BullMqOptions::new(&std::env::var("FORKLAUNCH_TEST_REDIS_URL")?);
    if let Some(limit) = input["streamsMaxLenEvents"].as_u64() {
        options.queue.streams_max_len_events = limit;
    }
    options.queue.skip_metas_update = input["skipMetasUpdate"] == true;
    options.queue.skip_version_check = input["skipVersionCheck"] == true;
    if let Some(defaults) = input.get("defaultJobOptions") {
        options.queue.default_job_options = serde_json::from_value(defaults.clone())?;
    }
    if let Some(concurrency) = input["concurrency"].as_u64() {
        options.worker.concurrency = usize::try_from(concurrency)?;
    }
    if let Some(limiter) = input.get("limiter") {
        options.worker.limiter = Some(protocol::options::RateLimiterOptions {
            max: limiter["max"].as_u64().ok_or("Limiter max required")?,
            duration: limiter["duration"]
                .as_u64()
                .ok_or("Limiter duration required")?,
        });
    }
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
    if let Some(age) = input["keepAgeSeconds"].as_u64() {
        let retention = protocol::types::RemoveOnFinish::Options(protocol::types::KeepJobs {
            age: Some(age),
            count: input["keepCount"]
                .as_u64()
                .map(usize::try_from)
                .transpose()?,
            limit: input["keepLimit"]
                .as_u64()
                .map(usize::try_from)
                .transpose()?,
        });
        options.queue.default_job_options = protocol::JobOptions::default()
            .remove_on_complete(retention.clone())
            .remove_on_fail(retention);
    }
    if let Some(interval) = input["stalledInterval"].as_u64() {
        options.worker.stalled_interval = interval;
        options.worker.lock_duration = input["lockDuration"].as_u64().unwrap_or(1000);
        options.worker.max_stalled_count =
            u32::try_from(input["maxStalledCount"].as_u64().unwrap_or(1))?;
    }
    if let Some(maximum) = input["maxStartedAttempts"].as_u64() {
        options.worker.max_started_attempts = Some(u32::try_from(maximum)?);
    }
    options.close_timeout = Duration::from_secs(5);
    Ok(options)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input: Value = serde_json::from_reader(std::io::stdin())?;
    let name = input["queue"].as_str().ok_or("Queue required")?;
    if input["mode"] == "connectProbe" {
        let result = if input["consumer"] == true {
            match BullMqConsumer::connect(
                name,
                |_: protocol::Job, _token| async { Ok(Value::Null) },
                options(&input)?,
            )
            .await
            {
                Ok(worker) => {
                    worker.close().await?;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        } else {
            match BullMqProducer::connect(name, options(&input)?).await {
                Ok(queue) => {
                    queue.close().await;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        println!(
            "{}",
            match result {
                Ok(()) => json!({"ok":true}),
                Err(error) => json!({"error":error.code}),
            }
        );
        return Ok(());
    }
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
        "metadata" => json!({"maxLenEvents":producer.inner().get_meta().await?.max_len_events}),
        "produceRejected" => {
            let error = producer
                .enqueue_job(input["event"].clone())
                .await
                .unwrap_err();
            json!({"error":error.code,"counts":producer.inner().get_job_counts().await?})
        }
        "abandon" => {
            let worker = BullMqConsumer::connect(
                name,
                |_: protocol::Job, _token| async { Ok(Value::Null) },
                options(&input)?,
            )
            .await?;
            let job = worker
                .inner()
                .get_next_job_nonblocking("fixture-abandoned-lock")
                .await?
                .ok_or("Waiting job required")?;
            let output = json!({"id":job.id(),"event":job.data()});
            worker.close().await?;
            output
        }
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
    let active = Arc::new(AtomicUsize::new(0));
    let high_water = Arc::new(AtomicUsize::new(0));
    let observed_peak = high_water.clone();
    let captured = attempts.clone();
    let started = std::time::Instant::now();
    let worker = BullMqConsumer::connect(
        name,
        move |job: protocol::Job, _token| {
            let captured = captured.clone();
            let active = active.clone();
            let high_water = high_water.clone();
            async move {
                if let Some(rows) = job.data()["logRows"].as_array() {
                    for row in rows {
                        job.log(row.as_str().ok_or_else(|| {
                            protocol::Error::ProcessingError("log_row_required".into())
                        })?)
                        .await?;
                    }
                }
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                high_water.fetch_max(count, Ordering::SeqCst);
                captured
                    .lock()
                    .await
                    .push(json!({"event":job.data(),"attempt":job.attempts_made(),"startedAtMs":started.elapsed().as_millis()}));
                if let Some(delay) = job.data()["sleepMs"].as_u64() {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                active.fetch_sub(1, Ordering::SeqCst);
                if u64::from(job.attempts_made()) < job.data()["failAttempts"].as_u64().unwrap_or(0)
                {
                    let reason = if job.data()["failureHistory"] == true {
                        format!("fixture_failure_{}", job.attempts_made() + 1)
                    } else {
                        "fixture_failure".into()
                    };
                    return Err(protocol::Error::ProcessingError(reason));
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
        json!({"peek":peek,"attempts":*attempts.lock().await,"failures":*failures.lock().unwrap(),"completed":counts.completed,"failed":counts.failed,"peakConcurrency":observed_peak.load(Ordering::SeqCst)}),
    )
}
