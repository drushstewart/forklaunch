//! Seven-calendar-year Account anonymization. Cutoffs use the server's local
//! timezone and JavaScript-compatible leap-day/DST disambiguation.
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{
    DateTime, Datelike, Days, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc,
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

#[derive(Debug, Clone)]
pub struct RetentionOptions {
    pub dry_run: bool,
    pub batch_size: usize,
    pub entities: Option<Vec<String>>,
}
impl Default for RetentionOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            batch_size: 1000,
            entities: None,
        }
    }
}
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct EntityCounts {
    pub deleted: usize,
    pub anonymized: usize,
    pub errors: usize,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnforcementResult {
    pub processed: usize,
    pub deleted: usize,
    pub anonymized: usize,
    pub errors: usize,
    pub by_entity: BTreeMap<String, EntityCounts>,
    pub duration_ms: u64,
}
pub struct RetentionService<S> {
    store: Arc<S>,
}
impl<S: AuthStore> RetentionService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
    /// An error count is an incomplete enforcement, including read/commit
    /// failures. Earlier committed batches remain reflected in the totals.
    pub async fn enforce(
        &self,
        now: DateTime<Utc>,
        options: &RetentionOptions,
    ) -> EnforcementResult {
        let started = Instant::now();
        let mut result = EnforcementResult::default();
        if options
            .entities
            .as_ref()
            .is_none_or(|names| names.iter().any(|name| name == "Account"))
        {
            let mut counts = EntityCounts::default();
            match account_cutoff(now) {
                Ok(cutoff) => loop {
                    if let Ok(count) = self.batch(cutoff, now, options).await {
                        counts.anonymized += count;
                        if options.dry_run || count == 0 {
                            break;
                        }
                    } else {
                        counts.errors += 1;
                        break;
                    }
                },
                Err(_) => counts.errors += 1,
            }
            result.processed = counts.anonymized;
            result.anonymized = counts.anonymized;
            result.errors = counts.errors;
            result.by_entity.insert("Account".into(), counts);
        }
        result.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        result
    }
    async fn batch(
        &self,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
        options: &RetentionOptions,
    ) -> BlueprintResult<usize> {
        let mut tx = self.store.begin(!options.dry_run).await?;
        // Keep only the oldest batch, with insertion order breaking equal dates.
        // Dry runs count all matches without retaining decrypted account records.
        let mut rows = BTreeMap::new();
        let mut matches = 0;
        let mut after = None;
        loop {
            let page = tx.list_page(Model::Account, after, 256).await?;
            for row in page.records {
                if row.get("retentionAnonymizedAt").is_none_or(Value::is_null)
                    && let Some(date) = created(&row).filter(|date| *date < cutoff)
                {
                    matches += 1;
                    if !options.dry_run && options.batch_size > 0 {
                        rows.insert((date, matches), row);
                        if rows.len() > options.batch_size {
                            rows.pop_last();
                        }
                    }
                }
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        if options.dry_run {
            tx.commit().await?;
            return Ok(matches);
        }
        let count = rows.len();
        let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        for mut row in rows.into_values() {
            for field in ["accessToken", "refreshToken", "idToken", "password"] {
                row[field] = Value::Null;
            }
            row["retentionAnonymizedAt"] = Value::String(stamp.clone());
            row["updatedAt"] = Value::String(stamp.clone());
            tx.replace(Model::Account, row).await?;
        }
        tx.commit().await?;
        Ok(count)
    }
}
fn created(row: &Value) -> Option<DateTime<Utc>> {
    row.get("createdAt")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}
/// # Errors
/// Rejects an out-of-range calendar cutoff or unresolved local timezone.
pub fn account_cutoff(now: DateTime<Utc>) -> BlueprintResult<DateTime<Utc>> {
    let now = DateTime::<Utc>::from_timestamp_millis(now.timestamp_millis()).ok_or_else(invalid)?;
    let local = now.with_timezone(&Local);
    let year = local.year().checked_sub(7).ok_or_else(invalid)?;
    // JS setFullYear rolls Feb 29 forward to Mar 1 rather than clamping.
    let day = NaiveDate::from_ymd_opt(year, local.month(), 1)
        .ok_or_else(invalid)?
        .checked_add_days(Days::new(u64::from(local.day() - 1)))
        .ok_or_else(invalid)?;
    let target = day.and_time(local.time());
    if let Some(mapped) = earliest(target) {
        return Ok(mapped);
    }
    // In a skipped local interval JS uses the offset before the gap, advancing
    // by its width (including half-hour and skipped-day timezone transitions).
    for days in 1..=7 {
        let prior = target
            .checked_sub_signed(Duration::days(days))
            .ok_or_else(invalid)?;
        if let Some(prior) = earliest(prior) {
            let utc = target
                .checked_sub_signed(Duration::seconds(i64::from(
                    prior.with_timezone(&Local).offset().local_minus_utc(),
                )))
                .ok_or_else(invalid)?;
            return Ok(DateTime::from_naive_utc_and_offset(utc, Utc));
        }
    }
    Err(invalid())
}
// Some system-TZ mappings order ambiguous offsets rather than UTC instants
// and include transition endpoints. Select by UTC and validate round-trips.
fn earliest(local: NaiveDateTime) -> Option<DateTime<Utc>> {
    let candidates = match Local.from_local_datetime(&local) {
        LocalResult::None => [None, None],
        LocalResult::Single(value) => [Some(value), None],
        LocalResult::Ambiguous(first, second) => [Some(first), Some(second)],
    };
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.with_timezone(&Utc))
        .filter(|value| value.with_timezone(&Local).naive_local() == local)
        .min()
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "better_auth_retention_invalid",
        "Unable to resolve account retention cutoff",
    )
}

#[cfg(test)]
pub(crate) mod tests;
