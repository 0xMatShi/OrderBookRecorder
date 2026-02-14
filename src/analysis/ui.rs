use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use std::collections::HashMap;
use std::io::{self, stdout};
use std::time::Duration;

use super::tracker::{SizeTrackerState, TrackedOrderStatus};
use crate::replay::player::SPEEDS;

pub fn run_tui(mut state: SizeTrackerState) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_event_loop(&mut terminal, &mut state);

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SizeTrackerState,
) -> Result<()> {
    loop {
        terminal.draw(|frame| draw_ui(frame, state))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => state.toggle_pause(),
                        KeyCode::Char('a') => state.move_ticks(-1),
                        KeyCode::Char('d') => state.move_ticks(1),
                        KeyCode::Char('j') => state.move_ticks(-10),
                        KeyCode::Char('l') => state.move_ticks(10),
                        KeyCode::Char('+') | KeyCode::Char('=') => state.speed_up(),
                        KeyCode::Char('-') => state.speed_down(),
                        KeyCode::Char('1') => state.jump_to_quarter(1),
                        KeyCode::Char('2') => state.jump_to_quarter(2),
                        KeyCode::Char('3') => state.jump_to_quarter(3),
                        KeyCode::Char('4') => state.jump_to_quarter(4),
                        KeyCode::Char('o') => { state.seek_to_start(); }
                        _ => {}
                    }
                }
            }
        }

        state.update();
    }
}

fn draw_ui(frame: &mut Frame, state: &SizeTrackerState) {
    let area = frame.area();

    // Main layout: Left panels | Right order books
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left side: Info, Portfolio, Open Orders, History
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Info
            Constraint::Length(10), // Portfolio
            Constraint::Min(8),     // Open Orders
            Constraint::Min(8),     // History
        ])
        .split(main_layout[0]);

    // Right side: OBI on top, then Up Bids | Down Bids side by side
    let right_vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(10)])
        .split(main_layout[1]);

    let right_books = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_vertical[1]);

    draw_info_panel(frame, state, left_layout[0]);
    draw_portfolio_panel(frame, state, left_layout[1]);
    draw_open_orders_panel(frame, state, left_layout[2]);
    draw_history_panel(frame, state, left_layout[3]);
    draw_obi_panel(frame, state, right_vertical[0]);
    draw_order_book(frame, state, right_books[0], true);
    draw_order_book(frame, state, right_books[1], false);
}

fn draw_info_panel(frame: &mut Frame, state: &SizeTrackerState, area: Rect) {
    let status = if state.is_paused {
        "PAUSED"
    } else {
        "PLAYING"
    };
    let speed_str = format!("{}x", SPEEDS[state.speed_index]);

    let total_ticks = state.recording.ticks.len();
    let current_quarter = if total_ticks > 0 {
        ((state.current_tick as f64 / total_ticks as f64) * 4.0).ceil() as u8
    } else {
        0
    };

    let tick_info = format!(
        "{} / {} (Q{})",
        state.current_tick + 1,
        total_ticks,
        current_quarter
    );
    let time_info = format!("{} / {}", state.current_time_str(), state.total_time_str());

    // Форматируем latency если доступна
    let latency_text = if let Some(latency) = state.recording.metadata.avg_latency_ms {
        format!("{} ms", latency)
    } else {
        "N/A".to_string()
    };

    let text = vec![
        Line::from(vec![
            Span::styled("Title: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &state.recording.metadata.title,
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Latency: ", Style::default().fg(Color::Gray)),
            Span::styled(
                latency_text,
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(vec![
            Span::styled("Target Size: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.0}", state.target_size),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tick: ", Style::default().fg(Color::Gray)),
            Span::styled(tick_info, Style::default().fg(Color::Yellow)),
            Span::styled(
                "  [1-4: Jump]",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::Gray)),
            Span::styled(time_info, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Speed: ", Style::default().fg(Color::Gray)),
            Span::styled(speed_str, Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                status,
                Style::default().fg(if state.is_paused {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Size Analysis "));
    frame.render_widget(paragraph, area);
}

fn draw_portfolio_panel(frame: &mut Frame, state: &SizeTrackerState, area: Rect) {
    let filled_orders: Vec<_> = state
        .history
        .iter()
        .filter(|o| o.status == TrackedOrderStatus::Filled)
        .collect();
    let cancelled_count = state
        .history
        .iter()
        .filter(|o| o.status == TrackedOrderStatus::Cancelled)
        .count();

    let up_filled: f64 = filled_orders
        .iter()
        .filter(|o| o.side == "up")
        .map(|o| o.size)
        .sum();
    let down_filled: f64 = filled_orders
        .iter()
        .filter(|o| o.side == "down")
        .map(|o| o.size)
        .sum();
    let up_spent: f64 = filled_orders
        .iter()
        .filter(|o| o.side == "up")
        .map(|o| o.size * o.price)
        .sum();
    let down_spent: f64 = filled_orders
        .iter()
        .filter(|o| o.side == "down")
        .map(|o| o.size * o.price)
        .sum();
    let total_spent = up_spent + down_spent;

    let text = vec![
        Line::from(vec![
            Span::styled("Orders: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} open", state.open_orders.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} filled", filled_orders.len()),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} cancelled", cancelled_count),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("UP: ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("{:.0} shares", up_filled),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!(" (${:.2})", up_spent),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if up_filled > 0.0 {
                    format!("  avg: {}", format_price(up_spent / up_filled))
                } else {
                    String::new()
                },
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("DOWN: ", Style::default().fg(Color::Red)),
            Span::styled(
                format!("{:.0} shares", down_filled),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!(" (${:.2})", down_spent),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if down_filled > 0.0 {
                    format!("  avg: {}", format_price(down_spent / down_filled))
                } else {
                    String::new()
                },
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from({
            let up_avg = if up_filled > 0.0 { up_spent / up_filled } else { 0.0 };
            let down_avg = if down_filled > 0.0 { down_spent / down_filled } else { 0.0 };
            let total_avg = up_avg + down_avg;
            vec![
                Span::styled("Total Spent: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("${:.2}", total_spent),
                    Style::default().fg(Color::White),
                ),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled("Total Avg: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format_price(total_avg),
                    Style::default().fg(Color::Cyan),
                ),
            ]
        }),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Portfolio "));
    frame.render_widget(paragraph, area);
}

fn draw_open_orders_panel(frame: &mut Frame, state: &SizeTrackerState, area: Rect) {
    if state.open_orders.is_empty() {
        let paragraph = Paragraph::new("No tracked orders...")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Open Orders "),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Side").style(Style::default().fg(Color::Gray)),
        Cell::from("Price").style(Style::default().fg(Color::Gray)),
        Cell::from("Size").style(Style::default().fg(Color::Gray)),
        Cell::from("Filled").style(Style::default().fg(Color::Gray)),
        Cell::from("Queue").style(Style::default().fg(Color::Gray)),
        Cell::from("Status").style(Style::default().fg(Color::Gray)),
    ])
    .height(1)
    .bottom_margin(1);

    let current_ts = if !state.recording.ticks.is_empty() {
        state.recording.ticks[state.current_tick].ts
    } else {
        0
    };

    let rows: Vec<Row> = state
        .open_orders
        .iter()
        .map(|order| {
            let side_color = if order.side == "up" {
                Color::Green
            } else {
                Color::Red
            };

            let fill_pct = if order.size > 0.0 {
                (order.filled / order.size * 100.0) as u32
            } else {
                0
            };

            let status_str = if order.filled > 0.01 {
                format!("Filling {}%", fill_pct)
            } else {
                let wait_ms = current_ts - order.placed_ts;
                if wait_ms < 1000 {
                    format!("In queue {}ms", wait_ms)
                } else {
                    format!("In queue {}s", wait_ms / 1000)
                }
            };

            let status_color = if order.filled > 0.01 {
                Color::Yellow
            } else {
                Color::DarkGray
            };

            Row::new(vec![
                Cell::from(order.side.to_uppercase())
                    .style(Style::default().fg(side_color)),
                Cell::from(format_price(order.price))
                    .style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0}", order.size))
                    .style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0}/{:.0}", order.filled, order.size))
                    .style(Style::default().fg(Color::Yellow)),
                Cell::from(format!("{:.0}", order.queue_ahead))
                    .style(Style::default().fg(Color::Cyan)),
                Cell::from(status_str)
                    .style(Style::default().fg(status_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Open Orders "),
    );

    frame.render_widget(table, area);
}

fn draw_history_panel(frame: &mut Frame, state: &SizeTrackerState, area: Rect) {
    if state.history.is_empty() {
        let paragraph = Paragraph::new("No history yet...")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" History "));
        frame.render_widget(paragraph, area);
        return;
    }

    let current_ts = if !state.recording.ticks.is_empty() {
        state.recording.ticks[state.current_tick].ts
    } else {
        0
    };

    let lines: Vec<Line> = state
        .history
        .iter()
        .rev()
        .map(|order| {
            let side_color = if order.side == "up" {
                Color::Green
            } else {
                Color::Red
            };

            let (result_str, result_color) = match order.status {
                TrackedOrderStatus::Filled => ("FILLED", Color::Green),
                TrackedOrderStatus::Cancelled => ("CANCELLED", Color::Red),
                TrackedOrderStatus::Open => ("OPEN", Color::Yellow),
            };

            let wait_ms = order.resolved_ts.unwrap_or(current_ts) - order.placed_ts;
            let wait_str = if wait_ms < 1000 {
                format!("{}ms", wait_ms)
            } else if wait_ms < 60000 {
                format!("{}s", wait_ms / 1000)
            } else {
                let m = wait_ms / 60000;
                let s = (wait_ms % 60000) / 1000;
                format!("{}m {}s", m, s)
            };

            Line::from(vec![
                Span::styled(
                    order.side.to_uppercase(),
                    Style::default().fg(side_color),
                ),
                Span::styled(
                    format!(" {} ", format_price(order.price)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:.0} ", order.size),
                    Style::default().fg(Color::White),
                ),
                Span::styled(result_str, Style::default().fg(result_color)),
                Span::styled(
                    format!("  {}", wait_str),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();

    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" History "));
    frame.render_widget(paragraph, area);
}

fn draw_obi_panel(frame: &mut Frame, state: &SizeTrackerState, area: Rect) {
    const OBI_DEPTH: usize = 6;

    let (up_vol, down_vol, up_bid_vol, up_ask_vol) = if !state.recording.ticks.is_empty() {
        let tick = &state.recording.ticks[state.current_tick];
        let uv: f64 = tick.up_bids.iter().take(OBI_DEPTH).map(|b| b[0] * b[1]).sum();
        let dv: f64 = tick.down_bids.iter().take(OBI_DEPTH).map(|b| b[0] * b[1]).sum();
        // Full OBI: UP bids vs UP asks (derived from DOWN bids)
        // DOWN bid at price P = UP ask at price (1-P)
        let ua: f64 = tick.down_bids.iter().take(OBI_DEPTH).map(|b| (1.0 - b[0]) * b[1]).sum();
        (uv, dv, uv, ua)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    // OBI: (V_up - V_down) / (V_up + V_down)
    let total = up_vol + down_vol;
    let obi = if total > 0.0 { (up_vol - down_vol) / total } else { 0.0 };

    // Full OBI: (V_bid - V_ask) / (V_bid + V_ask) for UP side
    let full_total = up_bid_vol + up_ask_vol;
    let full_obi = if full_total > 0.0 { (up_bid_vol - up_ask_vol) / full_total } else { 0.0 };

    let obi_color = if obi > 0.1 { Color::Green } else if obi < -0.1 { Color::Red } else { Color::Yellow };
    let full_obi_color = if full_obi > 0.1 { Color::Green } else if full_obi < -0.1 { Color::Red } else { Color::Yellow };

    let lines = vec![
        Line::from(vec![
            Span::styled("OBI: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:+.2}", obi), Style::default().fg(obi_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  (U: ${:.0} | D: ${:.0})", up_vol, down_vol), Style::default().fg(Color::DarkGray)),
        ]).alignment(Alignment::Center),
        Line::from(vec![
            Span::styled("Full: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:+.2}", full_obi), Style::default().fg(full_obi_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  (Bid: ${:.0} | Ask: ${:.0})", up_bid_vol, up_ask_vol), Style::default().fg(Color::DarkGray)),
        ]).alignment(Alignment::Center),
    ];

    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" OBI ")
                .title_alignment(Alignment::Center),
        );
    frame.render_widget(paragraph, area);
}

fn draw_order_book(frame: &mut Frame, state: &SizeTrackerState, area: Rect, is_up: bool) {
    let title = if is_up { " UP Bids " } else { " DOWN Bids " };
    let title_color = if is_up { Color::Green } else { Color::Red };

    let empty_bids: Vec<[f64; 2]> = Vec::new();
    let (bids, prev_bids) = if state.recording.ticks.is_empty() {
        (&empty_bids, &empty_bids)
    } else {
        let tick = &state.recording.ticks[state.current_tick];
        let current = if is_up { &tick.up_bids } else { &tick.down_bids };
        if state.current_tick > 0 {
            let prev_tick = &state.recording.ticks[state.current_tick - 1];
            let prev = if is_up {
                &prev_tick.up_bids
            } else {
                &prev_tick.down_bids
            };
            (current, prev)
        } else {
            (current, &empty_bids)
        }
    };

    // UP: Cum D SIZE PRICE — right-aligned (price toward center)
    // DOWN: PRICE SIZE D Cum — left-aligned (price toward center)
    let header = if is_up {
        Row::new(vec![
            Cell::from(Line::from("Cum$").alignment(Alignment::Right)).style(Style::default().fg(Color::DarkGray)),
            Cell::from(Line::from("D").alignment(Alignment::Right)).style(Style::default().fg(Color::Gray)),
            Cell::from(Line::from("Size").alignment(Alignment::Right)).style(Style::default().fg(Color::Gray)),
            Cell::from(Line::from("Price").alignment(Alignment::Right)).style(Style::default().fg(Color::Gray)),
        ])
    } else {
        Row::new(vec![
            Cell::from("Price").style(Style::default().fg(Color::Gray)),
            Cell::from("Size").style(Style::default().fg(Color::Gray)),
            Cell::from("D").style(Style::default().fg(Color::Gray)),
            Cell::from("Cum$").style(Style::default().fg(Color::DarkGray)),
        ])
    }
    .height(1)
    .bottom_margin(1);

    // Count tracked orders per price level
    let side_str = if is_up { "up" } else { "down" };
    let mut tracked_counts: HashMap<u32, usize> = HashMap::new();
    for o in state.open_orders.iter().filter(|o| o.side == side_str) {
        let key = (o.price * 100.0).round() as u32;
        *tracked_counts.entry(key).or_insert(0) += 1;
    }

    let mut cum_value = 0.0_f64;
    let rows: Vec<Row> = bids
        .iter()
        .map(|bid| {
            let price = bid[0];
            let size = bid[1];
            cum_value += price * size;

            let price_key = (price * 100.0).round() as u32;
            let tracked_count = tracked_counts.get(&price_key).copied().unwrap_or(0);

            let price_text = {
                let formatted = format_price(price);
                if tracked_count > 0 {
                    format!("{} *({})", formatted, tracked_count)
                } else {
                    formatted
                }
            };

            // Compute delta
            let (delta_str, delta_color) = if prev_bids.is_empty() {
                ("·".to_string(), Color::DarkGray)
            } else if let Some(prev_level) =
                prev_bids.iter().find(|b| (b[0] - price).abs() < 1e-9)
            {
                let diff = size - prev_level[1];
                if diff.abs() < 0.5 {
                    ("·".to_string(), Color::DarkGray)
                } else if diff > 0.0 {
                    (format!("+{:.0}", diff), Color::Green)
                } else {
                    (format!("{:.0}", diff), Color::Red)
                }
            } else {
                ("NEW".to_string(), Color::Cyan)
            };

            let price_style = if tracked_count > 0 {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let cum_text = format!("{:.0}", cum_value);

            if is_up {
                let cum_cell = Cell::from(Line::from(cum_text).alignment(Alignment::Right)).style(Style::default().fg(Color::DarkGray));
                let delta_cell = Cell::from(Line::from(delta_str).alignment(Alignment::Right)).style(Style::default().fg(delta_color));
                let size_cell = Cell::from(Line::from(format!("{:.0}", size)).alignment(Alignment::Right)).style(Style::default().fg(Color::Yellow));
                let price_cell = Cell::from(Line::from(price_text).alignment(Alignment::Right)).style(price_style);
                Row::new(vec![cum_cell, delta_cell, size_cell, price_cell])
            } else {
                let delta_cell = Cell::from(delta_str).style(Style::default().fg(delta_color));
                let size_cell = Cell::from(format!("{:.0}", size)).style(Style::default().fg(Color::Yellow));
                let price_cell = Cell::from(price_text).style(price_style);
                let cum_cell = Cell::from(cum_text).style(Style::default().fg(Color::DarkGray));
                Row::new(vec![price_cell, size_cell, delta_cell, cum_cell])
            }
        })
        .collect();

    let constraints = if is_up {
        [Constraint::Min(7), Constraint::Length(7), Constraint::Length(8), Constraint::Length(10)]
    } else {
        [Constraint::Length(10), Constraint::Length(8), Constraint::Length(7), Constraint::Min(7)]
    };

    let total_value: f64 = bids.iter().map(|b| b[0] * b[1]).sum();
    let total_shares: f64 = bids.iter().map(|b| b[1]).sum();
    let bottom_title = format!(" ${:.0} | {:.0} shares ", total_value, total_shares);

    let block = if is_up {
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Right)
            .title_style(Style::default().fg(title_color))
            .title_bottom(Line::from(bottom_title).right_aligned().style(Style::default().fg(Color::DarkGray)))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(title_color))
            .title_bottom(Line::from(bottom_title).style(Style::default().fg(Color::DarkGray)))
    };

    let table = Table::new(rows, constraints)
        .header(header)
        .block(block);

    frame.render_widget(table, area);
}

fn format_price(price: f64) -> String {
    let cents = price * 100.0;
    if (cents - cents.round()).abs() < 0.01 {
        format!("{:.0}c", cents)
    } else {
        format!("{:.1}c", cents)
    }
}
