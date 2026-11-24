#!/bin/bash
# cleanup-old-csv.sh — PCP 一直錄的舊檔清理 (只刪每日盤口 CSV, 不碰 log 與測試檔)
#
# 背景：六間錄製每天約 3.8GB, 磁碟剩 147GB, 約 38 天會塞滿。
# 策略 (2026-11-24 使用者問「要怎麼才可以一直錄、不斷」時提出)：
#   只留最近 14 天的每日 CSV, 約 53GB, 夠每日策略與回測用。
# 用法：
#   bash cleanup-old-csv.sh            # 真刪 (給排程用)
#   bash cleanup-old-csv.sh --dry-run  # 只列出會刪的檔, 不刪
#   DATA_DIR=/tmp/xxx bash cleanup-old-csv.sh --dry-run   # 測試用
# 安全規則：
#   - 只刪檔名符合 pcp_<binance|bybit|derive|gate|okx|umx>_YYYYMMDD.csv 的檔
#   - 只刪 mtime 超過 14 天的檔 (find -mtime +14), 今天的檔絕對不會被刪
#   - 絕不刪 service-*.log、pcp.csv、測試檔 (test_*.csv、*_30s.csv 等)
#   - 每次執行都把結果寫進 data/cleanup.log (刪了幾個、哪幾個、省了多少)
# 注意：刪掉的舊檔救不回來。14 天是使用者在 2026-11-24 同意的保留天數，
# 要改天數就改下面的 RETENTION_DAYS, 不要直接改 find 的參數。

set -u
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${DATA_DIR:-$PROJECT/data}"
LOG_FILE="$DATA_DIR/cleanup.log"
RETENTION_DAYS=14
# 上傳用的壓縮暫存檔 (upload_staging/<日期>/) 上傳完就沒用了, 只留 3 天以便重傳
STAGING_DIR="$PROJECT/upload_staging"
STAGING_RETENTION_DAYS=3
DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

if [ ! -d "$DATA_DIR" ]; then
  echo "cleanup: 找不到資料夾：$DATA_DIR" >&2
  exit 1
fi

# 列出符合條件的舊檔 (只限六間的每日 CSV)
mapfile -t OLD_FILES < <(find "$DATA_DIR" -maxdepth 1 -type f \
  -regextype posix-extended \
  -regex '.*/pcp_(binance|bybit|derive|gate|okx|umx)_[0-9]{8}\.csv' \
  -mtime +"$RETENTION_DAYS" 2>/dev/null)

STAMP="$(date '+%Y-%m-%d %H:%M:%S %Z')"

# 先清上傳暫存 (只刪 upload_staging/YYYYMMDD 這種日期資料夾)
if [ -d "$STAGING_DIR" ] && [ "$DRY_RUN" -eq 0 ]; then
  find "$STAGING_DIR" -mindepth 1 -maxdepth 1 -type d -regextype posix-extended \
    -regex '.*/[0-9]{8}' -mtime +"$STAGING_RETENTION_DAYS" -exec rm -rf {} + 2>/dev/null
fi
if [ "${#OLD_FILES[@]}" -eq 0 ]; then
  echo "[$STAMP] cleanup: 沒有超過 ${RETENTION_DAYS} 天的每日 CSV, 不用刪" | tee -a "$LOG_FILE"
  exit 0
fi

TOTAL_BYTES=0
for f in "${OLD_FILES[@]}"; do
  TOTAL_BYTES=$((TOTAL_BYTES + $(stat -c %s "$f" 2>/dev/null || echo 0)))
done

if [ "$DRY_RUN" -eq 1 ]; then
  echo "cleanup (dry-run): 會刪 ${#OLD_FILES[@]} 個檔, 共 $((TOTAL_BYTES / 1024 / 1024)) MB:"
  printf '  %s\n' "${OLD_FILES[@]}"
  exit 0
fi

for f in "${OLD_FILES[@]}"; do
  rm -f "$f"
done
{
  echo "[$STAMP] cleanup: 刪了 ${#OLD_FILES[@]} 個超過 ${RETENTION_DAYS} 天的每日 CSV, 省了 $((TOTAL_BYTES / 1024 / 1024)) MB"
  printf '  %s\n' "${OLD_FILES[@]}"
} | tee -a "$LOG_FILE"
exit 0
