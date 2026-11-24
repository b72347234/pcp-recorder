#!/usr/bin/env python3
"""PCP 盤口錄製資料每日上傳：Google 雲端硬碟 + Hugging Face 公開資料集 + Kaggle 公開資料集。

用法：
    upload_daily.py [--date YYYYMMDD] [--dry-run] [--only gdrive|hf|kaggle]

預設上傳「昨天」（台北日期）的六個 CSV，因為當天檔案還在寫、沒寫完。
每個 CSV 以 gzip -1 壓縮後上傳（實測壓縮比約 5 倍，每天約 0.76 GB）。

憑證（絕對不要寫進這個檔案，也不要印進 log；設定放 upload.env）：
- Google 雲端硬碟：用 rclone，先跑 `rclone config` 建 remote（預設名稱 gdrive），
  上傳目的地由環境變數 GDRIVE_REMOTE 指定，預設 gdrive:pcp-recorder，
  檔案放在 <GDRIVE_REMOTE>/<YYYY-MM-DD>/pcp_<ex>_<date>.csv.gz。
- HF：環境變數 HF_TOKEN，或事先跑 `huggingface-cli login`（token 存在
  ~/.cache/huggingface/token）。資料集 repo 由環境變數 HF_DATASET_REPO 指定，
  例如 liumuse/pcp-options-orderbook。
- Kaggle：~/.kaggle/kaggle.json（使用者到 kaggle.com/settings/api 產生 API token），
  資料集 slug 由環境變數 KAGGLE_DATASET_SLUG 指定，例如
  liumuse/pcp-options-orderbook-2026q4。Kaggle 單一資料集上限 100 GB，
  快滿時腳本會拒絕上傳並在 log 寫明要開新一季的資料集。

沒設定的目標會跳過，不算失敗。
冪等：Google 雲端硬碟用 rclone copy（同名同大小會跳過）；HF 會先列出 repo 已有檔案、跳過已上傳的；Kaggle 用本機 state 檔記錄
已推過的日期，同一天重跑會跳過。失敗時寫 log、不寄通知，exit code 非 0。
"""

import argparse
import gzip
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime, timedelta
from pathlib import Path

try:
    from zoneinfo import ZoneInfo
except ImportError:  # pragma: no cover
    ZoneInfo = None

PROJECT = Path(__file__).resolve().parent
DATA_DIR = PROJECT / "data"
STAGING_DIR = PROJECT / "upload_staging"
# Kaggle 版本是整包快照：mirror 必須是 kaggle_dataset 本身（平鋪），
# 因為 `kaggle datasets version` 預設 dir-mode 是 skip，會忽略子目錄。
# 2026-11-24 教訓：原本 mirror 設在 kaggle_dataset/data，推上去的新版
# 只有 README.md，六個 gz 檔全被跳過。
KAGGLE_MIRROR_DIR = PROJECT / "kaggle_dataset"
STATE_FILE = PROJECT / "upload_state.json"
LOG_FILE = PROJECT / "upload.log"

EXCHANGES = ["bybit", "gate", "okx", "binance", "umx", "derive"]
TAIPEI = ZoneInfo("Asia/Taipei") if ZoneInfo else None

# Kaggle 單一資料集 100 GB 上限，留安全邊際，超過就停手等人工開新資料集。
KAGGLE_DATASET_LIMIT_BYTES = 95 * 1024**3


def log(msg: str) -> None:
    line = f"{datetime.now(TAIPEI).isoformat(timespec='seconds')} {msg}"
    print(line, flush=True)
    with open(LOG_FILE, "a", encoding="utf-8") as f:
        f.write(line + "\n")


def load_state() -> dict:
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text(encoding="utf-8"))
    return {"gdrive_uploaded": [], "hf_uploaded": [], "kaggle_uploaded": [],
            "kaggle_bytes": 0, "kaggle_dataset": None}


def save_state(state: dict) -> None:
    tmp = STATE_FILE.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=2, ensure_ascii=False), encoding="utf-8")
    tmp.replace(STATE_FILE)


def taipei_today() -> str:
    now = datetime.now(TAIPEI) if TAIPEI else datetime.now()
    return now.strftime("%Y%m%d")


def gzip_csv(src: Path, dst: Path) -> int:
    """gzip -1 壓縮，回傳壓縮後位元組數。"""
    dst.parent.mkdir(parents=True, exist_ok=True)
    with open(src, "rb") as fin, gzip.open(dst, "wb", compresslevel=1) as fout:
        shutil.copyfileobj(fin, fout, length=1024 * 1024)
    return dst.stat().st_size


def upload_gdrive(date_str: str, files: list[tuple[Path, str]], dry_run: bool,
                  state: dict) -> bool | None:
    """用 rclone 把 gz 檔傳到 Google 雲端硬碟。回傳 None 表示沒設定、跳過。"""
    remote = os.environ.get("GDRIVE_REMOTE", "gdrive:pcp-recorder").strip()
    if not remote or ":" not in remote:
        log("[gdrive] 未設定 GDRIVE_REMOTE，跳過 Google 雲端硬碟上傳")
        return None
    if not shutil.which("rclone"):
        log("[gdrive] 找不到 rclone，跳過（sudo apt install rclone）")
        return None
    name = remote.split(":", 1)[0] + ":"
    try:
        remotes = subprocess.run(["rclone", "listremotes"], capture_output=True,
                                 text=True, timeout=60).stdout.split()
    except Exception as e:  # noqa: BLE001
        log(f"[gdrive] rclone listremotes 失敗：{e}")
        return False
    if name not in remotes:
        log(f"[gdrive] rclone 還沒有 {name} 這個 remote，跳過（請先跑 rclone config）")
        return None
    dest = f"{remote.rstrip('/')}/{date_str[:4]}-{date_str[4:6]}-{date_str[6:]}"
    if dry_run:
        log(f"[gdrive] dry-run：會上傳 {len(files)} 個檔到 {dest}")
        return True
    ok = True
    for local, _ in files:
        try:
            r = subprocess.run(["rclone", "copy", str(local), dest, "--retries", "5"],
                               capture_output=True, text=True, timeout=3 * 3600)
        except Exception as e:  # noqa: BLE001
            log(f"[gdrive] 上傳 {local.name} 出錯：{e}")
            ok = False
            continue
        if r.returncode != 0:
            log(f"[gdrive] 上傳 {local.name} 失敗（exit {r.returncode}）：{r.stderr[-800:]}")
            ok = False
            continue
        log(f"[gdrive] 已上傳：{dest}/{local.name}（{local.stat().st_size / 1e6:.1f} MB）")
        key = f"{date_str}/{local.name}"
        if key not in state.setdefault("gdrive_uploaded", []):
            state["gdrive_uploaded"].append(key)
    save_state(state)
    return ok


def upload_hf(date_str: str, files: list[tuple[Path, str]], dry_run: bool,
              state: dict) -> bool | None:
    """files: [(本機 gz 路徑, repo 內路徑)]。回傳是否全部成功；None 表示沒設定、跳過。"""
    repo = os.environ.get("HF_DATASET_REPO", "")
    if not repo:
        log("[hf] 未設定 HF_DATASET_REPO，跳過 HF 上傳")
        return None
    from huggingface_hub import HfApi, list_repo_files

    token = os.environ.get("HF_TOKEN")
    if dry_run:
        log(f"[hf] dry-run：會上傳 {len(files)} 個檔到 {repo}")
        return True
    api = HfApi(token=token)
    try:
        existing = set(list_repo_files(repo, repo_type="dataset", token=token))
    except Exception as e:  # noqa: BLE001
        log(f"[hf] 列出 repo 檔案失敗：{e}")
        return False
    ok = True
    for local, repo_path in files:
        if repo_path in existing:
            log(f"[hf] 已存在，跳過：{repo_path}")
            state["hf_uploaded"].append(repo_path)
            continue
        try:
            api.upload_file(path_or_fileobj=str(local), path_in_repo=repo_path,
                            repo_id=repo, repo_type="dataset", token=token)
            log(f"[hf] 已上傳：{repo_path} ({local.stat().st_size / 1e6:.1f} MB)")
            state["hf_uploaded"].append(repo_path)
        except Exception as e:  # noqa: BLE001
            log(f"[hf] 上傳失敗 {repo_path}：{e}")
            ok = False
    save_state(state)
    return ok


def upload_kaggle(date_str: str, files: list[tuple[Path, str]], dry_run: bool,
                  state: dict) -> bool | None:
    """Kaggle 資料集是整包版本快照：把新檔放進本機 mirror 目錄再推一個新版。"""
    slug = os.environ.get("KAGGLE_DATASET_SLUG", "")
    if not slug:
        log("[kaggle] 未設定 KAGGLE_DATASET_SLUG，跳過 Kaggle 上傳")
        return None
    if date_str in state.get("kaggle_uploaded", []):
        log(f"[kaggle] {date_str} 已推過，跳過")
        return True
    if state.get("kaggle_dataset") and state["kaggle_dataset"] != slug:
        log(f"[kaggle] state 記錄的資料集是 {state['kaggle_dataset']}，"
            f"與目前 {slug} 不同，請人工確認後再跑")
        return False
    new_bytes = sum(p.stat().st_size for p, _ in files)
    if state.get("kaggle_bytes", 0) + new_bytes > KAGGLE_DATASET_LIMIT_BYTES:
        log(f"[kaggle] 加上今天 {new_bytes / 1e9:.2f} GB 後將超過 "
            f"{KAGGLE_DATASET_LIMIT_BYTES / 1e9:.0f} GB 安全上限，"
            "請開新一季資料集（例如把 2026q4 改成 2027q1）並更新 KAGGLE_DATASET_SLUG")
        return False
    if dry_run:
        log(f"[kaggle] dry-run：會把 {len(files)} 個檔放進 mirror 並推新版到 {slug}")
        return True
    KAGGLE_MIRROR_DIR.mkdir(parents=True, exist_ok=True)
    for local, _ in files:
        shutil.copy2(local, KAGGLE_MIRROR_DIR / local.name)
    meta_dir = PROJECT / "kaggle_dataset"
    cmd = [sys.executable, "-m", "kaggle", "datasets", "version",
           "-p", str(meta_dir), "-m", f"Add {date_str} order book snapshots"]
    env = dict(os.environ)
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=3600, env=env)
    except Exception as e:  # noqa: BLE001
        log(f"[kaggle] 推新版時出錯：{e}")
        return False
    if r.returncode != 0:
        log(f"[kaggle] 推新版失敗（exit {r.returncode}）："
            f"{(r.stderr or r.stdout)[-1000:]}")
        return False
    log(f"[kaggle] 已推新版到 {slug}：{date_str}（{new_bytes / 1e6:.1f} MB）")
    state["kaggle_uploaded"].append(date_str)
    state["kaggle_bytes"] = state.get("kaggle_bytes", 0) + new_bytes
    state["kaggle_dataset"] = slug
    save_state(state)
    return True


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--date", default=None,
                    help="要上傳的日期 YYYYMMDD，預設為昨天（台北日期）")
    ap.add_argument("--dry-run", action="store_true", help="只壓縮、不上傳")
    ap.add_argument("--only", choices=["gdrive", "hf", "kaggle"], default=None)
    args = ap.parse_args()

    if args.date:
        date_str = args.date
    else:
        y = (datetime.now(TAIPEI) - timedelta(days=1)) if TAIPEI else \
            (datetime.now() - timedelta(days=1))
        date_str = y.strftime("%Y%m%d")

    log(f"=== 上傳 {date_str}（dry-run={args.dry_run}）===")
    state = load_state()

    # 1. 壓縮：六個 CSV -> staging/<date>/pcp_<ex>_<date>.csv.gz
    gz_files: list[tuple[Path, str]] = []
    missing = []
    for ex in EXCHANGES:
        src = DATA_DIR / f"pcp_{ex}_{date_str}.csv"
        if not src.exists():
            missing.append(ex)
            continue
        dst = STAGING_DIR / date_str / f"pcp_{ex}_{date_str}.csv.gz"
        if dst.exists() and dst.stat().st_mtime >= src.stat().st_mtime:
            log(f"[gzip] 已壓縮過，跳過：{dst.name}")
        else:
            size = gzip_csv(src, dst)
            log(f"[gzip] {src.name} -> {dst.name}（{size / 1e6:.1f} MB）")
        gz_files.append((dst, f"{date_str[:4]}-{date_str[4:6]}-{date_str[6:]}/"
                              f"pcp_{ex}_{date_str}.csv.gz"))
    if missing:
        log(f"[warn] {date_str} 缺少這些所的檔案：{','.join(missing)}，繼續上傳有的部分")
    if not gz_files:
        log(f"[error] {date_str} 一個檔案都沒有，結束")
        return 2

    # 各目標獨立：一個失敗不影響其他；None = 沒設定、跳過
    results: dict[str, bool | None] = {}
    targets = [("gdrive", upload_gdrive), ("hf", upload_hf), ("kaggle", upload_kaggle)]
    for name, fn in targets:
        if args.only in (None, name):
            try:
                results[name] = fn(date_str, gz_files, args.dry_run, state)
            except Exception as e:  # noqa: BLE001
                log(f"[{name}] 未預期錯誤：{e}")
                results[name] = False
    done = [k for k, v in results.items() if v is True]
    failed = [k for k, v in results.items() if v is False]
    skipped = [k for k, v in results.items() if v is None]
    log(f"=== {date_str} 完成：成功 {done or '無'}、失敗 {failed or '無'}、"
        f"未設定跳過 {skipped or '無'} ===")
    if not done and not failed:
        log("[warn] 三個上傳目標都沒設定，資料只留在本機")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
