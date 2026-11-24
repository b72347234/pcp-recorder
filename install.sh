#!/bin/bash
# install.sh — 在一台全新的 Linux 機器 (Oracle Cloud Ubuntu / Debian) 上裝好 PCP 常駐錄製。
#
# 用法 (在專案資料夾裡):
#   sudo bash install.sh            # 安裝 / 更新 (可重複跑, 冪等)
#   sudo bash install.sh --no-build # 只重裝 systemd 設定, 不重新編譯
#
# 做的事:
#   1. apt 裝編譯工具、rclone、python3-venv
#   2. 幫執行使用者裝 Rust (沒有才裝)
#   3. cargo build --release, 複製成 bin/recorder-<時間>, 把 bin/recorder-current 指過去
#      (不會蓋掉正在跑的 binary)
#   4. 建上傳用的 Python venv (huggingface_hub、kaggle)
#   5. 把 systemd/ 的樣板換上實際路徑, 裝到 /etc/systemd/system/
#   6. 啟動六間錄製服務 + 每日上傳 / 清理排程; 已在跑的服務會重啟以換上新 binary
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "請用 sudo 執行: sudo bash install.sh" >&2
  exit 1
fi

PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_USER="${SUDO_USER:-root}"
RUN_HOME="$(getent passwd "$RUN_USER" | cut -d: -f6)"
EXCHANGES="gate okx bybit binance umx derive"
BUILD=1
[ "${1:-}" = "--no-build" ] && BUILD=0

echo "==> 專案: $PROJECT  執行使用者: $RUN_USER"
as_user() { sudo -u "$RUN_USER" -H bash -lc "$*"; }

# 1. 系統套件
if command -v apt-get >/dev/null; then
  echo "==> 安裝系統套件"
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq build-essential pkg-config curl git rclone python3-venv ca-certificates >/dev/null
else
  echo "找不到 apt-get, 請手動安裝: build-essential pkg-config curl git rclone python3-venv" >&2
fi

mkdir -p "$PROJECT/data" "$PROJECT/bin"
# 舊版服務以 root 跑, 留下的每日 CSV / 上傳狀態檔是 root 的; 現在改用 RUN_USER 跑,
# 要把擁有者一起轉過來, 否則 append 開檔會失敗、服務一直重啟。
chown -R "$RUN_USER": "$PROJECT/data" "$PROJECT/bin"
for p in upload_staging kaggle_dataset upload_state.json upload.log; do
  [ -e "$PROJECT/$p" ] && chown -R "$RUN_USER": "$PROJECT/$p"
done

if [ "$BUILD" -eq 1 ]; then
  # 2. Rust
  if ! as_user 'command -v cargo >/dev/null || [ -x "$HOME/.cargo/bin/cargo" ]'; then
    echo "==> 安裝 Rust"
    as_user 'curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
  fi
  # 3. 編譯並換上新 binary
  echo "==> 編譯 (ARM 4 核約 3-5 分鐘)"
  as_user "cd '$PROJECT' && \$HOME/.cargo/bin/cargo build --release"
  NEW_BIN="$PROJECT/bin/recorder-$(date +%Y%m%d-%H%M%S)"
  cp "$PROJECT/target/release/bybit-pcp-recorder" "$NEW_BIN"
  chown "$RUN_USER": "$NEW_BIN"
  ln -sfn "$(basename "$NEW_BIN")" "$PROJECT/bin/recorder-current"
  chown -h "$RUN_USER": "$PROJECT/bin/recorder-current"
  echo "==> 新 binary: $NEW_BIN"
  # 只留最近 3 個舊 binary
  ls -1t "$PROJECT"/bin/recorder-2* 2>/dev/null | tail -n +4 | xargs -r rm -f
fi

if [ ! -x "$PROJECT/bin/recorder-current" ]; then
  echo "bin/recorder-current 不存在, 請先不加 --no-build 跑一次" >&2
  exit 1
fi

# 4. 上傳用 venv
if [ ! -x "$PROJECT/.venv-upload/bin/python" ]; then
  echo "==> 建立上傳用 Python venv"
  as_user "python3 -m venv '$PROJECT/.venv-upload'"
fi
as_user "'$PROJECT/.venv-upload/bin/pip' install -q --upgrade huggingface_hub kaggle"

# 上傳設定檔: 沒有就從範例複製一份
if [ ! -f "$PROJECT/upload.env" ]; then
  cp "$PROJECT/upload.env.example" "$PROJECT/upload.env"
  chown "$RUN_USER": "$PROJECT/upload.env"
  chmod 600 "$PROJECT/upload.env"
fi

# 5. systemd
echo "==> 安裝 systemd 設定"
for f in pcp-recorder@.service pcp-upload.service pcp-upload.timer pcp-cleanup.service pcp-cleanup.timer; do
  sed -e "s#@PROJECT@#$PROJECT#g" -e "s#@USER@#$RUN_USER#g" \
    "$PROJECT/systemd/$f" > "/etc/systemd/system/$f"
done
systemctl daemon-reload

# 6. 啟動
for ex in $EXCHANGES; do
  if systemctl is-active --quiet "pcp-recorder@$ex"; then
    systemctl restart "pcp-recorder@$ex"
  else
    systemctl enable --now "pcp-recorder@$ex" >/dev/null 2>&1
  fi
done
systemctl enable --now pcp-upload.timer pcp-cleanup.timer >/dev/null 2>&1

sleep 3
echo
echo "==> 狀態"
for ex in $EXCHANGES; do
  printf "  %-8s %s\n" "$ex" "$(systemctl is-active "pcp-recorder@$ex")"
done
systemctl list-timers 'pcp-*' --no-pager | head -5
echo
if ! as_user "rclone listremotes" | grep -q .; then
  echo "提醒: 還沒設定 Google 雲端硬碟。請照 ORACLE_SETUP.md 的「設定 Google 雲端硬碟」做 rclone config。"
fi
echo "完成。資料在 $PROJECT/data/, log 在 data/service-<交易所>.log, 上傳 log 在 upload.log。"
