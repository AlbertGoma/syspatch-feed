use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct AtomEntry {
    pub id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub link: String,
    pub content: String,
}
