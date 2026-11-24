// 探測 Bybit 期權 publicTrade 有成交的標的: 一條 WS 連線同時訂閱多個 publicTrade.<base>, 看 10 秒內哪些 base 出現成交符號。
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let host = "stream.bybit.com";
    let proxy = std::env::var("HTTPS_PROXY").unwrap();
    let no_scheme = proxy.strip_prefix("http://").unwrap().to_string();
    let (auth, hostport) = no_scheme.split_once('@').unwrap();
    let (phost, pport) = hostport.split_once(':').unwrap();
    let mut s = TcpStream::connect((phost, pport.parse::<u16>().unwrap())).await.unwrap();
    let req = format!("CONNECT {}:443 HTTP/1.1\r\nHost: {}:443\r\nProxy-Authorization: Basic {}\r\nProxy-Connection: Keep-Alive\r\n\r\n", host, host, STANDARD.encode(auth.as_bytes()));
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new(); let mut b=[0u8;1];
    loop { let n=s.read(&mut b).await.unwrap(); if n==0 {break;} buf.push(b[0]); if buf.ends_with(b"\r\n\r\n") {break;} }
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    { let r = rustls_native_certs::load_native_certs(); for c in r.certs { let _ = roots.add(c); } }
    if let Ok(path) = std::env::var("SSL_CERT_FILE") { if let Ok(pem)=std::fs::read(&path) { let mut rd=std::io::BufReader::new(&pem[..]); for cert in rustls_pemfile::certs(&mut rd) { if let Ok(c)=cert { let _=roots.add(c); } } } }
    let config = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let sn = ServerName::try_from(host.to_string()).unwrap();
    let tls = connector.connect(sn, s).await.unwrap();
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let hs = format!("GET /v5/public/option HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\nOrigin: https://www.bybit.com\r\nUser-Agent: Mozilla/5.0\r\n\r\n", host, key);
    let mut tls = tls;
    tls.write_all(hs.as_bytes()).await.unwrap();
    let mut resp = Vec::new(); let mut b2=[0u8;1];
    loop { let n=tls.read(&mut b2).await.unwrap(); if n==0 {break;} resp.push(b2[0]); if resp.ends_with(b"\r\n\r\n") {break;} if resp.len()>8192 {break;} }
    println!("handshake: {}", String::from_utf8_lossy(&resp).lines().next().unwrap_or(""));
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(tls, Role::Client, None).await;
    let (mut write, mut read) = ws.split();
    let bases = ["BTC","ETH","SOL","XRP","BNB","DOGE","LTC","ADA","TON","TRX","LINK","AVAX"];
    for chunk in bases.chunks(6) {
        let args: Vec<String> = chunk.iter().map(|b| format!("publicTrade.{}", b)).collect();
        let sub = serde_json::json!({"op":"subscribe","args":args}).to_string();
        write.send(Message::Text(sub.into())).await.unwrap();
    }
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(12) {
        let msg = match tokio::time::timeout(Duration::from_secs(12), read.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        if let Message::Text(txt) = msg {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
                    for item in arr {
                        if let Some(s) = item.get("s").and_then(|x| x.as_str()) {
                            let base = s.split('-').next().unwrap_or("").to_string();
                            let e = seen.entry(base).or_default();
                            if !e.contains(&s.to_string()) && e.len() < 3 { e.push(s.to_string()); }
                        }
                    }
                }
            }
        }
    }
    println!("=== 12 秒內有成交的 Bybit 期權標的 ===");
    for b in bases {
        match seen.get(b) {
            Some(syms) => println!("{}: 有成交, 例如 {:?}", b, syms),
            None => println!("{}: 無成交", b),
        }
    }
}
