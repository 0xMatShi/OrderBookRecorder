# AGENTS.md

This file provides coding guidelines and conventions for AI coding agents working in this repository.

## Project Overview

Rust-based Polymarket order book recorder and replay tool. Records live order book data from WebSocket API for cryptocurrency prediction markets and provides TUI-based playback with demo trading capabilities.

## Build, Lint, and Test Commands

### Build
```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Check code without building
cargo check
```

### Run
```bash
# Development mode
cargo run

# Release mode
cargo run --release
```

### Linting
```bash
# Run clippy (Rust linter)
cargo clippy

# Run clippy with strict warnings
cargo clippy -- -D warnings

# Format code
cargo fmt

# Check formatting without modifying files
cargo fmt -- --check
```

### Testing
```bash
# Run all tests
cargo test

# Run a single test by name
cargo test test_name

# Run tests matching a pattern
cargo test pattern

# Run tests with output shown
cargo test -- --nocapture

# Run tests in a specific module
cargo test module_name::

# List all tests without running
cargo test -- --list
```

**Note:** This project currently has no unit tests (0 tests in codebase).

## Code Style Guidelines

### Module Organization

Use a three-module architecture:
- **models/** - Data structures and types (api.rs, websocket.rs, recording.rs)
- **download/** - Live data recording (scanner.rs, recorder.rs, storage.rs)
- **replay/** - Playback and visualization (player.rs, ui.rs)

Each module should have a `mod.rs` that re-exports public types:
```rust
pub mod scanner;
pub mod recorder;
pub use scanner::AutoScanner;
pub use recorder::Recorder;
```

### Imports

Order imports by scope (std → external crates → internal modules):
```rust
use std::fs::{self, File};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::time::{sleep, Duration};

use crate::models::{Recording, Tick};
use crate::download::storage::RecordingStorage;
```

Group imports logically and use `self` for renaming module imports when appropriate.

### Types and Structures

**Derive traits consistently:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    pub title: String,
    pub slug: String,
    // ...
}
```

**Use serde annotations for API compatibility:**
```rust
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolymarketEvent {
    pub slug: String,
    pub end_date: String,
    // ...
}
```

**Prefer enums for discriminated types:**
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coin {
    BTC,
    ETH,
    SOL,
    XRP,
}
```

### Naming Conventions

- **Types/Structs:** PascalCase (`RecordingMetadata`, `AutoScanner`)
- **Functions/Variables:** snake_case (`find_next_target`, `current_tick`)
- **Constants:** SCREAMING_SNAKE_CASE (`ORDER_BOOK_DEPTH`, `RECORDINGS_DIR`)
- **Module files:** snake_case (`recording.rs`, `demo_trading/`)

### Error Handling

**Use `anyhow::Result` for functions that can fail:**
```rust
pub async fn record(&self, target: &TargetMarket) -> Result<()> {
    // ...
}
```

**Use `anyhow::bail!` for custom error messages:**
```rust
if !response.status().is_success() {
    anyhow::bail!("API returned error: {}", response.status());
}
```

**Use `ok_or_else` for Option to Result conversion:**
```rust
let metadata = metadata.ok_or_else(|| anyhow::anyhow!("No metadata found"))?;
```

**Use `tracing` macros for logging:**
```rust
use tracing::{info, warn};

info!("🎯 Found target: {} ({})", target.title, target.slug);
warn!("⚠️ Error during scan: {}", e);
```

### Async/Await Patterns

**Mark async functions correctly:**
```rust
pub async fn find_next_target(&self, prefix: &str) -> Option<TargetMarket> {
    // Use .await for async operations
    let response = self.client.get(&self.api_url).send().await?;
}
```

**Use tokio::select! for concurrent operations:**
```rust
loop {
    tokio::select! {
        msg = ws_stream.next() => {
            // handle message
        }
        _ = check_interval.tick() => {
            // periodic check
        }
    }
}
```

### Constants and Configuration

Define module-level constants at the top:
```rust
const ORDER_BOOK_DEPTH: usize = 20;
const RECORDINGS_DIR: &str = "recordings";
pub const SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
```

### Comments and Documentation

- Use `//` for inline comments
- Add doc comments with `///` for public APIs when helpful
- Comments may include emoji for visual clarity (🎯, ✅, ⚠️, 📝)
- Keep comments concise and focused on "why" not "what"

### Dead Code

Use `#[allow(dead_code)]` for intentionally unused helper functions:
```rust
#[allow(dead_code)]
pub fn duration_ms(&self) -> i64 {
    // ...
}
```

## Project-Specific Patterns

### WebSocket Reconnection

Implement automatic reconnection in recorder:
```rust
loop {
    match self.run_stream_once(target, &filepath, end_date).await {
        Ok(_) => return Ok(()),
        Err(e) => {
            warn!("📉 WS disconnected: {}. Reconnecting...", e);
        }
    }
}
```

### File Format (JSONL)

Line 1: Metadata, Lines 2+: Ticks
```rust
let line = RecordingLine::Metadata(metadata);
writeln!(file, "{}", serde_json::to_string(&line)?)?;
```

### TUI State Management

Use mutable state structs with update methods:
```rust
pub struct ReplayState {
    pub current_tick: usize,
    pub is_paused: bool,
    pub speed_index: usize,
    // ...
}

impl ReplayState {
    pub fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused;
    }
}
```

## Common Patterns

### Pagination Handling
```rust
let mut offset = 0;
loop {
    let response = client.get(url)
        .query(&[("offset", &offset.to_string()), ("limit", "500")])
        .send().await?;
    // ...
    offset += 500;
}
```

### Sorting and Truncating
```rust
sorted_bids.sort_by(|a, b| b[0].partial_cmp(&a[0]).unwrap_or(std::cmp::Ordering::Equal));
sorted_bids.truncate(ORDER_BOOK_DEPTH);
```

### Time-based Logic
```rust
use chrono::{DateTime, Utc};

let end_dt = event.end_date.parse::<DateTime<Utc>>()?;
let minutes_left = end_dt.signed_duration_since(Utc::now()).num_seconds() as f64 / 60.0;
```
