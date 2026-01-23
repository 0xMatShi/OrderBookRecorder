use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Market {
    #[serde(rename = "clobTokenIds")]
    pub clob_token_ids: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolymarketEvent {
    pub slug: String,
    pub title: String,
    pub end_date: String,
    pub active: bool,
    pub markets: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct TargetMarket {
    pub slug: String,
    pub title: String,
    pub up_token: String,
    pub down_token: String,
    pub end_date: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coin {
    BTC,
    ETH,
    SOL,
    XRP,
}

impl Coin {
    pub fn slug_prefix(&self) -> &'static str {
        match self {
            Coin::BTC => "btc-updown-15m",
            Coin::ETH => "eth-updown-15m",
            Coin::SOL => "sol-updown-15m",
            Coin::XRP => "xrp-updown-15m",
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            1 => Some(Coin::BTC),
            2 => Some(Coin::ETH),
            3 => Some(Coin::SOL),
            4 => Some(Coin::XRP),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Coin::BTC => "BTC",
            Coin::ETH => "ETH",
            Coin::SOL => "SOL",
            Coin::XRP => "XRP",
        }
    }
}
