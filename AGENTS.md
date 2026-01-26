# AGENTS.md

This file provides guidelines for AI coding agents working in this Rust-based Polymarket order book recorder repository.

## Project Overview

A Rust application that records live order book data from Polymarket's WebSocket API for cryptocurrency prediction markets and provides TUI-based replay and demo-trading capabilities.

**Key Modules:**
- `models/` - Data structures (API types, WebSocket messages, recording format)
- `download/` - Live data recording (scanner, recorder, storage)
- `replay/` - Playback visualization with TUI
- `demo_trading/` - Virtual trading simulation with precalculated strategy snapshots

## Build, Run, and Test Commands

### Build Commands
```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Check code without building (fast)
cargo check
```

### Run Commands
```bash
# Run in development mode
cargo run

# Run in release mode (recommended for production)
cargo run --release
```

### Test Commands
```bash
# Run all tests
cargo test

# Run a specific test
cargo test test_name

# Run tests in a specific module
cargo test module_name::

# Run tests with output shown
cargo test -- --nocapture

# Run tests in release mode
cargo test --release
```

### Other Useful Commands
```bash
# Format code
cargo fmt

# Run clippy linter
cargo clippy

# Clean build artifacts
cargo clean

# Build documentation
cargo doc --open
```

## Code Style Guidelines

### Module Organization

- **Public re-exports:** Use `mod.rs` to re-export public items:
  ```rust
  pub mod api;
  pub mod recording;
  pub mod websocket;
  
  pub use api::*;
  pub use recording::*;
  pub use websocket::*;
  ```

- **Module hierarchy:** Follow the three-module structure: `models/`, `download/`, `replay/`, `demo_trading/`

### Imports

- **Standard library first, then external crates, then local modules:**
  ```rust
  use anyhow::Result;
  use chrono::{DateTime, Utc};
  use std::fs::{self, File};
  use tokio::time::{sleep, Duration};
  
  use crate::download::storage::RecordingStorage;
  use crate::models::{BookMessage, TargetMarket};
  ```

- **Group related imports:** Use `{}` for multiple items from same module
- **Prefer explicit imports:** Avoid wildcard imports except for preludes and re-exports

### Formatting

- **Use `cargo fmt`** before committing (uses rustfmt)
- **Line length:** Generally keep under 100 characters
- **Indentation:** 4 spaces (enforced by rustfmt)
- **Trailing commas:** Use them in multi-line lists/structs

### Types and Structs

- **Derive traits explicitly:**
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct RecordingMetadata {
      pub title: String,
      pub slug: String,
      // ...
  }
  ```

- **Use `#[serde(rename_all = "camelCase")]`** for API compatibility when needed
- **Field visibility:** Make fields `pub` only when necessary
- **Enums for fixed sets:**
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

- **Types/Structs/Enums:** `PascalCase` (e.g., `RecordingMetadata`, `TargetMarket`)
- **Functions/methods:** `snake_case` (e.g., `find_next_target`, `load_recording`)
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `ORDER_BOOK_DEPTH`, `RECORDINGS_DIR`)
- **Module files:** `snake_case.rs`

### Error Handling

- **Use `anyhow::Result<T>`** for most functions that can fail
- **Use `?` operator** for error propagation:
  ```rust
  pub async fn record(&self, target: &TargetMarket) -> Result<()> {
      let end_date = target.end_date.parse::<DateTime<Utc>>()?;
      // ...
      Ok(())
  }
  ```

- **Use `anyhow::bail!`** for custom error messages:
  ```rust
  if events.is_empty() {
      anyhow::bail!("No events found");
  }
  ```

- **Log errors with tracing:**
  ```rust
  use tracing::{info, warn, error};
  
  if let Err(e) = operation() {
      warn!("Operation failed: {}", e);
  }
  ```

### Async/Await

- **Mark async functions clearly:**
  ```rust
  pub async fn find_next_target(&self, prefix: &str) -> Option<TargetMarket> {
      // ...
  }
  ```

- **Use `tokio::select!`** for concurrent operations:
  ```rust
  tokio::select! {
      msg = ws_stream.next() => { /* handle message */ }
      _ = check_interval.tick() => { /* handle tick */ }
  }
  ```

- **Sleep with tokio:** `tokio::time::sleep(Duration::from_secs(5)).await;`

### Comments and Documentation

- **Doc comments for public items:**
  ```rust
  /// Calculates the final result of a trading strategy on a recording.
  /// Returns EventResult with PnL and winner information.
  pub fn calculate_event_result(recording: &Recording) -> EventResult {
      // ...
  }
  ```

- **Inline comments for complex logic:**
  ```rust
  // Binary search for the closest tick
  let target_tick = self.recording.ticks
      .binary_search_by(|t| t.ts.cmp(&target_ts))
      .unwrap_or_else(|i| i.saturating_sub(1));
  ```

- **Use Russian for user-facing log messages** (matches existing codebase):
  ```rust
  info!("🎯 Найдена цель: {} ({})", target.title, target.slug);
  warn!("⚠️ Ошибка при сканировании: {}", e);
  ```

### Allow Directives

- **Use `#[allow(dead_code)]`** sparingly for intentionally unused code:
  ```rust
  #[allow(dead_code)]
  pub fn duration_ms(&self) -> i64 {
      // Helper method that may be used in future
  }
  ```

### Constants

- **Define at top of file or module:**
  ```rust
  const ORDER_BOOK_DEPTH: usize = 20;
  const RECORDINGS_DIR: &str = "recordings";
  const SPEEDS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0];
  ```

### WebSocket Patterns

- **Reconnection loop:**
  ```rust
  loop {
      match self.run_stream_once(target).await {
          Ok(_) => return Ok(()),
          Err(e) => warn!("WS disconnected: {}. Reconnecting...", e),
      }
  }
  ```

- **Message handling:**
  ```rust
  match msg {
      Some(Ok(Message::Text(text))) => { /* parse and process */ }
      Some(Ok(Message::Close(_))) => anyhow::bail!("WebSocket closed"),
      Some(Err(e)) => return Err(e.into()),
      None => anyhow::bail!("Connection lost"),
      _ => {}
  }
  ```

### State Management

- **Use structs for complex state:**
  ```rust
  pub struct ReplayState {
      pub recording: Recording,
      pub current_tick: usize,
      pub is_paused: bool,
      pub speed_index: usize,
      pub last_frame_time: Instant,
      pub accumulated_time_ms: f64,
  }
  ```

- **Provide builder/constructor methods:**
  ```rust
  impl ReplayState {
      pub fn new(recording: Recording) -> Self {
          Self {
              recording,
              current_tick: 0,
              is_paused: true,
              // ... initialize other fields
          }
      }
  }
  ```

### Performance Considerations

- **Pre-allocate collections when size is known:**
  ```rust
  self.snapshots.reserve(total_ticks);
  ```

- **Clone strategically:**
  ```rust
  // Clone tick data to avoid borrow checker issues
  let tick = self.recording.ticks[self.current_tick].clone();
  ```

- **Use references where possible:**
  ```rust
  pub fn append_tick(filepath: &Path, tick: &Tick) -> Result<()> {
      // ...
  }
  ```

### File I/O Patterns

- **Use `PathBuf` for paths:**
  ```rust
  pub fn create_recording_file(&self, slug: &str) -> Result<PathBuf> {
      let filename = format!("{}_{}.jsonl", slug, timestamp);
      let filepath = self.recordings_dir.join(&filename);
      // ...
  }
  ```

- **JSON Lines format for recordings:**
  ```rust
  // Line 1: metadata
  writeln!(file, "{}", serde_json::to_string(&RecordingLine::Metadata(metadata))?)?;
  // Lines 2+: ticks
  writeln!(file, "{}", serde_json::to_string(&RecordingLine::Tick(tick))?)?;
  ```

### Serde Custom Deserializers

- **When API returns strings instead of numbers:**
  ```rust
  fn deserialize_f64_from_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
  where
      D: serde::Deserializer<'de>,
  {
      use serde::de::Error;
      let s = String::deserialize(deserializer)?;
      s.parse::<f64>().map_err(D::Error::custom)
  }
  
  #[derive(Debug, Deserialize)]
  pub struct OrderSummary {
      #[serde(deserialize_with = "deserialize_f64_from_string")]
      pub price: f64,
  }
  ```

## Common Patterns

### API Pagination
```rust
let mut offset = 0;
let limit = 500;

loop {
    let response = self.client.get(&self.api_url)
        .query(&[("limit", &limit.to_string()), ("offset", &offset.to_string())])
        .send()
        .await?;
    
    let events: Vec<PolymarketEvent> = serde_json::from_slice(&response.bytes().await?)?;
    
    if events.is_empty() {
        break; // No more pages
    }
    
    // Process events...
    
    offset += limit;
}
```

### Time-based Tick Advancement
```rust
pub fn update(&mut self) {
    if self.is_paused { return; }
    
    let elapsed = self.last_frame_time.elapsed();
    self.last_frame_time = Instant::now();
    
    self.accumulated_time_ms += elapsed.as_secs_f64() * 1000.0 * self.current_speed();
    
    while self.current_tick < self.recording.ticks.len() - 1 {
        let delta_ms = (next_ts - current_ts) as f64;
        if self.accumulated_time_ms >= delta_ms {
            self.accumulated_time_ms -= delta_ms;
            self.current_tick += 1;
        } else {
            break;
        }
    }
}
```

## Testing Guidelines

- Tests are currently minimal; add tests in `#[cfg(test)]` modules
- Test naming: `test_<functionality>` in `snake_case`
- Use `#[tokio::test]` for async tests
- Prefer integration tests in `tests/` directory for complex scenarios

## Dependencies

**Key dependencies:**
- `tokio` - Async runtime (features: `full`)
- `serde`/`serde_json` - Serialization
- `reqwest` - HTTP client (features: `json`)
- `chrono` - Date/time handling
- `ratatui`/`crossterm` - TUI
- `tracing` - Logging
- `anyhow` - Error handling

When adding dependencies, prefer minimal feature sets to reduce compile time.

## Commit Guidelines

- Write clear, concise commit messages in English
- Focus on "why" rather than "what" in commit descriptions
- Use conventional commits format when appropriate (e.g., `feat:`, `fix:`, `refactor:`)

## Notes for AI Agents

- **User-facing messages:** Use Russian (matches existing codebase)
- **Code/comments/docs:** Use English
- **Emoji in logs:** Use sparingly (🎯, ✅, ❌, 📊, 🔍, ⚠️) as seen in existing code
- **Main entry point:** `src/main.rs` with three modes (download, replay, demo-trading)
- **Data flow:** Scanner → Recorder → Storage → Player/TUI
- **File format:** `.jsonl` with metadata on line 1, ticks on subsequent lines
