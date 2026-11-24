use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use base64::{Engine as _, engine::general_purpose::STANDARD};
#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = "wss://stream.bybit.com/v5/public/option";
    let host = "stream.bybit.com";
    let proxy = std::env::var("HTTPS_PROXY").unwrap();
    println!("proxy present, len {}", proxy.len());
    let no_scheme = proxy.strip_prefix("http://").unwrap().to_string();
    let (auth, hostport) = no_scheme.split_once('@').unwrap();
    let (phost, pport) = hostport.split_once(':').unwrap();
    let mut s = TcpStream::connect((phost, pport.parse::<u16>().unwrap())).await.unwrap();
    let req = format!("CONNECT {}:443 HTTP/1.1\r\nHost: {}:443\r\nProxy-Authorization: Basic {}\r\nProxy-Connection: Keep-Alive\r\n\r\n", host, host, STANDARD.encode(auth.as_bytes()));
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new(); let mut b=[0u8;1];
    loop { let n=s.read(&mut b).await.unwrap(); if n==0 {break;} buf.push(b[0]); if buf.ends_with(b"\r\n\r\n") {break;} }
    println!("CONNECT resp: {}", String::from_utf8_lossy(&buf));
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    { let r = rustls_native_certs::load_native_certs(); for c in r.certs { let _ = roots.add(c); } }
    if let Ok(path) = std::env::var("SSL_CERT_FILE") { if let Ok(pem)=std::fs::read(&path) { let mut rd=std::io::BufReader::new(&pem[..]); for cert in rustls_pemfile::certs(&mut rd) { if let Ok(c)=cert { let _=roots.add(c); } } } }
    let config = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let sn = ServerName::try_from(host.to_string()).unwrap();
    let mut tls = connector.connect(sn, s).await.unwrap();
    println!("TLS ok, sending WS handshake");
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let hs = format!("GET /v5/public/option HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\nOrigin: https://www.bybit.com\r\nUser-Agent: Mozilla/5.0\r\n\r\n", host, key);
    tls.write_all(hs.as_bytes()).await.unwrap();
    let mut resp = Vec::new(); let mut b2=[0u8;1];
    loop { let n=tls.read(&mut b2).await.unwrap(); if n==0 {break;} resp.push(b2[0]); if resp.ends_with(b"\r\n\r\n") {break;} if resp.len()>8192 {break;} }
    println!("WS handshake resp:\n{}", String::from_utf8_lossy(&resp));
}
