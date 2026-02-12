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
use std::io::{self, stdout};
use std::time::Duration;

use crate::demo_trading::trader::{DemoTradingState, Outcome};
use crate::replay::player::SPEEDS;

pub fn run_tui(mut state: DemoTradingState) -> Result<()> {
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
    state: &mut DemoTradingState,
) -> Result<()> {
    // Process initial tick to check for order placement opportunities
    state.process_current_tick();

    loop {
        terminal.draw(|frame| draw_ui(frame, state))?;

        // Poll for events with timeout for smooth playback
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => state.toggle_pause(),
                        KeyCode::Char('r') => {
                            state.toggle_trading_pause();
                            // Process current tick when resuming trading
                            state.process_current_tick();
                        }
                        KeyCode::Char('o') => {
                            state.reset();
                            // Process first tick after reset
                            state.process_current_tick();
                        }
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
                        _ => {}
                    }
                }
            }
        }

        state.update();
    }
}

fn draw_ui(frame: &mut Frame, state: &DemoTradingState) {
    let area = frame.area();

    // Main layout: Left panels | Right order books
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left side: Info, Portfolio, Open Orders, History (stacked vertically)
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Info
            Constraint::Length(10), // Portfolio
            Constraint::Min(8),     // Open Orders
            Constraint::Min(8),     // History
        ])
        .split(main_layout[0]);

    // Right side: Up Bids, Down Bids (stacked vertically)
    let right_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    draw_info_panel(frame, state, left_layout[0]);
    draw_portfolio_panel(frame, state, left_layout[1]);
    draw_open_orders_panel(frame, state, left_layout[2]);
    draw_history_panel(frame, state, left_layout[3]);
    draw_order_book(frame, state, right_layout[0], true);
    draw_order_book(frame, state, right_layout[1], false);
}

fn draw_info_panel(frame: &mut Frame, state: &DemoTradingState, area: Rect) {
    let status = if state.is_paused {
        "⏸ PAUSED"
    } else if state.is_trading_paused {
        "▶ PLAYING (Trading PAUSED)"
    } else {
        "▶ PLAYING"
    };
    let speed_str = format!("{}x", SPEEDS[state.speed_index]);

    // Calculate current quarter
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
            Span::styled("Slug: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &state.recording.metadata.slug,
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("Latency: ", Style::default().fg(Color::Gray)),
            Span::styled(
                latency_text,
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tick: ", Style::default().fg(Color::Gray)),
            Span::styled(tick_info, Style::default().fg(Color::Yellow)),
            Span::styled(
                "  [1-4: Jump to Quarters]",
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

    let paragraph =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Info "));

    frame.render_widget(paragraph, area);
}

fn draw_portfolio_panel(frame: &mut Frame, state: &DemoTradingState, area: Rect) {
    let p = &state.portfolio;

    let text = vec![
        Line::from(vec![
            Span::styled("Balance: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("${:.2}", p.balance),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("UP: ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("{:.2} shares @ avg {:.2} ", p.up_shares, p.up_avg()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("(${:.2})", p.up_spent),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("DOWN: ", Style::default().fg(Color::Red)),
            Span::styled(
                format!("{:.2} shares @ avg {:.2} ", p.down_shares, p.down_avg()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("(${:.2})", p.down_spent),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Total Avg: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.2}", p.total_avg()),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Total Spent: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("${:.2}", p.total_spent()),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    let paragraph =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Portfolio "));

    frame.render_widget(paragraph, area);
}

fn draw_open_orders_panel(frame: &mut Frame, state: &DemoTradingState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Side").style(Style::default().fg(Color::Gray)),
        Cell::from("Outcome").style(Style::default().fg(Color::Gray)),
        Cell::from("Price").style(Style::default().fg(Color::Gray)),
        Cell::from("Filled").style(Style::default().fg(Color::Gray)),
        Cell::from("Total").style(Style::default().fg(Color::Gray)),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = state
        .open_orders
        .iter()
        .map(|order| {
            let outcome_color = match order.outcome {
                Outcome::Up => Color::Green,
                Outcome::Down => Color::Red,
            };

            Row::new(vec![
                Cell::from("Buy").style(Style::default().fg(Color::White)),
                Cell::from(order.outcome.as_str()).style(Style::default().fg(outcome_color)),
                Cell::from(format!("{:.0}¢", order.price * 100.0))
                    .style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0} / {:.0}", order.filled, order.size))
                    .style(Style::default().fg(Color::White)),
                Cell::from(format!("${:.2}", order.total()))
                    .style(Style::default().fg(Color::White)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(8),
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

fn draw_history_panel(frame: &mut Frame, state: &DemoTradingState, area: Rect) {
    if state.trade_history.is_empty() {
        let paragraph = Paragraph::new("No trades yet...")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" History "));
        frame.render_widget(paragraph, area);
        return;
    }

    // Calculate how much time has passed in the replay
    let current_ts = if !state.recording.ticks.is_empty() {
        state.recording.ticks[state.current_tick].ts
    } else {
        0
    };

    let lines: Vec<Line> = state
        .trade_history
        .iter()
        .rev()
        .map(|trade| {
            let outcome_color = match trade.outcome {
                Outcome::Up => Color::Green,
                Outcome::Down => Color::Red,
            };

            let trade_ts = if !state.recording.ticks.is_empty()
                && trade.executed_at_tick < state.recording.ticks.len()
            {
                state.recording.ticks[trade.executed_at_tick].ts
            } else {
                current_ts
            };

            let time_ago_ms = current_ts - trade_ts;
            let time_ago_str = if time_ago_ms < 1000 {
                format!("{}ms", time_ago_ms)
            } else if time_ago_ms < 60000 {
                format!("{}s", time_ago_ms / 1000)
            } else {
                let minutes = time_ago_ms / 60000;
                let seconds = (time_ago_ms % 60000) / 1000;
                format!("{}m {}s", minutes, seconds)
            };

            Line::from(vec![
                Span::styled("Bought ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:.2} {}", trade.size, trade.outcome.as_str()),
                    Style::default().fg(outcome_color),
                ),
                Span::styled(" at ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:.0}¢", trade.price * 100.0),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!(" (${:.2})", trade.total()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  {}", time_ago_str),
                    Style::default().fg(Color::Gray),
                ),
            ])
        })
        .collect();

    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" History "));

    frame.render_widget(paragraph, area);
}

fn draw_order_book(frame: &mut Frame, state: &DemoTradingState, area: Rect, is_up: bool) {
    let title = if is_up { " Up Bids " } else { " Down Bids " };
    let title_color = if is_up { Color::Green } else { Color::Red };

    let bids = if state.recording.ticks.is_empty() {
        &vec![]
    } else {
        let tick = &state.recording.ticks[state.current_tick];
        if is_up {
            &tick.up_bids
        } else {
            &tick.down_bids
        }
    };

    let header = Row::new(vec![
        Cell::from("Price").style(Style::default().fg(Color::Gray)),
        Cell::from("Size").style(Style::default().fg(Color::Gray)),
        Cell::from("Total").style(Style::default().fg(Color::Gray)),
    ])
    .height(1)
    .bottom_margin(1);

    // Find which prices have pending orders
    let outcome = if is_up { Outcome::Up } else { Outcome::Down };
    let pending_prices: Vec<f64> = state
        .open_orders
        .iter()
        .filter(|o| o.outcome == outcome)
        .map(|o| o.price)
        .collect();

    let rows: Vec<Row> = bids
        .iter()
        .map(|bid| {
            let price = bid[0];
            let size = bid[1];
            let total = price * size;

            let has_order = pending_prices.iter().any(|&p| (p - price).abs() < 0.001);

            let price_text = {
                let cents = price * 100.0;
                let cents_str = if (cents - cents.round()).abs() < 0.01 {
                    format!("{:.0}¢", cents)
                } else {
                    format!("{:.1}¢", cents)
                };
                if has_order {
                    format!("{} ⏱", cents_str)
                } else {
                    cents_str
                }
            };

            Row::new(vec![
                Cell::from(price_text).style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0}", size)).style(Style::default().fg(Color::Yellow)),
                Cell::from(format!("${:.2}", total)).style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(title_color)),
    );

    frame.render_widget(table, area);
}
