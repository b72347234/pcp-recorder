use std::time::Duration;
#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    for (label, body) in [
        ("perp tickers", r#"{"instrument_type":"perp"}"#),
        ("option tickers 20261124", r#"{"instrument_type":"option","currency":"BTC","expiry_date":"20261124"}"#),
    ] {
        match client.post("https://api.derive.xyz/v3/public/get_tickers")
            .header("User-Agent", "pcp-recorder/0.1 rust")
            .header("Content-Type", "application/json")
            .body(body.to_string()).send().await {
            Ok(r) => {
                let status = r.status();
                let txt = r.text().await.unwrap_or_default();
                println!("{} -> {} len={} head={}", label, status, txt.len(), &txt[..txt.len().min(300)]);
            }
            Err(e) => println!("{} -> ERR {:?}", label, e),
        }
    }
}
