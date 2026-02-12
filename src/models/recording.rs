use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const ORDER_BOOK_DEPTH: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    #[serde(default)]
    pub name: String,
    pub title: String,
    pub slug: String,
    pub up_token: String,
    pub down_token: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub total_ticks: u32,
    /// Средняя разница между локальным временем получения и серверным timestamp (в миллисекундах)
    /// Положительное значение означает, что локальное время опережает серверное (обычный случай из-за latency)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_latency_ms: Option<i64>,
}

impl RecordingMetadata {
    /// Get display name - uses `name` if not empty, otherwise falls back to `slug`
    pub fn display_name(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else {
            &self.slug
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    pub ts: i64,                  // Unix ms
    pub up_bids: Vec<[f64; 2]>,   // [price, size] x 20
    pub down_bids: Vec<[f64; 2]>, // [price, size] x 20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChangeLevel {
    pub outcome: String,
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChangeTick {
    pub ts: i64,
    pub changes: Vec<PriceChangeLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickSource {
    Book,
    PriceChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordingLine {
    Metadata(RecordingMetadata),
    Tick(Tick),
    PriceChange(PriceChangeTick),
}

#[derive(Debug, Clone)]
pub struct Recording {
    pub metadata: RecordingMetadata,
    pub ticks: Vec<Tick>,
    pub tick_sources: Vec<TickSource>,
    pub price_changes: Vec<PriceChangeTick>,
}

impl Recording {
    pub fn new(
        metadata: RecordingMetadata,
        ticks: Vec<Tick>,
        tick_sources: Vec<TickSource>,
        price_changes: Vec<PriceChangeTick>,
    ) -> Self {
        Self {
            metadata,
            ticks,
            tick_sources,
            price_changes,
        }
    }

    pub fn book_count(&self) -> usize {
        self.tick_sources
            .iter()
            .filter(|s| **s == TickSource::Book)
            .count()
    }

    pub fn price_change_count(&self) -> usize {
        self.tick_sources
            .iter()
            .filter(|s| **s == TickSource::PriceChange)
            .count()
    }

    #[allow(dead_code)]
    pub fn duration_ms(&self) -> i64 {
        if self.ticks.is_empty() {
            return 0;
        }
        self.ticks.last().unwrap().ts - self.ticks.first().unwrap().ts
    }
}
