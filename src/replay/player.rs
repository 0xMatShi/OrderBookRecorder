use std::time::Instant;
use crate::models::Recording;

pub const SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

pub struct ReplayState {
    pub recording: Recording,
    pub current_tick: usize,
    pub is_paused: bool,
    pub speed_index: usize,
    pub last_frame_time: Instant,
    pub accumulated_time_ms: f64,
}

impl ReplayState {
    pub fn new(recording: Recording) -> Self {
        Self {
            recording,
            current_tick: 0,
            is_paused: true,
            speed_index: 3, // 1x speed
            last_frame_time: Instant::now(),
            accumulated_time_ms: 0.0,
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
        self.last_frame_time = Instant::now();
        self.accumulated_time_ms = 0.0;
    }

    pub fn move_ticks(&mut self, delta: i32) {
        let new_tick = self.current_tick as i32 + delta;
        self.current_tick = new_tick.clamp(0, self.recording.ticks.len().saturating_sub(1) as i32) as usize;
    }

    pub fn move_seconds(&mut self, delta_seconds: i64) {
        if self.recording.ticks.is_empty() {
            return;
        }

        let current_ts = self.recording.ticks[self.current_tick].ts;
        let target_ts = current_ts + (delta_seconds * 1000);

        // Binary search for the closest tick
        let target_tick = self.recording.ticks
            .binary_search_by(|t| t.ts.cmp(&target_ts))
            .unwrap_or_else(|i| i.saturating_sub(1));

        self.current_tick = target_tick.min(self.recording.ticks.len() - 1);
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
            } else {
                break;
            }
        }
    }

    pub fn current_time_str(&self) -> String {
        if self.recording.ticks.is_empty() {
            return "00:00.000".to_string();
        }

        let first_ts = self.recording.ticks[0].ts;
        let current_ts = self.recording.ticks[self.current_tick].ts;
        let elapsed_ms = current_ts - first_ts;

        let minutes = elapsed_ms / 60000;
        let seconds = (elapsed_ms % 60000) / 1000;
        let millis = elapsed_ms % 1000;

        format!("{:02}:{:02}.{:03}", minutes, seconds, millis)
    }

    pub fn total_time_str(&self) -> String {
        let duration_ms = self.recording.duration_ms();
        let minutes = duration_ms / 60000;
        let seconds = (duration_ms % 60000) / 1000;
        let millis = duration_ms % 1000;

        format!("{:02}:{:02}.{:03}", minutes, seconds, millis)
    }
    
    #[allow(dead_code)]
    pub fn is_at_end(&self) -> bool {
        self.current_tick >= self.recording.ticks.len().saturating_sub(1)
    }
}
