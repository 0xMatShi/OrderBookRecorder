use serde::{Deserialize, Serialize};

fn deserialize_f64_from_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    s.parse::<f64>().map_err(D::Error::custom)
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderSummary {
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub price: f64,
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub size: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct BookMessage {
    pub event_type: String,
    pub asset_id: String,
    pub bids: Vec<OrderSummary>,
    pub asks: Vec<OrderSummary>,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub timestamp: i64,
}

fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    s.parse::<i64>().map_err(D::Error::custom)
}

#[derive(Debug, Deserialize, Clone)]
pub struct PriceChangeItem {
    pub asset_id: String,
    pub side: String,
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub price: f64,
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub size: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct PriceChangeMessage {
    pub event_type: String,
    pub price_changes: Vec<PriceChangeItem>,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct SubscribeMessage {
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub msg_type: String,
}
