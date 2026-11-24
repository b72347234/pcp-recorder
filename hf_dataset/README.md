---
license: cc0-1.0
task_categories:
- tabular-regression
tags:
- crypto
- options
- order-book
- put-call-parity
- market-data
---

# Crypto Options Order Book Snapshots — PCP Arbitrage Research

Daily snapshots of crypto options order books (best bid/ask) plus perpetual
quotes, recorded from public exchange APIs for put–call parity (PCP) research.
No orders are placed; this is observation-only market data.

## Exchanges (6)

| Exchange | Settle | Notes |
|---|---|---|
| bybit | USDT | options + linear perp, via public WebSocket |
| gate | USDT | options + perp |
| okx | USD_UM | USD-margined options (`BTC-USD_UM` family) + perp |
| binance | USDT | options + perp |
| umx | USDT | BTC/ETH only |
| derive | USDC | options + perp |

## Files

One gzip file per exchange per day (Taipei date), under `YYYY-MM-DD/`:

```
2026-11-24/pcp_gate_20261124.csv.gz
2026-11-24/pcp_bybit_20261124.csv.gz
...
```

Each CSV is one row per option pair snapshot (same underlying, strike and
expiry, call + put together with the perpetual quote). About 3.8 GB/day raw,
~0.76 GB/day gzipped. A new folder is added every morning (Taipei time).

## Columns (40)

- `ts`, `ts_ms`: snapshot timestamp (ISO-8601 UTC, epoch ms)
- `exchange`, `settle`: exchange, settlement currency
- `expiry`: expiry as Unix seconds (options expiry, 08:00 UTC, except
  Binance XAU/XAG which expire 22:00 UTC)
- `strike`: strike price
- `spot_bid`, `spot_ask`, `spot_mid`: spot best bid/ask/mid
  (Binance XAU/XAG have no spot; perpetual index is used as proxy)
- `perp_bid`, `perp_ask`, `perp_mid`, `perp_mark`, `perp_index`: perpetual
  contract quotes
- `basis_usdt`: `perp_mid - spot_mid`
- `funding_rate_8h`, `hours_to_expiry`, `funding_est_short_usdt`: funding
  rate and its estimated cost over the remaining time to expiry
- `c_bid`, `c_ask`, `c_mid`, `p_bid`, `p_ask`, `p_mid`: call/put best
  bid/ask/mid (executable prices, not marks)
- `dev_mid_perp_usdt`, `dev_mid_perp_pct`: PCP deviation
  `(C - P) - (perp_mid - K)`, in USDT and percent
- `dev_buy_perp_usdt`, `dev_sell_perp_usdt`: deviation on executable
  bid/ask (buy/sell the synthetic)
- `dev_mid_spot_usdt`: deviation against spot instead of perp
- `fee_taker_usdt`, `fee_maker_usdt`, `fee_mixed_usdt`: worst-case
  (VIP0/Regular, no discounts, no rebates) round-trip fees per 1 unit of
  underlying for all-taker / all-maker / options-maker + perp-taker
- `net_buy_taker_usdt`, `net_sell_taker_usdt`, `net_buy_maker_usdt`,
  `net_sell_maker_usdt`, `net_buy_mixed_usdt`, `net_sell_mixed_usdt`:
  deviation minus fees, i.e. net edge after costs (usually ≤ 0)
- `call_sym`, `put_sym`: exchange option symbols

## Data quality notes

- Drop rows whose field count is not 40 (a restart can tear a row).
- Binance small-cap options sometimes return placeholder quotes
  (`bid = 0.01`, `ask = 1.0`); filter rows where `|dev|` is implausibly
  large relative to spot (e.g. `|dev_mid_perp_usdt| > 0.1 * spot_mid`).
- Books thin out in the last ~30 minutes before expiry; the recorder
  tracks the nearest 2–3 expiries and rolls automatically.
- There is a gap 2026-11-24 17:13–18:04 Taipei time: the recording VM was
  replaced and the recorder had to be restored (no data was recorded then).

## License

CC0 — factual public market data collected from exchanges' public APIs.
