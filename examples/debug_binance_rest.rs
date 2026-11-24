use std::time::Duration;
#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    println!("HTTPS_PROXY set: {}", std::env::var("HTTPS_PROXY").is_ok());
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    for url in [
        "https://www.binance.com/eapi/v1/depth?symbol=BTC-261124-60000-C&limit=10",
        "https://www.binance.com/fapi/v1/depth?symbol=BTCUSDT&limit=5",
        "https://www.binance.com/fapi/v1/premiumIndex?symbol=BTCUSDT",
        "https://www.binance.com/api/v3/depth?symbol=BTCUSDT&limit=5",
    ] {
        match client.get(url).header("User-Agent", "pcp-recorder/0.1 rust").send().await {
            Ok(r) => {
                let status = r.status();
                let txt = r.text().await.unwrap_or_default();
                println!("{} -> {} len={} head={}", url, status, txt.len(), &txt[..txt.len().min(200)]);
            }
            Err(e) => println!("{} -> ERR {:?}", url, e),
        }
    }
}
