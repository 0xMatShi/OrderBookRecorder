use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use anyhow::Result;
use chrono::{DateTime, Utc};
use crate::models::{Recording, RecordingLine, RecordingMetadata, Tick};

pub struct RecordingStorage {
    recordings_dir: PathBuf,
}

impl RecordingStorage {
    pub fn new(recordings_dir: &str) -> Result<Self> {
        let path = PathBuf::from(recordings_dir);
        fs::create_dir_all(&path)?;
        Ok(Self { recordings_dir: path })
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
            title: title.to_string(),
            slug: slug.to_string(),
            up_token: up_token.to_string(),
            down_token: down_token.to_string(),
            start_time,
            end_time,
            total_ticks: 0,
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

    pub fn update_total_ticks(filepath: &Path, total_ticks: u32) -> Result<()> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().collect::<std::io::Result<_>>()?;

        if let Some(first_line) = lines.first_mut() {
            if let Ok(RecordingLine::Metadata(mut metadata)) = serde_json::from_str(first_line) {
                metadata.total_ticks = total_ticks;
                *first_line = serde_json::to_string(&RecordingLine::Metadata(metadata))?;
            }
        }

        let mut file = File::create(filepath)?;
        for line in lines {
            writeln!(file, "{}", line)?;
        }

        Ok(())
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
        let mut ticks = Vec::new();

        for line in reader.lines() {
            let line = line?;
            match serde_json::from_str::<RecordingLine>(&line)? {
                RecordingLine::Metadata(m) => metadata = Some(m),
                RecordingLine::Tick(t) => ticks.push(t),
            }
        }

        let metadata = metadata.ok_or_else(|| anyhow::anyhow!("No metadata found in recording"))?;
        Ok(Recording::new(metadata, ticks))
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
