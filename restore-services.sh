#!/bin/bash
# restore-services.sh — PCP 一直錄的換機恢復腳本 (冪等，可重複跑)
#
# 背景：2026-11-24 17:13 VM 被換掉，/etc/systemd/system/pcp-recorder@.service
# 跟著消失，六間錄製全停、沒自己回來。/etc 不會跟著 home 留下來，所以
# unit 的正本存在 home (systemd/pcp-recorder@.service)，這支腳本負責裝回去。
#
# 做法：
#   1. /etc 裡沒有 unit 檔 → 從 home 複製過去，再 daemon-reload。
#   2. 六間服務沒在跑 → enable --now 啟動（已在跑的就跳過，不重啟）。
# 什麼都不缺時，什麼都不做。
# 注意：如果使用者明確說要停錄，必須同時停掉每小時檢查排程，否則這支腳本
# 會在下次檢查時又把它們裝回來。

set -u
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIT_DST="/etc/systemd/system/pcp-recorder@.service"
EXCHANGES="bybit gate okx binance umx derive"

# 檢查 recorder binary 還在不在（換機後 home 會留下來，照理應該在）
if [ ! -x "$PROJECT/bin/recorder-current" ]; then
  echo "restore: binary 不見了：$PROJECT/bin/recorder-current，請跑 sudo bash install.sh" >&2
  exit 1
fi

CHANGED=0
if [ ! -f "$UNIT_DST" ]; then
  # unit 不見了 (換機): 用 install.sh 重裝 systemd 設定, 不重新編譯
  bash "$PROJECT/install.sh" --no-build
  echo "restore: 已用 install.sh --no-build 裝回 systemd 設定"
  CHANGED=1
fi

for ex in $EXCHANGES; do
  if ! systemctl is-active --quiet "pcp-recorder@$ex.service"; then
    systemctl enable --now "pcp-recorder@$ex.service"
    echo "restore: 已啟動 pcp-recorder@$ex"
    CHANGED=1
  fi
done

if [ "$CHANGED" -eq 0 ]; then
  echo "restore: 六間都正常，不用動"
fi
exit 0
