use crate::models::{Recording, TickSource};
use crate::replay::player::SPEEDS;
use chrono::{TimeZone, Utc};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const SIZE_EPSILON: f64 = 0.5;
const CHECKPOINT_INTERVAL: usize = 500;
const MAX_PLACEMENT_LEVEL: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackedOrderStatus {
    Open,
    Filled,
    Cancelled,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackedOrder {
    pub id: usize,
    pub side: String,         // "up" / "down"
    pub price: f64,
    pub size: f64,            // target_size
    pub queue_ahead: f64,     // shares before trader when placed
    pub filled: f64,          // how much has been filled
    pub remaining: f64,       // size - filled
    pub placed_tick: usize,
    pub placed_ts: i64,
    pub status: TrackedOrderStatus,
    pub resolved_tick: Option<usize>,
    pub resolved_ts: Option<i64>,
}

/// Sparse checkpoint — saved every CHECKPOINT_INTERVAL ticks
#[derive(Debug, Clone)]
struct Checkpoint {
    open_orders: Vec<TrackedOrder>,
    history: Vec<TrackedOrder>,
    next_id: usize,
}

pub struct SizeTrackerState {
    pub recording: Recording,
    pub target_size: f64,
    pub current_tick: usize,
    pub open_orders: Vec<TrackedOrder>,
    pub history: Vec<TrackedOrder>,
    next_id: usize,

    // Playback control
    pub is_paused: bool,
    pub speed_index: usize,
    pub last_frame_time: Instant,
    pub accumulated_time_ms: f64,

    // Sparse checkpoints (every CHECKPOINT_INTERVAL ticks)
    checkpoints: Vec<Checkpoint>,
    // Pre-built set of timestamps that have book ticks (for O(1) lookup)
    book_timestamps: HashSet<i64>,
}

impl SizeTrackerState {
    pub fn new(recording: Recording, target_size: f64) -> Self {
        // Pre-build book_timestamps set for O(1) lookup
        let book_timestamps: HashSet<i64> = recording
            .tick_sources
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == TickSource::Book)
            .map(|(i, _)| recording.ticks[i].ts)
            .collect();

        let mut state = Self {
            recording,
            target_size,
            current_tick: 0,
            open_orders: Vec::new(),
            history: Vec::new(),
            next_id: 1,
            is_paused: true,
            speed_index: 5, // 1x speed
            last_frame_time: Instant::now(),
            accumulated_time_ms: 0.0,
            checkpoints: Vec::new(),
            book_timestamps,
        };

        state.precalculate();
        state
    }

    pub fn current_speed(&self) -> f64 {
        SPEEDS[self.speed_index]
    }

    pub fn speed_up(&mut self) {
        if self.speed_index < SPEEDS.len() - 1 {
            self.speed_index += 1;
        }
    }

    pub fn speed_down(&mut self) {
        if self.speed_index > 0 {
            self.speed_index -= 1;
        }
    }

    pub fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused;
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    pub fn move_ticks(&mut self, delta: i32) {
        let new_tick = self.current_tick as i32 + delta;
        let target = new_tick.clamp(0, self.recording.ticks.len().saturating_sub(1) as i32) as usize;
        self.seek_to(target);
    }

    pub fn seek_to_start(&mut self) {
        self.seek_to(0);
        self.is_paused = true;
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    pub fn jump_to_quarter(&mut self, quarter: u8) {
        if !(1..=4).contains(&quarter) || self.recording.ticks.is_empty() {
            return;
        }
        let total = self.recording.ticks.len();
        let target = ((total as f64 * quarter as f64) / 4.0).floor() as usize;
        let target = target.min(total - 1);
        self.seek_to(target);
        self.is_paused = true;
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    pub fn update(&mut self) {
        if self.is_paused || self.recording.ticks.is_empty() {
            self.last_frame_time = Instant::now();
            return;
        }

        let elapsed = self.last_frame_time.elapsed();
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms += elapsed.as_secs_f64() * 1000.0 * self.current_speed();

        while self.current_tick < self.recording.ticks.len() - 1 {
            let current_ts = self.recording.ticks[self.current_tick].ts;
            let next_ts = self.recording.ticks[self.current_tick + 1].ts;
            let delta_ms = (next_ts - current_ts) as f64;

            if self.accumulated_time_ms >= delta_ms {
                self.accumulated_time_ms -= delta_ms;
                self.current_tick += 1;
                // Forward playback: just process the next tick (no snapshot restore)
                if self.current_tick > 0 {
                    self.process_tick(self.current_tick);
                }
            } else {
                break;
            }
        }
    }

    pub fn current_time_str(&self) -> String {
        if self.recording.ticks.is_empty() {
            return "00:00:00.000".to_string();
        }
        let ts = self.recording.ticks[self.current_tick].ts;
        let dt = Utc.timestamp_millis_opt(ts).unwrap();
        dt.format("%H:%M:%S%.3f").to_string()
    }

    pub fn total_time_str(&self) -> String {
        self.recording.metadata.end_time.format("%H:%M:%S%.3f").to_string()
    }

    // --- Seek: restore nearest checkpoint, then replay forward ---

    fn seek_to(&mut self, target: usize) {
        if self.recording.ticks.is_empty() {
            return;
        }

        // If moving forward by 1 tick, just process (common case during playback)
        if target == self.current_tick + 1 {
            self.current_tick = target;
            self.process_tick(target);
            return;
        }

        // Find nearest checkpoint at or before target
        let cp_index = target / CHECKPOINT_INTERVAL;
        let cp_index = cp_index.min(self.checkpoints.len().saturating_sub(1));
        let cp_tick = cp_index * CHECKPOINT_INTERVAL;

        // Restore checkpoint
        let cp = &self.checkpoints[cp_index];
        self.open_orders = cp.open_orders.clone();
        self.history = cp.history.clone();
        self.next_id = cp.next_id;

        // Replay forward from checkpoint to target
        for t in (cp_tick + 1)..=target {
            self.process_tick(t);
        }

        self.current_tick = target;
    }

    // --- Precalculation: build sparse checkpoints ---

    fn precalculate(&mut self) {
        if self.recording.ticks.is_empty() {
            return;
        }

        let total = self.recording.ticks.len();
        let num_checkpoints = total / CHECKPOINT_INTERVAL + 1;
        println!(
            "Precalculating size analysis for {} ticks ({} checkpoints)...",
            total, num_checkpoints
        );

        self.open_orders.clear();
        self.history.clear();
        self.next_id = 1;
        self.checkpoints.clear();
        self.checkpoints.reserve(num_checkpoints);

        // Save checkpoint for tick 0 (no processing needed)
        self.save_checkpoint();

        let progress_step = (total / 10).max(1);
        for tick_idx in 1..total {
            self.current_tick = tick_idx;
            self.process_tick(tick_idx);

            // Save checkpoint every CHECKPOINT_INTERVAL ticks
            if tick_idx % CHECKPOINT_INTERVAL == 0 {
                self.save_checkpoint();
            }

            if tick_idx % progress_step == 0 {
                let pct = (tick_idx as f64 / total as f64 * 100.0) as u32;
                println!("  {}% ({}/{})", pct, tick_idx, total);
            }
        }

        println!(
            "Done! {} orders detected ({} open, {} in history)",
            self.next_id - 1,
            self.open_orders.len(),
            self.history.len()
        );

        // Reset to beginning
        self.current_tick = 0;
        self.seek_to(0);
    }

    fn save_checkpoint(&mut self) {
        self.checkpoints.push(Checkpoint {
            open_orders: self.open_orders.clone(),
            history: self.history.clone(),
            next_id: self.next_id,
        });
    }

    fn process_tick(&mut self, tick_idx: usize) {
        if tick_idx == 0 {
            return;
        }

        let tick = &self.recording.ticks[tick_idx];
        let prev_tick = &self.recording.ticks[tick_idx - 1];
        let ts = tick.ts;

        let is_from_book = if tick_idx < self.recording.tick_sources.len() {
            self.recording.tick_sources[tick_idx] == TickSource::Book
        } else {
            false
        };

        // Collect price_change prices at current ts for fill detection
        let pc_prices_at_ts: Vec<(String, f64)> = self
            .recording
            .price_changes
            .iter()
            .filter(|pc| pc.ts == ts)
            .flat_map(|pc| &pc.changes)
            .map(|c| (c.outcome.clone(), c.price))
            .collect();

        // O(1) lookup instead of O(n) scan
        let has_book_at_ts = if is_from_book {
            true
        } else {
            self.book_timestamps.contains(&ts)
        };

        // Step 1: Detect new placements
        for side in &["up", "down"] {
            let (current_bids, prev_bids) = if *side == "up" {
                (&tick.up_bids, &prev_tick.up_bids)
            } else {
                (&tick.down_bids, &prev_tick.down_bids)
            };

            let top_levels = &current_bids[..current_bids.len().min(MAX_PLACEMENT_LEVEL)];
            for level in top_levels {
                let price = level[0];
                let size = level[1];

                let prev_size = prev_bids
                    .iter()
                    .find(|b| (b[0] - price).abs() < 1e-9)
                    .map(|b| b[1])
                    .unwrap_or(0.0);

                let delta = size - prev_size;

                // Match only exact target_size (not multiples)
                if (delta - self.target_size).abs() < SIZE_EPSILON && delta > 0.0 {
                    let order = TrackedOrder {
                        id: self.next_id,
                        side: side.to_string(),
                        price,
                        size: self.target_size,
                        queue_ahead: prev_size,
                        filled: 0.0,
                        remaining: self.target_size,
                        placed_tick: tick_idx,
                        placed_ts: ts,
                        status: TrackedOrderStatus::Open,
                        resolved_tick: None,
                        resolved_ts: None,
                    };
                    self.next_id += 1;
                    self.open_orders.push(order);
                }
            }
        }

        // Step 2: Pre-compute cancel budget per (side, price_cents)
        let mut cancel_budget: HashMap<(String, u32), usize> = HashMap::new();

        for side in &["up", "down"] {
            let (current_bids, prev_bids) = if *side == "up" {
                (&tick.up_bids, &prev_tick.up_bids)
            } else {
                (&tick.down_bids, &prev_tick.down_bids)
            };

            for prev_level in prev_bids.iter() {
                let price = prev_level[0];
                let prev_size = prev_level[1];
                let current_size = current_bids
                    .iter()
                    .find(|b| (b[0] - price).abs() < 1e-9)
                    .map(|b| b[1])
                    .unwrap_or(0.0);
                let level_delta = current_size - prev_size;

                // Match only exact target_size cancel (not multiples)
                if (level_delta + self.target_size).abs() < SIZE_EPSILON && level_delta < 0.0 {
                    let is_fill = is_from_book
                        || (has_book_at_ts
                            && pc_prices_at_ts.iter().any(|(o, p)| {
                                o.as_str() == *side && (p - price).abs() < 1e-6
                            }));
                    if !is_fill {
                        let key = (side.to_string(), (price * 100.0).round() as u32);
                        cancel_budget.insert(key, 1);
                    }
                }
            }
        }

        // Step 3: Update open orders
        // Uses decremental queue_ahead instead of peak_size:
        // - FILL (FIFO): queue_ahead decreases by fill amount, then our order fills
        // - CANCEL: queue_ahead decreases proportionally (cancel position unknown)
        // - ADDITION: queue_ahead unchanged (new orders go behind us in FIFO)
        let mut resolved_indices = Vec::new();

        for (i, order) in self.open_orders.iter_mut().enumerate() {
            let (current_bids, prev_bids_for_order) = if order.side == "up" {
                (&tick.up_bids, &prev_tick.up_bids)
            } else {
                (&tick.down_bids, &prev_tick.down_bids)
            };

            let best_bid = current_bids.first().map(|b| b[0]).unwrap_or(0.0);
            let current_size_at_level = current_bids
                .iter()
                .find(|b| (b[0] - order.price).abs() < 1e-9)
                .map(|b| b[1]);

            let prev_size_at_level = prev_bids_for_order
                .iter()
                .find(|b| (b[0] - order.price).abs() < 1e-9)
                .map(|b| b[1])
                .unwrap_or(0.0);

            // Full fill: best_bid dropped below our price
            if best_bid < order.price - 0.001 && best_bid > 0.0 {
                order.filled = order.size;
                order.remaining = 0.0;
                order.status = TrackedOrderStatus::Filled;
                order.resolved_tick = Some(tick_idx);
                order.resolved_ts = Some(ts);
                resolved_indices.push(i);
                continue;
            }

            let is_at_best_bid = (best_bid - order.price).abs() < 0.001;

            match current_size_at_level {
                Some(current_size) => {
                    let delta = current_size - prev_size_at_level;

                    if delta < -SIZE_EPSILON {
                        // Size decreased at our level
                        let decrease = -delta;

                        // Determine if this decrease is a fill (trade) or cancel
                        // Fill = book event at best_bid, or price_change + book with same timestamp
                        let is_fill_at_level = (is_from_book && is_at_best_bid)
                            || (has_book_at_ts
                                && pc_prices_at_ts.iter().any(|(o, p)| {
                                    o.as_str() == order.side
                                        && (p - order.price).abs() < 1e-6
                                }));

                        if is_fill_at_level {
                            // FIFO: fills consume from front of queue
                            if order.queue_ahead > SIZE_EPSILON {
                                let from_ahead = decrease.min(order.queue_ahead);
                                order.queue_ahead -= from_ahead;
                                let remainder = decrease - from_ahead;
                                if remainder > SIZE_EPSILON {
                                    let our_fill = remainder.min(order.remaining);
                                    order.filled += our_fill;
                                    order.remaining -= our_fill;
                                }
                            } else {
                                // Queue cleared, fills go into our order
                                order.queue_ahead = 0.0;
                                let our_fill = decrease.min(order.remaining);
                                order.filled += our_fill;
                                order.remaining -= our_fill;
                            }
                        } else {
                            // Cancel: proportional decrease of queue_ahead
                            // (cancel position in queue is unknown)
                            if order.queue_ahead > SIZE_EPSILON {
                                let total_excluding_us =
                                    prev_size_at_level - order.remaining;
                                if total_excluding_us > SIZE_EPSILON {
                                    let fraction_ahead = (order.queue_ahead
                                        / total_excluding_us)
                                        .min(1.0);
                                    order.queue_ahead = (order.queue_ahead
                                        - decrease * fraction_ahead)
                                        .max(0.0);
                                }
                            }
                        }

                        // Check if fully filled
                        if order.remaining <= 0.01 {
                            order.remaining = 0.0;
                            order.filled = order.size;
                            order.status = TrackedOrderStatus::Filled;
                            order.resolved_tick = Some(tick_idx);
                            order.resolved_ts = Some(ts);
                            resolved_indices.push(i);
                            continue;
                        }
                    }

                    // Detect cancellation of our tracked order using pre-computed budget
                    if order.filled < 0.01 {
                        let key = (order.side.clone(), (order.price * 100.0).round() as u32);
                        if let Some(budget) = cancel_budget.get_mut(&key) {
                            if *budget > 0 {
                                *budget -= 1;
                                order.status = TrackedOrderStatus::Cancelled;
                                order.resolved_tick = Some(tick_idx);
                                order.resolved_ts = Some(ts);
                                resolved_indices.push(i);
                                continue;
                            }
                        }
                    }
                }
                None => {
                    // Level disappeared entirely
                    if order.filled < 0.01 {
                        let is_fill = is_from_book
                            || (has_book_at_ts
                                && pc_prices_at_ts.iter().any(|(o, p)| {
                                    *o == order.side && (p - order.price).abs() < 1e-6
                                }));
                        if is_fill {
                            order.filled = order.size;
                            order.remaining = 0.0;
                            order.status = TrackedOrderStatus::Filled;
                        } else {
                            order.status = TrackedOrderStatus::Cancelled;
                        }
                    } else {
                        order.filled = order.size;
                        order.remaining = 0.0;
                        order.status = TrackedOrderStatus::Filled;
                    }
                    order.resolved_tick = Some(tick_idx);
                    order.resolved_ts = Some(ts);
                    resolved_indices.push(i);
                }
            }
        }

        // Move resolved orders to history
        resolved_indices.sort_unstable();
        for &i in resolved_indices.iter().rev() {
            let order = self.open_orders.remove(i);
            self.history.push(order);
        }
    }
}
