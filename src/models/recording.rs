use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    pub title: String,
    pub slug: String,
    pub up_token: String,
    pub down_token: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub total_ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    pub ts: i64,                  // Unix ms
    pub up_bids: Vec<[f64; 2]>,   // [price, size] x 20
    pub down_bids: Vec<[f64; 2]>, // [price, size] x 20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RecordingLine {
    Metadata(RecordingMetadata),
    Tick(Tick),
}

#[derive(Debug, Clone)]
pub struct Recording {
    pub metadata: RecordingMetadata,
    pub ticks: Vec<Tick>,
}

impl Recording {
    pub fn new(metadata: RecordingMetadata, ticks: Vec<Tick>) -> Self {
        Self { metadata, ticks }
    }

    #[allow(dead_code)]
    pub fn duration_ms(&self) -> i64 {
        if self.ticks.is_empty() {
            return 0;
        }
        self.ticks.last().unwrap().ts - self.ticks.first().unwrap().ts
    }
}
