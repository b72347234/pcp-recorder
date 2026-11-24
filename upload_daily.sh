#!/bin/bash
# upload_daily.sh — PCP 盤口資料每日上傳（HF + Kaggle）
# 由每天 08:30 台北時間的排程呼叫。憑證不在這裡：HF 用 HF_TOKEN 環境變數或
# huggingface-cli login，Kaggle 用 ~/.kaggle/kaggle.json。
# 失敗只寫 upload.log，不發通知；連續失敗由排程檢查時回報。
set -u
PROJECT="/home/hatch/workspace/bybit-pcp-recorder"
VENV_PY="$PROJECT/.venv-upload/bin/python"

# HF 資料集 repo 與 Kaggle 資料集 slug：帳號建好、token 備妥後再填上。
export HF_DATASET_REPO="${HF_DATASET_REPO:-}"
export KAGGLE_DATASET_SLUG="${KAGGLE_DATASET_SLUG:-liumuse/pcp-options-orderbook-2026q4}"

exec "$VENV_PY" "$PROJECT/upload_daily.py" "$@" >> "$PROJECT/upload.log" 2>&1
