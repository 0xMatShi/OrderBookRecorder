mod demo_trading;
mod download;
mod models;
mod replay;

use anyhow::Result;
use std::io::{self, Write};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::download::{AutoScanner, Recorder, RecordingStorage};
use crate::models::Coin;
use crate::replay::{run_tui, ReplayState};

const RECORDINGS_DIR: &str = "recordings";

fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
    io::stdout().flush().ok();
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    loop {
        clear_screen();
        println!("=== Order Book Recorder ===");
        println!("1. Download (record live data)");
        println!("2. Replay (playback recording)");
        println!("3. Demo-Trading (trade with virtual balance)");
        println!("4. Exit");
        print!("\nSelect option: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim() {
            "1" => download_mode().await?,
            "2" => replay_mode()?,
            "3" => demo_trading_mode()?,
            "4" => {
                clear_screen();
                println!("Goodbye!");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn download_mode() -> Result<()> {
    clear_screen();
    println!("=== Download Mode ===");
    println!("Select coin:");
    println!("1. BTC");
    println!("2. ETH");
    println!("3. SOL");
    println!("4. XRP");
    print!("\nSelect coin (Enter to go back): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Empty input - return to main menu
    if input.trim().is_empty() {
        return Ok(());
    }

    let coin = match input.trim().parse::<u8>() {
        Ok(n) => match Coin::from_index(n) {
            Some(c) => c,
            None => {
                return Ok(());
            }
        },
        Err(_) => {
            return Ok(());
        }
    };

    clear_screen();
    println!("Starting continuous recording for {}...", coin.name());
    println!("Press Ctrl+C to stop\n");

    let scanner = AutoScanner::new();
    let recorder = Recorder::new();
    let storage = RecordingStorage::new(RECORDINGS_DIR)?;

    // Continuous recording loop
    loop {
        // Find next target (0-15 min window)
        let target = scanner
            .find_next_target(coin.slug_prefix(), 0.0, 15.0)
            .await;

        if let Some(target) = target {
            info!("🎯 Найдена цель: {} ({})", target.title, target.slug);

            // Record until event ends
            if let Err(e) = recorder.record(&target, &storage).await {
                tracing::error!("❌ Ошибка записи: {}", e);
            }

            info!("🔄 Поиск следующего события...");
        }
    }
}

fn replay_mode() -> Result<()> {
    clear_screen();
    let storage = RecordingStorage::new(RECORDINGS_DIR)?;
    let recordings = storage.list_recordings()?;

    if recordings.is_empty() {
        println!("No recordings found in '{}'", RECORDINGS_DIR);
        println!("\nPress Enter to continue...");
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        return Ok(());
    }

    println!("=== Available Recordings ===");
    for (i, path) in recordings.iter().enumerate() {
        let metadata = RecordingStorage::load_metadata(path);
        match metadata {
            Ok(m) => {
                println!("{}. {} ({} ticks)", i + 1, m.display_name(), m.total_ticks);
            }
            Err(_) => {
                println!(
                    "{}. {} (error reading metadata)",
                    i + 1,
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
        }
    }

    print!(
        "\nSelect recording (1-{}, Enter to go back): ",
        recordings.len()
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Empty input - return to main menu
    if input.trim().is_empty() {
        return Ok(());
    }

    let index: usize = match input.trim().parse::<usize>() {
        Ok(n) if n >= 1 && n <= recordings.len() => n - 1,
        _ => {
            return Ok(());
        }
    };

    let recording_path = &recordings[index];
    let recording = RecordingStorage::load_recording(recording_path)?;

    let state = ReplayState::new(recording);
    run_tui(state)?;

    Ok(())
}

fn demo_trading_mode() -> Result<()> {
    clear_screen();
    let storage = RecordingStorage::new(RECORDINGS_DIR)?;
    let recordings = storage.list_recordings()?;

    if recordings.is_empty() {
        println!("No recordings found in '{}'", RECORDINGS_DIR);
        println!("\nPress Enter to continue...");
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        return Ok(());
    }

    println!("=== Available Recordings ===");
    println!("Calculating results...\n");
    
    let mut all_results = Vec::new();
    
    for (i, path) in recordings.iter().enumerate() {
        let metadata = RecordingStorage::load_metadata(path);
        match metadata {
            Ok(m) => {
                // Load recording and calculate result
                match RecordingStorage::load_recording(path) {
                    Ok(recording) => {
                        let result = crate::demo_trading::calculate_event_result(&recording);
                        let winner_str = match result.winner {
                            Some(crate::demo_trading::Outcome::Up) => "UP",
                            Some(crate::demo_trading::Outcome::Down) => "DOWN",
                            None => "NONE",
                        };
                        
                        let pnl_color = if result.pnl > 0.0 { "+" } else { "" };
                        let result_str = format!(
                            " (Up: {:.2} sh | Down: {:.2} sh | Spent: ${:.2} | Winner: {} | PnL: {}{:.2})",
                            result.up_shares, result.down_shares, result.total_spent, winner_str, pnl_color, result.pnl
                        );
                        
                        println!("{}. {} ({} ticks){}", 
                            i + 1, m.display_name(), m.total_ticks, result_str);
                        
                        all_results.push(result);
                    }
                    Err(_) => {
                        println!("{}. {} ({} ticks) (error calculating)", 
                            i + 1, m.display_name(), m.total_ticks);
                    }
                };
            }
            Err(_) => {
                println!(
                    "{}. {} (error reading metadata)",
                    i + 1,
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
        }
    }
    
    // Calculate and display statistics
    if !all_results.is_empty() {
        let total_pnl: f64 = all_results.iter().map(|r| r.pnl).sum();
        let wins = all_results.iter().filter(|r| r.pnl > 0.0).count();
        let total = all_results.len();
        let winrate = (wins as f64 / total as f64) * 100.0;
        
        println!("\n=== Statistics ===");
        println!("Total PnL: ${:.2}", total_pnl);
        println!("Winrate: {:.1}% ({}/{})", winrate, wins, total);
    }

    print!(
        "\nSelect recording (1-{}, Enter to go back): ",
        recordings.len()
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Empty input - return to main menu
    if input.trim().is_empty() {
        return Ok(());
    }

    let index: usize = match input.trim().parse::<usize>() {
        Ok(n) if n >= 1 && n <= recordings.len() => n - 1,
        _ => {
            return Ok(());
        }
    };

    let recording_path = &recordings[index];
    let recording = RecordingStorage::load_recording(recording_path)?;

    let state = crate::demo_trading::DemoTradingState::new(recording);
    crate::demo_trading::run_tui(state)?;

    Ok(())
}
