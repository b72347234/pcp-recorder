# 在 Oracle Cloud 永久免費 VM 上 24 小時錄製

整個流程大約 30 分鐘，分成四步：

1. 註冊 Oracle Cloud（約 10 分鐘，要用你本人的資料）
2. 開一台免費 VM（約 5 分鐘）
3. 連進 VM，貼指令安裝（約 10 分鐘，大多是等編譯）
4. 設定 Google 雲端硬碟上傳（約 5 分鐘）

裝好之後，六間交易所會 24 小時錄製。每天早上 08:30（台北時間）自動壓縮前一天的資料、上傳到你的 Google 雲端硬碟，10:30 自動刪掉本機超過 14 天的舊檔。

---

## 1. 註冊 Oracle Cloud

1. 打開 <https://www.oracle.com/cloud/free/>，按「Start for free」。
2. 填 email、國家（Taiwan），收驗證信。
3. **Home Region（主要區域）一定要選 `Japan East (Tokyo)`**，之後**不能改**。
   - 不要選美國的區域：Bybit、OKX 會封鎖美國 IP。
   - 東京連各交易所的延遲最低。如果東京一直開不到機器（見下面），註冊時改選 `Japan Central (Osaka)` 也可以。
4. 填地址、手機（收簡訊驗證碼）。
5. 綁信用卡：只是驗證身分，**不會扣款**（可能會先預扣約 1 美元再退回）。
6. 等帳號開通，通常幾分鐘，最久可能要等幾小時，開通後會收到 email。

### 強烈建議：升級成 Pay As You Go（還是免費）

免費帳號的 VM 如果連續 7 天 CPU 使用率都很低，Oracle 會把它**回收（停機）**。錄製程式很省 CPU，剛好會被判定成閒置。

解法：到 **Billing & Cost Management → Upgrade and Manage Payment** 升級成 **Pay As You Go**。
- 只要用量在 Always Free 額度內（下面開的機器就在額度內），**一樣是 0 元**
- 升級後不會被回收，開 ARM 機器也比較容易成功
- 怕被扣錢的話，可以到 **Budgets** 設一個 1 美元的預算提醒

---

## 2. 開一台免費 VM

1. 登入 <https://cloud.oracle.com>，左上角選單 → **Compute → Instances → Create instance**。
2. **Name**：`pcp-recorder`
3. **Image and shape** → Edit：
   - Image：按 **Change image** → **Ubuntu** → 選 **Canonical Ubuntu 24.04**（不要選 Minimal）
   - Shape：按 **Change shape** → **Ampere** → **VM.Standard.A1.Flex**，設 **4 OCPU、24 GB 記憶體**（這是免費額度的上限）
4. **Add SSH keys**：選 **Generate a key pair for me**，按 **Save private key**，把檔案（`ssh-key-xxxx.key`）存好，**這是之後連進去唯一的鑰匙**。
5. **Boot volume**：勾 **Specify a custom boot volume size**，填 **200** GB（免費額度上限，預設的 47 GB 不夠放 14 天的資料）。
6. 按 **Create**，等狀態變成綠色的 **Running**，記下 **Public IP address**。

> **出現 "Out of capacity" 錯誤？** 這表示東京的免費 ARM 機器暫時被開完了。可以：
> - 隔幾小時或半夜再試
> - 先改成 2 OCPU / 12 GB 試試
> - 升級成 Pay As You Go（見上面），幾乎都會馬上開成功

---

## 3. 連進 VM 安裝

### 連線

**Windows**：開「PowerShell」，輸入（換成你的金鑰檔路徑和 IP）：

```bash
ssh -i "C:\Users\你的名字\Downloads\ssh-key-xxxx.key" ubuntu@你的PublicIP
```

**Mac**：開「終端機」：

```bash
chmod 600 ~/Downloads/ssh-key-xxxx.key
```

```bash
ssh -i ~/Downloads/ssh-key-xxxx.key ubuntu@你的PublicIP
```

第一次連線會問 `Are you sure you want to continue connecting`，輸入 `yes`。

> Windows 如果出現 `UNPROTECTED PRIVATE KEY FILE`，在檔案總管對金鑰檔按右鍵 → 內容 → 安全性 → 進階 → 停用繼承 → 移除全部使用者，只加回你自己。
> 也可以不用自己的電腦：在 Oracle 網頁右上角開 **Cloud Shell**，把金鑰檔上傳上去再用同一個 ssh 指令。

### 安裝（貼這兩行）

```bash
git clone https://github.com/b72347234/pcp-recorder.git ~/pcp-recorder
```

```bash
cd ~/pcp-recorder && sudo bash install.sh
```

安裝程式會自動完成以下步驟：安裝套件 → 安裝 Rust → 編譯 → 啟動六間錄製服務 → 設定每日上傳和每日清理排程。最後應該看到六行 `active`：

```
  gate     active
  okx      active
  bybit    active
  ...
```

### 確認有在錄

```bash
ls -lh ~/pcp-recorder/data/
```

```bash
tail -5 ~/pcp-recorder/data/service-bybit.log
```

看到 `pcp_<交易所>_YYYYMMDD.csv` 這些檔案一直變大，就是在錄了。

---

## 4. 設定 Google 雲端硬碟上傳

VM 沒有瀏覽器，所以 Google 登入要在**你自己的電腦**上完成，再把登入結果（token）貼回 VM。

### 4-1. 在你自己的電腦裝 rclone

- **Windows**：到 <https://rclone.org/downloads/> 下載 `Windows - Intel/AMD - 64 Bit` 的 zip，解壓縮後在那個資料夾按 Shift + 右鍵 → 「在這裡開啟 PowerShell」
- **Mac**：終端機輸入 `brew install rclone`（沒有 brew 的話就到上面的網址下載）

### 4-2. 在 VM 上跑設定

```bash
rclone config
```

依序回答：

| 它問 | 你輸入 |
|---|---|
| `n) New remote` | `n` |
| `name>` | `gdrive` |
| `Storage>` | `drive`（或找 Google Drive 對應的數字） |
| `client_id>`、`client_secret>` | 直接按 Enter（留空） |
| `scope>` | `1`（完整存取） |
| `service_account_file>` | 直接按 Enter |
| `Edit advanced config?` | `n` |
| `Use web browser to automatically authenticate?` | **`n`** |

這時它會印出一行像這樣的指令：

```
rclone authorize "drive" "eyJzY29wZSI6ImRyaXZlIn0"
```

### 4-3. 在你自己的電腦完成登入

把上面那行**完整複製**，貼到你自己電腦的 PowerShell 或終端機執行（Windows 前面要加 `.\`，變成 `.\rclone authorize ...`）。瀏覽器會打開 Google 登入頁，選你的帳號並按允許。

終端機會印出一大段 `{"access_token":...}` 開頭的文字，**整段複製**。

### 4-4. 貼回 VM

回到 VM，在 `config_token>` 貼上那段文字，然後：

| 它問 | 你輸入 |
|---|---|
| `Configure this as a Shared Drive?` | `n` |
| `Keep this "gdrive" remote?` | `y` |
| 最後的選單 | `q` 離開 |

### 4-5. 測試上傳

```bash
rclone lsd gdrive:
```

能列出你雲端硬碟的資料夾就是成功了。上傳會放在雲端硬碟的 `pcp-recorder/YYYY-MM-DD/` 資料夾裡（就是第一次試錄時建的那個資料夾）。

要立刻試傳今天已經錄的部分（正常情況下每天 08:30 只傳前一天的資料）：

```bash
~/pcp-recorder/upload_daily.sh --date $(TZ=Asia/Taipei date +%Y%m%d) --only gdrive && tail ~/pcp-recorder/upload.log
```

---

## 選用：Hugging Face / Kaggle

編輯 `~/pcp-recorder/upload.env`（`nano ~/pcp-recorder/upload.env`）填上 `HF_DATASET_REPO`、`HF_TOKEN`、`KAGGLE_DATASET_SLUG`；Kaggle 另外要把 `kaggle.json` 放到 `~/.kaggle/`。沒填的就自動跳過，不影響 Google 雲端硬碟。

> Kaggle 每次推新版是整包快照，本機的 `kaggle_dataset/` 會每天多約 0.76 GB 而且不會被清理。200 GB 硬碟大約撐 3 個月，要用 Kaggle 的話記得定期檢查。

---

## 常用指令

| 想做什麼 | 指令 |
|---|---|
| 看六間狀態 | `systemctl status 'pcp-recorder@*' --no-pager` |
| 看某間 log | `tail -f ~/pcp-recorder/data/service-gate.log` |
| 看上傳 log | `tail -30 ~/pcp-recorder/upload.log` |
| 看排程下次何時跑 | `systemctl list-timers 'pcp-*'` |
| 看硬碟剩多少 | `df -h /` |
| 更新程式碼 | `cd ~/pcp-recorder && git pull && sudo bash install.sh` |
| 全部停錄 | `sudo systemctl disable --now pcp-recorder@{gate,okx,bybit,binance,umx,derive} pcp-upload.timer pcp-cleanup.timer` |

更新時 `install.sh` 會編譯成新檔再切換過去，不會蓋掉正在跑的程式，可以放心重跑。
