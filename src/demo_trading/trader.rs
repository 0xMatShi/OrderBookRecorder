use crate::models::Recording;
use crate::replay::player::SPEEDS;
use chrono::{TimeZone, Utc};
use std::collections::HashSet;
use std::time::Instant;

const INITIAL_BALANCE: f64 = 5000.0;
const ORDER_SIZE: f64 = 1.0; // Number of shares per order
const SIZE_THRESHOLD: f64 = 100.0; // Place orders when both best_bid sizes < threshold
const SECOND_LEVEL_THRESHOLD: f64 = 5000.0; // Sum of levels 2-6 must be >= this size

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Up,
    Down,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Up => "Up",
            Outcome::Down => "Down",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Order {
    pub outcome: Outcome,
    pub price: f64,
    pub size: f64,
    pub filled: f64,
    pub placed_at_tick: usize,
}

impl Order {
    pub fn total(&self) -> f64 {
        self.size * self.price
    }
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub outcome: Outcome,
    pub price: f64,
    pub size: f64,
    pub executed_at_tick: usize,
}

impl Trade {
    pub fn total(&self) -> f64 {
        self.size * self.price
    }
}

#[derive(Debug, Clone)]
pub struct Portfolio {
    pub balance: f64,
    pub up_shares: f64,
    pub up_spent: f64,
    pub down_shares: f64,
    pub down_spent: f64,
}

impl Portfolio {
    pub fn new() -> Self {
        Self {
            balance: INITIAL_BALANCE,
            up_shares: 0.0,
            up_spent: 0.0,
            down_shares: 0.0,
            down_spent: 0.0,
        }
    }

    pub fn up_avg(&self) -> f64 {
        if self.up_shares > 0.0 {
            self.up_spent / self.up_shares
        } else {
            0.0
        }
    }

    pub fn down_avg(&self) -> f64 {
        if self.down_shares > 0.0 {
            self.down_spent / self.down_shares
        } else {
            0.0
        }
    }

    pub fn total_avg(&self) -> f64 {
        self.up_avg() + self.down_avg()
    }

    pub fn total_spent(&self) -> f64 {
        self.up_spent + self.down_spent
    }

    pub fn execute_trade(&mut self, trade: &Trade) {
        let cost = trade.total();
        self.balance -= cost;

        match trade.outcome {
            Outcome::Up => {
                self.up_shares += trade.size;
                self.up_spent += cost;
            }
            Outcome::Down => {
                self.down_shares += trade.size;
                self.down_spent += cost;
            }
        }
    }

    #[allow(dead_code)]
    pub fn reverse_trade(&mut self, trade: &Trade) {
        let cost = trade.total();
        self.balance += cost;

        match trade.outcome {
            Outcome::Up => {
                self.up_shares -= trade.size;
                self.up_spent -= cost;
            }
            Outcome::Down => {
                self.down_shares -= trade.size;
                self.down_spent -= cost;
            }
        }
    }
}

/// Price key for tracking placed orders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PriceKey {
    outcome: Outcome,
    price_cents: u32, // Price in cents to avoid float precision issues
}

impl PriceKey {
    fn new(outcome: Outcome, price: f64) -> Self {
        Self {
            outcome,
            price_cents: (price * 100.0).round() as u32,
        }
    }
}

/// Snapshot of trading state at a specific tick
#[derive(Debug, Clone)]
struct TickSnapshot {
    portfolio: Portfolio,
    open_orders: Vec<Order>,
    trade_history: Vec<Trade>,
    placed_first_order_prices: HashSet<PriceKey>,
    pending_opposite_orders: Vec<(Outcome, f64)>,
}

/// Final result of the trading strategy
#[derive(Debug, Clone)]
pub struct EventResult {
    pub up_shares: f64,
    pub down_shares: f64,
    pub total_spent: f64,
    pub winner: Option<Outcome>,
    pub pnl: f64, // winner_shares - total_spent
}

/// Process a single tick's trading logic
/// This is the core strategy logic used by both calculate_event_result and DemoTradingState
fn process_tick_logic(
    tick: &crate::models::Tick,
    tick_idx: usize,
    portfolio: &mut Portfolio,
    open_orders: &mut Vec<Order>,
    trade_history: &mut Vec<Trade>,
    placed_first_order_prices: &mut HashSet<PriceKey>,
    pending_opposite_orders: &mut Vec<(Outcome, f64)>,
) {
    // Step 1: Check order fills
    let up_best_bid = tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
    let down_best_bid = tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

    let mut filled_indices = Vec::new();
    for (i, order) in open_orders.iter().enumerate() {
        let best_bid = match order.outcome {
            Outcome::Up => up_best_bid,
            Outcome::Down => down_best_bid,
        };

        if best_bid < order.price && best_bid > 0.0 {
            let trade = Trade {
                outcome: order.outcome,
                price: order.price,
                size: order.size,
                executed_at_tick: tick_idx,
            };
            portfolio.execute_trade(&trade);
            trade_history.push(trade);
            filled_indices.push(i);

            let price_key = PriceKey::new(order.outcome, order.price);
            if placed_first_order_prices.remove(&price_key) {
                pending_opposite_orders.push((order.outcome, order.price));
            }
        }
    }

    for &i in filled_indices.iter().rev() {
        open_orders.remove(i);
    }

    // Step 2: Check order placement
    let up_best_bid_size = tick.up_bids.first().map(|b| b[1]).unwrap_or(0.0);
    let down_best_bid_size = tick.down_bids.first().map(|b| b[1]).unwrap_or(0.0);
    let up_best_bid_price = tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
    let down_best_bid_price = tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

    // Sum of levels 2-6 (indices 1-5)
    let up_levels_2_to_6_size: f64 = tick.up_bids.iter().skip(1).take(5).map(|b| b[1]).sum();
    let down_levels_2_to_6_size: f64 = tick.down_bids.iter().skip(1).take(5).map(|b| b[1]).sum();

    // Place pending opposite orders
    if !pending_opposite_orders.is_empty() {
        let pending = pending_opposite_orders.clone();
        pending_opposite_orders.clear();

        for (first_side, first_side_price) in pending {
            let opposite_side = match first_side {
                Outcome::Up => Outcome::Down,
                Outcome::Down => Outcome::Up,
            };

            let price = 0.99 - first_side_price;
            if price > 0.0 && price < 1.0 {
                open_orders.push(Order {
                    outcome: opposite_side,
                    price,
                    size: ORDER_SIZE,
                    filled: 0.0,
                    placed_at_tick: tick_idx,
                });
            }
        }
    }

    // Place first orders
    let strong_side = if up_best_bid_price >= 0.5 {
        Some(Outcome::Up)
    } else if down_best_bid_price >= 0.5 {
        Some(Outcome::Down)
    } else {
        None
    };

    if let Some(side) = strong_side {
        let (price, best_size, levels_2_to_6_size) = match side {
            Outcome::Up => (up_best_bid_price, up_best_bid_size, up_levels_2_to_6_size),
            Outcome::Down => (
                down_best_bid_price,
                down_best_bid_size,
                down_levels_2_to_6_size,
            ),
        };

        let price_key = PriceKey::new(side, price);

        if !placed_first_order_prices.contains(&price_key)
            && best_size < SIZE_THRESHOLD
            && levels_2_to_6_size >= SECOND_LEVEL_THRESHOLD
            && price > 0.0
        {
            let has_open_order_at_price = open_orders
                .iter()
                .any(|o| o.outcome == side && (o.price - price).abs() < 0.01);

            if !has_open_order_at_price {
                open_orders.push(Order {
                    outcome: side,
                    price,
                    size: ORDER_SIZE,
                    filled: 0.0,
                    placed_at_tick: tick_idx,
                });
                placed_first_order_prices.insert(price_key);
            }
        }
    }
}

pub struct DemoTradingState {
    pub recording: Recording,
    pub current_tick: usize,
    pub is_paused: bool,
    pub is_trading_paused: bool,
    pub speed_index: usize,
    pub last_frame_time: Instant,
    pub accumulated_time_ms: f64,

    pub portfolio: Portfolio,
    pub open_orders: Vec<Order>,
    pub trade_history: Vec<Trade>,
    pub history_start_time: Instant,

    // Track prices where we've placed first orders (strong side)
    placed_first_order_prices: HashSet<PriceKey>,
    // Track which first orders have been filled and need opposite side order
    pending_opposite_orders: Vec<(Outcome, f64)>, // (first_side, first_side_price)

    // Precalculated snapshots for instant seeking
    snapshots: Vec<TickSnapshot>,

    // Final result after precalculation
    pub final_result: Option<EventResult>,
}

/// Quick calculation to get event result without creating full state
pub fn calculate_event_result(recording: &Recording) -> EventResult {
    if recording.ticks.is_empty() {
        return EventResult {
            up_shares: 0.0,
            down_shares: 0.0,
            total_spent: 0.0,
            winner: None,
            pnl: 0.0,
        };
    }

    let mut portfolio = Portfolio::new();
    let mut open_orders: Vec<Order> = Vec::new();
    let mut trade_history: Vec<Trade> = Vec::new();
    let mut placed_first_order_prices: HashSet<PriceKey> = HashSet::new();
    let mut pending_opposite_orders: Vec<(Outcome, f64)> = Vec::new();

    // Process all ticks using shared logic
    for (tick_idx, tick) in recording.ticks.iter().enumerate() {
        process_tick_logic(
            tick,
            tick_idx,
            &mut portfolio,
            &mut open_orders,
            &mut trade_history,
            &mut placed_first_order_prices,
            &mut pending_opposite_orders,
        );
    }

    // Determine winner from last tick
    let winner = {
        let last_tick = &recording.ticks[recording.ticks.len() - 1];
        let up_best_bid = last_tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
        let down_best_bid = last_tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

        if up_best_bid > 0.5 {
            Some(Outcome::Up)
        } else if down_best_bid > 0.5 {
            Some(Outcome::Down)
        } else {
            None
        }
    };

    // Calculate PnL: winner_shares - total_spent
    let total_spent = portfolio.total_spent();
    let pnl = match winner {
        Some(Outcome::Up) => portfolio.up_shares - total_spent,
        Some(Outcome::Down) => portfolio.down_shares - total_spent,
        None => {
            // When winner is unclear (both bids < 0.5)
            // If equal shares, one side will definitely win and we get that amount back
            // If unequal, we don't know which side wins, so assume loss
            if portfolio.up_shares == portfolio.down_shares {
                portfolio.up_shares - total_spent
            } else {
                -total_spent
            }
        }
    };

    EventResult {
        up_shares: portfolio.up_shares,
        down_shares: portfolio.down_shares,
        total_spent,
        winner,
        pnl,
    }
}

impl DemoTradingState {
    pub fn new(recording: Recording) -> Self {
        let mut state = Self {
            recording,
            current_tick: 0,
            is_paused: true,
            is_trading_paused: false,
            speed_index: 3, // 1x speed
            last_frame_time: Instant::now(),
            accumulated_time_ms: 0.0,
            portfolio: Portfolio::new(),
            open_orders: Vec::new(),
            trade_history: Vec::new(),
            history_start_time: Instant::now(),
            placed_first_order_prices: HashSet::new(),
            pending_opposite_orders: Vec::new(),
            snapshots: Vec::new(),
            final_result: None,
        };

        // Precalculate all ticks
        state.precalculate_all_ticks();

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
        self.is_trading_paused = !self.is_trading_paused;
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    pub fn toggle_trading_pause(&mut self) {
        self.is_trading_paused = !self.is_trading_paused;
    }

    pub fn reset(&mut self) {
        self.current_tick = 0;
        self.is_paused = true;
        self.is_trading_paused = false;
        self.portfolio = Portfolio::new();
        self.open_orders.clear();
        self.trade_history.clear();
        self.history_start_time = Instant::now();
        self.accumulated_time_ms = 0.0;
        self.placed_first_order_prices.clear();
        self.pending_opposite_orders.clear();
    }

    pub fn move_ticks(&mut self, delta: i32) {
        let new_tick = self.current_tick as i32 + delta;
        let target_tick =
            new_tick.clamp(0, self.recording.ticks.len().saturating_sub(1) as i32) as usize;

        // Use snapshots for instant seeking
        self.current_tick = target_tick;
        self.restore_snapshot(target_tick);
    }

    /// Process current tick for trading logic (used when paused but need to check conditions)
    /// With precalculation, this just ensures we're on the right snapshot
    pub fn process_current_tick(&mut self) {
        // State is already correct from snapshots, no need to reprocess
        self.restore_snapshot(self.current_tick);
    }

    /// Precalculate trading strategy for all ticks
    /// This runs the entire strategy once and saves snapshots for instant seeking
    fn precalculate_all_ticks(&mut self) {
        if self.recording.ticks.is_empty() {
            return;
        }

        let total_ticks = self.recording.ticks.len();
        println!("⏳ Precalculating strategy for {} ticks...", total_ticks);

        // Reset state
        self.current_tick = 0;
        self.portfolio = Portfolio::new();
        self.open_orders.clear();
        self.trade_history.clear();
        self.placed_first_order_prices.clear();
        self.pending_opposite_orders.clear();
        self.snapshots.clear();

        // Reserve capacity for snapshots
        self.snapshots.reserve(total_ticks);

        // Process first tick and save snapshot
        self.process_tick();
        self.save_snapshot();

        // Process remaining ticks with progress indicator
        let progress_step = (total_ticks / 10).max(1);
        for tick_idx in 1..total_ticks {
            self.current_tick = tick_idx;
            self.process_tick();
            self.save_snapshot();

            // Show progress every 10%
            if tick_idx % progress_step == 0 {
                let percent = (tick_idx as f64 / total_ticks as f64 * 100.0) as u32;
                println!("  {}% complete ({}/{})", percent, tick_idx, total_ticks);
            }
        }

        println!(
            "✅ Precalculation complete! {} snapshots saved.",
            self.snapshots.len()
        );

        // Determine winner from last tick
        let winner = if !self.recording.ticks.is_empty() {
            let last_tick = &self.recording.ticks[total_ticks - 1];
            let up_best_bid = last_tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
            let down_best_bid = last_tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

            if up_best_bid > 0.5 {
                Some(Outcome::Up)
            } else if down_best_bid > 0.5 {
                Some(Outcome::Down)
            } else {
                None
            }
        } else {
            None
        };

        // Save final result with PnL
        let total_spent = self.portfolio.total_spent();
        let pnl = match winner {
            Some(Outcome::Up) => self.portfolio.up_shares - total_spent,
            Some(Outcome::Down) => self.portfolio.down_shares - total_spent,
            None => {
                // When winner is unclear (both bids < 0.5)
                // If equal shares, one side will definitely win and we get that amount back
                // If unequal, we don't know which side wins, so assume loss
                if self.portfolio.up_shares == self.portfolio.down_shares {
                    self.portfolio.up_shares - total_spent
                } else {
                    -total_spent
                }
            }
        };

        self.final_result = Some(EventResult {
            up_shares: self.portfolio.up_shares,
            down_shares: self.portfolio.down_shares,
            total_spent,
            winner,
            pnl,
        });

        // Reset to beginning
        self.current_tick = 0;
        self.restore_snapshot(0);
    }

    /// Save current state as snapshot
    fn save_snapshot(&mut self) {
        self.snapshots.push(TickSnapshot {
            portfolio: self.portfolio.clone(),
            open_orders: self.open_orders.clone(),
            trade_history: self.trade_history.clone(),
            placed_first_order_prices: self.placed_first_order_prices.clone(),
            pending_opposite_orders: self.pending_opposite_orders.clone(),
        });
    }

    /// Restore state from snapshot
    fn restore_snapshot(&mut self, tick: usize) {
        if tick >= self.snapshots.len() {
            return;
        }

        let snapshot = &self.snapshots[tick];
        self.portfolio = snapshot.portfolio.clone();
        self.open_orders = snapshot.open_orders.clone();
        self.trade_history = snapshot.trade_history.clone();
        self.placed_first_order_prices = snapshot.placed_first_order_prices.clone();
        self.pending_opposite_orders = snapshot.pending_opposite_orders.clone();
    }

    /// Fast-forward to a specific quarter (1-4) of the recording
    /// Uses precalculated snapshots for instant seeking
    pub fn jump_to_quarter(&mut self, quarter: u8) {
        if !(1..=4).contains(&quarter) || self.recording.ticks.is_empty() {
            return;
        }

        let total_ticks = self.recording.ticks.len();
        let target_tick = ((total_ticks as f64 * quarter as f64) / 4.0).floor() as usize;
        let target_tick = target_tick.min(total_ticks - 1);

        // Restore snapshot instantly
        self.current_tick = target_tick;
        self.restore_snapshot(target_tick);

        // Update pause state - pause after jump to show results
        self.is_paused = true;
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    #[allow(dead_code)]
    fn rewind_to_tick(&mut self, target_tick: usize) {
        // Remove all trades executed after target_tick
        while let Some(trade) = self.trade_history.last() {
            if trade.executed_at_tick > target_tick {
                self.portfolio.reverse_trade(trade);
                self.trade_history.pop();
            } else {
                break;
            }
        }

        // Remove all orders placed after target_tick
        self.open_orders
            .retain(|order| order.placed_at_tick <= target_tick);

        // Rebuild tracking state based on remaining orders and trades
        self.placed_first_order_prices.clear();
        self.pending_opposite_orders.clear();

        // Rebuild placed_first_order_prices from remaining open orders
        // We can only track orders that are still open (unfilled)
        for order in &self.open_orders {
            let price_key = PriceKey::new(order.outcome, order.price);
            self.placed_first_order_prices.insert(price_key);
        }
    }

    pub fn update(&mut self) {
        if self.is_paused || self.recording.ticks.is_empty() {
            self.last_frame_time = Instant::now();
            return;
        }

        let elapsed = self.last_frame_time.elapsed();
        self.last_frame_time = Instant::now();

        // Accumulate time adjusted by speed
        self.accumulated_time_ms += elapsed.as_secs_f64() * 1000.0 * self.current_speed();

        // Advance ticks based on timestamps using snapshots
        while self.current_tick < self.recording.ticks.len() - 1 {
            let current_ts = self.recording.ticks[self.current_tick].ts;
            let next_ts = self.recording.ticks[self.current_tick + 1].ts;
            let delta_ms = (next_ts - current_ts) as f64;

            if self.accumulated_time_ms >= delta_ms {
                self.accumulated_time_ms -= delta_ms;
                self.current_tick += 1;

                // Restore state from snapshot instead of processing
                self.restore_snapshot(self.current_tick);
            } else {
                break;
            }
        }
    }

    fn process_tick(&mut self) {
        if self.recording.ticks.is_empty() {
            return;
        }

        // Clone tick data to avoid borrow checker issues
        let tick = self.recording.ticks[self.current_tick].clone();

        // Use shared logic
        process_tick_logic(
            &tick,
            self.current_tick,
            &mut self.portfolio,
            &mut self.open_orders,
            &mut self.trade_history,
            &mut self.placed_first_order_prices,
            &mut self.pending_opposite_orders,
        );
    }

    pub fn current_time_str(&self) -> String {
        if self.recording.ticks.is_empty() {
            return "00:00:00.000".to_string();
        }

        let current_ts = self.recording.ticks[self.current_tick].ts;
        let datetime = Utc.timestamp_millis_opt(current_ts).unwrap();
        datetime.format("%H:%M:%S%.3f").to_string()
    }

    pub fn total_time_str(&self) -> String {
        self.recording
            .metadata
            .end_time
            .format("%H:%M:%S%.3f")
            .to_string()
    }
}
