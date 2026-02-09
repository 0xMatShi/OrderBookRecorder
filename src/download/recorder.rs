use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, warn};

use crate::download::storage::RecordingStorage;
use crate::models::{
    BookMessage, PriceChangeLevel, PriceChangeMessage, PriceChangeTick,
    SubscribeMessage, TargetMarket, Tick, ORDER_BOOK_DEPTH,
};

// Enum для хранения всех типов событий с единым timestamp
#[derive(Debug, Clone)]
enum RecordingEvent {
    Tick(Tick),
    PriceChange(PriceChangeTick),
}

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

            match self
                .run_stream_once(target, &filepath, end_date, tick_count.clone())
                .await
            {
                Ok(_) => {
                    let total = tick_count.load(Ordering::Relaxed);
                    RecordingStorage::update_total_ticks(&filepath, total)?;
                    info!("✅ Запись завершена. Всего тиков: {}", total);
                    return Ok(());
                }
                Err(e) => {
                    warn!("📉 WS отключен: {}. Переподключение...", e);
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
        ws_stream
            .send(Message::Text(serde_json::to_string(&sub)?.into()))
            .await?;

        info!("✅ WebSocket подключен");

        let mut check_interval = interval(Duration::from_secs(1));
        let mut flush_interval = interval(Duration::from_millis(200));
        let mut current_up_bids: Vec<[f64; 2]> = Vec::new();
        let mut current_down_bids: Vec<[f64; 2]> = Vec::new();

        // Отслеживание timestamps для синхронизации book сообщений
        let mut last_up_ts: Option<i64> = None;
        let mut last_down_ts: Option<i64> = None;

        // Буфер для сортировки событий по серверному timestamp
        // BTreeMap автоматически сортирует по ключу (timestamp)
        let mut event_buffer: BTreeMap<(i64, usize), RecordingEvent> = BTreeMap::new();
        let mut event_counter: usize = 0;

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

                                // Обновляем соответствующий массив и timestamp
                                if book.asset_id == target.up_token {
                                    current_up_bids = sorted_bids;
                                    last_up_ts = Some(book.timestamp);
                                } else if book.asset_id == target.down_token {
                                    current_down_bids = sorted_bids;
                                    last_down_ts = Some(book.timestamp);
                                }

                                // Записываем тик ТОЛЬКО если получили оба book сообщения с одинаковым timestamp
                                // Это гарантирует согласованное состояние для торгового бота
                                let should_write_tick = match (last_up_ts, last_down_ts) {
                                    (Some(up_ts), Some(down_ts)) if up_ts == down_ts && up_ts == book.timestamp => {
                                        !current_up_bids.is_empty() && !current_down_bids.is_empty()
                                    }
                                    _ => false,
                                };

                                if should_write_tick {
                                    let tick = Tick {
                                        ts: book.timestamp,
                                        up_bids: current_up_bids.clone(),
                                        down_bids: current_down_bids.clone(),
                                    };
                                    event_buffer.insert((book.timestamp, event_counter), RecordingEvent::Tick(tick));
                                    event_counter += 1;
                                }
                            } else if let Ok(pc) = serde_json::from_str::<PriceChangeMessage>(&text) {
                                let changes: Vec<PriceChangeLevel> = pc.price_changes.iter()
                                    .filter(|c| c.side == "BUY")
                                    .map(|c| {
                                        let outcome = if c.asset_id == target.up_token {
                                            "up".to_string()
                                        } else {
                                            "down".to_string()
                                        };
                                        PriceChangeLevel {
                                            outcome,
                                            price: c.price,
                                            size: c.size,
                                        }
                                    })
                                    .collect();

                                if !changes.is_empty() {
                                    let pc_tick = PriceChangeTick {
                                        ts: pc.timestamp,
                                        changes,
                                    };
                                    event_buffer.insert((pc.timestamp, event_counter), RecordingEvent::PriceChange(pc_tick));
                                    event_counter += 1;
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            // Сбрасываем оставшиеся события перед закрытием
                            Self::flush_buffer(&mut event_buffer, filepath, &tick_count, i64::MAX)?;
                            anyhow::bail!("WebSocket закрыт сервером");
                        }
                        Some(Err(e)) => {
                            Self::flush_buffer(&mut event_buffer, filepath, &tick_count, i64::MAX)?;
                            return Err(e.into());
                        }
                        None => {
                            Self::flush_buffer(&mut event_buffer, filepath, &tick_count, i64::MAX)?;
                            anyhow::bail!("Соединение потеряно");
                        }
                        _ => {}
                    }
                }
                _ = flush_interval.tick() => {
                    // Периодически сбрасываем события старше 300ms
                    // Это дает время для упорядочивания событий, которые пришли не по порядку
                    // Используем максимальный серверный timestamp в буфере как точку отсчета
                    if let Some((max_ts, _)) = event_buffer.keys().next_back() {
                        let cutoff_ts = max_ts - 300;
                        Self::flush_buffer(&mut event_buffer, filepath, &tick_count, cutoff_ts)?;
                    }
                }
                _ = check_interval.tick() => {
                    if Utc::now() >= end_date {
                        // Сбрасываем все оставшиеся события
                        Self::flush_buffer(&mut event_buffer, filepath, &tick_count, i64::MAX)?;
                        return Ok(());
                    }
                }
            }
        }
    }

    fn flush_buffer(
        buffer: &mut BTreeMap<(i64, usize), RecordingEvent>,
        filepath: &PathBuf,
        tick_count: &Arc<AtomicU32>,
        cutoff_ts: i64,
    ) -> Result<()> {
        // Получаем все события с timestamp <= cutoff_ts
        let keys_to_flush: Vec<_> = buffer
            .range(..(cutoff_ts + 1, 0))
            .map(|(k, _)| *k)
            .collect();

        for key in keys_to_flush {
            if let Some(event) = buffer.remove(&key) {
                match event {
                    RecordingEvent::Tick(tick) => {
                        RecordingStorage::append_tick(filepath, &tick)?;
                        let count = tick_count.fetch_add(1, Ordering::Relaxed) + 1;
                        if count % 5000 == 0 {
                            info!("📊 Записано событий: {}", count);
                        }
                    }
                    RecordingEvent::PriceChange(pc_tick) => {
                        RecordingStorage::append_price_change(filepath, &pc_tick)?;
                    }
                }
            }
        }

        Ok(())
    }
}
