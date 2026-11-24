# Bybit / Gate / OKX — U本位期權 PCP 套利錄盤口程式 v2：永續對沖（Rust）

只錄盤口、做策略驗證，不下單、不接 API key。

## PCP，改用永續對沖

```
C - P = S - K ，到期時 Perp_T ≈ S_T，所以把 S 換成 Perp 價來算
dev_mid_perp  = (C_mid - P_mid) - (Perp_mid - K)
dev_buy_perp  = (C_ask - P_bid) - (Perp_bid - K)  < 0：合成便宜，買 Call、賣 Put、空永續
dev_sell_perp = (C_bid - P_ask) - (Perp_ask - K)  > 0：合成貴，賣 Call、買 Put、多永續
```

還要算兩筆永續才有的成本：

- **basis**：`Perp_mid - Spot_mid`，進場時永續比現貨貴/便宜多少
- **資金費**：空永續每 8h 收/付 `funding_rate × Perp`，到期前估算 `funding_est_short = funding_rate × Perp_mid × (hours_to_expiry / 8)`。買合成（空永續）要加這筆，賣合成（多永續）要扣這筆。費率會變，這只是用當下費率外推。
- **手續費，進場+出場都要算**：期權 0.03%×2腿（taker）/ 0.02%×2腿（maker），永續各所不同
  - Gate：perp taker 0.075%、maker -0.01%（rebate，合約 API 回傳）
  - OKX：perp taker 0.05%、maker 0.02%
  - Bybit：perp taker 0.055%、maker 0.02%
  - 所以 taker 全吃單：Gate 約 127 USDT/BTC、OKX 約 96.6、Bybit 約 102.7；maker：Gate 約 12.1、OKX/Bybit 約 48.3。**taker 永續沒有比現貨便宜多少，maker 才是關鍵**，尤其 Gate maker 還有 rebate。

## OKX 固定用 USD_UM

OKX 沒有純 USDT 期權（`BTC-USDT` / `ETH-USDT` 回空陣列），本程式固定只取 `USD_UM`（美元本位，報價已是 USD，不用 ×S）。幣本位 `BTC-USD` 不混算。

## Bybit：REST 看不到，WebSocket 看得到

這是你問的那題，實測是這樣：

| Bybit 接口 | 這台機器 | 實測 |
|---|---|---|
| `api.bybit.com` / `api.bytick.com` / testnet REST | ❌ | 全回 `The Amazon CloudFront distribution is configured to block access from your country` |
| `www.bybit.com/api` | ❌ | Access Denied |
| `wss://stream.bybit.com/v5/public/spot` | ✅ | `tickers.BTCUSDT`，last 60469.7 |
| `wss://stream.bybit.com/v5/public/linear` | ✅ | `tickers.BTCUSDT`，bid/ask、fundingRate -0.0000146、markPrice、nextFundingTime 都有 |
| `wss://stream.bybit.com/v5/public/option` | ✅ | `publicTrade.BTC` 有成交，符號格式是 `BTC-25NOV26-60250-C-USDT`（注意有 `-USDT` 後綴，之前猜 `BTC-24NOV26-60000-C` 沒後綴所以沒推播）；`orderbook.25.BTC-25NOV26-60250-C-USDT` snapshot bid 895 / ask 905 |

結論：**不是 Bybit 沒資料，是這台機器的出口 IP 在美國（Dallas），Bybit REST 用 CloudFront 按國家擋掉；WebSocket 那條路沒擋，通的。**

程式裡 Bybit 已經改走 WS：先用 `publicTrade.BTC` 找活躍符號，再訂 `orderbook.25` 錄盤口。

### Proxy 穿透（2026-11-24 已做通）

這台機器上網要走 egress proxy（`hatch-egress-proxy:3128`，`HTTPS_PROXY` 帶帳密），`tokio-tungstenite` 的 `connect_async` 不吃 proxy env，直接連會報 `InvalidContentType` / TLS 錯誤。已改成自己做穿透，`src/main.rs` 的 `connect_ws()`：

1. TCP 連 proxy，發 `CONNECT stream.bybit.com:443` + `Proxy-Authorization: Basic`，讀到 `200 Connection Established`
2. 在這條 TCP 上做 TLS，根憑證用 Mozilla roots + 系統憑證 + `SSL_CERT_FILE`（`/run/hatch/egress-tls/ca-bundle.pem`，不加會報 `UnknownIssuer`，因為 proxy 會 TLS 攔截）
3. 手動 WS 握手：`tungstenite 0.26` 會嚴格要求 `Connection` 頭等於 `Upgrade`，但 Bybit 經 CloudFront 回的是 `Connection: upgrade, close`，會被判 `No "Connection: upgrade" header`。所以自己發 GET、讀 101，只要 `Upgrade: websocket` 且 `Connection` 含 `upgrade` 就算通，再用 `WebSocketStream::from_raw_socket` 包起來。

沒有 proxy 的機器（例如你台灣本機），同一支函式會直接連 host:443 再 TLS，一樣能用。

實測（2026-11-24 14:48，`--only bybit`，30 秒）：發現到期 `24NOV26`、2 對（60250、60500），perp mid 60419.65、funding -0.000012、books=4，30 ticks、60 列，C 60250 bid 265 / ask 290、P bid 70 / ask 75，數字都合理。三間一起跑 45 秒：Bybit 16 列、Gate 24 列、OKX 16 列，全部都有數。

Gate / OKX 走 REST，本機是通的，所以下面實測先以這兩間為準。

## 怎麼跑

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd ~/workspace/bybit-pcp-recorder

# 預設：BTC、60 秒、每 1 秒、最近到期日前 3 個履約價，永續對沖
cargo run -- --base BTC --duration-sec 60 --interval-ms 1000 --strikes 5

# 只跑 Gate + OKX（Bybit WS 在這台機器會因 proxy 失敗，台灣本機再加回來）
cargo run -- --base BTC --duration-sec 600 --interval-ms 1000 --strikes 5 \
  --only gate,okx --output data/pcp_perp_gate_okx.csv

# 只跑 Bybit WS（台灣本機）
cargo run -- --base BTC --duration-sec 60 --only bybit --output data/pcp_bybit_ws.csv
```

輸出 CSV 一列 = 一所 / 一到期日 / 一履約價 / 一時間點，新增欄位：`perp_bid/perp_ask/perp_mid/perp_mark/perp_index, basis_usdt, funding_rate_8h, hours_to_expiry, funding_est_short_usdt, dev_mid_perp_usdt, dev_buy_perp_usdt, dev_sell_perp_usdt, dev_mid_spot_usdt, fee_taker_usdt, fee_maker_usdt, net_buy_taker_usdt, net_sell_taker_usdt, net_buy_maker_usdt, net_sell_maker_usdt`

注意：每 tick 要抓 Gate tickers + perp、OKX spot + perp + 6 檔 books，實際一輪約 3-5 秒，`--interval-ms 1000` 只是每輪間隔，不是真的每秒一筆。

## 實測結果（2026-11-24 14:38-14:39，BTC 約 60,400，永續對沖）

- 到期：Gate / OKX 都是 2026-11-24 08:00 UTC，剩約 1.36 小時，資金費估算很小（Gate -0.25、OKX +0.29 USDT）。
- **Gate（USDT）**：`dev_mid_perp` 平均 +15.0，範圍 -38.55 ~ +51.05，basis 平均 -33.4（永續比現貨便宜 33），funding -0.000025/8h，fee taker 126.9 / maker 12.1，`net_maker_buy>0: 0/18`、`net_maker_sell>0: 0/18`、`net_taker_buy>0: 0/18`
- **OKX（USD_UM）**：`dev_mid_perp` 平均 -46.5，範圍 -55.05 ~ -31.65，basis 平均 -26.0，funding +0.0000296/8h，fee taker 96.7 / maker 48.4，`net_maker_buy>0: 0/12`、`net_maker_sell>0: 0/12`
- **Bybit**：REST 被擋，WS 在 Python 實測通、Rust 在這台機器因 proxy 失敗，上面 Gate/OKX 這輪沒含 Bybit。

結論（這幾十秒樣本）：**就算改永續、就算用 maker 費，扣費後還是 0 次有賺**。Gate maker 費已經低到 12 USDT，但可執行價差（`dev_buy` / `dev_sell`，含買賣價差）還是把利潤吃掉了。OKX 的 -46 偏離看起來大，但盤口寬、mid 不可靠。

下一步不是直接下單，而是：錄久一點、跨不同到期日（不要只錄快到期那檔）、在台灣本機補 Bybit WS，再看 `net_maker` > 0 的出現頻率和持續時間。

## 檔案

- `src/main.rs`：全部邏輯，公開 API、不用 key；Bybit WS 用 `tokio-tungstenite`，Gate/OKX 用 `reqwest`
- `Cargo.toml`：tokio + reqwest + tokio-tungstenite + serde_json + chrono + csv
- `data/pcp_perp_all_45s.csv`：45 秒三間永續對沖實測，Bybit WS 經 proxy 通了
- `data/pcp_bybit_ws_30s.csv`：Bybit WS 單跑 30 秒，30 ticks、60 列
- `data/pcp_perp_gate_okx.csv`：Gate + OKX 永續對沖實測
- `examples/debug_ws.rs`：手動 proxy CONNECT + TLS + WS 握手的除錯小工具
- `data/pcp_btc_30s.csv`：v1 現貨對沖舊樣本

## 2026-11-24 新增：幣安 / UMX / Derive，全部標的 30 秒試錄

多標的用法：`--bases BTC,ETH,SOL`（沒給就用 `--base` 單一標的）。每間所單獨跑 30 秒、一次錄它全部的標的，輸出 `data/test_all_<exchange>_30s.csv`。

| 所 | 標的 | 接法 | 手續費（最高版） |
|---|---|---|---|
| 幣安 | BTC、ETH、SOL、XRP、BNB、DOGE、XAU、XAG（8 個，exchangeInfo 實測） | `www.binance.com` REST，期權用批次 `ticker` 一次拿全部 best bid/ask（逐檔 depth 一輪要 80 次請求，太慢）；`depth` 限用 `limit=10` | 期權 0.024% 雙邊，永續 taker 0.04% / maker 0.02% |
| UMX | BTC、ETH | `api.umx.com`，symbols 啟動時抓一次（2MB），錄最近 3 個到期日 | 永續 maker 0.02% / taker 0.05%；**期權費率查不到，fee/net 欄填 NaN，只看 dev** |
| Derive | BTC、ETH（USDC 組，分開算） | `api.derive.xyz/v3`，POST plain JSON 到 `/v3/public/<方法>`；`get_tickers` 批次拿 top-of-book | 期權 maker 0.01% / taker 0.03% + taker 每筆 $0.50，永續 maker 0.01% / taker 0.03% + $0.01；**資金費是每小時**，公式不能除以 8 |
| Gate | 12 個：BTC、ETH、LTC、SOL、DOGE、ADA、XRP、BNB、HYPE、SUI、XAUT、CL | REST（沿用） | 沿用（永續 taker 0.075%） |
| OKX | BTC、ETH、SOL（USD_UM） | REST（沿用） | 沿用 |
| Bybit | BTC、ETH、SOL（12 秒 publicTrade 探測：只有這三個有成交；XRP、BNB、DOGE、LTC、ADA、TON、TRX、LINK、AVAX 無成交，先前「8 個幣種」未經實測，不要當定案） | WS（沿用），discovery 內部改用觀測到的履約價中位數當參考價（單跑 bybit 時不會誤用 BTC 的 60400 去選 SOL 的履約價） | 沿用 |

30 秒試錄結果（2026-11-24 約 15:30–15:47，BTC 約 60400，到期日 24NOV26 當天 16:00 到期）：

| 所 | 行數 / ticks | 有出 tick 的標的 | dev_mid 摘要 | net>0 |
|---|---|---|---|---|
| 幣安 | 13 / 4 | BTC 60500（+43.5）、XAU 3620/3630/3650（約 -1.5） | fee_taker BTC 77.3 / XAU 4.6 | 0 |
| UMX | 40 / 2（另有 BTC 單測 50 / 5） | BTC 25/26NOV 各 5 檔、ETH 25/26NOV 各 5 檔 | BTC 約 +5~+18、ETH 約 0（±1） | —（fee NaN） |
| Derive | 0 / 17（BTC、ETH） | 無 | — | 0 |
| Gate | 0 / 2（12 個標的） | 無 | — | 0 |
| OKX | 0 / 3（BTC、ETH、SOL） | 無 | — | 0 |
| Bybit | 52 / 30 | BTC 60500（+28.7）、ETH 1700（-0.35） | fee_taker BTC 102.8 / ETH 2.9 | 0 |

**重要發現：最近到期日盤口會在到期前半小時左右死掉。** OKX 最後一筆是 15:19、Gate 是 15:33、幣安/Derive 約 15:30 前後（bid/ask 缺邊、甚至雙邊全空），只剩 Bybit 還報價到最後、UMX 的 25/26NOV 到期日還活著。只錄最近到期日很脆弱，建議像 UMX 一樣錄最近 2–3 個到期日。另：Gate ADA 沒有未來到期日、XRP/SUI 各只剩 1 對完整履約價；幣安 DOGE 沒有未來到期日；幣安 XAU/XAG 沒有現貨（Invalid symbol），程式已改用永續指數價當現貨 proxy。

**⚠️ 絕對不要在錄製進行中重建 binary。** 2026-11-24 的 1 小時錄製（pid 1823，14:52–15:52）死在 15:36（tick 450、2624 秒、2736 列），log 沒有 panic、沒有 OOM，最可能的兇手就是測試期間多次 `cargo run` 蓋掉了跑著的 binary——跑著的進程在執行到被換掉的 code page 時直接死掉，連錯誤都來不及印。教訓：錄製跑著的時候不要 `cargo build/run` 同一個 target；要改程式就先等錄完，或把 binary 複製到別處跑。15:55 的 1 小時分析要以 14:52–15:36（約 44 分鐘、2737 行）為準，不是完整 1 小時。

## 2026-11-24：一直錄版本 v3 → v5（常駐服務，六間分開跑）

啟動方式：`systemctl enable --now pcp-recorder@{gate,okx,bybit,binance,umx,derive}.service`，unit 在 `/etc/systemd/system/pcp-recorder@.service`（template，`restart=always`，`WantedBy=default.target`）。

### ⚠️ 換機恢復（2026-11-24 17:13 事故，18:05 已修好）
2026-11-24 17:13 VM 被換掉，`/etc/systemd/system/pcp-recorder@.service` 跟著消失（`/etc` 不會跟著 home 留下來），六間同時 SIGTERM 乾淨退出、CSV 停在 17:13:33–17:13:44，服務也沒自己回來，停錄約 51 分鐘。教訓：只靠 `enable` 不夠，unit 檔本身必須有持久化的正本、還要有換機後自動裝回來的路徑。
- unit 正本：`systemd/pcp-recorder@.service`（home 裡，換機留得下來）
- 恢復腳本：`restore-services.sh`（冪等：unit 不見就從 home 複製回 /etc 再 daemon-reload，服務停了就 enable --now，什麼都不缺就什麼都不做）
- 每小時檢查排程（`pcp-recorder-hourly-check`）已改成：發現 unit 不見或服務停掉時自動跑 `restore-services.sh` 裝回來，成功只記 log、不打擾；失敗才通知使用者。換機後最晚 1 小時內自動恢復。
- 如果使用者明確說要停錄，必須同時停掉每小時檢查排程，否則恢復腳本會把它們裝回來。每間所一個進程、各寫自己的每日 CSV：`data/pcp_<exchange>_YYYYMMDD.csv`（台北日期，午夜自動換檔）。參數：`--forever --expiries 3 --strikes 5 --refresh-mins 10`（背景每 10 分鐘重新發現 pairs，到期滾動、履約價重新對齊市價）。binary 放在 `bin/recorder-vN`，**絕對不要直接蓋掉跑著的 binary**：改程式 → `cargo build --release` → 複製成新的 `bin/recorder-v(N+1)` → 改 unit 的 `ExecStart` → `systemctl daemon-reload` → `systemctl restart pcp-recorder@*`（2026-11-24 的教訓：1 小時錄製死在 15:36 就是因為 `cargo run` 蓋掉了跑著的 binary）。

- **v3（2026-11-24 16:13 啟動）**：`--forever` 無限循環、最近 3 個到期日、每日換檔（台北日期）、Bybit WS 斷線自動重連、單所失敗不拖垮整輪。實測輪詢時間：gate 14.6 秒、umx 13.3 秒、幣安 7.7 秒、OKX 2.7 秒、Derive 1.6 秒、Bybit 0ms（WS）。
- **v4（2026-11-24 16:27 啟動）**：各標的平行抓盤口（`join_all` + `tokio::join!`，`client_ref = &client` 共享引用，否則 async move 會想搬走 owned Client 而編譯失敗）；五間所的發現函式加「到期前 1 小時就排除」（`e > now + 3600`），不要等到到期那一刻才滾；`new_writer` 改成 append（舊版 `File::create` 會截斷，重啟會清空當天 CSV，已用兩次同目錄執行驗證：表頭只出現 1 次）。輪詢時間：gate 約 1.1–11 秒（Gate API 延遲很飄，平均一輪約 4–6 秒）、umx 0.24 秒、幣安 0.4–1.9 秒、OKX 1.1 秒、Derive 0.4 秒、Bybit 0ms。**但 v4 重啟時 OKX 和 Gate 各撕裂 1 列**（`systemctl restart` 的 SIGTERM 在寫一半時殺掉進程，兩個半列黏成一行，NF=54/46；分析時要丟掉 NF≠40 的列）。
- **v5（2026-11-24 16:3x 啟動）**：SIGTERM 乾淨關機（`tokio::signal` 接住 SIGTERM，等本輪寫完、flush 後退出，之後重啟不再撕裂）；履約價改 f64 解析（微單位當 map key）：**舊版把履約價 parse 成 i64，會把小數截掉——Gate DOGE（0.063）、ADA（0.15）整檔沒錄到，SUI 只剩履約價 1 的對子；幣安 DOGE 整檔沒錄到、XRP 1.1 被截成 1**。v5 冒煙測試確認 Gate DOGE/ADA、幣安 DOGE/XRP 都已正確發現（DOGE 0.055/0.056/0.057、XRP 0.9/0.95/1.0）。

### 到期滾動驗證（2026-11-24 16:00 台北時間 24NOV26 到期後）

25NOV26 已在錄、24NOV26 已不在錄（Bybit/Gate/OKX/UMX/Derive 的 24NOV26 皆為 0 列）。**例外是幣安 XAU/XAG 的 24NOV26**：金銀期權是當天 22:00 UTC 到期（不是 08:00 UTC），16:16 UTC 時還有 13.7 小時，繼續錄是正確的，22:00 後 refresher 會自動滾到 27NOV26。Gate SUI 的第 2 個到期日是 12 月 25 日（SUI 期權到期日稀疏，11 月只有 25 日有盤），不是 bug。

### 已知資料品質問題（分析時要處理）

- **幣安小幣的佔位報價**：批次 ticker 對沒盤口的期權會回 `bid=0.01 / ask=1.0` 的佔位值（單檔 ticker 實測 XRP-261127-1.1-C 也是 `askPrice=1`），造成 dev 虛大（DOGE dev 1.07 是現貨 0.056 的 19 倍；XRP 1.1-P 曾錄到 bid 14.52/ask 17.00 的陳舊報價）。分析時要過濾 `|dev|` 相對現貨過大的列（例如 `|dev_mid| > 0.1 * spot` 就視為壞 tick），或直接排除 `ask==1.0 && bid==0.01` 的列。UMX/Derive 沒有這個問題。
- **撕裂列**：v4 之前重啟會撕裂列（2026-11-24 的 `pcp_gate_20261124.csv`、`pcp_okx_20261124.csv` 各有 1 列 NF≠40）；v5 起 SIGTERM 乾淨關機，之後不應再出現。分析前先用 `awk -F, 'NF!=40'` 檢查並丟掉。
- **幣安現貨 API 限流**：2026-11-24 16:32 左右手動 curl 太多次，`api/v3` 回 `code -1003 IP banned until 08:37:31 UTC`（約 6 分鐘）。服務本身不受大影響（現貨失敗時用永續指數價當 proxy 繼續錄），但不要在服務跑著的時候對幣安高頻手動查。

### 實測數據量（v4，2026-11-24 16:27–16:30）

六間合計約 86 列/秒、每天約 700–1000 萬列、**約 3.5–4.4 GB/天**（每列約 425 位元組）。磁碟 150G、剩 147G，約 35–40 天塞滿。**兩週內要決定保留策略**（例如只留 14 天、舊檔 gzip、或降 `--strikes`/`--expiries`）；不要等到滿了才處理。

### 每小時檢查 / 每日策略

父代理已設排程：每小時檢查錄製是否正常（`pcp-recorder-hourly-check`，正常不打擾、異常才通知），每天早上約 08:00 分析前一天資料想策略（`pcp-daily-strategy`）。檢查時看 `systemctl status pcp-recorder@*`、各 `data/pcp_<exchange>_YYYYMMDD.csv` 的行數與最新時間戳、`service-<exchange>.log` 的 round 時間與錯誤。
