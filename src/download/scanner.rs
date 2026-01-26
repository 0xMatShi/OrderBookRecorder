use crate::models::{Market, PolymarketEvent, TargetMarket};
use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

pub struct AutoScanner {
    client: Client,
    api_url: String,
}

impl AutoScanner {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_url: "https://gamma-api.polymarket.com/events".to_string(),
        }
    }

    pub async fn find_next_target(
        &self,
        target_prefix: &str,
        min_m: f64,
        max_m: f64,
    ) -> Option<TargetMarket> {
        info!(
            "🔍 Авто-поиск {} (окно: {}-{} мин)",
            target_prefix, min_m, max_m
        );

        loop {
            match self.perform_scan(target_prefix, min_m, max_m).await {
                Ok(Some(target)) => return Some(target),
                Ok(None) => (),
                Err(e) => warn!("⚠️ Ошибка при сканировании: {}", e),
            }

            sleep(Duration::from_secs(5)).await;
        }
    }

    async fn perform_scan(
        &self,
        prefix: &str,
        min_m: f64,
        max_m: f64,
    ) -> Result<Option<TargetMarket>> {
        let mut offset = 0;
        let limit = 500;

        loop {
            let response = self
                .client
                .get(&self.api_url)
                .query(&[
                    ("active", "true"),
                    ("closed", "false"),
                    ("limit", &limit.to_string()),
                    ("offset", &offset.to_string()),
                    ("order", "endDate"),
                    ("ascending", "true"),
                ])
                .send()
                .await?;

            if !response.status().is_success() {
                anyhow::bail!("API вернул ошибку: {}", response.status());
            }

            let start = std::time::Instant::now();
            let bytes = response.bytes().await?;
            let network_time = start.elapsed();

            let start = std::time::Instant::now();
            let events: Vec<PolymarketEvent> = serde_json::from_slice(&bytes)?;
            let parse_time = start.elapsed();

            if events.is_empty() {
                info!("📍 Достигнут конец списка событий. Ничего не найдено.");
                return Ok(None);
            }

            info!(
                "📡 Загружено {} событий (offset: {}). Сеть: {:?} | Парсинг: {:?}",
                events.len(),
                offset,
                network_time,
                parse_time
            );

            let target = events
                .into_iter()
                .filter(|e| e.active && e.slug.starts_with(prefix))
                .filter_map(|e| self.process_event(e, min_m, max_m))
                .next();

            if target.is_some() {
                return Ok(target);
            }

            offset += limit;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn process_event(
        &self,
        event: PolymarketEvent,
        min_m: f64,
        max_m: f64,
    ) -> Option<TargetMarket> {
        let end_dt = event.end_date.parse::<DateTime<Utc>>().ok()?;
        let minutes_left = end_dt.signed_duration_since(Utc::now()).num_seconds() as f64 / 60.0;

        if minutes_left <= min_m || minutes_left > max_m {
            return None;
        }

        let markets: Vec<Market> = serde_json::from_value(event.markets).ok()?;

        for market in markets {
            let tokens: Vec<String> = serde_json::from_str(&market.clob_token_ids).ok()?;
            if tokens.len() >= 2 {
                info!("🔎 НАЙДЕНО: {} ({:.1} мин)", event.slug, minutes_left);
                return Some(TargetMarket {
                    slug: event.slug,
                    title: event.title,
                    up_token: tokens[0].clone(),
                    down_token: tokens[1].clone(),
                    end_date: event.end_date,
                });
            }
        }

        None
    }
}
