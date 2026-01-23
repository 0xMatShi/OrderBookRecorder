use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use chrono::{DateTime, Utc};
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::models::{BookMessage, SubscribeMessage, TargetMarket, Tick};
use crate::download::storage::RecordingStorage;

const ORDER_BOOK_DEPTH: usize = 20;

pub struct Recorder {
    ws_url: String,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string(),
        }
    }

    pub async fn record(&self, target: &TargetMarket, storage: &RecordingStorage) -> Result<()> {
        let end_date = target.end_date.parse::<DateTime<Utc>>()?;
        let start_time = Utc::now();

        let filepath = storage.create_recording_file(
            &target.slug,
            &target.title,
            &target.up_token,
            &target.down_token,
            start_time,
            end_date,
        )?;

        info!("📝 Запись в файл: {:?}", filepath);

        let tick_count = Arc::new(AtomicU32::new(0));

        loop {
            if Utc::now() >= end_date {
                let total = tick_count.load(Ordering::Relaxed);
                RecordingStorage::update_total_ticks(&filepath, total)?;
                info!("✅ Запись завершена. Всего тиков: {}", total);
                return Ok(());
            }

            match self.run_stream_once(target, &filepath, end_date, tick_count.clone()).await {
                Ok(_) => {
                    let total = tick_count.load(Ordering::Relaxed);
                    RecordingStorage::update_total_ticks(&filepath, total)?;
                    info!("✅ Запись завершена. Всего тиков: {}", total);
                    return Ok(());
                }
                Err(e) => {
                    warn!("📉 WS отключен: {}. Переподключение через 1 сек...", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn run_stream_once(
        &self,
        target: &TargetMarket,
        filepath: &PathBuf,
        end_date: DateTime<Utc>,
        tick_count: Arc<AtomicU32>,
    ) -> Result<()> {
        let (mut ws_stream, _) = connect_async(&self.ws_url).await?;

        let sub = SubscribeMessage {
            assets_ids: vec![target.up_token.clone(), target.down_token.clone()],
            msg_type: "market".to_string(),
        };
        ws_stream.send(Message::Text(serde_json::to_string(&sub)?.into())).await?;

        info!("✅ WebSocket подключен");

        let mut check_interval = interval(Duration::from_secs(1));
        let mut current_up_bids: Vec<[f64; 2]> = Vec::new();
        let mut current_down_bids: Vec<[f64; 2]> = Vec::new();

        loop {
            tokio::select! {
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(book) = serde_json::from_str::<BookMessage>(&text) {
                                let mut sorted_bids: Vec<_> = book.bids.iter()
                                    .map(|o| [o.price, o.size])
                                    .collect();
                                sorted_bids.sort_by(|a, b| b[0].partial_cmp(&a[0]).unwrap_or(std::cmp::Ordering::Equal));
                                sorted_bids.truncate(ORDER_BOOK_DEPTH);

                                if book.asset_id == target.up_token {
                                    current_up_bids = sorted_bids;
                                } else if book.asset_id == target.down_token {
                                    current_down_bids = sorted_bids;
                                }

                                // Save tick with current state
                                if !current_up_bids.is_empty() || !current_down_bids.is_empty() {
                                    let tick = Tick {
                                        ts: Utc::now().timestamp_millis(),
                                        up_bids: current_up_bids.clone(),
                                        down_bids: current_down_bids.clone(),
                                    };

                                    RecordingStorage::append_tick(filepath, &tick)?;
                                    let count = tick_count.fetch_add(1, Ordering::Relaxed) + 1;

                                    if count % 100 == 0 {
                                        info!("📊 Записано тиков: {}", count);
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            anyhow::bail!("WebSocket закрыт сервером");
                        }
                        Some(Err(e)) => {
                            return Err(e.into());
                        }
                        None => {
                            anyhow::bail!("Соединение потеряно");
                        }
                        _ => {}
                    }
                }
                _ = check_interval.tick() => {
                    if Utc::now() >= end_date {
                        return Ok(());
                    }
                }
            }
        }
    }
}
