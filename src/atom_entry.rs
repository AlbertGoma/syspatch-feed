use chrono::{DateTime, FixedOffset};

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
