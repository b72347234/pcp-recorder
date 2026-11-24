use std::error::Error;
#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap();
    for url in ["https://api.gateio.ws/api/v4/spot/tickers?currency_pair=BTC_USDT", "https://www.okx.com/api/v5/market/ticker?instId=BTC-USDT", "https://api.gateio.ws/api/v4/options/tickers?underlying=BTC_USDT"] {
        println!("GET {}", url);
        match client.get(url).header("User-Agent", "pcp-recorder/0.1 rust").send().await {
            Ok(r) => println!("  status={} len={}", r.status(), r.text().await.map(|t| t.len()).unwrap_or(0)),
            Err(e) => {
                println!("  ERR: {:?}", e);
                let mut s = e.source();
                while let Some(src) = s { println!("    caused by: {}", src); s = src.source(); }
            }
        }
    }
}
