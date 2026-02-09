//! Trading strategy implementation for Polymarket order book demo trading.
//!
//! # Strategy Overview - Spread Closing Strategy
//!
//! The strategy places limit orders on the "expensive" side (higher best_bid) when the
//! spread opens due to price movement on the "decreasing" side.
//!
//! ## Terminology
//! - `bbu` - best bid up (best bid price on Up side)
//! - `bbd` - best bid down (best bid price on Down side)
//! - Strong/Expensive side - the side with higher best_bid (>= 0.5)
//! - Weak/Cheap side - the side with lower best_bid (< 0.5)
//!
//! ## First Leg (Expensive Side) - Entry Condition
//!
//! Place limit order on the EXPENSIVE side when the WEAK side's best_bid decreases
//! without the expensive side's best_bid changing. This creates a spread we want to close.
//!
//! Additional requirement: The second level (one cent below our target price) must have
//! at least SECOND_LEVEL_MIN_SIZE shares to provide liquidity support.
//!
//! Example:
//! - Tick 1: bbu=0.35, bbd=0.64
//! - Tick 2: bbu=0.34, bbd=0.64 → bbu decreased, bbd unchanged
//! - Target price = 0.99 - 0.34 = 0.65
//! - Check: size at 0.64 level >= SECOND_LEVEL_MIN_SIZE? If yes, place order
//!
//! ## First Leg Fill Logic (Queue Position)
//!
//! We assume we're FIRST in queue at our target price. When the market reaches our
//! target price, we wait for it to appear in the order book, then track the size.
//! Once the size decreases by ORDER_SIZE, we consider ourselves filled.
//!
//! Example:
//! - We want to place at 0.65
//! - Next tick shows bbd=0.65 with size=10 → we "joined" at the front
//! - Later tick shows size=3 (decreased by 7 >= ORDER_SIZE) → we're filled
//! - But if size goes to 15 → not filled yet
//! - If size goes from 15 to 14 (decreased by 1 >= ORDER_SIZE) → we're filled
//!
//! ## Second Leg (Weak Side) - After First Leg Fills
//!
//! When first leg fills, place order on weak side at current best_bid price.
//! Track queue position: we're placed AFTER the current size at that price.
//!
//! IMPORTANT: Second leg can ONLY be filled in two cases:
//! 1. Our order IS at best_bid AND enough shares cleared from queue
//! 2. Best_bid dropped BELOW our price (entire level was bought out)
//!
//! If our order is at 0.46 but best_bid is 0.47, we're NOT first in queue
//! and cannot get filled even if size at 0.46 decreases!
//!
//! Example:
//! - First leg fills when bbd=0.65
//! - At that moment, bbu (weak side) has size=100 at best_bid=0.34
//! - We place order at 0.34 → we're position 101 in queue (after those 100 shares)
//! - If best_bid moves to 0.35 → we're NOT at best_bid, cannot get filled
//! - If best_bid drops to 0.33 → entire 0.34 level was bought, we're filled!
//! - If best_bid stays at 0.34 and size drops by 101+ from peak → we're filled

use crate::models::Recording;
use crate::replay::player::SPEEDS;
use chrono::{TimeZone, Utc};
use std::collections::HashSet;
use std::time::Instant;

const INITIAL_BALANCE: f64 = 5000.0;
const ORDER_SIZE: f64 = 1.0; // Number of shares per order
const SECOND_LEVEL_MIN_SIZE: f64 = 1000.0; // Minimum size required on second level to place first leg

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

/// Order state for tracking queue position and fill status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderState {
    /// Waiting for best_bid to reach our target price (we become the new best_bid)
    WaitingForPrice,
    /// We ARE the best_bid, tracking peak size to detect fill
    /// Fill happens when best_bid drops BELOW our price
    AtBestBid { peak_size: f64 },
    /// Second leg: tracking queue position (we're after shares_ahead shares)
    TrackingQueuePosition { shares_ahead: f64, peak_size: f64 },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Order {
    pub outcome: Outcome,
    pub price: f64,
    pub size: f64,
    pub filled: f64,
    pub placed_at_tick: usize,
    pub is_second_leg: bool,
    pub second_leg_id: Option<u64>,
    /// State machine for fill tracking
    pub state: OrderState,
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

/// Price key for tracking placed first leg orders (strong side)
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

/// Tracks previous tick's best bid prices for detecting spread openings
#[derive(Debug, Clone, Copy, Default)]
struct PreviousBids {
    bbu: f64, // previous best bid up
    bbd: f64, // previous best bid down
}

/// Snapshot of trading state at a specific tick
#[derive(Debug, Clone)]
struct TickSnapshot {
    portfolio: Portfolio,
    open_orders: Vec<Order>,
    trade_history: Vec<Trade>,
    placed_first_leg_prices: HashSet<PriceKey>, // Track placed first leg orders (expensive side)
    previous_bids: PreviousBids,                // Previous tick's best bids for spread detection
    next_order_id: u64,                         // Counter for generating unique order IDs
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
    placed_first_leg_prices: &mut HashSet<PriceKey>,
    previous_bids: &mut PreviousBids,
    next_order_id: &mut u64,
) {
    // Extract market data
    let bbu = tick.up_bids.first().map(|b| b[0]).unwrap_or(0.0); // best bid up
    let bbd = tick.down_bids.first().map(|b| b[0]).unwrap_or(0.0); // best bid down
    let bbu_size = tick.up_bids.first().map(|b| b[1]).unwrap_or(0.0);
    let bbd_size = tick.down_bids.first().map(|b| b[1]).unwrap_or(0.0);

    // Step 1: Process first leg orders - check fills and cancellations
    let mut filled_indices = Vec::new();
    let mut cancelled_indices = Vec::new();
    let mut new_second_legs: Vec<(Outcome, f64, f64)> = Vec::new(); // (weak_side, price, shares_ahead)

    for (i, order) in open_orders.iter_mut().enumerate() {
        let (best_bid_price, best_bid_size) = match order.outcome {
            Outcome::Up => (bbu, bbu_size),
            Outcome::Down => (bbd, bbd_size),
        };

        // Skip second leg orders in this section - they're handled separately
        if order.is_second_leg {
            continue;
        }

        match order.state {
            OrderState::WaitingForPrice => {
                // First leg: waiting for best_bid to reach our target price
                // We become the new best_bid when price reaches our target

                // Cancel if best_bid moved HIGHER than our price (someone else became new bb)
                if best_bid_price > order.price + 0.001 {
                    // Our order is now below best_bid - cancel it
                    let price_key = PriceKey::new(order.outcome, order.price);
                    placed_first_leg_prices.remove(&price_key);
                    cancelled_indices.push(i);
                    continue;
                }

                // Check if we became the best_bid
                if (best_bid_price - order.price).abs() < 0.001 {
                    // Price reached! We ARE the best_bid now
                    order.state = OrderState::AtBestBid {
                        peak_size: best_bid_size,
                    };
                }
            }
            OrderState::AtBestBid { peak_size } => {
                // We are at best_bid (first in queue). Check for fill or cancellation.

                // CANCEL: if best_bid moved HIGHER than our price
                // (someone else placed order above us, we're no longer bb)
                if best_bid_price > order.price + 0.001 {
                    let price_key = PriceKey::new(order.outcome, order.price);
                    placed_first_leg_prices.remove(&price_key);
                    cancelled_indices.push(i);
                    continue;
                }

                // Helper closure to execute fill
                let execute_fill =
                    |portfolio: &mut Portfolio,
                     trade_history: &mut Vec<Trade>,
                     placed_first_leg_prices: &mut HashSet<PriceKey>,
                     new_second_legs: &mut Vec<(Outcome, f64, f64)>| {
                        let trade = Trade {
                            outcome: order.outcome,
                            price: order.price,
                            size: order.size,
                            executed_at_tick: tick_idx,
                        };
                        portfolio.execute_trade(&trade);
                        trade_history.push(trade);

                        // Remove from price tracking
                        let price_key = PriceKey::new(order.outcome, order.price);
                        placed_first_leg_prices.remove(&price_key);

                        // Prepare second leg on weak side
                        let weak_side = match order.outcome {
                            Outcome::Up => Outcome::Down,
                            Outcome::Down => Outcome::Up,
                        };

                        let weak_side_size = match weak_side {
                            Outcome::Up => bbu_size,
                            Outcome::Down => bbd_size,
                        };
                        let weak_side_price = match weak_side {
                            Outcome::Up => bbu,
                            Outcome::Down => bbd,
                        };

                        if weak_side_price > 0.0 {
                            new_second_legs.push((weak_side, weak_side_price, weak_side_size));
                        }
                    };

                // FILL condition 1: best_bid dropped BELOW our price
                // (entire level was consumed, price moved through us)
                if best_bid_price < order.price - 0.001 {
                    execute_fill(
                        portfolio,
                        trade_history,
                        placed_first_leg_prices,
                        &mut new_second_legs,
                    );
                    filled_indices.push(i);
                    continue;
                }

                // FILL condition 2: We're still at best_bid, but size decreased from peak by ORDER_SIZE
                // We were first in queue, so if size dropped, someone bought from us
                if (best_bid_price - order.price).abs() < 0.001 {
                    // Update peak (size can grow as others join behind us)
                    let new_peak = peak_size.max(best_bid_size);

                    // Check if size decreased from peak by at least ORDER_SIZE
                    if new_peak - best_bid_size >= ORDER_SIZE {
                        // Filled! Size dropped, we were first in queue
                        execute_fill(
                            portfolio,
                            trade_history,
                            placed_first_leg_prices,
                            &mut new_second_legs,
                        );
                        filled_indices.push(i);
                        continue;
                    }

                    // Not filled yet, update peak
                    order.state = OrderState::AtBestBid {
                        peak_size: new_peak,
                    };
                }
            }
            OrderState::TrackingQueuePosition { .. } => {
                // This shouldn't happen for first leg, but handle gracefully
            }
        }
    }

    // Step 2: Process second leg orders
    for (i, order) in open_orders.iter_mut().enumerate() {
        if !order.is_second_leg {
            continue;
        }

        // Skip if already marked for fill/cancel
        if filled_indices.contains(&i) || cancelled_indices.contains(&i) {
            continue;
        }

        let (best_bid_price, best_bid_size, _order_book) = match order.outcome {
            Outcome::Up => (bbu, bbu_size, &tick.up_bids),
            Outcome::Down => (bbd, bbd_size, &tick.down_bids),
        };

        match order.state {
            OrderState::TrackingQueuePosition {
                shares_ahead,
                peak_size,
            } => {
                // Second leg fill logic:
                // 1. Fill if best_bid dropped BELOW our price (our entire level was bought out)
                // 2. Fill if we ARE at best_bid AND enough shares cleared from queue
                //
                // IMPORTANT: We can ONLY get filled when our order is at best_bid!
                // If our order is at 0.46 but best_bid is 0.47, we're not first in queue
                // and won't get filled even if size at 0.46 decreases.

                // FILL condition 1: best_bid dropped BELOW our price
                // This means the entire level where our order was got bought out
                if best_bid_price < order.price - 0.001 {
                    let trade = Trade {
                        outcome: order.outcome,
                        price: order.price,
                        size: order.size,
                        executed_at_tick: tick_idx,
                    };
                    portfolio.execute_trade(&trade);
                    trade_history.push(trade);
                    filled_indices.push(i);
                    continue;
                }

                // FILL condition 2: We ARE at best_bid and enough shares cleared
                // Only check this if our order price matches best_bid
                let is_at_best_bid = (best_bid_price - order.price).abs() < 0.001;

                if is_at_best_bid {
                    // We're at best_bid, track queue position
                    let current_size = best_bid_size;

                    // Update peak if size grew (others joined behind us)
                    let new_peak = peak_size.max(current_size);

                    // Check if enough shares cleared (shares_ahead + our ORDER_SIZE)
                    let shares_needed = shares_ahead + ORDER_SIZE;
                    if new_peak - current_size >= shares_needed {
                        // Filled! Enough shares in front of us got executed
                        let trade = Trade {
                            outcome: order.outcome,
                            price: order.price,
                            size: order.size,
                            executed_at_tick: tick_idx,
                        };
                        portfolio.execute_trade(&trade);
                        trade_history.push(trade);
                        filled_indices.push(i);
                    } else {
                        // Not filled yet, update peak size
                        order.state = OrderState::TrackingQueuePosition {
                            shares_ahead,
                            peak_size: new_peak,
                        };
                    }
                } else {
                    // Our order is NOT at best_bid (e.g., our price is 0.46, best_bid is 0.47)
                    // We cannot get filled in this situation - just keep waiting
                    // Peak size tracking continues when we become best_bid again
                }
            }
            _ => {
                // Second leg should always be in TrackingQueuePosition state
                // but if not, check for fill condition (best_bid dropped below our price)
                if best_bid_price < order.price - 0.001 {
                    let trade = Trade {
                        outcome: order.outcome,
                        price: order.price,
                        size: order.size,
                        executed_at_tick: tick_idx,
                    };
                    portfolio.execute_trade(&trade);
                    trade_history.push(trade);
                    filled_indices.push(i);
                }
            }
        }
    }

    // Remove filled and cancelled orders (in reverse to maintain indices)
    let mut to_remove: Vec<usize> = filled_indices
        .iter()
        .chain(cancelled_indices.iter())
        .copied()
        .collect();
    to_remove.sort_unstable();
    to_remove.dedup();
    for &i in to_remove.iter().rev() {
        open_orders.remove(i);
    }

    // Step 3: Place second leg orders for filled first legs
    for (weak_side, weak_price, shares_ahead) in new_second_legs {
        let order_id = *next_order_id;
        *next_order_id += 1;

        open_orders.push(Order {
            outcome: weak_side,
            price: weak_price,
            size: ORDER_SIZE,
            filled: 0.0,
            placed_at_tick: tick_idx,
            is_second_leg: true,
            second_leg_id: Some(order_id),
            state: OrderState::TrackingQueuePosition {
                shares_ahead,
                peak_size: shares_ahead, // Initial peak is the size when we joined
            },
        });
    }

    // Step 4: Check for entry conditions (spread opening)
    let prev_bbu = previous_bids.bbu;
    let prev_bbd = previous_bids.bbd;

    // Update previous bids for next tick
    previous_bids.bbu = bbu;
    previous_bids.bbd = bbd;

    // Skip first tick (no previous data)
    if prev_bbu == 0.0 && prev_bbd == 0.0 {
        return;
    }

    // Determine expensive and weak sides
    // Expensive side has higher best_bid (typically >= 0.5)
    let (expensive_side, _expensive_bb, _weak_side, weak_bb, weak_prev_bb) = if bbd >= bbu {
        // bbd is expensive side (down)
        (Outcome::Down, bbd, Outcome::Up, bbu, prev_bbu)
    } else {
        // bbu is expensive side (up)
        (Outcome::Up, bbu, Outcome::Down, bbd, prev_bbd)
    };

    // Entry condition: weak side decreased
    // This creates a spread we want to close by placing on expensive side
    let weak_decreased = weak_bb < weak_prev_bb && weak_prev_bb > 0.0;

    if weak_decreased {
        // Calculate target price for first leg on expensive side
        // target = 0.99 - weak_bb (to close the spread)
        let target_price = 0.99 - weak_bb;

        // Validate target price
        if target_price <= 0.0 || target_price >= 1.0 {
            return;
        }

        // Get the order book for the expensive side
        let expensive_bids = match expensive_side {
            Outcome::Up => &tick.up_bids,
            Outcome::Down => &tick.down_bids,
        };

        // Check second level size requirement
        // We want to place at target_price, so we need to check that
        // the level just below target_price (second level when we become best_bid)
        // has enough size to provide liquidity support
        //
        // Example: target_price = 0.65, we check size at 0.64 level
        let second_level_price = target_price - 0.01;
        let second_level_size = expensive_bids
            .iter()
            .find(|bid| (bid[0] - second_level_price).abs() < 0.001)
            .map(|bid| bid[1])
            .unwrap_or(0.0);

        if second_level_size < SECOND_LEVEL_MIN_SIZE {
            return;
        }

        // Check if we already have an order at this price (only one per price)
        let price_key = PriceKey::new(expensive_side, target_price);
        if placed_first_leg_prices.contains(&price_key) {
            return;
        }

        // Check no open order at similar price
        let has_open_order = open_orders.iter().any(|o| {
            o.outcome == expensive_side
                && (o.price - target_price).abs() < 0.001
                && !o.is_second_leg
        });

        if has_open_order {
            return;
        }

        // Place first leg order
        let order_id = *next_order_id;
        *next_order_id += 1;

        open_orders.push(Order {
            outcome: expensive_side,
            price: target_price,
            size: ORDER_SIZE,
            filled: 0.0,
            placed_at_tick: tick_idx,
            is_second_leg: false,
            second_leg_id: Some(order_id),
            state: OrderState::WaitingForPrice,
        });

        placed_first_leg_prices.insert(price_key);
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

    // Track prices where we've placed first leg orders (expensive side)
    placed_first_leg_prices: HashSet<PriceKey>,
    // Previous tick's best bids for spread detection
    previous_bids: PreviousBids,
    // Counter for generating unique order IDs
    next_order_id: u64,

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
    let mut placed_first_leg_prices: HashSet<PriceKey> = HashSet::new();
    let mut previous_bids = PreviousBids::default();
    let mut next_order_id: u64 = 1;

    // Process all ticks using shared logic
    for (tick_idx, tick) in recording.ticks.iter().enumerate() {
        process_tick_logic(
            tick,
            tick_idx,
            &mut portfolio,
            &mut open_orders,
            &mut trade_history,
            &mut placed_first_leg_prices,
            &mut previous_bids,
            &mut next_order_id,
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
            placed_first_leg_prices: HashSet::new(),
            previous_bids: PreviousBids::default(),
            next_order_id: 1,
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
        self.placed_first_leg_prices.clear();
        self.previous_bids = PreviousBids::default();
        self.next_order_id = 1;
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
        self.placed_first_leg_prices.clear();
        self.previous_bids = PreviousBids::default();
        self.next_order_id = 1;
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
            placed_first_leg_prices: self.placed_first_leg_prices.clone(),
            previous_bids: self.previous_bids,
            next_order_id: self.next_order_id,
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
        self.placed_first_leg_prices = snapshot.placed_first_leg_prices.clone();
        self.previous_bids = snapshot.previous_bids;
        self.next_order_id = snapshot.next_order_id;
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

        // Rebuild tracking state based on remaining orders
        self.placed_first_leg_prices.clear();
        self.previous_bids = PreviousBids::default();
        // Note: Cannot fully reconstruct state from remaining orders. Use snapshots instead.
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
            &mut self.placed_first_leg_prices,
            &mut self.previous_bids,
            &mut self.next_order_id,
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
