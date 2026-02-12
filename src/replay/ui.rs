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
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_layout[0]);

    draw_info_panel(frame, state, top_layout[0]);
    draw_keys_panel(frame, top_layout[1]);

    // Bottom panel: UP Bids | DOWN Bids
    let books_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    draw_order_book(frame, state, books_layout[0], true);
    draw_order_book(frame, state, books_layout[1], false);
}

fn draw_info_panel(frame: &mut Frame, state: &ReplayState, area: Rect) {
    let status = if state.is_paused {
        "⏸ PAUSED"
    } else {
        "▶ PLAYING"
    };
    let speed_str = format!("{}x", SPEEDS[state.speed_index]);

    let tick_info = if state.price_change_count > 0 {
        format!(
            "{} / {} (book: {} | pc: {})",
            state.current_tick + 1,
            state.recording.ticks.len(),
            state.book_count,
            state.price_change_count,
        )
    } else {
        format!(
            "{} / {}",
            state.current_tick + 1,
            state.recording.ticks.len(),
        )
    };

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
        Line::from(""),
        Line::from(vec![
            Span::styled("Δ Colors: ", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("Magenta", Style::default().fg(Color::Magenta)),
            Span::raw("-Fill "),
            Span::styled("Red", Style::default().fg(Color::Red)),
            Span::raw("-Cancel"),
        ]),
    ];

    let paragraph =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Keys "));

    frame.render_widget(paragraph, area);
}

fn draw_order_book(frame: &mut Frame, state: &ReplayState, area: Rect, is_up: bool) {
    let base_title = if is_up { " UP Bids " } else { " DOWN Bids " };
    let title_color = if is_up { Color::Green } else { Color::Red };

    // Определяем источник текущего тика (Book = трейд, PriceChange = изменение ордеров)
    let (is_from_book, source_label) = if !state.recording.tick_sources.is_empty()
        && state.current_tick < state.recording.tick_sources.len()
    {
        let is_book = state.recording.tick_sources[state.current_tick] == crate::models::TickSource::Book;
        let label = if is_book { "[BOOK]" } else { "[PC]" };
        (is_book, label)
    } else {
        (false, "")
    };

    let title = format!("{}{}", base_title, source_label);

    let empty_bids: Vec<[f64; 2]> = Vec::new();
    let (bids, prev_bids) = if state.recording.ticks.is_empty() {
        (&empty_bids, &empty_bids)
    } else {
        let tick = &state.recording.ticks[state.current_tick];
        let current = if is_up {
            &tick.up_bids
        } else {
            &tick.down_bids
        };
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

    // Критический паттерн: price_change + book с одинаковым timestamp = трейд
    // Когда price_change и book приходят одновременно, price_change показывает
    // конкретные уровни цен, которые были затронуты трейдом.
    // Это наблюдается в 100% случаев трейдов.
    let current_ts = if !state.recording.ticks.is_empty() {
        state.recording.ticks[state.current_tick].ts
    } else {
        0
    };

    let outcome_filter = if is_up { "up" } else { "down" };

    // Собираем price_change события для текущего timestamp
    let current_price_changes: Vec<_> = state
        .recording
        .price_changes
        .iter()
        .filter(|pc| pc.ts == current_ts)
        .flat_map(|pc| &pc.changes)
        .filter(|c| c.outcome == outcome_filter)
        .collect();

    // Собираем цены, которые были затронуты price_change (используем f64, не строки)
    let filled_prices: Vec<f64> = current_price_changes
        .iter()
        .map(|c| c.price)
        .collect();

    // Проверяем есть ли book-тик с тем же timestamp (паттерн: price_change + book = трейд)
    // Для PC-тиков это единственный способ определить fill vs cancel
    let has_book_at_current_ts = if !is_from_book && current_ts != 0 {
        let mut found = false;
        // Ищем назад от текущей позиции
        let mut i = state.current_tick;
        loop {
            if state.recording.ticks[i].ts != current_ts {
                break;
            }
            if state.recording.tick_sources[i] == crate::models::TickSource::Book {
                found = true;
                break;
            }
            if i == 0 { break; }
            i -= 1;
        }
        if !found {
            // Ищем вперёд
            let mut i = state.current_tick + 1;
            while i < state.recording.ticks.len() && state.recording.ticks[i].ts == current_ts {
                if state.recording.tick_sources[i] == crate::models::TickSource::Book {
                    found = true;
                    break;
                }
                i += 1;
            }
        }
        found
    } else {
        is_from_book
    };

    // Собираем удаленные уровни (size=0) для отображения "фантомных" уровней
    let deleted_levels: Vec<f64> = current_price_changes
        .iter()
        .filter(|c| c.size == 0.0)
        .map(|c| c.price)
        .collect();

    let header = Row::new(vec![
        Cell::from("#").style(Style::default().fg(Color::Gray)),
        Cell::from("Price").style(Style::default().fg(Color::Gray)),
        Cell::from("Size").style(Style::default().fg(Color::Gray)),
        Cell::from("Δ").style(Style::default().fg(Color::Gray)),
        Cell::from("Cost($)").style(Style::default().fg(Color::Gray)),
    ])
    .height(1)
    .bottom_margin(1);

    // Создаем комбинированный список: существующие bids + фантомные уровни (size=0)
    //
    // Когда агрессивный трейд съедает весь уровень, приходит price_change с size=0
    // и этот уровень удаляется из snapshot. Но для анализа критично видеть эти уровни!
    //
    // ВАЖНО: Показываем фантомный уровень ТОЛЬКО если он был удален в ТЕКУЩЕМ тике
    // (т.е. был в prev_bids с ненулевым size, а сейчас в deleted_levels)
    //
    // Пример: трейд съел 0.74 (было 15) и зацепил 0.73 (было 20, стало 10):
    //   price_change: [{"price":0.74,"size":0}, {"price":0.73,"size":10}] ts=1000
    //   tick: [[0.73,10], [0.72,50], ...] ts=1000
    //
    // Replay покажет:
    //   0.74 | 0  | -15  (Magenta) <- фантомный уровень
    //   0.73 | 10 | -10  (Magenta)
    //   0.72 | 50 | ·
    let mut combined_levels: Vec<[f64; 2]> = bids.to_vec();

    // Добавляем фантомные уровни (удаленные) ТОЛЬКО если они были в предыдущем тике
    for deleted_price in &deleted_levels {
        // Проверяем что этого уровня нет в текущих bids
        let not_in_current = !bids.iter().any(|b| (b[0] - deleted_price).abs() < 1e-9);

        // Проверяем что этот уровень БЫЛ в предыдущем тике с ненулевым size
        let was_in_prev = prev_bids.iter().any(|b| (b[0] - deleted_price).abs() < 1e-9 && b[1] > 0.5);

        // Показываем фантомный уровень только если он был удален ПРЯМО СЕЙЧАС
        if not_in_current && was_in_prev {
            combined_levels.push([*deleted_price, 0.0]);
        }
    }

    // Сортируем по цене (descending)
    combined_levels.sort_by(|a, b| b[0].partial_cmp(&a[0]).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate cumulative costs
    let mut cumulative_cost = 0.0;
    let rows: Vec<Row> = combined_levels
        .iter()
        .enumerate()
        .map(|(i, bid)| {
            let price = bid[0];
            let size = bid[1];
            cumulative_cost += price * size;

            // Compute delta
            let (delta_str, delta_color) = if prev_bids.is_empty() {
                ("·".to_string(), Color::DarkGray)
            } else if let Some(prev_level) = prev_bids.iter().find(|b| (b[0] - price).abs() < 1e-9)
            {
                let diff = size - prev_level[1];
                if diff.abs() < 0.5 {
                    ("·".to_string(), Color::DarkGray)
                } else if diff > 0.0 {
                    (format!("+{:.0}", diff), Color::Green)
                } else {
                    // Размер уменьшился
                    // Проверяем: есть ли price_change с текущим timestamp для этой цены?
                    // Если да - это FILL (трейд), иначе - CANCEL (отмена ордера)
                    // Используем числовое сравнение с epsilon для точности (не строки!)
                    let is_fill = is_from_book ||
                        (has_book_at_current_ts && filled_prices.iter().any(|&p| (p - price).abs() < 1e-6));

                    let color = if is_fill {
                        Color::Magenta // FILL (трейд)
                    } else {
                        Color::Red // CANCEL (отмена ордера)
                    };
                    (format!("{:.0}", diff), color)
                }
            } else {
                ("NEW".to_string(), Color::Cyan)
            };

            Row::new(vec![
                Cell::from(format!("{}", i + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from({
                    let cents = price * 100.0;
                    if (cents - cents.round()).abs() < 0.01 {
                        format!("{:.0}¢", cents)
                    } else {
                        format!("{:.1}¢", cents)
                    }
                }).style(Style::default().fg(Color::White)),
                Cell::from(format!("{:.0}", size)).style(Style::default().fg(Color::Yellow)),
                Cell::from(delta_str).style(Style::default().fg(delta_color)),
                Cell::from(format!("${:.0}", cumulative_cost))
                    .style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
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
