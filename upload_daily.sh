#!/bin/bash
# upload_daily.sh — PCP 盤口資料每日上傳 (Google 雲端硬碟 + HF + Kaggle)
# 由 pcp-upload.timer 每天 08:30 台北時間呼叫。設定 (含 token) 放在 upload.env, 不在這裡。
# 失敗只寫 upload.log, 不發通知。
set -u
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV_PY="$PROJECT/.venv-upload/bin/python"

if [ -f "$PROJECT/upload.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$PROJECT/upload.env"
  set +a
fi

exec "$VENV_PY" "$PROJECT/upload_daily.py" "$@" >> "$PROJECT/upload.log" 2>&1
