# Crypto Options Order Book Snapshots — PCP Arbitrage Research

Daily snapshots of crypto options order books (best bid/ask) plus perpetual
quotes, recorded from public exchange APIs for put–call parity (PCP) research.
Observation only — no orders are placed.

## Exchanges (6)

bybit (USDT), gate (USDT), okx (USD_UM), binance (USDT), umx (USDT, BTC/ETH
only), derive (USDC).

## Files

One gzip file per exchange per day (Taipei date):

```
pcp_gate_20261124.csv.gz
pcp_bybit_20261124.csv.gz
...
```

Each CSV is one row per option pair snapshot (same underlying, strike and
expiry, call + put together with the perpetual quote). About 3.8 GB/day raw,
~0.76 GB/day gzipped. A new version is pushed every morning (Taipei time).

One dataset holds up to 100 GB (~130 days); when it fills up, a new dataset
is opened for the next quarter (e.g. `pcp-options-orderbook-2027q1`).

## Columns (40)

`ts`, `ts_ms`, `exchange`, `settle`, `expiry`, `strike`,
`spot_bid`, `spot_ask`, `spot_mid`,
`perp_bid`, `perp_ask`, `perp_mid`, `perp_mark`, `perp_index`,
`basis_usdt`, `funding_rate_8h`, `hours_to_expiry`, `funding_est_short_usdt`,
`c_bid`, `c_ask`, `c_mid`, `p_bid`, `p_ask`, `p_mid`,
`dev_mid_perp_usdt`, `dev_mid_perp_pct`, `dev_buy_perp_usdt`,
`dev_sell_perp_usdt`, `dev_mid_spot_usdt`,
`fee_taker_usdt`, `fee_maker_usdt`, `fee_mixed_usdt`,
`net_buy_taker_usdt`, `net_sell_taker_usdt`, `net_buy_maker_usdt`,
`net_sell_maker_usdt`, `net_buy_mixed_usdt`, `net_sell_mixed_usdt`,
`call_sym`, `put_sym`.

- `dev_*` = put–call parity deviation `(C - P) - (perp_mid - K)`.
- `fee_*` = worst-case round-trip fees (VIP0/Regular, no discounts, no
  rebates) per 1 unit of underlying.
- `net_*` = deviation minus fees (net edge after costs; usually ≤ 0).

## Data quality notes

- Drop rows whose field count is not 40 (a restart can tear a row).
- Binance small-cap options sometimes return placeholder quotes
  (`bid = 0.01`, `ask = 1.0`); filter rows where `|dev|` is implausibly
  large relative to spot.
- Books thin out in the last ~30 minutes before expiry; the recorder tracks
  the nearest 2–3 expiries and rolls automatically.
- Gap 2026-11-24 17:13–18:04 Taipei time: the recording VM was replaced and
  the recorder had to be restored.

## License

CC0 — factual public market data collected from exchanges' public APIs.
