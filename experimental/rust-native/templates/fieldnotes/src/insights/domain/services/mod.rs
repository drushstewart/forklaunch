mod insight;
mod publication;
pub use insight::analyze;
pub(crate) use publication::process_job;

pub(crate) mod runtime;
