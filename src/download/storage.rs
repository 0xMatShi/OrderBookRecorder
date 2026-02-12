use crate::models::{
    PriceChangeTick, Recording, RecordingLine, RecordingMetadata, Tick, TickSource,
    ORDER_BOOK_DEPTH,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct RecordingStorage {
    recordings_dir: PathBuf,
}

impl RecordingStorage {
    pub fn new(recordings_dir: &str) -> Result<Self> {
        let path = PathBuf::from(recordings_dir);
        fs::create_dir_all(&path)?;
        Ok(Self {
            recordings_dir: path,
        })
    }

    pub fn create_recording_file(
        &self,
        slug: &str,
        title: &str,
        up_token: &str,
        down_token: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<PathBuf> {
        let filename = format!("{}_{}.jsonl", slug, start_time.format("%Y%m%d_%H%M%S"));
        let filepath = self.recordings_dir.join(&filename);

        let metadata = RecordingMetadata {
            name: String::new(), // Empty by default
            title: title.to_string(),
            slug: slug.to_string(),
            up_token: up_token.to_string(),
            down_token: down_token.to_string(),
            start_time,
            end_time,
            total_ticks: 0,
            avg_latency_ms: None,
        };

        let line = RecordingLine::Metadata(metadata);
        let mut file = File::create(&filepath)?;
        writeln!(file, "{}", serde_json::to_string(&line)?)?;

        Ok(filepath)
    }

    pub fn append_tick(filepath: &Path, tick: &Tick) -> Result<()> {
        let line = RecordingLine::Tick(tick.clone());
        let mut file = OpenOptions::new().append(true).open(filepath)?;
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }

    pub fn append_price_change(filepath: &Path, pc: &PriceChangeTick) -> Result<()> {
        let line = RecordingLine::PriceChange(pc.clone());
        let mut file = OpenOptions::new().append(true).open(filepath)?;
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }

    pub fn update_metadata(filepath: &Path, total_ticks: u32, avg_latency_ms: Option<i64>) -> Result<()> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().collect::<std::io::Result<_>>()?;

        if let Some(first_line) = lines.first_mut() {
            if let Ok(RecordingLine::Metadata(mut metadata)) = serde_json::from_str(first_line) {
                metadata.total_ticks = total_ticks;
                metadata.avg_latency_ms = avg_latency_ms;
                *first_line = serde_json::to_string(&RecordingLine::Metadata(metadata))?;
            }
        }

        let mut file = File::create(filepath)?;
        for line in lines {
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }

    /// Обратная совместимость: обновить только total_ticks
    pub fn update_total_ticks(filepath: &Path, total_ticks: u32) -> Result<()> {
        Self::update_metadata(filepath, total_ticks, None)
    }

    pub fn list_recordings(&self) -> Result<Vec<PathBuf>> {
        let mut recordings = Vec::new();
        for entry in fs::read_dir(&self.recordings_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "jsonl") {
                recordings.push(path);
            }
        }
        recordings.sort();
        Ok(recordings)
    }

    pub fn load_recording(filepath: &Path) -> Result<Recording> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);

        let mut metadata: Option<RecordingMetadata> = None;
        let mut ticks: Vec<Tick> = Vec::new();
        let mut tick_sources: Vec<TickSource> = Vec::new();
        let mut price_changes: Vec<PriceChangeTick> = Vec::new();

        // Current snapshot state for applying price_change deltas
        let mut current_up_bids: Vec<[f64; 2]> = Vec::new();
        let mut current_down_bids: Vec<[f64; 2]> = Vec::new();
        let mut had_first_book = false;

        for line in reader.lines() {
            let line = line?;
            match serde_json::from_str::<RecordingLine>(&line)? {
                RecordingLine::Metadata(m) => metadata = Some(m),
                RecordingLine::Tick(t) => {
                    had_first_book = true;
                    current_up_bids = t.up_bids.clone();
                    current_down_bids = t.down_bids.clone();
                    ticks.push(t);
                    tick_sources.push(TickSource::Book);
                }
                RecordingLine::PriceChange(pc) => {
                    // Skip price_change events before first book snapshot
                    if !had_first_book {
                        continue;
                    }

                    // Save the price_change event for later analysis
                    price_changes.push(pc.clone());

                    // Apply each change to the current snapshot
                    for change in &pc.changes {
                        let bids = if change.outcome == "up" {
                            &mut current_up_bids
                        } else {
                            &mut current_down_bids
                        };

                        if change.size == 0.0 {
                            // Remove level
                            bids.retain(|b| (b[0] - change.price).abs() > 1e-9);
                        } else if let Some(level) =
                            bids.iter_mut().find(|b| (b[0] - change.price).abs() < 1e-9)
                        {
                            // Update existing level
                            level[1] = change.size;
                        } else {
                            // Add new level
                            bids.push([change.price, change.size]);
                        }

                        // Re-sort descending by price
                        bids.sort_by(|a, b| {
                            b[0].partial_cmp(&a[0]).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        // Truncate to depth
                        bids.truncate(ORDER_BOOK_DEPTH);
                    }

                    // Emit a full-snapshot tick
                    ticks.push(Tick {
                        ts: pc.ts,
                        up_bids: current_up_bids.clone(),
                        down_bids: current_down_bids.clone(),
                    });
                    tick_sources.push(TickSource::PriceChange);
                }
            }
        }

        let metadata = metadata.ok_or_else(|| anyhow::anyhow!("No metadata found in recording"))?;
        Ok(Recording::new(metadata, ticks, tick_sources, price_changes))
    }

    pub fn load_metadata(filepath: &Path) -> Result<RecordingMetadata> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);

        if let Some(Ok(line)) = reader.lines().next() {
            if let RecordingLine::Metadata(m) = serde_json::from_str(&line)? {
                return Ok(m);
            }
        }

        anyhow::bail!("No metadata found in recording")
    }
}
