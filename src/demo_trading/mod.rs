mod trader;
mod ui;

pub use trader::{calculate_event_result, DemoTradingState, Outcome};
pub use ui::run_tui;
