#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use forklaunch_blueprint_core::BlueprintResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthStatus {
    pub state: HealthState,
    pub service: String,
    pub message: Option<String>,
}

#[must_use]
pub fn healthy(service: impl Into<String>) -> HealthStatus {
    HealthStatus {
        state: HealthState::Healthy,
        service: service.into(),
        message: None,
    }
}

#[must_use]
pub fn degraded(service: impl Into<String>, message: impl Into<String>) -> HealthStatus {
    HealthStatus {
        state: HealthState::Degraded,
        service: service.into(),
        message: Some(message.into()),
    }
}

#[must_use]
pub fn unhealthy(service: impl Into<String>, message: impl Into<String>) -> HealthStatus {
    HealthStatus {
        state: HealthState::Unhealthy,
        service: service.into(),
        message: Some(message.into()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
}

impl Metric {
    #[must_use]
    pub fn count(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: Some("count".to_owned()),
        }
    }

    #[must_use]
    pub fn milliseconds(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: Some("ms".to_owned()),
        }
    }
}

/// The result of a single readiness check (e.g. database, cache).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessCheck {
    pub name: String,
    pub ready: bool,
    pub detail: Option<String>,
}

impl ReadinessCheck {
    #[must_use]
    pub fn up(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ready: true,
            detail: None,
        }
    }

    #[must_use]
    pub fn down(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ready: false,
            detail: Some(detail.into()),
        }
    }
}

/// An aggregate readiness report for a service and its dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub ready: bool,
    pub checks: Vec<ReadinessCheck>,
}

/// An audit event recording a security- or compliance-relevant action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub resource: String,
    pub at_unix_seconds: u64,
}

/// A structured telemetry event emitted alongside spans and metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryEvent {
    pub name: String,
    pub level: String,
    pub attributes: BTreeMap<String, String>,
}

impl TelemetryEvent {
    #[must_use]
    pub fn new(name: impl Into<String>, level: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: level.into(),
            attributes: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// A reusable liveness handler: a service that responds is considered alive.
#[must_use]
pub fn liveness(service: impl Into<String>) -> HealthStatus {
    healthy(service)
}

/// A reusable readiness handler aggregating dependency checks.
///
/// The service is ready only when every check is ready.
#[must_use]
pub fn aggregate_readiness(checks: Vec<ReadinessCheck>) -> Readiness {
    let ready = checks.iter().all(|check| check.ready);
    Readiness { ready, checks }
}

pub trait MonitoringSink {
    /// # Errors
    ///
    /// Returns an error when the health sink cannot publish the status.
    fn publish_health(&self, status: HealthStatus) -> BlueprintResult<()>;

    /// # Errors
    ///
    /// Returns an error when the metric sink cannot publish the metric.
    fn publish_metric(&self, metric: Metric) -> BlueprintResult<()>;

    /// # Errors
    ///
    /// Returns an error when the audit sink cannot publish the event.
    fn publish_audit_event(&self, _event: AuditEvent) -> BlueprintResult<()> {
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the telemetry sink cannot publish the event.
    fn publish_telemetry(&self, _event: TelemetryEvent) -> BlueprintResult<()> {
        Ok(())
    }
}

/// Monitoring HTTP route contracts (health, readiness, metrics).
pub mod contracts {
    use forklaunch_blueprint_core::contract::{
        BodyContract, ContractRegistry, RouteContract, ServiceContract,
    };

    /// Builds the monitoring service contract registry.
    #[must_use]
    pub fn registry() -> ContractRegistry {
        let mut registry =
            ContractRegistry::for_service(ServiceContract::new("monitoring", "0.1.0"));

        registry.register(
            RouteContract::get("Liveness", "Liveness probe", "/health")
                .with_response(200, Some(BodyContract::json("HealthStatus"))),
        );

        registry.register(
            RouteContract::get("Readiness", "Readiness probe", "/ready")
                .with_response(200, Some(BodyContract::json("Readiness")))
                .with_response(503, Some(BodyContract::json("Readiness"))),
        );

        registry.register(
            RouteContract::get("Metrics", "Prometheus metrics", "/metrics")
                .with_response(200, None),
        );

        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_helpers_track_state_and_message() {
        let status = degraded("accounts", "database latency");

        assert_eq!(status.state, HealthState::Degraded);
        assert_eq!(status.message.as_deref(), Some("database latency"));
    }

    #[test]
    fn readiness_aggregates_dependency_checks() {
        let ready = aggregate_readiness(vec![
            ReadinessCheck::up("database"),
            ReadinessCheck::up("cache"),
        ]);
        assert!(ready.ready);

        let not_ready = aggregate_readiness(vec![
            ReadinessCheck::up("database"),
            ReadinessCheck::down("cache", "connection refused"),
        ]);
        assert!(!not_ready.ready);
    }

    #[test]
    fn monitoring_contracts_expose_probes() {
        let registry = contracts::registry();
        let operation_names: Vec<&str> = registry
            .routes()
            .iter()
            .map(|route| route.operation_name.as_str())
            .collect();

        assert!(operation_names.contains(&"liveness"));
        assert!(operation_names.contains(&"readiness"));
        assert!(operation_names.contains(&"metrics"));
    }
}
