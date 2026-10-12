//! Negotiates the configured Express redirect's two media types.
use axum::http::{HeaderMap, header};
use std::cmp::Ordering;
#[derive(Clone, Copy)]
struct Priority {
    specificity: u8,
    quality: f64,
    order: usize,
}
impl Priority {
    fn specificity_cmp(self, other: Self) -> Ordering {
        self.specificity
            .cmp(&other.specificity)
            .then_with(|| self.quality.total_cmp(&other.quality))
            .then_with(|| other.order.cmp(&self.order))
    }
    fn preference_cmp(self, other: Self) -> Ordering {
        self.quality
            .total_cmp(&other.quality)
            .then_with(|| self.specificity.cmp(&other.specificity))
            .then_with(|| other.order.cmp(&self.order))
    }
}
pub(super) fn preferred(headers: &HeaderMap) -> Option<&'static str> {
    let values = headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>();
    let accept = if values.is_empty() || values.iter().all(|value| value.is_empty()) {
        "*/*".to_owned()
    } else {
        values.join(",")
    };
    let mut selected: Option<(&str, Priority)> = None;
    for media in ["text/plain", "text/html"] {
        let priority = accept
            .split(',')
            .enumerate()
            .filter_map(|(order, value)| {
                let mut parts = value.split(';');
                let mime = parts.next()?.trim().to_ascii_lowercase();
                let specificity = if mime == media {
                    6
                } else if mime == "text/*" {
                    4
                } else if mime == "*/*" {
                    0
                } else {
                    return None;
                };
                let mut priority = Priority {
                    specificity,
                    quality: 1.0,
                    order,
                };
                for parameter in parts {
                    let (name, value) = parameter.trim().split_once('=')?;
                    let value = value.trim().trim_matches('"');
                    if name.trim().eq_ignore_ascii_case("q") {
                        priority.quality = value.parse().ok()?;
                        continue;
                    }
                    // Candidate media types have no parameters.
                    if value != "*" {
                        return None;
                    }
                    priority.specificity |= 1;
                }
                priority.quality.is_finite().then_some(priority)
            })
            .max_by(|left, right| left.specificity_cmp(*right));
        if let Some(priority) = priority.filter(|priority| priority.quality > 0.0)
            && selected.is_none_or(|(_, previous)| priority.preference_cmp(previous).is_gt())
        {
            selected = Some((media, priority));
        }
    }
    selected.map(|(media, _)| media)
}
