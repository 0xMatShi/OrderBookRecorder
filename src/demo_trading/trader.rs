use crate::models::Recording;
use crate::replay::player::SPEEDS;
use chrono::{TimeZone, Utc};
use std::time::Instant;

const INITIAL_BALANCE: f64 = 5000.0;
const ORDER_SIZE: f64 = 1.0; // Number of shares per order
const SIZE_THRESHOLD: f64 = 100.0; // Place orders when both best_bid sizes < threshold
const SECOND_LEVEL_THRESHOLD: f64 = 999.0; // Second level bids must be > this size

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl DemoTradingState {
    pub fn new(recording: Recording) -> Self {
        Self {
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
        }
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
    }

    pub fn move_ticks(&mut self, delta: i32) {
        let old_tick = self.current_tick;
        let new_tick = self.current_tick as i32 + delta;
        self.current_tick =
            new_tick.clamp(0, self.recording.ticks.len().saturating_sub(1) as i32) as usize;

        // If moving backward, reverse trades and orders
        if self.current_tick < old_tick {
            self.rewind_to_tick(self.current_tick);
        } else if self.current_tick > old_tick && !self.is_trading_paused {
            // If moving forward, process the new tick
            self.process_tick();
        }
    }

    /// Process current tick for trading logic (used when paused but need to check conditions)
    pub fn process_current_tick(&mut self) {
        if !self.is_trading_paused {
            self.process_tick();
        }
    }

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

        // Advance ticks based on timestamps
        while self.current_tick < self.recording.ticks.len() - 1 {
            let current_ts = self.recording.ticks[self.current_tick].ts;
            let next_ts = self.recording.ticks[self.current_tick + 1].ts;
            let delta_ms = (next_ts - current_ts) as f64;

            if self.accumulated_time_ms >= delta_ms {
                self.accumulated_time_ms -= delta_ms;
                self.current_tick += 1;

                // Process trading logic for the new tick
                if !self.is_trading_paused {
                    self.process_tick();
                }
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

        // Check for order fills
        self.check_order_fills(&tick);

        // Check for new order placement conditions
        self.check_order_placement(&tick);
    }

    fn check_order_fills(&mut self, tick: &crate::models::Tick) {
        let up_best_bid = tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
        let down_best_bid = tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

        let mut filled_indices = Vec::new();

        for (i, order) in self.open_orders.iter().enumerate() {
            let best_bid = match order.outcome {
                Outcome::Up => up_best_bid,
                Outcome::Down => down_best_bid,
            };

            // Order is filled if best_bid < order price
            if best_bid < order.price && best_bid > 0.0 {
                let trade = Trade {
                    outcome: order.outcome,
                    price: order.price,
                    size: order.size,
                    executed_at_tick: self.current_tick,
                };

                self.portfolio.execute_trade(&trade);
                self.trade_history.push(trade);
                filled_indices.push(i);
            }
        }

        // Remove filled orders (in reverse to maintain indices)
        for &i in filled_indices.iter().rev() {
            self.open_orders.remove(i);
        }
    }

    fn check_order_placement(&mut self, tick: &crate::models::Tick) {
        // First level (best bid)
        let up_best_bid_size = tick.up_bids.first().map(|b| b[1]).unwrap_or(0.0);
        let down_best_bid_size = tick.down_bids.first().map(|b| b[1]).unwrap_or(0.0);
        let up_best_bid_price = tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0);
        let down_best_bid_price = tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0);

        // Second level bids
        let up_second_bid_size = tick.up_bids.get(1).map(|b| b[1]).unwrap_or(0.0);
        let down_second_bid_size = tick.down_bids.get(1).map(|b| b[1]).unwrap_or(0.0);

        // Place one order on each side if:
        // 1. Both best bids have size < SIZE_THRESHOLD
        // 2. Both second level bids have size > SECOND_LEVEL_THRESHOLD
        // 3. Prices are valid
        if up_best_bid_size < SIZE_THRESHOLD
            && down_best_bid_size < SIZE_THRESHOLD
            && up_second_bid_size > SECOND_LEVEL_THRESHOLD
            && down_second_bid_size > SECOND_LEVEL_THRESHOLD
            && up_best_bid_price > 0.0
            && down_best_bid_price > 0.0
        {
            self.open_orders.push(Order {
                outcome: Outcome::Up,
                price: up_best_bid_price,
                size: ORDER_SIZE,
                filled: 0.0,
                placed_at_tick: self.current_tick,
            });

            self.open_orders.push(Order {
                outcome: Outcome::Down,
                price: down_best_bid_price,
                size: ORDER_SIZE,
                filled: 0.0,
                placed_at_tick: self.current_tick,
            });
        }
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
