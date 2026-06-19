#![forbid(unsafe_code)]

use forklaunch_blueprint_core::BlueprintModule;

#[must_use]
pub fn module() -> BlueprintModule {
    BlueprintModule::new("sample-worker", "Sample Rust blueprint worker")
        .with_tag("kind", "worker")
        .with_tag("language", "rust")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleWorkerJob {
    pub id: String,
    pub payload_json: String,
}

#[must_use]
pub fn pending_jobs() -> Vec<SampleWorkerJob> {
    vec![SampleWorkerJob {
        id: "sample-job-1".to_owned(),
        payload_json: "{\"type\":\"sample\"}".to_owned(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_worker_has_a_real_job_shape() {
        let jobs = pending_jobs();

        assert_eq!(jobs[0].id, "sample-job-1");
        assert_eq!(
            module().tags.get("kind").map(String::as_str),
            Some("worker")
        );
    }
}
