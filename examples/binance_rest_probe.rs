// 幣安期權 PCP 錄盤口驗證探針 (Rust, 只錄盤口不下單)
// 背景: 這台機器出口在美國, eapi.binance.com / api1-4 全被地區擋,
// 但 2026-11-24 實測發現 www.binance.com 的 REST 全部通, 路徑不變:
//   GET https://www.binance.com/eapi/v1/exchangeInfo  -> 1914 檔, 8 個標的
//   GET https://www.binance.com/eapi/v1/depth?symbol=BTC-261124-60000-C&limit=10
//   GET https://www.binance.com/fapi/v1/premiumIndex?symbol=BTCUSDT
//   GET https://www.binance.com/fapi/v1/depth?symbol=BTCUSDT&limit=5
// 符號一律以 exchangeInfo 回傳為準, 不要自己拼 (格式 BTC-YYMMDD-履約價-C/P)。
// 坑: depth 限用 limit=10 (limit=5 回 -4021); bid/ask 回 "0" 或空陣列要跳過;
// 權重一次 2, 5 對每秒一輪約 1200/分鐘, 在 6000 上限內。
// 手續費 (最高費率 VIP0/Regular, 不含 BNB 折扣、不含 rebate):
//   期權 maker/taker 皆 0.024% (2026-11-24 主對話查核);
//   永續 maker 0.02% / taker 0.05% (多家費率表對官方頁面查核, 2026-10~11)。
//   因期權 maker=taker, 幣安的混合費 (期權 maker+永續 taker) 等於 taker 全吃單費。
//   以 BTC 約 60400 計, taker/混合約 89.4 USDT/BTC, maker 約 53.2 USDT/BTC。
// 幣安期權跟合約預設是分開帳戶 (不支援統一帳戶/SPAN, 除非期權增強計畫),
// 所以這支探針只錄價當對照, 不算實盤套利; 併入主程式時進 USDT 組、
// 單獨標註「非統一帳戶、僅價格對照」。
// 用法: cargo run --example binance_rest_probe -- --base BTC --duration-sec 60 --interval-ms 1000 --strikes 5 --output data/binance_probe_60s.csv

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::time::{Duration, Instant};

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
fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k)
}
fn arg_val(args: &[String], key: &str, def: &str) -> String {
    args.windows(2)
        .find(|w| w[0] == key)
        .map(|w| w[1].clone())
        .unwrap_or_else(|| def.to_string())
}

#[derive(Clone, Debug, Default)]
struct Quote {
    bid: f64,
    ask: f64,
    bid_sz: f64,
    ask_sz: f64,
}

async fn http_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..3 {
        match client
            .get(url)
            .header("User-Agent", "pcp-recorder/0.1 rust")
            .send()
            .await
        {
            Ok(r) => {
                let txt = r.text().await?;
                if txt.contains("CloudFront") || txt.contains("blocked") {
                    return Err(anyhow!("CloudFront blocked: {}", url));
                }
                let v: Value = serde_json::from_str(&txt).map_err(|e| {
                    anyhow!(
                        "JSON parse fail {}: {} | body={}",
                        url,
                        e,
                        &txt[..txt.len().min(300)]
                    )
                })?;
                if let Some(code) = get(&v, "code").and_then(fv) {
                    if code != 0.0 {
                        return Err(anyhow!("binance err {}: {}", url, txt[..txt.len().min(300)].to_string()));
                    }
                }
                return Ok(v);
            }
            Err(e) => {
                last_err = Some(anyhow!(e));
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    let _ = last_err;
    Err(anyhow!("http failed after 3 tries: {}", url))
}

// depth 回傳 bids/asks 是 [[price, qty], ...] 字串陣列
fn depth_top1(v: &Value) -> Quote {
    let parse = |key: &str| -> (f64, f64) {
        let arr = v.get(key).and_then(|x| x.as_array());
        if let Some(a) = arr {
            if let Some(row) = a.first() {
                let p = row.get(0).and_then(fv).unwrap_or(0.0);
                let q = row.get(1).and_then(fv).unwrap_or(0.0);
                return (p, q);
            }
        }
        (0.0, 0.0)
    };
    let (bid, bid_sz) = parse("bids");
    let (ask, ask_sz) = parse("asks");
    Quote { bid, ask, bid_sz, ask_sz }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let base = arg_val(&args, "--base", "BTC");
    let duration_sec: u64 = arg_val(&args, "--duration-sec", "60").parse().unwrap_or(60);
    let interval_ms: u64 = arg_val(&args, "--interval-ms", "1000").parse().unwrap_or(1000);
    let n_strikes: usize = arg_val(&args, "--strikes", "5").parse().unwrap_or(5);
    let output = arg_val(&args, "--output", "data/binance_probe_60s.csv");

    // 幣安最高費率 (見檔頭註解)
    let opt_taker = 0.00024;
    let opt_maker = 0.00024;
    let perp_taker = 0.0005;
    let perp_maker = 0.0002;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let base_url = "https://www.binance.com";

    // 1) 合約列表: 只取該標的、TRADING、USDT 結算, 以回傳符號為準
    let ex = http_json(&client, &format!("{}/eapi/v1/exchangeInfo", base_url)).await?;
    let syms = ex
        .get("optionSymbols")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow!("exchangeInfo 無 optionSymbols"))?;
    let underlying = format!("{}USDT", base);
    let now_ms = Utc::now().timestamp_millis();
    let mut groups: BTreeMap<i64, BTreeMap<i64, (String, String)>> = BTreeMap::new();
    let mut total = 0usize;
    for s in syms {
        if fs(get(s, "underlying").unwrap_or(&Value::Null)) != underlying {
            continue;
        }
        if fs(get(s, "quoteAsset").unwrap_or(&Value::Null)) != "USDT" {
            continue;
        }
        if fs(get(s, "status").unwrap_or(&Value::Null)) != "TRADING" {
            continue;
        }
        total += 1;
        let exp = fv(get(s, "expiryDate").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
        let strike = fv(get(s, "strikePrice").unwrap_or(&Value::Null)).unwrap_or(0.0) as i64;
        if strike == 0 {
            continue;
        }
        let sym = fs(get(s, "symbol").unwrap_or(&Value::Null));
        let entry = groups.entry(exp).or_default().entry(strike).or_default();
        match fs(get(s, "side").unwrap_or(&Value::Null)).as_str() {
            "CALL" => entry.0 = sym,
            "PUT" => entry.1 = sym,
            _ => {}
        }
    }
    println!(
        "[binance] {}USDT TRADING 合約 {} 檔, 到期日 {} 個",
        base,
        total,
        groups.len()
    );
    let (&exp_ms, strikes) = groups
        .iter()
        .find(|(&e, _)| e > now_ms)
        .ok_or_else(|| anyhow!("binance: 無未來到期日"))?;
    let expiry_dt = chrono::DateTime::from_timestamp_millis(exp_ms)
        .ok_or_else(|| anyhow!("expiry 時間錯誤"))?;
    println!(
        "[binance] 最近到期 {} (UTC), 該到期履約價 {} 個",
        expiry_dt.format("%Y-%m-%d %H:%M"),
        strikes.len()
    );

    // 2) 永續: mark / index / funding
    let perp_sym = format!("{}USDT", base);
    let prem = http_json(
        &client,
        &format!("{}/fapi/v1/premiumIndex?symbol={}", base_url, perp_sym),
    )
    .await?;
    let index_px = fv(get(&prem, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(0.0);
    let funding_rate = fv(get(&prem, "lastFundingRate").unwrap_or(&Value::Null)).unwrap_or(0.0);
    let mark_px = fv(get(&prem, "markPrice").unwrap_or(&Value::Null)).unwrap_or(0.0);
    println!(
        "[binance] perp index {:.2} mark {:.2} funding(8h) {:.8}",
        index_px, mark_px, funding_rate
    );

    // 3) 選最近到期日、離指數價最近的 n 個履約價 (C/P 都要有)
    let mut cand: Vec<(f64, i64, String, String)> = vec![];
    for (&strike, (c, p)) in strikes {
        if c.is_empty() || p.is_empty() {
            continue;
        }
        cand.push(((strike as f64 - index_px).abs(), strike, c.clone(), p.clone()));
    }
    cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    cand.truncate(n_strikes);
    cand.sort_by(|a, b| a.1.cmp(&b.1));
    println!(
        "[binance] 錄 {} 對: {:?}",
        cand.len(),
        cand.iter().map(|c| c.1).collect::<Vec<_>>()
    );

    let file = File::create(&output)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record([
        "ts", "ts_ms", "exchange", "settle", "expiry", "strike",
        "spot_bid", "spot_ask", "spot_mid",
        "perp_bid", "perp_ask", "perp_mid", "perp_mark", "perp_index",
        "basis_usdt", "funding_rate_8h", "hours_to_expiry", "funding_est_short_usdt",
        "c_bid", "c_ask", "c_mid", "p_bid", "p_ask", "p_mid",
        "dev_mid_perp_usdt", "dev_mid_perp_pct", "dev_buy_perp_usdt", "dev_sell_perp_usdt",
        "dev_mid_spot_usdt",
        "fee_taker_usdt", "fee_maker_usdt", "fee_mixed_usdt",
        "net_buy_taker_usdt", "net_sell_taker_usdt",
        "net_buy_maker_usdt", "net_sell_maker_usdt",
        "net_buy_mixed_usdt", "net_sell_mixed_usdt",
        "call_sym", "put_sym",
    ])?;

    let start = Instant::now();
    let mut ticks = 0u64;
    let mut rows = 0u64;
    let mut net_taker_sell_pos = 0u64;
    let mut net_taker_buy_pos = 0u64;
    let mut net_mixed_sell_pos = 0u64;
    let mut net_mixed_buy_pos = 0u64;
    let mut dev_min = f64::MAX;
    let mut dev_max = f64::MIN;
    while start.elapsed() < Duration::from_secs(duration_sec) {
        let tick_start = Instant::now();
        let now = Utc::now();
        let ts_ms = now.timestamp_millis();
        // 永續盤口 + funding (每輪重抓)
        let perp_book = http_json(
            &client,
            &format!("{}/fapi/v1/depth?symbol={}&limit=5", base_url, perp_sym),
        )
        .await;
        let prem2 = http_json(
            &client,
            &format!("{}/fapi/v1/premiumIndex?symbol={}", base_url, perp_sym),
        )
        .await;
        let (perp_q, fr, mp, ip) = match (perp_book, prem2) {
            (Ok(b), Ok(p)) => (
                depth_top1(&b),
                fv(get(&p, "lastFundingRate").unwrap_or(&Value::Null)).unwrap_or(funding_rate),
                fv(get(&p, "markPrice").unwrap_or(&Value::Null)).unwrap_or(mark_px),
                fv(get(&p, "indexPrice").unwrap_or(&Value::Null)).unwrap_or(index_px),
            ),
            _ => {
                eprintln!("[binance] perp 抓取失敗, 跳過本輪");
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                continue;
            }
        };
        let perp_mid = if perp_q.bid > 0.0 && perp_q.ask > 0.0 {
            (perp_q.bid + perp_q.ask) / 2.0
        } else {
            mp
        };
        if perp_mid <= 0.0 {
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            continue;
        }
        let hours_to_expiry = ((exp_ms - ts_ms) as f64 / 3_600_000.0).max(0.0);
        let funding_est_short = fr * perp_mid * (hours_to_expiry / 8.0);
        let basis = perp_mid - ip;
        let fee_taker = (opt_taker * 2.0 + perp_taker * 2.0) * perp_mid;
        let fee_maker = (opt_maker * 2.0 + perp_maker * 2.0) * perp_mid;
        let fee_mixed = (opt_maker * 2.0 + perp_taker * 2.0) * perp_mid;

        for (_, strike, csym, psym) in &cand {
            let c = http_json(
                &client,
                &format!("{}/eapi/v1/depth?symbol={}&limit=10", base_url, csym),
            )
            .await;
            let p = http_json(
                &client,
                &format!("{}/eapi/v1/depth?symbol={}&limit=10", base_url, psym),
            )
            .await;
            let (cq, pq) = match (c, p) {
                (Ok(c), Ok(p)) => (depth_top1(&c), depth_top1(&p)),
                _ => continue,
            };
            // 缺邊跳過 (bid/ask 為 0 或空陣列)
            if cq.bid <= 0.0 || cq.ask <= 0.0 || pq.bid <= 0.0 || pq.ask <= 0.0 {
                continue;
            }
            let c_mid = (cq.bid + cq.ask) / 2.0;
            let p_mid = (pq.bid + pq.ask) / 2.0;
            let dev_mid = (c_mid - p_mid) - (perp_mid - *strike as f64);
            let dev_buy = (cq.ask - pq.bid) - (perp_q.bid - *strike as f64);
            let dev_sell = (cq.bid - pq.ask) - (perp_q.ask - *strike as f64);
            let dev_spot = (c_mid - p_mid) - (ip - *strike as f64);
            let net_buy_taker = -dev_buy - fee_taker + funding_est_short;
            let net_sell_taker = dev_sell - fee_taker - funding_est_short;
            let net_buy_maker = -dev_buy - fee_maker + funding_est_short;
            let net_sell_maker = dev_sell - fee_maker - funding_est_short;
            let net_buy_mixed = -dev_buy - fee_mixed + funding_est_short;
            let net_sell_mixed = dev_sell - fee_mixed - funding_est_short;
            if net_sell_taker > 0.0 {
                net_taker_sell_pos += 1;
            }
            if net_buy_taker > 0.0 {
                net_taker_buy_pos += 1;
            }
            if net_sell_mixed > 0.0 {
                net_mixed_sell_pos += 1;
            }
            if net_buy_mixed > 0.0 {
                net_mixed_buy_pos += 1;
            }
            if dev_mid < dev_min {
                dev_min = dev_mid;
            }
            if dev_mid > dev_max {
                dev_max = dev_mid;
            }
            wtr.write_record([
                now.to_rfc3339(),
                ts_ms.to_string(),
                "binance".to_string(),
                "USDT".to_string(),
                expiry_dt.format("%d%b%y").to_string().to_uppercase(),
                strike.to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                perp_q.bid.to_string(),
                perp_q.ask.to_string(),
                perp_mid.to_string(),
                mp.to_string(),
                ip.to_string(),
                basis.to_string(),
                fr.to_string(),
                format!("{:.4}", hours_to_expiry),
                format!("{:.4}", funding_est_short),
                cq.bid.to_string(),
                cq.ask.to_string(),
                c_mid.to_string(),
                pq.bid.to_string(),
                pq.ask.to_string(),
                p_mid.to_string(),
                format!("{:.2}", dev_mid),
                format!("{:.6}", dev_mid / perp_mid),
                format!("{:.2}", dev_buy),
                format!("{:.2}", dev_sell),
                format!("{:.2}", dev_spot),
                format!("{:.2}", fee_taker),
                format!("{:.2}", fee_maker),
                format!("{:.2}", fee_mixed),
                format!("{:.2}", net_buy_taker),
                format!("{:.2}", net_sell_taker),
                format!("{:.2}", net_buy_maker),
                format!("{:.2}", net_sell_maker),
                format!("{:.2}", net_buy_mixed),
                format!("{:.2}", net_sell_mixed),
                csym.clone(),
                psym.clone(),
            ])?;
            rows += 1;
            println!(
                "[binance] K={} dev_perp={:.2} funding_est={:.2} net_taker_buy={:.2}/sell={:.2} net_mixed_buy={:.2}/sell={:.2}",
                strike, dev_mid, funding_est_short, net_buy_taker, net_sell_taker, net_buy_mixed, net_sell_mixed
            );
        }
        wtr.flush()?;
        ticks += 1;
        let elapsed = tick_start.elapsed();
        if elapsed < Duration::from_millis(interval_ms) {
            tokio::time::sleep(Duration::from_millis(interval_ms) - elapsed).await;
        }
    }
    println!(
        "[binance] 完成: {} ticks, {} 列, dev_mid_perp 範圍 [{:.2}, {:.2}], net_taker>0 買 {} / 賣 {} 次, net_mixed>0 買 {} / 賣 {} 次, 寫入 {}",
        ticks, rows, dev_min, dev_max, net_taker_buy_pos, net_taker_sell_pos, net_mixed_buy_pos, net_mixed_sell_pos, output
    );
    Ok(())
}
