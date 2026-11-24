// Bybit / Gate / OKX — U本位期權 PCP 套利錄盤口程式 v2: 永續合約對沖版
// 用途：只錄盤口、做策略驗證，不下單、不接 API key。
//
// PCP (歐式, 同標的/同履約價/同到期):
//   C - P = S - K * e^(-rT) , r≈0、T短時 ≈ S - K
// 永續對沖版: 把現貨 S 換成永續價 Perp, 因為永續到期時 Perp_T ≈ S_T,
//   買合成: dev_buy_perp  = (C_ask - P_bid) - (Perp_bid - K)  < 0 代表合成便宜
//   賣合成: dev_sell_perp = (C_bid - P_ask) - (Perp_ask - K)  > 0 代表合成貴
//   到期時總損益(每 1 單位標的, 忽略結算費):
//     買合成 + 空永續: (S_T-K) - (C_ask-P_bid) + (Perp_bid - Perp_T) ≈ -dev_buy_perp
//     賣合成 + 多永續: -(S_T-K) + (C_bid-P_ask) + (Perp_T - Perp_ask) ≈ dev_sell_perp
//   資金費: 空永續每 8h 收/付 funding_rate * Perp, 到期前估算
//     funding_est_short = funding_rate * Perp_mid * (hours_to_expiry / 8)
//     買合成(空永續)淨利要 +funding_est_short, 賣合成(多永續)要 -funding_est_short
//     funding_rate 會變, 這只是用當下費率外推的估算, 不是保證值。
//   手續費(2026-11-24 查核, 一律用最高費率 VIP0/Regular、不含 VIP 折扣、不含 GT/OKB 折扣、不含 maker rebate 當作判斷標準):
//     taker 全吃單: 期權 taker*2腿 + 永續 taker*2腿 (進場+出場, 永續開+平各 1 次)
//     maker 全掛單: 期權 maker*2腿 + 永續 maker*2腿
//     混合(實際最常用): 期權 maker*2腿 + 永續 taker*2腿
//       期權盤口薄、價差寬, 吃單會把價差吃掉, 所以期權掛 maker 等成交;
//       永續盤口深、價差只有約 0.5-1 USDT, 吃單貴一點但能立刻對沖, 不掛 maker 等。
//     各所最高費率:
//       Bybit VIP0: 期權 taker 0.03% maker 0.02%, 永續 taker 0.055% maker 0.02%, 交割費 0.015% (未計入, 到期持有才收)
//       Gate VIP0 官方: 期權 taker 0.028% maker 0.02% (2026-02 調降), 永續 taker 0.05% maker 0.02%;
//         合約 API 欄位回傳永續 taker 0.075% maker -0.01%, 為求最嚴格, taker 取 0.075%, maker 取 0.02% (不算 rebate), 期權 taker 取 0.03%
//       OKX Regular: 期權 maker/taker 皆 0.03% (上限為權利金 7%), 永續 taker 0.05% maker 0.02%, 期貨交割費 0.01%
//       Binance VIP0 (www.binance.com REST): 期權 maker/taker 皆 0.024%, 永續 taker 0.04% maker 0.02%
//       UMX: 永續 VIP0 maker 0.02% taker 0.05%; 期權費率公開文件查不到, 標 TBD (fee 欄填 NaN, 只算 dev)
//       Derive (USDC 組): 期權 maker 0.01% taker 0.03% + taker 每筆 $0.50 (上限權利金 12.5%), 永續 maker 0.01% taker 0.03% + $0.01; 資金費是每小時, 不是 8 小時, funding_est = f * Perp * hours (不除以 8)
//     以 BTC 約 60600 計, 每 1 BTC 最高手續費:
//       Bybit taker 0.06%+0.11%=0.17% ≈ 103 USDT; 混合 0.04%+0.11%=0.15% ≈ 91 USDT; maker 0.04%+0.04%=0.08% ≈ 48 USDT
//       Gate taker 0.06%+0.15%=0.21% ≈ 127 USDT; 混合 0.04%+0.15%=0.19% ≈ 115 USDT; maker 0.04%+0.04%=0.08% ≈ 48 USDT
//       OKX taker 0.06%+0.10%=0.16% ≈ 97 USDT; 混合 0.06%+0.10%=0.16% ≈ 97 USDT; maker 0.06%+0.04%=0.10% ≈ 61 USDT
//     注意: OKX 期權 maker 是 0.03% 不是 0.02%, Gate 永續 maker 最高版不算 -0.01% rebate, 舊版低估了這兩項。
//
// U本位 / OKX:
//   Bybit: USDT ✓ ; Gate: USDT ✓ ; OKX: 無純 USDT 期權, 本程式固定用 USD_UM (美元本位, 報價已是 USD, 不用 *S)
//
// Bybit 在這台機器上:
//   REST (api.bybit.com / api.bytick.com / testnet) 全被 CloudFront 擋: "blocked access from your country"
//   www.bybit.com/api 也是 Access Denied.
//   但 WebSocket 看得到, 實測通了:
//     wss://stream.bybit.com/v5/public/spot    tickers.BTCUSDT -> last 60469.7
//     wss://stream.bybit.com/v5/public/linear  tickers.BTCUSDT -> bid/ask, fundingRate -0.0000146, markPrice
//     wss://stream.bybit.com/v5/public/option  publicTrade.BTC -> 有成交, 符號格式 BTC-25NOV26-60250-C-USDT (注意有 -USDT 後綴)
//     wss://stream.bybit.com/v5/public/option  orderbook.25.BTC-25NOV26-60250-C-USDT -> snapshot bid 895 / ask 905
//   所以 Bybit 這段改走 WS: 先用 publicTrade 找活躍符號, 再訂 orderbook.25 錄盤口。
//   Gate / OKX 仍走 REST (本機通)。
//
// 執行:
//   cargo run -- --base BTC --duration-sec 60 --interval-ms 1000 --strikes 5
//   cargo run -- --base BTC --duration-sec 600 --only gate,okx --output data/pcp_perp_gate_okx.csv

use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::signal::unix::{signal, SignalKind};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, pki_types::ServerName};

fn fv(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}
fn fs(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> { v.get(k) }

fn fmt_expiry_ddmmmyy(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms).single().map(|dt| dt.format("%d%b%y").to_string().to_uppercase()).unwrap_or_else(|| format!("{}", ms))
}
fn fmt_expiry_yyyymmdd(sec: i64) -> String {
    Utc.timestamp_opt(sec, 0).single().map(|dt| dt.format("%Y%m%d").to_string()).unwrap_or_else(|| format!("{}", sec))
}

#[derive(Clone, Debug)]
struct Pair {
    exchange: String,
    base: String,
    settle: String,
    expiry: String,
    expiry_ts_ms: i64,
    strike: f64,
    call_sym: String,
    put_sym: String,
    usd_priced: bool,
}

#[derive(Clone, Debug, Default)]
struct Quote { bid: f64, ask: f64, bid_sz: f64, ask_sz: f64 }
impl Quote {
    fn mid(&self) -> f64 {
        if self.bid > 0.0 && self.ask > 0.0 { (self.bid + self.ask) / 2.0 } else { 0.0 }
    }
}

#[derive(Clone, Debug, Default)]
struct Perp {
    bid: f64, ask: f64,
    funding_rate: f64, // 每 8h
    mark_price: f64,
    index_price: f64,
    next_funding_ms: i64,
}
impl Perp {
    fn mid(&self) -> f64 {
        if self.bid > 0.0 && self.ask > 0.0 { (self.bid + self.ask) / 2.0 } else if self.mark_price > 0.0 { self.mark_price } else { 0.0 }
    }
}

async fn http_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..3 {
        match client.get(url).header("User-Agent", "pcp-recorder/0.1 rust").send().await {
            Ok(r) => {
                let txt = r.text().await?;
                if txt.contains("CloudFront") || txt.contains("blocked") {
                    return Err(anyhow!("CloudFront blocked: {}", url));
                }
                let v: Value = serde_json::from_str(&txt)
                    .map_err(|e| anyhow!("JSON parse fail {}: {} | body={}", url, e, &txt[..txt.len().min(300)]))?;
                return Ok(v);
            }
            Err(e) => {
                last_err = Some(anyhow!(e));
                if attempt < 2 { tokio::time::sleep(Duration::from_millis(500)).await; }
            }
        }
    }
    let _ = last_err;
    Err(anyhow!("http failed after 3 tries: {}", url))
}

async fn http_post_json(client: &reqwest::Client, url: &str, body: &str) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..3 {
        match client.post(url).header("User-Agent", "pcp-recorder/0.1 rust").header("Content-Type", "application/json").body(body.to_string()).send().await {
            Ok(r) => {
                let txt = r.text().await?;
                if txt.contains("CloudFront") || txt.contains("blocked") {
                    return Err(anyhow!("CloudFront blocked: {}", url));
                }
                let v: Value = serde_json::from_str(&txt)
                    .map_err(|e| anyhow!("JSON parse fail {}: {} | body={}", url, e, &txt[..txt.len().min(300)]))?;
                return Ok(v);
            }
            Err(e) => {
                last_err = Some(anyhow!(e));
                if attempt < 2 { tokio::time::sleep(Duration::from_millis(500)).await; }
            }
        }
    }
    let _ = last_err;
    Err(anyhow!("http POST failed after 3 tries: {}", url))
}

// ---------- Gate ----------
async fn gate_spot(client: &reqwest::Client, base: &str) -> Result<Quote> {
    let url = format!("https://api.gateio.ws/api/v4/spot/tickers?currency_pair={}_USDT", base);
    let v = http_json(client, &url).await?;
    let first = v.get(0).ok_or_else(|| anyhow!("gate spot empty"))?;
    Ok(Quote {
        bid: fv(get(first, "highest_bid").unwrap_or(&Value::Null)).unwrap_or(0.0),
        ask: fv(get(first, "lowest_ask").unwrap_or(&Value::Null)).unwrap_or(0.0),
        bid_sz: fv(get(first, "highest_size").unwrap_or(&Value::Null)).unwrap_or(0.0),
        ask_sz: fv(get(first, "lowest_size").unwrap_or(&Value::Null)).unwrap_or(0.0),
    })
}

async fn gate_perp(client: &reqwest::Client, base: &str) -> Result<Perp> {
    // order book top1 + contract 資訊平行抓 (原本順序 2 次請求)
    let url = format!("https://api.gateio.ws/api/v4/futures/usdt/order_book?contract={}_USDT&limit=5", base);
    let url2 = format!("https://api.gateio.ws/api/v4/futures/usdt/contracts/{}_USDT", base);
    let (v, c) = tokio::join!(http_json(client, &url), http_json(client, &url2));
    let v = v?;
    let c = c?;
    let bids = v.get("bids").and_then(|x| x.as_array());
    let asks = v.get("asks").and_then(|x| x.as_array());
    let bid = bids.and_then(|a| a.first()).and_then(|r| get(r, "p")).and_then(fv).unwrap_or(0.0);
    let ask = asks.and_then(|a| a.first()).and_then(|r| get(r, "p")).and_then(fv).unwrap_or(0.0);
    // contract: funding_rate, mark_price, index_price, funding_next_apply (sec)
    Ok(Perp {
        bid, ask,
        funding_rate: fv(get(&c, "funding_rate").unwrap_or(&Value::Null)).unwrap_or(0.0),
        mark_price: fv(get(&c, "mark_price").unwrap_or(&Value::Null)).unwrap_or(0.0),
        index_price: fv(get(&c, "index_price").unwrap_or(&Value::Null)).unwrap_or(0.0),
        next_funding_ms: fv(get(&c, "funding_next_apply").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64 * 1000,
    })
}

async fn fetch_gate_pairs(client: &reqwest::Client, base: &str, n_strikes: usize, n_expiries: usize) -> Result<(Vec<Pair>, f64)> {
    let url = format!("https://api.gateio.ws/api/v4/options/tickers?underlying={}_USDT", base);
    let v = http_json(client, &url).await?;
    let arr = v.as_array().ok_or_else(|| anyhow!("gate tickers not array"))?;
    let mut index_px = 0.0;
    if let Some(f) = arr.first() {
        index_px = fv(get(f, "index_price").unwrap_or(&Value::Null)).unwrap_or(0.0);
        if index_px == 0.0 { index_px = fv(get(f, "underlying_price").unwrap_or(&Value::Null)).unwrap_or(0.0); }
    }
    let mut groups: BTreeMap<i64, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    for t in arr {
        let name = fs(get(t, "name").unwrap_or(&Value::Null));
        let parts: Vec<&str> = name.split('-').collect();
        if parts.len() != 4 { continue; }
        // 履約價可以是小數 (DOGE 0.063、SUI 0.76、ADA 0.15): 用 f64 解析、微單位當 map key, 不要 parse 成 i64 (舊版會把小數截成 0 而整檔丟掉, DOGE/ADA 完全沒錄到)
        let strike_f: f64 = parts[2].parse().unwrap_or(0.0);
        if strike_f <= 0.0 { continue; }
        let exp = fv(get(t, "expiration_time").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
        let entry = groups.entry(exp).or_default().entry((strike_f * 1_000_000.0).round() as i64).or_default();
        if parts[3] == "C" { entry.0 = name.clone(); } else if parts[3] == "P" { entry.1 = name.clone(); }
    }
    let now = Utc::now().timestamp();
    let mut pairs = vec![];
    for (&exp, strikes) in groups.iter().filter(|(&e, _)| e > now + 3600).take(n_expiries) {
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (&strike_key, (c, p)) in strikes {
            if c.is_empty() || p.is_empty() { continue; }
            let strike = strike_key as f64 / 1_000_000.0;
            cand.push(((strike - index_px).abs(), strike, c.clone(), p.clone()));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        for (_, strike, c, p) in cand {
            pairs.push(Pair { exchange: "gate".into(), base: base.to_string(), settle: "USDT".into(), expiry: format!("{}", exp), expiry_ts_ms: exp * 1000, strike, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok((pairs, index_px))
}

async fn gate_ticker_map(client: &reqwest::Client, base: &str) -> Result<BTreeMap<String, Quote>> {
    let url = format!("https://api.gateio.ws/api/v4/options/tickers?underlying={}_USDT", base);
    let v = http_json(client, &url).await?;
    let mut m = BTreeMap::new();
    if let Some(arr) = v.as_array() {
        for t in arr {
            let name = fs(get(t, "name").unwrap_or(&Value::Null));
            let q = Quote {
                bid: fv(get(t, "bid1_price").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask: fv(get(t, "ask1_price").unwrap_or(&Value::Null)).unwrap_or(0.0),
                bid_sz: fv(get(t, "bid1_size").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask_sz: fv(get(t, "ask1_size").unwrap_or(&Value::Null)).unwrap_or(0.0),
            };
            m.insert(name, q);
        }
    }
    Ok(m)
}

// ---------- OKX (固定 USD_UM) ----------
async fn okx_spot(client: &reqwest::Client, base: &str) -> Result<Quote> {
    let url = format!("https://www.okx.com/api/v5/market/ticker?instId={}-USDT", base);
    let v = http_json(client, &url).await?;
    let d = v.get("data").and_then(|x| x.get(0)).ok_or_else(|| anyhow!("okx spot empty"))?;
    Ok(Quote {
        bid: fv(get(d, "bidPx").unwrap_or(&Value::Null)).unwrap_or(0.0),
        ask: fv(get(d, "askPx").unwrap_or(&Value::Null)).unwrap_or(0.0),
        bid_sz: fv(get(d, "bidSz").unwrap_or(&Value::Null)).unwrap_or(0.0),
        ask_sz: fv(get(d, "askSz").unwrap_or(&Value::Null)).unwrap_or(0.0),
    })
}

async fn okx_perp(client: &reqwest::Client, base: &str) -> Result<Perp> {
    let inst = format!("{}-USDT-SWAP", base);
    let url = format!("https://www.okx.com/api/v5/market/books?instId={}&sz=5", inst);
    let url2 = format!("https://www.okx.com/api/v5/public/funding-rate?instId={}", inst);
    let (v, f) = tokio::join!(http_json(client, &url), http_json(client, &url2));
    let v = v?;
    let f = f?;
    let d = v.get("data").and_then(|x| x.get(0)).ok_or_else(|| anyhow!("okx perp book empty"))?;
    let bids = d.get("bids").and_then(|x| x.as_array());
    let asks = d.get("asks").and_then(|x| x.as_array());
    let bid = bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let ask = asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let fd = f.get("data").and_then(|x| x.get(0));
    Ok(Perp {
        bid, ask,
        funding_rate: fd.and_then(|x| get(x, "fundingRate")).and_then(fv).unwrap_or(0.0),
        mark_price: 0.0,
        index_price: 0.0,
        next_funding_ms: fd.and_then(|x| get(x, "nextFundingTime")).map(|x| fs(x).parse().unwrap_or(0)).unwrap_or(0),
    })
}

async fn fetch_okx_pairs(client: &reqwest::Client, base: &str, n_strikes: usize, n_expiries: usize, spot_mid: f64) -> Result<Vec<Pair>> {
    let url = format!("https://www.okx.com/api/v5/public/instruments?instType=OPTION&uly={}-USD", base);
    let v = http_json(client, &url).await?;
    let arr = v.get("data").and_then(|x| x.as_array()).ok_or_else(|| anyhow!("okx instruments empty"))?;
    // 只取 USD_UM (美元本位, 報價已是 USD), 幣本位 BTC-USD 不混算
    let mut groups: BTreeMap<i64, BTreeMap<String, (String, String)>> = BTreeMap::new();
    for it in arr {
        if fs(get(it, "state").unwrap_or(&Value::Null)) != "live" { continue; }
        let family = fs(get(it, "instFamily").unwrap_or(&Value::Null));
        if !family.ends_with("_UM") { continue; }
        let exp: i64 = fs(get(it, "expTime").unwrap_or(&Value::Null)).parse().unwrap_or(0);
        let stk = fs(get(it, "stk").unwrap_or(&Value::Null));
        let sym = fs(get(it, "instId").unwrap_or(&Value::Null));
        let opt = fs(get(it, "optType").unwrap_or(&Value::Null));
        let e = groups.entry(exp).or_default().entry(stk).or_default();
        if opt == "C" { e.0 = sym; } else if opt == "P" { e.1 = sym; }
    }
    let now_ms = Utc::now().timestamp_millis();
    let mut pairs = vec![];
    for (&exp, strikes) in groups.iter().filter(|(&e, _)| e > now_ms + 3_600_000).take(n_expiries) {
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (stk_s, (c, p)) in strikes {
            if c.is_empty() || p.is_empty() { continue; }
            let k: f64 = stk_s.parse().unwrap_or(0.0);
            cand.push(((k - spot_mid).abs(), k, c.clone(), p.clone()));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        for (_, k, c, p) in cand {
            pairs.push(Pair { exchange: "okx".into(), base: base.to_string(), settle: "USD_UM".into(), expiry: format!("{}", exp), expiry_ts_ms: exp, strike: k, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok(pairs)
}

async fn okx_book(client: &reqwest::Client, inst_id: &str) -> Result<Quote> {
    let url = format!("https://www.okx.com/api/v5/market/books?instId={}&sz=5", inst_id);
    let v = http_json(client, &url).await?;
    let d = v.get("data").and_then(|x| x.get(0)).ok_or_else(|| anyhow!("okx book empty {}", inst_id))?;
    let bids = d.get("bids").and_then(|x| x.as_array());
    let asks = d.get("asks").and_then(|x| x.as_array());
    let bid = bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let ask = asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    Ok(Quote { bid, ask, bid_sz: 0.0, ask_sz: 0.0 })
}

async fn okx_ticker_map(client: &reqwest::Client, base: &str) -> Result<BTreeMap<String, Quote>> {
    // 批次 ticker 一次拿該標的全部期權的 best bid/ask (BTC-USD 家族共 1708 檔, 其中 USD_UM 792 檔)。
    // 常駐版必用: 多到期日逐檔查 books 一輪要上百次請求, 跑不動; 批次只要 1 次。
    // 注意: 流動性差的遠價外檔 bidPx/askPx 會回空字串, fv 會轉成 0.0, 呼叫端用 mid()==0 跳過。
    let url = format!("https://www.okx.com/api/v5/market/tickers?instType=OPTION&uly={}-USD", base);
    let v = http_json(client, &url).await?;
    let mut m = BTreeMap::new();
    if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
        for t in arr {
            let inst = fs(get(t, "instId").unwrap_or(&Value::Null));
            if !inst.contains("_UM-") { continue; }
            m.insert(inst, Quote {
                bid: fv(get(t, "bidPx").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask: fv(get(t, "askPx").unwrap_or(&Value::Null)).unwrap_or(0.0),
                bid_sz: fv(get(t, "bidSz").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask_sz: fv(get(t, "askSz").unwrap_or(&Value::Null)).unwrap_or(0.0),
            });
        }
    }
    Ok(m)
}

// ---------- Bybit WebSocket ----------
type WsTlsStream = tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

/// 經 egress proxy CONNECT 穿透再做 TLS + WS 握手。
/// 這台機器上網要走 hatch-egress-proxy:3128 (HTTPS_PROXY, 帶帳密),
/// tokio-tungstenite 的 connect_async 不吃 proxy env, 直接連會被擋 / 收到 corrupt message。
/// 台灣本機沒有 proxy 時, 這支函式會直接連 host:443 再 TLS, 同樣能用。
async fn connect_ws(url: &str) -> Result<WsTlsStream> {
    let stripped = url.strip_prefix("wss://").ok_or_else(|| anyhow!("only wss supported: {}", url))?;
    let (host_port, _) = stripped.split_once('/').unwrap_or((stripped, ""));
    let (host, port_str) = host_port.split_once(':').unwrap_or((host_port, "443"));
    let port: u16 = port_str.parse().unwrap_or(443);
    let host = host.to_string();
    let proxy_url = env::var("HTTPS_PROXY")
        .or_else(|_| env::var("https_proxy"))
        .or_else(|_| env::var("ALL_PROXY"))
        .or_else(|_| env::var("all_proxy"))
        .ok();
    let tcp = if let Some(proxy) = proxy_url {
        let no_scheme = proxy
            .strip_prefix("http://")
            .or_else(|| proxy.strip_prefix("https://"))
            .unwrap_or(&proxy)
            .to_string();
        let (auth, hostport) = match no_scheme.split_once('@') {
            Some((a, h)) => (Some(a.to_string()), h.to_string()),
            None => (None, no_scheme.clone()),
        };
        let (proxy_host, proxy_port_str) = hostport.split_once(':').unwrap_or((hostport.as_str(), "3128"));
        let proxy_port: u16 = proxy_port_str.trim_end_matches('/').parse().unwrap_or(3128);
        let mut s = TcpStream::connect((proxy_host, proxy_port))
            .await
            .map_err(|e| anyhow!("proxy connect {}:{} fail: {}", proxy_host, proxy_port, e))?;
        let mut req = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\nProxy-Connection: Keep-Alive\r\n",
            host, port, host, port
        );
        if let Some(a) = auth {
            req.push_str(&format!("Proxy-Authorization: Basic {}\r\n", STANDARD.encode(a.as_bytes())));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).await?;
        // 一個 byte 一個 byte 讀, 直到 \r\n\r\n, 避免 BufReader 多讀掉 TLS 資料
        let mut buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = s.read(&mut byte).await?;
            if n == 0 {
                return Err(anyhow!("proxy closed during CONNECT"));
            }
            buf.push(byte[0]);
            if buf.ends_with(b"\r\n\r\n") {
                break;
            }
            if buf.len() > 8192 {
                return Err(anyhow!("proxy response too large"));
            }
        }
        let resp = String::from_utf8_lossy(&buf).to_string();
        if !resp.contains(" 200 ") {
            return Err(anyhow!("proxy CONNECT failed: {}", resp.trim()));
        }
        s
    } else {
        TcpStream::connect((host.as_str(), port)).await?
    };
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // egress proxy 會做 TLS 攔截, 伺服器憑證是由 /run/hatch/egress-tls/ca-bundle.pem 簽的,
    // 只用 Mozilla roots 會報 UnknownIssuer, 所以再加系統 / SSL_CERT_FILE 的憑證。
    {
        let result = rustls_native_certs::load_native_certs();
        for c in result.certs {
            let _ = roots.add(c);
        }
    }
    if let Ok(path) = env::var("SSL_CERT_FILE") {
        if let Ok(pem) = std::fs::read(&path) {
            let mut reader = std::io::BufReader::new(&pem[..]);
            for cert in rustls_pemfile::certs(&mut reader) {
                if let Ok(c) = cert {
                    let _ = roots.add(c);
                }
            }
        }
    }
    let config = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(host.clone())
        .map_err(|e| anyhow!("invalid server name {}: {}", host, e))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| anyhow!("TLS to {} fail: {}", host, e))?;
    // 手動 WS 握手: tungstenite 0.26 會嚴格要求 Connection 頭等於 "Upgrade",
    // 但 Bybit 經 CloudFront 回的是 "Connection: upgrade, close", 會被判 No "Connection: upgrade" header。
    // 所以自己發 GET + 讀 101, 只要 Upgrade 是 websocket、Connection 含 upgrade 就算通,
    // 再用 from_raw_socket 包成 WebSocketStream。
    let path = format!("/{}", stripped.split_once('/').map(|(_, p)| p).unwrap_or(""));
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
    let mut key_bytes = [0u8; 16];
    key_bytes[..8].copy_from_slice(&nanos.to_le_bytes());
    key_bytes[8..].copy_from_slice(&(nanos ^ 0x9e3779b97f4a7c15).to_le_bytes());
    let key = STANDARD.encode(key_bytes);
    let hs = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\nOrigin: https://www.bybit.com\r\nUser-Agent: Mozilla/5.0\r\n\r\n",
        path, host, key
    );
    let mut tls = tls;
    tls.write_all(hs.as_bytes()).await?;
    let mut resp: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = tls.read(&mut byte).await?;
        if n == 0 {
            return Err(anyhow!("ws handshake closed"));
        }
        resp.push(byte[0]);
        if resp.ends_with(b"\r\n\r\n") {
            break;
        }
        if resp.len() > 8192 {
            return Err(anyhow!("ws handshake response too large"));
        }
    }
    let resp_s = String::from_utf8_lossy(&resp).to_string();
    let lower = resp_s.to_lowercase();
    if !resp_s.contains("101") || !lower.contains("upgrade: websocket") || !lower.contains("connection:") || !lower.contains("upgrade") {
        return Err(anyhow!("ws handshake fail: {}", resp_s.trim()));
    }
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(tls, Role::Client, None).await;
    Ok(ws)
}

#[derive(Default, Clone)]
struct BybitState {
    perps: BTreeMap<String, Perp>,
    // symbol -> (bids map price*100 -> size, asks map)
    books: BTreeMap<String, (BTreeMap<i64, f64>, BTreeMap<i64, f64>)>,
    // 最後更新時間 (毫秒 epoch): WS 斷線後資料會停在舊值, 超過 60 秒沒更新就視為過期、跳過不錄
    perp_updated_ms: BTreeMap<String, i64>,
    book_updated_ms: BTreeMap<String, i64>,
}
impl BybitState {
    fn top_quote(&self, sym: &str) -> Quote {
        if let Some((bids, asks)) = self.books.get(sym) {
            let bid = bids.iter().next_back().map(|(p, _)| *p as f64 / 100.0).unwrap_or(0.0);
            let ask = asks.iter().next().map(|(p, _)| *p as f64 / 100.0).unwrap_or(0.0);
            let bid_sz = bids.iter().next_back().map(|(_, s)| *s).unwrap_or(0.0);
            let ask_sz = asks.iter().next().map(|(_, s)| *s).unwrap_or(0.0);
            Quote { bid, ask, bid_sz, ask_sz }
        } else {
            Quote::default()
        }
    }
}

fn parse_bybit_expiry_ms(exp: &str) -> i64 {
    // "25NOV26" -> 2026-11-25 08:00 UTC
    if exp.len() != 7 { return 0; }
    let day: u32 = exp[0..2].parse().unwrap_or(0);
    let mon_s = &exp[2..5];
    let mon: u32 = match mon_s {
        "JAN" => 1, "FEB" => 2, "MAR" => 3, "APR" => 4, "MAY" => 5, "JUN" => 6,
        "JUL" => 7, "AUG" => 8, "SEP" => 9, "OCT" => 10, "NOV" => 11, "DEC" => 12,
        _ => 0,
    };
    let year: i32 = 2000 + exp[5..7].parse::<i32>().unwrap_or(0);
    if day == 0 || mon == 0 { return 0; }
    let dt = chrono::NaiveDate::from_ymd_opt(year, mon, day)
        .and_then(|d| d.and_hms_opt(8, 0, 0))
        .map(|ndt| Utc.from_utc_datetime(&ndt));
    dt.map(|d| d.timestamp_millis()).unwrap_or(0)
}

async fn bybit_discover_pairs(base: &str, n_strikes: usize, n_expiries: usize, ref_price: f64) -> Result<Vec<Pair>> {
    let url = "wss://stream.bybit.com/v5/public/option";
    let ws = connect_ws(url).await.map_err(|e| anyhow!("bybit ws connect fail: {}", e))?;
    let (mut write, mut read) = ws.split();
    write.send(Message::Text(format!("{{\"op\":\"subscribe\",\"args\":[\"publicTrade.{}\"]}}", base).into())).await?;
    let mut symbols: BTreeMap<String, ()> = BTreeMap::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        let msg = match tokio::time::timeout(Duration::from_secs(8), read.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        if let Message::Text(txt) = msg {
            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
                    for item in arr {
                        let s = fs(get(item, "s").unwrap_or(&Value::Null));
                        if !s.is_empty() { symbols.insert(s, ()); }
                    }
                }
            }
        }
        if symbols.len() >= 20 { break; }
    }
    let _ = write.close().await;
    // group: expiry -> strike -> (C, P), symbol BTC-25NOV26-60250-C-USDT
    let mut groups: BTreeMap<String, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    for sym in symbols.keys() {
        let parts: Vec<&str> = sym.split('-').collect();
        if parts.len() != 5 { continue; }
        if parts[0] != base { continue; }
        let exp_s = parts[1].to_string();
        let strike: i64 = parts[2].parse().unwrap_or(0);
        if strike == 0 { continue; }
        let e = groups.entry(exp_s).or_default().entry(strike).or_default();
        if parts[3] == "C" { e.0 = sym.clone(); } else if parts[3] == "P" { e.1 = sym.clone(); }
    }
    let now_ms = Utc::now().timestamp_millis();
    // 取最近的 N 個未來到期日。publicTrade 只會推有成交的符號, 常常只有 C 沒有 P,
    // 不能要求 C/P 都在成交裡出現過, 所以見到某到期日/履約價就直接拼出 C/P 再訂 orderbook 驗證。
    // 若 8 秒內完全沒成交, 直接回空、讓呼叫端跳過該標的, 不要憑空拼履約價
    // (舊版退回用 ref_price 生成, ref 不可靠時會拼出負履約價 BTC-25NOV26--500-C-USDT)。
    let mut future: Vec<(i64, String, BTreeMap<i64, (String, String)>)> = vec![];
    for (exp_s, strikes) in groups {
        let exp_ms = parse_bybit_expiry_ms(&exp_s);
        if exp_ms > now_ms {
            future.push((exp_ms, exp_s, strikes));
        }
    }
    future.sort_by_key(|(ms, _, _)| *ms);
    let mut pairs = vec![];
    for (exp_ms, exp_s, strikes) in future.into_iter().take(n_expiries) {
        // ref_price 是呼叫端給的 (通常是 Gate 指數)。每到期日各別對中位數:
        // 若 ref_price 離觀測到的履約價中位數太遠 (>50%), 改用中位數,
        // 否則會選到遠離市價的履約價 (例如 SOL 誤用 BTC 的 60400 當參考)。
        let mut ref_price = ref_price;
        {
            let mut all: Vec<f64> = strikes.keys().map(|k| *k as f64).collect();
            if !all.is_empty() {
                all.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let median = all[all.len() / 2];
                if ref_price <= 0.0 || (ref_price - median).abs() / median > 0.5 {
                    ref_price = median;
                }
            }
        }
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (strike, (_c, _p)) in &strikes {
            let k = *strike as f64;
            let c = format!("{}-{}-{}-C-USDT", base, exp_s, strike);
            let p = format!("{}-{}-{}-P-USDT", base, exp_s, strike);
            cand.push(((k - ref_price).abs(), k, c, p));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        for (_, k, c, p) in cand {
            pairs.push(Pair { exchange: "bybit".into(), base: base.to_string(), settle: "USDT".into(), expiry: exp_s.clone(), expiry_ts_ms: exp_ms, strike: k, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok(pairs)
}

async fn bybit_linear_task(state: Arc<Mutex<BybitState>>, base: String) {
    // 常駐版: WS 斷線自動重連 (舊版斷線就直接 return, 之後永續價會一直停在舊值, 錄到的全是過期資料)
    let url = "wss://stream.bybit.com/v5/public/linear";
    loop {
        match connect_ws(url).await {
            Ok(ws) => {
                let (mut write, mut read) = ws.split();
                let _ = write.send(Message::Text(format!("{{\"op\":\"subscribe\",\"args\":[\"tickers.{}USDT\"]}}", base).into())).await;
                let mut ping = tokio::time::interval(Duration::from_secs(20));
                loop {
                    tokio::select! {
                        _ = ping.tick() => { if write.send(Message::Text("{\"op\":\"ping\"}".into())).await.is_err() { break; } }
                        msg = read.next() => {
                            let Some(Ok(Message::Text(txt))) = msg else { break; };
                            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                if let Some(d) = v.get("data") {
                                    let now_ms = Utc::now().timestamp_millis();
                                    let mut st = state.lock().await;
                                    let perp = st.perps.entry(base.clone()).or_default();
                                    if let Some(x) = fv(get(d, "bid1Price").unwrap_or(&Value::Null)) { perp.bid = x; }
                                    if let Some(x) = fv(get(d, "ask1Price").unwrap_or(&Value::Null)) { perp.ask = x; }
                                    if let Some(x) = fv(get(d, "fundingRate").unwrap_or(&Value::Null)) { perp.funding_rate = x; }
                                    if let Some(x) = fv(get(d, "markPrice").unwrap_or(&Value::Null)) { perp.mark_price = x; }
                                    if let Some(x) = fv(get(d, "indexPrice").unwrap_or(&Value::Null)) { perp.index_price = x; }
                                    if let Some(x) = fv(get(d, "nextFundingTime").unwrap_or(&Value::Null)) { perp.next_funding_ms = x as i64; }
                                    st.perp_updated_ms.insert(base.clone(), now_ms);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => { eprintln!("[bybit linear {}] connect fail: {} (2s 後重連)", base, e); }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

type BybitWsWrite = futures_util::stream::SplitSink<WsTlsStream, Message>;

async fn bybit_subscribe(write: &mut BybitWsWrite, pairs: &[Pair]) -> Vec<String> {
    let mut syms: Vec<String> = vec![];
    let mut args: Vec<String> = vec![];
    for pr in pairs {
        args.push(format!("orderbook.25.{}", pr.call_sym));
        args.push(format!("orderbook.25.{}", pr.put_sym));
        syms.push(pr.call_sym.clone());
        syms.push(pr.put_sym.clone());
    }
    // 一次訂閱最多 10 個 topic, 分批送 (多標的時 topic 數會超過 10)
    for chunk in args.chunks(10) {
        let sub = serde_json::json!({"op":"subscribe","args":chunk}).to_string();
        let _ = write.send(Message::Text(sub.into())).await;
    }
    syms
}

async fn bybit_option_task(state: Arc<Mutex<BybitState>>, shared: Arc<Mutex<Discovery>>) {
    // 常駐版: WS 斷線自動重連; 每 30 秒對一次 shared 的 pairs, 到期滾動後自動退訂舊到期日、訂閱新到期日。
    let url = "wss://stream.bybit.com/v5/public/option";
    let mut subscribed: Vec<String> = vec![];
    loop {
        match connect_ws(url).await {
            Ok(ws) => {
                let (mut write, mut read) = ws.split();
                {
                    let pairs = shared.lock().await.bybit_pairs.clone();
                    subscribed = bybit_subscribe(&mut write, &pairs).await;
                }
                let mut ping = tokio::time::interval(Duration::from_secs(20));
                let mut check = tokio::time::interval(Duration::from_secs(30));
                loop {
                    tokio::select! {
                        _ = ping.tick() => { if write.send(Message::Text("{\"op\":\"ping\"}".into())).await.is_err() { break; } }
                        _ = check.tick() => {
                            let cur = shared.lock().await.bybit_pairs.clone();
                            let cur_syms: Vec<String> = cur.iter().flat_map(|p| [p.call_sym.clone(), p.put_sym.clone()]).collect();
                            if cur_syms != subscribed {
                                let old_topics: Vec<String> = subscribed.iter().map(|s| format!("orderbook.25.{}", s)).collect();
                                if !old_topics.is_empty() {
                                    for chunk in old_topics.chunks(10) {
                                        let un = serde_json::json!({"op":"unsubscribe","args":chunk}).to_string();
                                        let _ = write.send(Message::Text(un.into())).await;
                                    }
                                }
                                subscribed = bybit_subscribe(&mut write, &cur).await;
                                let mut st = state.lock().await;
                                st.books.retain(|k, _| subscribed.contains(k));
                                st.book_updated_ms.retain(|k, _| subscribed.contains(k));
                                println!("[bybit option] 重新訂閱 {} 個符號", subscribed.len());
                            }
                        }
                        msg = read.next() => {
                            let Some(Ok(Message::Text(txt))) = msg else { break; };
                            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                let topic = fs(get(&v, "topic").unwrap_or(&Value::Null));
                                let typ = fs(get(&v, "type").unwrap_or(&Value::Null));
                                if !topic.starts_with("orderbook.") { continue; }
                                if let Some(d) = v.get("data") {
                                    let sym = fs(get(d, "s").unwrap_or(&Value::Null));
                                    if sym.is_empty() { continue; }
                                    let now_ms = Utc::now().timestamp_millis();
                                    let mut st = state.lock().await;
                                    let entry = st.books.entry(sym.clone()).or_default();
                                    let apply = |map: &mut BTreeMap<i64, f64>, arr: Option<&Vec<Value>>| {
                                        if let Some(a) = arr {
                                            for row in a {
                                                if let (Some(p), Some(s)) = (row.get(0).and_then(fv), row.get(1).and_then(fv)) {
                                                    let key = (p * 100.0) as i64;
                                                    if s == 0.0 { map.remove(&key); } else { map.insert(key, s); }
                                                }
                                            }
                                        }
                                    };
                                    if typ == "snapshot" {
                                        entry.0.clear(); entry.1.clear();
                                    }
                                    apply(&mut entry.0, d.get("b").and_then(|x| x.as_array()));
                                    apply(&mut entry.1, d.get("a").and_then(|x| x.as_array()));
                                    st.book_updated_ms.insert(sym, now_ms);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => { eprintln!("[bybit option] connect fail: {} (2s 後重連)", e); }
        }
        subscribed.clear();
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ---------- Binance (www.binance.com REST, USDT 組) ----------
// api/fapi/eapi 主機全被地區擋, www.binance.com 走不同通道、通, 路徑不變, 不用 WS。
// exchangeInfo: optionSymbols [{symbol, underlying "BTCUSDT", expiryDate(ms), strikePrice, side "CALL"/"PUT", quoteAsset "USDT"}]
// depth limit 必須用 10 (limit=5 回 -4021)。bids/asks 是 [價格, 數量] 字串二元組; 空陣列或 "0" = 缺邊。
// 權重: depth 一次扣 2, 一分鐘預算 6000。
async fn binance_index(client: &reqwest::Client, base: &str) -> Result<f64> {
    let url = format!("https://www.binance.com/eapi/v1/index?underlying={}USDT", base);
    let v = http_json(client, &url).await?;
    Ok(fv(get(&v, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(0.0))
}

async fn fetch_binance_pairs(client: &reqwest::Client, base: &str, n_strikes: usize, n_expiries: usize) -> Result<(Vec<Pair>, f64)> {
    let index_px = binance_index(client, base).await?;
    let v = http_json(client, "https://www.binance.com/eapi/v1/exchangeInfo").await?;
    let arr = v.get("optionSymbols").and_then(|x| x.as_array()).ok_or_else(|| anyhow!("binance exchangeInfo: no optionSymbols"))?;
    let mut groups: BTreeMap<i64, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    for it in arr {
        if fs(get(it, "underlying").unwrap_or(&Value::Null)) != format!("{}USDT", base) { continue; }
        if fs(get(it, "quoteAsset").unwrap_or(&Value::Null)) != "USDT" { continue; }
        let exp: i64 = fv(get(it, "expiryDate").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
        // 履約價可以是小數 (XRP 1.1、DOGE 0.055): 用 f64 解析、微單位當 map key, 不要 as i64 截斷 (舊版 XRP 1.1 變 1、DOGE 整檔丟掉)
        let strike_f: f64 = fv(get(it, "strikePrice").unwrap_or(&Value::Null)).unwrap_or(0.0);
        if exp == 0 || strike_f <= 0.0 { continue; }
        let sym = fs(get(it, "symbol").unwrap_or(&Value::Null));
        let e = groups.entry(exp).or_default().entry((strike_f * 1_000_000.0).round() as i64).or_default();
        match fs(get(it, "side").unwrap_or(&Value::Null)).as_str() {
            "CALL" => e.0 = sym,
            "PUT" => e.1 = sym,
            _ => {}
        }
    }
    let now_ms = Utc::now().timestamp_millis();
    let mut pairs = vec![];
    for (&exp, strikes) in groups.iter().filter(|(&e, _)| e > now_ms + 3_600_000).take(n_expiries) {
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (&strike_key, (c, p)) in strikes {
            if c.is_empty() || p.is_empty() { continue; }
            let strike = strike_key as f64 / 1_000_000.0;
            cand.push(((strike - index_px).abs(), strike, c.clone(), p.clone()));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        for (_, k, c, p) in cand {
            pairs.push(Pair { exchange: "binance".into(), base: base.to_string(), settle: "USDT".into(), expiry: fmt_expiry_ddmmmyy(exp), expiry_ts_ms: exp, strike: k, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok((pairs, index_px))
}

async fn binance_ticker_map(client: &reqwest::Client) -> Result<BTreeMap<String, Quote>> {
    // 一次拿全部期權 ticker 的 best bid/ask (top-of-book, 與 depth 的 top1 相同),
    // 比逐檔 depth 省權重, 多標的時必用 (8 個標的逐檔查一輪要 80 次請求)。
    // 注意: 批次 ticker 沒有 bidQty/askQty, 量欄位填 0。
    let v = http_json(client, "https://www.binance.com/eapi/v1/ticker").await?;
    let mut m = BTreeMap::new();
    if let Some(arr) = v.as_array() {
        for t in arr {
            let sym = fs(get(t, "symbol").unwrap_or(&Value::Null));
            m.insert(sym, Quote {
                bid: fv(get(t, "bidPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask: fv(get(t, "askPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
                bid_sz: 0.0,
                ask_sz: 0.0,
            });
        }
    }
    Ok(m)
}

async fn binance_spot(client: &reqwest::Client, base: &str) -> Result<Quote> {
    let url = format!("https://www.binance.com/api/v3/depth?symbol={}USDT&limit=5", base);
    let v = http_json(client, &url).await?;
    let bids = v.get("bids").and_then(|x| x.as_array());
    let asks = v.get("asks").and_then(|x| x.as_array());
    Ok(Quote {
        bid: bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0),
        ask: asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0),
        bid_sz: bids.and_then(|a| a.first()).and_then(|r| r.get(1)).and_then(fv).unwrap_or(0.0),
        ask_sz: asks.and_then(|a| a.first()).and_then(|r| r.get(1)).and_then(fv).unwrap_or(0.0),
    })
}

async fn binance_perp(client: &reqwest::Client, base: &str) -> Result<Perp> {
    let url = format!("https://www.binance.com/fapi/v1/depth?symbol={}USDT&limit=5", base);
    let url2 = format!("https://www.binance.com/fapi/v1/premiumIndex?symbol={}USDT", base);
    let (v, f) = tokio::join!(http_json(client, &url), http_json(client, &url2));
    let v = v?;
    let f = f?;
    let bids = v.get("bids").and_then(|x| x.as_array());
    let asks = v.get("asks").and_then(|x| x.as_array());
    let bid = bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let ask = asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    Ok(Perp {
        bid, ask,
        funding_rate: fv(get(&f, "lastFundingRate").unwrap_or(&Value::Null)).unwrap_or(0.0),
        mark_price: fv(get(&f, "markPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
        index_price: fv(get(&f, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
        next_funding_ms: fv(get(&f, "nextFundingTime").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64,
    })
}

// ---------- UMX (api.umx.com, USDT 組) ----------
// symbols 一次回約 2MB, 啟動時抓一次就好, 不要每 tick 重抓。
// 符號以回傳為準 (BTC-USDT-24NOV26-60000-C, 有連字號), 不要自己拼。
// 期權費率公開文件查不到, opt fee 用 NaN, 只算 dev, fee/net 欄位會是 NaN。
// 錄最近 3 個到期日 (不只最近一日): 今天 11-24 到期只剩約 1 小時、盤口最薄, 11-25/26/27 的單比較多。
async fn umx_ticker_row(client: &reqwest::Client, base: &str) -> Result<Value> {
    let url = format!("https://api.umx.com/api/v1/market/ticker/24hr?symbol={}-USDT-PERP&businessType=linear_perpetual", base);
    let t = http_json(client, &url).await?;
    let row = t.get("data").and_then(|x| x.get(0)).ok_or_else(|| anyhow!("umx perp ticker empty"))?;
    Ok(row.clone())
}

async fn umx_perp(client: &reqwest::Client, base: &str) -> Result<Perp> {
    let url = format!("https://api.umx.com/api/v1/market/depth?symbol={}-USDT-PERP&limit=5", base);
    let (v, row) = tokio::join!(http_json(client, &url), umx_ticker_row(client, base));
    let v = v?;
    let row = row?;
    let d = v.get("data").ok_or_else(|| anyhow!("umx perp depth no data"))?;
    let bids = d.get("bids").and_then(|x| x.as_array());
    let asks = d.get("asks").and_then(|x| x.as_array());
    let bid = bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let ask = asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0);
    let to_next_ms = fv(get(&row, "toNextFundRateTime").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
    Ok(Perp {
        bid, ask,
        funding_rate: fv(get(&row, "fundingRate").unwrap_or(&Value::Null)).unwrap_or(0.0),
        mark_price: fv(get(&row, "markPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
        index_price: fv(get(&row, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(0.0),
        next_funding_ms: if to_next_ms > 0 { Utc::now().timestamp_millis() + to_next_ms } else { 0 },
    })
}

async fn umx_depth(client: &reqwest::Client, sym: &str) -> Result<Quote> {
    let url = format!("https://api.umx.com/api/v1/market/depth?symbol={}&limit=5", sym);
    let v = http_json(client, &url).await?;
    if fs(get(&v, "code").unwrap_or(&Value::Null)) != "0" {
        return Ok(Quote::default());
    }
    let d = match v.get("data") { Some(d) => d, None => return Ok(Quote::default()) };
    let bids = d.get("bids").and_then(|x| x.as_array());
    let asks = d.get("asks").and_then(|x| x.as_array());
    Ok(Quote {
        bid: bids.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0),
        ask: asks.and_then(|a| a.first()).and_then(|r| r.get(0)).and_then(fv).unwrap_or(0.0),
        bid_sz: bids.and_then(|a| a.first()).and_then(|r| r.get(1)).and_then(fv).unwrap_or(0.0),
        ask_sz: asks.and_then(|a| a.first()).and_then(|r| r.get(1)).and_then(fv).unwrap_or(0.0),
    })
}

async fn fetch_umx_symbols(client: &reqwest::Client) -> Result<Value> {
    http_json(client, "https://api.umx.com/api/v2/public/symbols").await
}

async fn fetch_umx_pairs(client: &reqwest::Client, base: &str, n_strikes: usize, n_expiries: usize, symbols: &Value) -> Result<(Vec<Pair>, f64)> {
    // 履約價選擇只需要指數價, 直接用 ticker (setup 時永續 depth 偶爾失敗會拖垮整所, 這裡不依賴它)
    let row = umx_ticker_row(client, base).await?;
    let index_px = fv(get(&row, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(0.0);
    let arr = symbols.get("data").and_then(|x| x.as_array()).ok_or_else(|| anyhow!("umx symbols no data"))?;
    let mut groups: BTreeMap<i64, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    for it in arr {
        if fs(get(it, "businessType").unwrap_or(&Value::Null)) != "options" { continue; }
        if fs(get(it, "settleCurrency").unwrap_or(&Value::Null)) != "USDT" { continue; }
        if fs(get(it, "status").unwrap_or(&Value::Null)) != "trading" { continue; }
        if fs(get(it, "symbolFamily").unwrap_or(&Value::Null)) != format!("{}-USDT", base) { continue; }
        let exp: i64 = fs(get(it, "deliveryTime").unwrap_or(&Value::Null)).parse().unwrap_or(0);
        let strike: i64 = fs(get(it, "strikePrice").unwrap_or(&Value::Null)).parse().unwrap_or(0);
        if exp == 0 || strike == 0 { continue; }
        let sym = fs(get(it, "symbol").unwrap_or(&Value::Null));
        let e = groups.entry(exp).or_default().entry(strike).or_default();
        match fs(get(it, "optType").unwrap_or(&Value::Null)).as_str() {
            "call" => e.0 = sym,
            "put" => e.1 = sym,
            _ => {}
        }
    }
    let now_ms = Utc::now().timestamp_millis();
    let mut pairs = vec![];
    for (&exp, strikes) in groups.iter().filter(|(&e, _)| e > now_ms + 3_600_000).take(n_expiries) {
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (&strike, (c, p)) in strikes {
            if c.is_empty() || p.is_empty() { continue; }
            cand.push(((strike as f64 - index_px).abs(), strike as f64, c.clone(), p.clone()));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        for (_, k, c, p) in cand {
            pairs.push(Pair { exchange: "umx".into(), base: base.to_string(), settle: "USDT".into(), expiry: fmt_expiry_ddmmmyy(exp), expiry_ts_ms: exp, strike: k, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok((pairs, index_px))
}

// ---------- Derive v3 (api.derive.xyz, USDC 組) ----------
// POST plain JSON 到 /v3/public/<方法>, 不是 JSON-RPC、不是 GET。舊版 api.lyra.finance 不要用。
// 資金費是每小時 (f 約 0.0000125/hr), funding_est = f * Perp * hours, 不能除以 8。
// REST 沒有 orderbook, 這版先用 get_tickers 的 top-of-book (b/B/a/A/I/M/f)。
async fn derive_all_option_instruments(client: &reqwest::Client, base: &str) -> Result<Vec<Value>> {
    let mut all: Vec<Value> = vec![];
    let mut page: i64 = 1;
    let mut num_pages: i64 = 1;
    while page <= num_pages {
        let body = serde_json::json!({"instrument_type": "option", "currency": base, "expired": false, "page": page, "page_size": 100}).to_string();
        let v = http_post_json(client, "https://api.derive.xyz/v3/public/get_all_instruments", &body).await?;
        if let Some(err) = v.get("error") {
            return Err(anyhow!("derive get_all_instruments error: {}", err));
        }
        let r = v.get("result").ok_or_else(|| anyhow!("derive: no result"))?;
        if let Some(p) = r.get("pagination") {
            num_pages = fv(get(p, "num_pages").unwrap_or(&Value::Null)).unwrap_or(1.0) as i64;
        }
        if let Some(arr) = r.get("instruments").and_then(|x| x.as_array()) {
            all.extend(arr.iter().cloned());
        }
        page += 1;
    }
    Ok(all)
}

async fn derive_perp_ticker(client: &reqwest::Client, base: &str) -> Result<Perp> {
    let body = serde_json::json!({"instrument_type": "perp"}).to_string();
    let v = http_post_json(client, "https://api.derive.xyz/v3/public/get_tickers", &body).await?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("derive get_tickers perp error: {}", err));
    }
    let tickers = v.get("result").and_then(|r| r.get("tickers")).ok_or_else(|| anyhow!("derive perp tickers empty"))?;
    let t = tickers.get(format!("{}-PERP", base)).ok_or_else(|| anyhow!("derive: no {}-PERP ticker", base))?;
    Ok(Perp {
        bid: fv(get(t, "b").unwrap_or(&Value::Null)).unwrap_or(0.0),
        ask: fv(get(t, "a").unwrap_or(&Value::Null)).unwrap_or(0.0),
        funding_rate: fv(get(t, "f").unwrap_or(&Value::Null)).unwrap_or(0.0),
        mark_price: fv(get(t, "M").unwrap_or(&Value::Null)).unwrap_or(0.0),
        index_price: fv(get(t, "I").unwrap_or(&Value::Null)).unwrap_or(0.0),
        next_funding_ms: 0,
    })
}

async fn fetch_derive_pairs(client: &reqwest::Client, base: &str, n_strikes: usize, n_expiries: usize) -> Result<(Vec<Pair>, Vec<String>)> {
    let perp = derive_perp_ticker(client, base).await?;
    let index_px = perp.index_price;
    let all = derive_all_option_instruments(client, base).await?;
    let mut groups: BTreeMap<i64, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    for it in &all {
        if fs(get(it, "instrument_type").unwrap_or(&Value::Null)) != "option" { continue; }
        if fs(get(it, "quote_currency").unwrap_or(&Value::Null)) != "USDC" { continue; }
        let od = match it.get("option_details") { Some(o) => o, None => continue };
        let exp: i64 = fv(get(od, "expiry").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
        let strike: i64 = fs(get(od, "strike").unwrap_or(&Value::Null)).parse().unwrap_or(0);
        if exp == 0 || strike == 0 { continue; }
        let sym = fs(get(it, "instrument_name").unwrap_or(&Value::Null));
        let e = groups.entry(exp).or_default().entry(strike).or_default();
        match fs(get(od, "option_type").unwrap_or(&Value::Null)).as_str() {
            "C" => e.0 = sym,
            "P" => e.1 = sym,
            _ => {}
        }
    }
    let now_s = Utc::now().timestamp();
    let mut pairs = vec![];
    let mut exps: Vec<String> = vec![];
    for (&exp, strikes) in groups.iter().filter(|(&e, _)| e > now_s + 3600).take(n_expiries) {
        let mut cand: Vec<(f64, f64, String, String)> = vec![];
        for (&strike, (c, p)) in strikes {
            if c.is_empty() || p.is_empty() { continue; }
            cand.push(((strike as f64 - index_px).abs(), strike as f64, c.clone(), p.clone()));
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.truncate(n_strikes);
        if cand.is_empty() { continue; }
        exps.push(fmt_expiry_yyyymmdd(exp));
        for (_, k, c, p) in cand {
            pairs.push(Pair { exchange: "derive".into(), base: base.to_string(), settle: "USDC".into(), expiry: fmt_expiry_yyyymmdd(exp), expiry_ts_ms: exp * 1000, strike: k, call_sym: c, put_sym: p, usd_priced: true });
        }
    }
    pairs.sort_by(|a, b| (a.expiry_ts_ms, a.strike).partial_cmp(&(b.expiry_ts_ms, b.strike)).unwrap());
    Ok((pairs, exps))
}

async fn derive_option_tickers(client: &reqwest::Client, base: &str, expiry_yyyymmdd: &str) -> Result<BTreeMap<String, Quote>> {
    let body = serde_json::json!({"instrument_type": "option", "currency": base, "expiry_date": expiry_yyyymmdd}).to_string();
    let v = http_post_json(client, "https://api.derive.xyz/v3/public/get_tickers", &body).await?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("derive get_tickers option error: {}", err));
    }
    let tickers = v.get("result").and_then(|r| r.get("tickers")).ok_or_else(|| anyhow!("derive option tickers empty"))?;
    let mut m = BTreeMap::new();
    if let Some(obj) = tickers.as_object() {
        for (name, t) in obj {
            m.insert(name.clone(), Quote {
                bid: fv(get(t, "b").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask: fv(get(t, "a").unwrap_or(&Value::Null)).unwrap_or(0.0),
                bid_sz: fv(get(t, "B").unwrap_or(&Value::Null)).unwrap_or(0.0),
                ask_sz: fv(get(t, "A").unwrap_or(&Value::Null)).unwrap_or(0.0),
            });
        }
    }
    Ok(m)
}

fn arg_val(args: &[String], key: &str, def: &str) -> String {
    for i in 0..args.len() {
        if args[i] == key && i + 1 < args.len() { return args[i + 1].clone(); }
        if let Some(v) = args[i].strip_prefix(&format!("{}=", key)) { return v.to_string(); }
    }
    def.to_string()
}

// 永續對沖 PCP 計算, 回傳 (dev_mid_perp, dev_buy, dev_sell, basis, funding_est_short, fee_taker, fee_maker, net_buy_taker, net_sell_taker, net_buy_maker, net_sell_maker, fee_mixed, net_buy_mixed, net_sell_mixed)
//   fee_mixed = 期權 maker*2腿 + 永續 taker*2腿, 是實際最常用的執行方式: 期權掛 maker、永續吃 taker 立刻對沖。
fn calc_perp(perp: &Perp, spot_mid: f64, k: f64, c: &Quote, p: &Quote, expiry_ms: i64, opt_taker: f64, opt_maker: f64, perp_taker: f64, perp_maker: f64, fund_period_h: f64, opt_base_taker: f64, perp_base_taker: f64) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
    let pm = perp.mid();
    let dev_mid = (c.mid() - p.mid()) - (pm - k);
    let dev_buy = (c.ask - p.bid) - (perp.bid - k);
    let dev_sell = (c.bid - p.ask) - (perp.ask - k);
    let basis = pm - spot_mid;
    let hours = ((expiry_ms - Utc::now().timestamp_millis()) as f64 / 3_600_000.0).max(0.0);
    // fund_period_h: 資金費週期 (小時), 一般是 8, Derive 是 1 (每小時)
    let funding_est_short = perp.funding_rate * pm * (hours / fund_period_h);
    // opt_base_taker / perp_base_taker: taker 每筆固定手續費 (Derive 期權 $0.50 / 永續 $0.01, 進場+出場各算一次)
    // opt_taker / opt_maker 為 NaN 時 (UMX 期權費率未公布), fee 與 net 全為 NaN, 只看 dev
    let fee_taker = (opt_taker * 2.0 + perp_taker * 2.0) * pm + (opt_base_taker + perp_base_taker) * 2.0;
    let fee_maker = (opt_maker * 2.0 + perp_maker * 2.0) * pm;
    let fee_mixed = (opt_maker * 2.0 + perp_taker * 2.0) * pm + perp_base_taker * 2.0;
    let net_buy_taker = -dev_buy - fee_taker + funding_est_short;
    let net_sell_taker = dev_sell - fee_taker - funding_est_short;
    let net_buy_maker = -dev_buy - fee_maker + funding_est_short;
    let net_sell_maker = dev_sell - fee_maker - funding_est_short;
    let net_buy_mixed = -dev_buy - fee_mixed + funding_est_short;
    let net_sell_mixed = dev_sell - fee_mixed - funding_est_short;
    (dev_mid, dev_buy, dev_sell, basis, funding_est_short, fee_taker, fee_maker, net_buy_taker, net_sell_taker, net_buy_maker, net_sell_maker, fee_mixed, net_buy_mixed, net_sell_mixed)
}

fn taipei_date() -> String {
    // 台北 = UTC+8, 沒有夏令時, 直接加 8 小時 (不用再加 chrono-tz 相依)
    (Utc::now() + chrono::Duration::hours(8)).format("%Y%m%d").to_string()
}

fn default_bases(exchange: &str) -> Vec<String> {
    // 常駐模式沒給 --bases 時, 單一交易所的全部標的
    let list: &[&str] = match exchange {
        "gate" => &["BTC", "ETH", "LTC", "SOL", "DOGE", "ADA", "XRP", "BNB", "HYPE", "SUI", "XAUT", "CL"],
        "okx" => &["BTC", "ETH", "SOL"],
        "bybit" => &["BTC", "ETH", "SOL"],
        "binance" => &["BTC", "ETH", "SOL", "XRP", "BNB", "DOGE", "XAU", "XAG"],
        "umx" => &["BTC", "ETH"],
        "derive" => &["BTC", "ETH"],
        _ => &["BTC"],
    };
    list.iter().map(|s| s.to_string()).collect()
}

const CSV_HEADER: &[&str] = &["ts", "ts_ms", "exchange", "settle", "expiry", "strike",
    "spot_bid", "spot_ask", "spot_mid",
    "perp_bid", "perp_ask", "perp_mid", "perp_mark", "perp_index",
    "basis_usdt", "funding_rate_8h", "hours_to_expiry", "funding_est_short_usdt",
    "c_bid", "c_ask", "c_mid", "p_bid", "p_ask", "p_mid",
    "dev_mid_perp_usdt", "dev_mid_perp_pct", "dev_buy_perp_usdt", "dev_sell_perp_usdt",
    "dev_mid_spot_usdt",
    "fee_taker_usdt", "fee_maker_usdt", "fee_mixed_usdt",
    "net_buy_taker_usdt", "net_sell_taker_usdt", "net_buy_maker_usdt", "net_sell_maker_usdt",
    "net_buy_mixed_usdt", "net_sell_mixed_usdt",
    "call_sym", "put_sym"];

fn new_writer(path: &str) -> Result<csv::Writer<File>> {
    if let Some(p) = std::path::Path::new(path).parent() {
        if !p.as_os_str().is_empty() { std::fs::create_dir_all(p)?; }
    }
    // 常駐重啟不丟資料: 檔已存在且非空就 append (不重寫表頭); 只有新檔/空檔才寫表頭。
    // (舊版用 File::create 會截斷, 重啟會清空當天已錄的 CSV, v4 修掉。)
    let non_empty = std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false);
    let file = std::fs::OpenOptions::new().create(true).append(true).write(true).open(path)?;
    let mut wtr = csv::WriterBuilder::new().has_headers(false).from_writer(file);
    if !non_empty {
        wtr.write_record(CSV_HEADER)?;
    }
    Ok(wtr)
}

#[derive(Default, Clone)]
struct Discovery {
    gate_pairs: Vec<Pair>,
    gate_index: BTreeMap<String, f64>,
    okx_pairs: Vec<Pair>,
    bybit_pairs: Vec<Pair>,
    binance_pairs: Vec<Pair>,
    umx_pairs: Vec<Pair>,
    derive_pairs: Vec<Pair>,
    derive_expiries: BTreeMap<String, Vec<String>>,
}

impl Discovery {
    fn is_empty(&self) -> bool {
        self.gate_pairs.is_empty() && self.okx_pairs.is_empty() && self.bybit_pairs.is_empty()
            && self.binance_pairs.is_empty() && self.umx_pairs.is_empty() && self.derive_pairs.is_empty()
    }
}

fn replace_base_pairs(old: &mut Vec<Pair>, new: &[Pair], base: &str) {
    // 該標的新發現有對才取代舊的, 沒發現就保留舊的 (不要讓一次失敗的發現清空正在錄的標的)
    if new.iter().any(|p| p.base == base) {
        old.retain(|p| p.base != base);
        old.extend(new.iter().filter(|p| p.base == base).cloned());
    }
}

fn merge_discovery(old: &mut Discovery, new: Discovery, bases: &[String]) {
    for b in bases {
        replace_base_pairs(&mut old.gate_pairs, &new.gate_pairs, b);
        replace_base_pairs(&mut old.okx_pairs, &new.okx_pairs, b);
        replace_base_pairs(&mut old.bybit_pairs, &new.bybit_pairs, b);
        replace_base_pairs(&mut old.binance_pairs, &new.binance_pairs, b);
        replace_base_pairs(&mut old.umx_pairs, &new.umx_pairs, b);
        replace_base_pairs(&mut old.derive_pairs, &new.derive_pairs, b);
        if let Some(idx) = new.gate_index.get(b) { old.gate_index.insert(b.clone(), *idx); }
        if let Some(e) = new.derive_expiries.get(b) { old.derive_expiries.insert(b.clone(), e.clone()); }
    }
}

// 啟動 + 定期刷新共用: 各所、各標的發現 pairs (最近 n_expiries 個到期日、各 n_strikes 檔履約價)。
// 單一交易所掛掉只會印錯、不會拖垮其他所; 某標的沒發現就跳過該標的。
async fn discover(client: &reqwest::Client, only: &str, bases: &[String], n_strikes: usize, n_expiries: usize) -> Discovery {
    // --- Gate ---
    let mut gate_pairs: Vec<Pair> = vec![];
    let mut gate_index: BTreeMap<String, f64> = BTreeMap::new();
    if only.contains("gate") {
        for b in bases {
            match fetch_gate_pairs(&client, b, n_strikes, n_expiries).await {
                Ok((p, idx)) => {
                    if p.is_empty() { println!("[gate {}] 選到 0 對, 跳過", b); } else {
                        println!("[gate {}] expiry={} index={:.2} pairs={}", b, p[0].expiry, idx, p.len());
                        for pr in &p { println!("  gate {} {} C={} P={}", b, pr.strike, pr.call_sym, pr.put_sym); }
                    }
                    gate_index.insert(b.clone(), idx);
                    gate_pairs.extend(p);
                }
                Err(e) => println!("[gate {}] 選對失敗: {}", b, e),
            }
        }
    }
    // --- OKX USD_UM ---
    let mut okx_pairs: Vec<Pair> = vec![];
    if only.contains("okx") {
        for b in bases {
            match okx_spot(&client, b).await {
                Ok(q) => match fetch_okx_pairs(&client, b, n_strikes, n_expiries, q.mid()).await {
                    Ok(p) => {
                        if p.is_empty() { println!("[okx {}] 選到 0 對, 跳過", b); } else {
                            println!("[okx {}] expiry={} spot_mid={:.2} pairs={} settle=USD_UM", b, p[0].expiry, q.mid(), p.len());
                            for pr in &p { println!("  okx {} {} C={} P={}", b, pr.strike, pr.call_sym, pr.put_sym); }
                        }
                        okx_pairs.extend(p);
                    }
                    Err(e) => println!("[okx {}] 選對失敗: {}", b, e),
                },
                Err(e) => println!("[okx {}] 現貨失敗: {}", b, e),
            }
        }
    }
    // --- Bybit via WS discovery (REST 被擋, 不走 REST) ---
    let mut bybit_pairs: Vec<Pair> = vec![];
    if only.contains("bybit") {
        for b in bases {
            // ref_price 優先用 Gate 指數, 沒有就傳 0 (discovery 內部會改用觀測到的履約價中位數)
            let ref_price = gate_index.get(b).copied().unwrap_or(0.0);
            match bybit_discover_pairs(b, n_strikes, n_expiries, ref_price).await {
                Ok(p) => {
                    if p.is_empty() { println!("[bybit WS {}] 發現 0 對, 跳過", b); } else {
                        println!("[bybit WS {}] expiry={} pairs={} (publicTrade 發現)", b, p[0].expiry, p.len());
                        for pr in &p { println!("  bybit {} {} C={} P={}", b, pr.strike, pr.call_sym, pr.put_sym); }
                    }
                    bybit_pairs.extend(p);
                }
                Err(e) => println!("[bybit WS {}] 發現符號失敗: {}", b, e),
            }
        }
    }
    // --- Binance (www.binance.com REST, USDT) ---
    let mut binance_pairs: Vec<Pair> = vec![];
    if only.contains("binance") {
        for b in bases {
            match fetch_binance_pairs(&client, b, n_strikes, n_expiries).await {
                Ok((p, idx)) => {
                    if p.is_empty() { println!("[binance {}] 選到 0 對, 跳過", b); } else {
                        println!("[binance {}] expiry={} index={:.2} pairs={} settle=USDT (www.binance.com)", b, p[0].expiry, idx, p.len());
                        for pr in &p { println!("  binance {} {} C={} P={}", b, pr.strike, pr.call_sym, pr.put_sym); }
                    }
                    binance_pairs.extend(p);
                }
                Err(e) => println!("[binance {}] 選對失敗: {}", b, e),
            }
        }
    }
    // --- UMX (USDT, 最近 3 個到期日) ---
    let mut umx_pairs: Vec<Pair> = vec![];
    if only.contains("umx") {
        // symbols 約 2MB, 只抓一次, 各標的共用
        match fetch_umx_symbols(&client).await {
            Ok(symbols) => {
                for b in bases {
                    match fetch_umx_pairs(&client, b, n_strikes, n_expiries, &symbols).await {
                        Ok((p, idx)) => {
                            if p.is_empty() { println!("[umx {}] 選到 0 對, 跳過", b); } else {
                                println!("[umx {}] index={:.2} pairs={} settle=USDT (最近 3 個到期日)", b, idx, p.len());
                                for pr in &p { println!("  umx {} {} {} C={} P={}", b, pr.expiry, pr.strike, pr.call_sym, pr.put_sym); }
                            }
                            umx_pairs.extend(p);
                        }
                        Err(e) => println!("[umx {}] 選對失敗: {}", b, e),
                    }
                }
            }
            Err(e) => println!("[umx] symbols 抓取失敗: {}", e),
        }
    }
    // --- Derive (USDC 組) ---
    let mut derive_pairs: Vec<Pair> = vec![];
    let mut derive_expiries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if only.contains("derive") {
        for b in bases {
            match fetch_derive_pairs(&client, b, n_strikes, n_expiries).await {
                Ok((p, exp)) => {
                    if p.is_empty() { println!("[derive {}] 選到 0 對, 跳過", b); } else {
                        println!("[derive {}] expiry={:?} pairs={} settle=USDC (資金費每小時)", b, exp, p.len());
                        for pr in &p { println!("  derive {} {} C={} P={}", b, pr.strike, pr.call_sym, pr.put_sym); }
                    }
                    derive_expiries.insert(b.clone(), exp);
                    derive_pairs.extend(p);
                }
                Err(e) => println!("[derive {}] 選對失敗: {}", b, e),
            }
        }
    }
    Discovery { gate_pairs, gate_index, okx_pairs, bybit_pairs, binance_pairs, umx_pairs, derive_pairs, derive_expiries }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = env::args().collect();
    let base = arg_val(&args, "--base", "BTC");
    let duration_sec: u64 = arg_val(&args, "--duration-sec", "60").parse().unwrap_or(60);
    let interval_ms: u64 = arg_val(&args, "--interval-ms", "1000").parse().unwrap_or(1000);
    let n_strikes: usize = arg_val(&args, "--strikes", "5").parse().unwrap_or(5);
    let output = arg_val(&args, "--output", "data/pcp.csv");
    let only = arg_val(&args, "--only", "gate,okx,bybit");
    // 常駐模式: --forever 無限循環 (不要固定 --duration-sec, 跑到被停掉為止);
    // --expiries: 每標的錄最近 N 個到期日 (預設 3); --refresh-mins: 背景多久重新發現一次 pairs
    let forever = args.iter().any(|a| a == "--forever");
    let n_expiries: usize = arg_val(&args, "--expiries", "3").parse().unwrap_or(3);
    let output_dir = arg_val(&args, "--output-dir", "data");
    let refresh_mins: u64 = arg_val(&args, "--refresh-mins", "10").parse().unwrap_or(10);
    let bases: Vec<String> = {
        let b = arg_val(&args, "--bases", "");
        if !b.is_empty() { b.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() }
        else if forever {
            // 常駐模式沒給 --bases: 單一交易所就錄該所全部標的
            let only_list: Vec<&str> = only.split(',').map(|s| s.trim()).collect();
            if only_list.len() == 1 { default_bases(only_list[0]) } else { vec![base.clone()] }
        } else { vec![base.clone()] }
    };

    println!("PCP 錄盤口 v2 永續對沖 | bases={:?} duration={}s interval={}ms strikes={} expiries={} only={} forever={} output={}", bases, duration_sec, interval_ms, n_strikes, n_expiries, only, forever, if forever { format!("{}/pcp_<exchange>_YYYYMMDD.csv", output_dir) } else { output.clone() });
    println!("對沖: 永續 (不是現貨). OKX 固定用 USD_UM. Bybit REST 被擋, 改走 WebSocket.");

    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;
    // 常駐模式: 每日換檔 (台北日期), 檔名 data/pcp_<exchange>_YYYYMMDD.csv; 固定時長模式維持 --output 單一檔案
    let file_prefix = only.split(',').map(|s| s.trim()).collect::<Vec<_>>().join("_");
    let mut current_date = taipei_date();
    let mut wtr = if forever {
        new_writer(&format!("{}/pcp_{}_{}.csv", output_dir, file_prefix, current_date))?
    } else {
        new_writer(&output)?
    };

    // --- 發現 pairs (啟動時一次, 常駐模式由背景任務定期刷新) ---
    let bybit_state = Arc::new(Mutex::new(BybitState::default()));
    let disc0 = discover(&client, &only, &bases, n_strikes, n_expiries).await;
    if disc0.is_empty() {
        return Err(anyhow!("六間都選不到對, 無法錄製"));
    }
    let shared: Arc<Mutex<Discovery>> = Arc::new(Mutex::new(disc0));
    if !shared.lock().await.bybit_pairs.is_empty() {
        for b in &bases {
            tokio::spawn(bybit_linear_task(bybit_state.clone(), b.clone()));
        }
        tokio::spawn(bybit_option_task(bybit_state.clone(), shared.clone()));
        tokio::time::sleep(Duration::from_secs(2)).await;
        let st = bybit_state.lock().await;
        println!("[bybit WS] perps={} books={}", st.perps.len(), st.books.len());
    }


    // 常駐模式: 背景定期重新發現 pairs (到期滾動、履約價重新對齊市價), 跑在獨立任務裡、不阻塞錄製
    if forever {
        let refresh_client = client.clone();
        let refresh_shared = shared.clone();
        let refresh_only = only.clone();
        let refresh_bases = bases.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(refresh_mins * 60));
            iv.tick().await; // 第一次 tick 立刻觸發, 先吃掉, 之後每 refresh_mins 才跑一次
            loop {
                iv.tick().await;
                let nd = discover(&refresh_client, &refresh_only, &refresh_bases, n_strikes, n_expiries).await;
                let mut g = refresh_shared.lock().await;
                merge_discovery(&mut g, nd, &refresh_bases);
                println!("[refresh] pairs 更新: gate={} okx={} bybit={} binance={} umx={} derive={}",
                    g.gate_pairs.len(), g.okx_pairs.len(), g.bybit_pairs.len(), g.binance_pairs.len(), g.umx_pairs.len(), g.derive_pairs.len());
            }
        });
    }

    let start = Instant::now();
    // 乾淨關機: systemd restart/stop 發 SIGTERM; tokio::signal 接住後進程不會被預設殺掉, 在 sleep 處收到就等本輪寫完、flush 後退出, 不要死在寫一半 (v4 重啟時 OKX/Gate 各撕裂 1 列)。
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut tick: u64 = 0;
    let mut rows: u64 = 0;
    loop {
        if !forever && start.elapsed().as_secs() >= duration_sec { break; }
        let round_start = Instant::now();
        tick += 1;
        // 常駐模式: 每日換檔 (台北日期)
        if forever {
            let today = taipei_date();
            if today != current_date {
                wtr.flush()?;
                println!("[rotate] 換檔: {}", today);
                wtr = new_writer(&format!("{}/pcp_{}_{}.csv", output_dir, file_prefix, today))?;
                current_date = today;
            }
        }
        // 每 tick 從 shared 拿最新 pairs (背景 refresher 會更新, 到期滾動、履約價重新對齊市價)
        let disc = shared.lock().await.clone();
        let gate_pairs = &disc.gate_pairs;
        let gate_index = &disc.gate_index;
        let okx_pairs = &disc.okx_pairs;
        let bybit_pairs = &disc.bybit_pairs;
        let binance_pairs = &disc.binance_pairs;
        let umx_pairs = &disc.umx_pairs;
        let derive_pairs = &disc.derive_pairs;
        let derive_expiries = &disc.derive_expiries;
        let ts = Utc::now();
        let ts_ms = ts.timestamp_millis();

        // 平行抓盤口用的共享引用: &Client 是 Copy, 各 async move 都能各自持有; 直接在 async move 裡用 &client 會想把 owned Client 移進 future 而編譯失敗
        let client_ref = &client;
        // Gate perp hedge (第一階段: 各標的 spot/tickers/perp 平行抓; 原本順序抓 12 標的 x 3 次請求, 一輪約 14 秒)
        if !gate_pairs.is_empty() {
            let gate_fetched = futures_util::future::join_all(bases.iter().filter(|b| gate_pairs.iter().any(|p| &p.base == *b)).map(|b| {
                let base = b.clone();
                let gi = gate_index.get(&base).copied().unwrap_or(0.0);
                async move {
                    let spot = gate_spot(client_ref, &base).await.unwrap_or_default();
                    let s_mid = if spot.mid() > 0.0 { spot.mid() } else { gi };
                    let (map_r, perp_r) = tokio::join!(gate_ticker_map(client_ref, &base), gate_perp(client_ref, &base));
                    (base, spot, s_mid, map_r, perp_r)
                }
            })).await;
            for (b, spot, s_mid, map_r, perp_r) in gate_fetched {
                if let (Ok(map), Ok(perp)) = (map_r, perp_r) {
                    let bp: Vec<&Pair> = gate_pairs.iter().filter(|p| &p.base == &b).collect();
                    for pr in bp {
                        let c = map.get(&pr.call_sym).cloned().unwrap_or_default();
                        let p = map.get(&pr.put_sym).cloned().unwrap_or_default();
                        if c.mid() == 0.0 || p.mid() == 0.0 || perp.mid() == 0.0 { continue; }
                        let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, 0.0003, 0.0002, 0.00075, 0.0002, 8.0, 0.0, 0.0);
                        write_row(&mut wtr, ts_ms, &ts, pr, &spot, &perp, &c, &p, r, s_mid)?;
                        rows += 1;
                        if r.7 > 0.0 || r.8 > 0.0 || r.9 > 0.0 || r.10 > 0.0 {
                            println!("[gate perp] {} {} K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_maker_buy={:.2}/sell={:.2}",
                                ts.format("%H:%M:%S"), b, pr.strike, r.0, r.4, r.7, r.8, r.9, r.10);
                        }
                    }
                }
            }
        }

        // OKX USD_UM perp hedge (第一階段: 各標的 spot/perp/tickers 平行抓)
        if !okx_pairs.is_empty() {
            let okx_fetched = futures_util::future::join_all(bases.iter().filter(|b| okx_pairs.iter().any(|p| &p.base == *b)).map(|b| {
                let base = b.clone();
                async move {
                    let (spot_r, perp_r, map_r) = tokio::join!(okx_spot(client_ref, &base), okx_perp(client_ref, &base), okx_ticker_map(client_ref, &base));
                    (base, spot_r, perp_r, map_r)
                }
            })).await;
            for (b, spot_r, perp_r, map_r) in okx_fetched {
                if let (Ok(spot), Ok(perp), Ok(map)) = (spot_r, perp_r, map_r) {
                    let s_mid = spot.mid();
                    if s_mid > 0.0 && perp.mid() > 0.0 {
                        let bp: Vec<&Pair> = okx_pairs.iter().filter(|p| &p.base == &b).collect();
                        for pr in bp {
                            let c = map.get(&pr.call_sym).cloned().unwrap_or_default();
                            let p = map.get(&pr.put_sym).cloned().unwrap_or_default();
                            if c.mid() == 0.0 || p.mid() == 0.0 { continue; }
                            let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, 0.0003, 0.0003, 0.0005, 0.0002, 8.0, 0.0, 0.0);
                            write_row(&mut wtr, ts_ms, &ts, pr, &spot, &perp, &c, &p, r, s_mid)?;
                            rows += 1;
                            if r.7 > 0.0 || r.8 > 0.0 || r.9 > 0.0 || r.10 > 0.0 {
                                println!("[okx perp USD_UM] {} {} K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_maker_buy={:.2}/sell={:.2}",
                                    ts.format("%H:%M:%S"), b, pr.strike, r.0, r.4, r.7, r.8, r.9, r.10);
                            }
                        }
                    }
                }
            }
        }

        // Bybit WS perp hedge
        if !bybit_pairs.is_empty() {
            for b in &bases {
                let bp: Vec<&Pair> = bybit_pairs.iter().filter(|p| &p.base == b).collect();
                if bp.is_empty() { continue; }
                let st = bybit_state.lock().await;
                let perp = match st.perps.get(b) { Some(p) => p.clone(), None => continue };
                // WS 斷線後資料會停在舊值: 永續價超過 60 秒沒更新就跳過, 不要把舊盤口當成活的來錄
                let perp_fresh = st.perp_updated_ms.get(b).map(|u| ts_ms - *u < 60_000).unwrap_or(false);
                if !perp_fresh { continue; }
                let s_mid = if perp.index_price > 0.0 { perp.index_price } else { perp.mid() };
                if perp.mid() > 0.0 {
                    for pr in bp {
                        let book_fresh = st.book_updated_ms.get(&pr.call_sym).map(|u| ts_ms - *u < 60_000).unwrap_or(false)
                            && st.book_updated_ms.get(&pr.put_sym).map(|u| ts_ms - *u < 60_000).unwrap_or(false);
                        if !book_fresh { continue; }
                        let c = st.top_quote(&pr.call_sym);
                        let p = st.top_quote(&pr.put_sym);
                        if c.mid() == 0.0 || p.mid() == 0.0 { continue; }
                        let spot_q = Quote { bid: s_mid, ask: s_mid, bid_sz: 0.0, ask_sz: 0.0 };
                        let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, 0.0003, 0.0002, 0.00055, 0.0002, 8.0, 0.0, 0.0);
                        write_row(&mut wtr, ts_ms, &ts, pr, &spot_q, &perp, &c, &p, r, s_mid)?;
                        rows += 1;
                        if r.7 > 0.0 || r.8 > 0.0 || r.9 > 0.0 || r.10 > 0.0 {
                            println!("[bybit perp WS] {} {} K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_maker_buy={:.2}/sell={:.2}",
                                ts.format("%H:%M:%S"), b, pr.strike, r.0, r.4, r.7, r.8, r.9, r.10);
                        }
                    }
                }
            }
        }

        // Binance USDT perp hedge (www.binance.com REST; 期權用批次 ticker 的 top-of-book, 一次拿全部標的; 各標的 perp/spot 平行抓)
        if !binance_pairs.is_empty() {
            let (map_r, bin_fetched) = tokio::join!(
                binance_ticker_map(client_ref),
                futures_util::future::join_all(bases.iter().filter(|b| binance_pairs.iter().any(|p| &p.base == *b)).map(|b| {
                    let base = b.clone();
                    async move {
                        let (perp_r, spot_r) = tokio::join!(binance_perp(client_ref, &base), binance_spot(client_ref, &base));
                        (base, perp_r, spot_r)
                    }
                }))
            );
            if let Ok(map) = map_r {
                for (b, perp_r, spot_r) in bin_fetched {
                    let perp = match perp_r { Ok(p) => p, Err(_) => continue };
                    // XAU/XAG 沒有現貨 (Invalid symbol), 用永續指數價當現貨 proxy
                    let s_mid = match spot_r {
                        Ok(spot) if spot.mid() > 0.0 => spot.mid(),
                        _ => perp.index_price,
                    };
                    let spot_q = Quote { bid: s_mid, ask: s_mid, bid_sz: 0.0, ask_sz: 0.0 };
                    if perp.mid() == 0.0 || s_mid == 0.0 { continue; }
                    let bp: Vec<&Pair> = binance_pairs.iter().filter(|p| &p.base == &b).collect();
                    for pr in bp {
                        let c = map.get(&pr.call_sym).cloned().unwrap_or_default();
                        let p = map.get(&pr.put_sym).cloned().unwrap_or_default();
                        if c.mid() == 0.0 || p.mid() == 0.0 { continue; }
                        let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, 0.00024, 0.00024, 0.0004, 0.0002, 8.0, 0.0, 0.0);
                        write_row(&mut wtr, ts_ms, &ts, pr, &spot_q, &perp, &c, &p, r, s_mid)?;
                        rows += 1;
                        if r.7 > 0.0 || r.8 > 0.0 || r.9 > 0.0 || r.10 > 0.0 {
                            println!("[binance perp] {} {} K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_maker_buy={:.2}/sell={:.2}",
                                ts.format("%H:%M:%S"), b, pr.strike, r.0, r.4, r.7, r.8, r.9, r.10);
                        }
                    }
                }
            }
        }

        // UMX USDT perp hedge (期權費率 TBD, fee/net 欄位為 NaN, 只看 dev; 全部期權 depth 平行抓, 原本每對 2 次順序請求、一輪 60 次約 13 秒)
        if !umx_pairs.is_empty() {
            let umx_fetched = futures_util::future::join_all(bases.iter().filter(|b| umx_pairs.iter().any(|p| &p.base == *b)).map(|b| {
                let base = b.clone();
                let bp: Vec<Pair> = umx_pairs.iter().filter(|p| &p.base == &base).cloned().collect();
                async move {
                    let syms: Vec<String> = bp.iter().flat_map(|pr| [pr.call_sym.clone(), pr.put_sym.clone()]).collect();
                    let (perp_r, depths) = tokio::join!(
                        umx_perp(client_ref, &base),
                        futures_util::future::join_all(syms.iter().map(|s| {
                            let sym = s.clone();
                            async move { (sym.clone(), umx_depth(client_ref, &sym).await.unwrap_or_default()) }
                        }))
                    );
                    let depth_map: BTreeMap<String, Quote> = depths.into_iter().collect();
                    (base, perp_r, depth_map, bp)
                }
            })).await;
            for (b, perp_r, depth_map, bp) in umx_fetched {
                if let Ok(perp) = perp_r {
                    let s_mid = perp.index_price;
                    if s_mid > 0.0 && perp.mid() > 0.0 {
                        let spot_q = Quote { bid: s_mid, ask: s_mid, bid_sz: 0.0, ask_sz: 0.0 };
                        for pr in &bp {
                            let c = depth_map.get(&pr.call_sym).cloned().unwrap_or_default();
                            let p = depth_map.get(&pr.put_sym).cloned().unwrap_or_default();
                            if c.mid() == 0.0 || p.mid() == 0.0 { continue; }
                            let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, f64::NAN, f64::NAN, 0.0005, 0.0002, 8.0, 0.0, 0.0);
                            write_row(&mut wtr, ts_ms, &ts, pr, &spot_q, &perp, &c, &p, r, s_mid)?;
                            rows += 1;
                            if r.0.abs() > 50.0 {
                                println!("[umx perp] {} {} {} K={} dev_perp={:.2} c={:.0}/{:.0} p={:.0}/{:.0}",
                                    ts.format("%H:%M:%S"), b, pr.expiry, pr.strike, r.0, c.bid, c.ask, p.bid, p.ask);
                            }
                        }
                    }
                }
            }
        }

        // Derive USDC perp hedge (資金費每小時, 期權 taker 每筆 +$0.50, 永續 taker 每筆 +$0.01; 各到期日 tickers + 永續平行抓)
        if !derive_pairs.is_empty() {
            let der_fetched = futures_util::future::join_all(bases.iter().filter(|b| derive_pairs.iter().any(|p| &p.base == *b)).map(|b| {
                let base = b.clone();
                let exps = derive_expiries.get(&base).cloned().unwrap_or_default();
                async move {
                    let base_str = base.as_str();
                    let (tickers, perp_r) = tokio::join!(
                        futures_util::future::join_all(exps.iter().map(|e| {
                            let exp = e.clone();
                            async move { derive_option_tickers(client_ref, base_str, &exp).await.unwrap_or_default() }
                        })),
                        derive_perp_ticker(client_ref, base_str)
                    );
                    let mut map: BTreeMap<String, Quote> = BTreeMap::new();
                    for m in tickers { map.extend(m); }
                    (base, map, perp_r)
                }
            })).await;
            for (b, map, perp_r) in der_fetched {
                if map.is_empty() { continue; }
                if let Ok(perp) = perp_r {
                    let s_mid = perp.index_price;
                    if s_mid > 0.0 && perp.mid() > 0.0 {
                        let spot_q = Quote { bid: s_mid, ask: s_mid, bid_sz: 0.0, ask_sz: 0.0 };
                        let bp: Vec<&Pair> = derive_pairs.iter().filter(|p| &p.base == &b).collect();
                        for pr in bp {
                            let c = map.get(&pr.call_sym).cloned().unwrap_or_default();
                            let p = map.get(&pr.put_sym).cloned().unwrap_or_default();
                            if c.mid() == 0.0 || p.mid() == 0.0 { continue; }
                            let r = calc_perp(&perp, s_mid, pr.strike, &c, &p, pr.expiry_ts_ms, 0.0003, 0.0001, 0.0003, 0.0001, 1.0, 0.5, 0.01);
                            write_row(&mut wtr, ts_ms, &ts, pr, &spot_q, &perp, &c, &p, r, s_mid)?;
                            rows += 1;
                            if r.7 > 0.0 || r.8 > 0.0 || r.9 > 0.0 || r.10 > 0.0 {
                                println!("[derive perp USDC] {} {} K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_maker_buy={:.2}/sell={:.2}",
                                    ts.format("%H:%M:%S"), b, pr.strike, r.0, r.4, r.7, r.8, r.9, r.10);
                            }
                        }
                    }
                }
            }
        }

        wtr.flush()?;
        if tick % 10 == 0 {
            println!("... tick {} 已錄 {} 列, 經過 {:?} / round {}ms", tick, rows, start.elapsed(), round_start.elapsed().as_millis());
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(interval_ms)) => {},
            _ = sigterm.recv() => {
                println!("[shutdown] 收到 SIGTERM, 乾淨退出 (已錄 {} 列)", rows);
                break;
            }
        }
    }

    wtr.flush()?;
    println!("完成: {} ticks, {} 列, 輸出 {}", tick, rows, output);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_row(
    wtr: &mut csv::Writer<File>, ts_ms: i64, ts: &chrono::DateTime<Utc>, pr: &Pair,
    spot: &Quote, perp: &Perp, c: &Quote, p: &Quote,
    r: (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64), spot_mid: f64,
) -> Result<()> {
    let (dev_mid, _dev_buy, _dev_sell, basis, funding_est, fee_taker, fee_maker, net_bt, net_st, net_bm, net_sm, fee_mixed, net_bmix, net_smix) = r;
    let dev_buy = _dev_buy; let dev_sell = _dev_sell;
    let dev_pct = if perp.mid() > 0.0 { dev_mid / perp.mid() * 100.0 } else { 0.0 };
    let dev_spot = (c.mid() - p.mid()) - (spot_mid - pr.strike);
    let hours = ((pr.expiry_ts_ms - ts_ms) as f64 / 3_600_000.0).max(0.0);
    wtr.write_record([
        ts.to_rfc3339(), ts_ms.to_string(), pr.exchange.clone(), pr.settle.clone(), pr.expiry.clone(),
        format!("{:.0}", pr.strike),
        format!("{:.4}", spot.bid), format!("{:.4}", spot.ask), format!("{:.4}", spot_mid),
        format!("{:.4}", perp.bid), format!("{:.4}", perp.ask), format!("{:.4}", perp.mid()),
        format!("{:.4}", perp.mark_price), format!("{:.4}", perp.index_price),
        format!("{:.4}", basis), format!("{:.8}", perp.funding_rate), format!("{:.2}", hours), format!("{:.4}", funding_est),
        format!("{:.4}", c.bid), format!("{:.4}", c.ask), format!("{:.4}", c.mid()),
        format!("{:.4}", p.bid), format!("{:.4}", p.ask), format!("{:.4}", p.mid()),
        format!("{:.4}", dev_mid), format!("{:.6}", dev_pct), format!("{:.4}", dev_buy), format!("{:.4}", dev_sell),
        format!("{:.4}", dev_spot),
        format!("{:.4}", fee_taker), format!("{:.4}", fee_maker), format!("{:.4}", fee_mixed),
        format!("{:.4}", net_bt), format!("{:.4}", net_st), format!("{:.4}", net_bm), format!("{:.4}", net_sm),
        format!("{:.4}", net_bmix), format!("{:.4}", net_smix),
        pr.call_sym.clone(), pr.put_sym.clone(),
    ])?;
    Ok(())
}
