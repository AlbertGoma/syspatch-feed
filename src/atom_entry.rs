use chrono::{DateTime, FixedOffset};
use std::cmp::Ordering;

#[derive(Debug)]
pub struct AtomEntry {
    pub id: String,
    pub title: String,
    pub updated: DateTime<FixedOffset>,
    pub link: String,
    pub content: String,
    pub release_version: u16,
    pub iteration_count: usize,
}

impl AtomEntry {
    pub(crate) fn cmp_entries(a: &AtomEntry, b: &AtomEntry) -> Ordering {
        match b.updated.cmp(&a.updated) {
            Ordering::Equal => match b.release_version.cmp(&a.release_version) {
                Ordering::Equal => b.iteration_count.cmp(&a.iteration_count),
                o => o,
            },
            o => o,
        }
    }
}
