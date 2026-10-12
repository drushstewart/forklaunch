use super::{BlueprintResult, Value, adapter_error, field, values};
use icu_collator::{Collator, options::CollatorOptions};
use icu_locale_core::locale;
use std::cmp::Ordering;
pub(super) fn sort(rows: &mut [Value], query: &Value) -> BlueprintResult<()> {
    let Some(selected) = query["sortBy"].as_str().filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    // The actual memory adapter resolves the schema field inside its comparator:
    // zero/one row never invokes it, even for an unknown sort field.
    if rows.len() < 2 {
        return Ok(());
    }
    field(selected)?;
    let collator = Collator::try_new(locale!("en-US").into(), CollatorOptions::default())
        .map_err(|_| adapter_error())?;
    rows.sort_by(|left, right| {
        let (left, right) = (&left[selected], &right[selected]);
        let ordering = if left.is_null() && right.is_null() {
            Ordering::Equal
        } else if left.is_null() {
            Ordering::Less
        } else if right.is_null() {
            Ordering::Greater
        } else if selected == "createdAt" {
            let date = |value: &Value| {
                value
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            };
            date(left).cmp(&date(right))
        } else if left.is_number() && right.is_number() {
            values::number(Some(left))
                .partial_cmp(&values::number(Some(right)))
                .unwrap_or(Ordering::Equal)
        } else if left.is_boolean() && right.is_boolean() {
            left.as_bool().cmp(&right.as_bool())
        } else {
            collator.compare(&values::string(Some(left)), &values::string(Some(right)))
        };
        if query["sortDirection"] == "desc" {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(())
}
