# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust-based Polymarket order book recorder and replay tool. It records live order book data from Polymarket's WebSocket API for cryptocurrency prediction markets (BTC, ETH, SOL, XRP) and allows playback of recorded sessions with a TUI interface.

## Common Commands

### Build and Run
```bash
cargo build --release
cargo run --release
```

### Development
```bash
cargo build
cargo run
cargo check
```

### Testing
```bash
cargo test
```

## Architecture

### Three-Module Design

The application is structured into three main modules:

1. **models/** - Data structures and types
   - `api.rs` - Polymarket API types (PolymarketEvent, TargetMarket, Coin enum)
   - `websocket.rs` - WebSocket message types (BookMessage, SubscribeMessage)
   - `recording.rs` - Recording format (RecordingMetadata, Tick, RecordingLine)

2. **download/** - Live data recording system
   - `scanner.rs` - AutoScanner continuously polls Polymarket API to find active markets within time windows (0-15 min to end)
   - `recorder.rs` - Recorder connects to WebSocket, subscribes to order book updates, and saves ticks
   - `storage.rs` - RecordingStorage manages .jsonl files (metadata on first line, ticks on subsequent lines)

3. **replay/** - Playback and visualization
   - `player.rs` - ReplayState manages playback state, speed control (0.25x-2x), and time-based tick advancement
   - `ui.rs` - TUI using ratatui/crossterm for visualizing order books during replay

### Data Flow

**Recording Flow:**
1. AutoScanner polls `https://gamma-api.polymarket.com/events` in paginated batches
2. Filters events by coin slug prefix (e.g., "btc-updown-15m") and time window
3. Extracts up/down token IDs from matched events
4. Recorder establishes WebSocket connection to `wss://ws-subscriptions-clob.polymarket.com/ws/market`
5. Subscribes to both tokens, receives BookMessage updates
6. Maintains top 20 bids for each token, writes Tick snapshots to .jsonl file
7. Continues until event end_date, then searches for next target

**Replay Flow:**
1. RecordingStorage lists .jsonl files from `recordings/` directory
2. User selects recording, loads metadata and all ticks into memory
3. ReplayState advances current_tick based on elapsed real time × playback speed
4. UI renders current tick's order book data in TUI

### File Format

Recordings are stored as `.jsonl` files with this structure:
- **Line 1:** JSON object with `{"type":"metadata", "title":..., "slug":..., "up_token":..., "down_token":..., "start_time":..., "end_time":..., "total_ticks":...}`
- **Lines 2+:** JSON objects with `{"type":"tick", "ts":..., "up_bids":[[price,size],...], "down_bids":[[price,size],...]}`

The `total_ticks` field in metadata is updated when recording completes.

### Key Constants

- `ORDER_BOOK_DEPTH: usize = 20` - Number of bid levels recorded per token
- `RECORDINGS_DIR: &str = "recordings"` - Output directory for recordings
- `SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0]` - Available playback speeds

### WebSocket Reconnection

The Recorder implements automatic reconnection on WebSocket failures. If the connection drops, it reconnects and continues appending to the same recording file until the event's end_date is reached.

### API Pagination

AutoScanner handles paginated API responses with `offset`/`limit` parameters (limit=500). It continues fetching pages until either a matching event is found or no more events are returned.
