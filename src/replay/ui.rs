use std::io::{self, stdout};
use std::time::Duration;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use anyhow::Result;

use crate::replay::player::{ReplayState, SPEEDS};

pub fn run_tui(mut state: ReplayState) -> Result<()> {
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
    state: &mut ReplayState,
) -> Result<()> {
    loop {
        terminal.draw(|frame| draw_ui(frame, state))?;

        // Poll for events with timeout for smooth playback
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => state.toggle_pause(),
                        KeyCode::Char('+') | KeyCode::Char('=') => state.speed_up(),
                        KeyCode::Char('-') => state.speed_down(),
                        KeyCode::Char('a') => state.move_ticks(-1),
                        KeyCode::Char('d') => state.move_ticks(1),
                        KeyCode::Char('j') => state.move_ticks(-10),
                        KeyCode::Char('l') => state.move_ticks(10),
                        KeyCode::Char('z') => state.move_seconds(-1),
                        KeyCode::Char('c') => state.move_seconds(1),
                        _ => {}
                    }
                }
            }
        }

        state.update();
    }
}

fn draw_ui(frame: &mut Frame, state: &ReplayState) {
    let area = frame.area();

    // Main layout: Info panel on top, order books below
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Info + Keys panel
            Constraint::Min(10),    // Order books
        ])
        .split(area);

    // Top panel: Info | Keys
    let top_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(main_layout[0]);

    draw_info_panel(frame, state, top_layout[0]);
    draw_keys_panel(frame, top_layout[1]);

    // Bottom panel: UP Bids | DOWN Bids
    let books_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(main_layout[1]);

    draw_order_book(frame, state, books_layout[0], true);
    draw_order_book(frame, state, books_layout[1], false);
}

fn draw_info_panel(frame: &mut Frame, state: &ReplayState, area: Rect) {
    let status = if state.is_paused { "⏸ PAUSED" } else { "▶ PLAYING" };
    let speed_str = format!("{}x", SPEEDS[state.speed_index]);

    let tick_info = format!(
        "{} / {}",
        state.current_tick + 1,
        state.recording.ticks.len()
    );

    let time_info = format!(
        "{} / {}",
        state.current_time_str(),
        state.total_time_str()
    );

    let text = vec![
        Line::from(vec![
            Span::styled("Title: ", Style::default().fg(Color::Gray)),
            Span::styled(&state.recording.metadata.title, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Slug: ", Style::default().fg(Color::Gray)),
            Span::styled(&state.recording.metadata.slug, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tick: ", Style::default().fg(Color::Gray)),
            Span::styled(tick_info, Style::default().fg(Color::Yellow)),
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
                Style::default().fg(if state.is_paused { Color::Red } else { Color::Green }),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Info "));

    frame.render_widget(paragraph, area);
}

fn draw_keys_panel(frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(vec![
            Span::styled("Space", Style::default().fg(Color::Cyan)),
            Span::raw(" - Pause/Resume"),
        ]),
        Line::from(vec![
            Span::styled("+/-", Style::default().fg(Color::Cyan)),
            Span::raw(" - Speed up/down"),
        ]),
        Line::from(vec![
            Span::styled("a/d", Style::default().fg(Color::Cyan)),
            Span::raw(" - -1/+1 tick"),
        ]),
        Line::from(vec![
            Span::styled("j/l", Style::default().fg(Color::Cyan)),
            Span::raw(" - -10/+10 ticks"),
        ]),
        Line::from(vec![
            Span::styled("z/c", Style::default().fg(Color::Cyan)),
            Span::raw(" - -1/+1 second"),
        ]),
        Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" - Quit"),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Keys "));

    frame.render_widget(paragraph, area);
}

fn draw_order_book(frame: &mut Frame, state: &ReplayState, area: Rect, is_up: bool) {
    let title = if is_up { " UP Bids " } else { " DOWN Bids " };
    let title_color = if is_up { Color::Green } else { Color::Red };

    let bids = if state.recording.ticks.is_empty() {
        &vec![]
    } else {
        let tick = &state.recording.ticks[state.current_tick];
        if is_up { &tick.up_bids } else { &tick.down_bids }
    };

    let header = Row::new(vec![
        Cell::from("#").style(Style::default().fg(Color::Gray)),
        Cell::from("Price").style(Style::default().fg(Color::Gray)),
        Cell::from("Size").style(Style::default().fg(Color::Gray)),
        Cell::from("Cost($)").style(Style::default().fg(Color::Gray)),
    ])
    .height(1)
    .bottom_margin(1);

    // Calculate cumulative costs
    let mut cumulative_cost = 0.0;
    let rows: Vec<Row> = bids
        .iter()
        .enumerate()
        .map(|(i, bid)| {
            let price = bid[0];
            let size = bid[1];
            cumulative_cost += price * size;

            Row::new(vec![
                Cell::from(format!("{}", i + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{:.2}", price)).style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0}", size)).style(Style::default().fg(Color::Yellow)),
                Cell::from(format!("${:.0}", cumulative_cost)).style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Min(8),
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
