# PixelNode CLI ツール (`pxtool`) リファレンスマニュアル

`pxtool` は、PixelNode 照明システムの現場診断、ネットワーク監視、設定生成、および単体リグ点検を行うためのコマンドラインツールです。

照明卓がまだ接続されていない仕込み段階での単体点灯テストや、SSH 経由での状態確認、スクリプトによる自動検証などに利用できます。

> [!NOTE]
> **実機検証ステータス**:  
> USB シリアル経由での Arduino Uno 接続（`-s /dev/ttyUSB0`）による単体点灯テストは実機検証済みです。Raspberry Pi のハードウェア SPI（`--spi /dev/spidev0.0`）による点灯は現在コード実装段階であり、実機での物理検証は今後実施予定です。

---

## コマンド一覧

```bash
pxtool <COMMAND>

Commands:
  status       稼働中の pixelnoded デーモンからステータス・FPS・DMXチャンネル値を取得
  monitor      Art-Net / sACN 照明ネットワークのパケット監視（スニファー）
  test         単体点灯テスト（Arduino シリアル、Raspberry Pi SPI、またはデーモン経由）
  config       設定ファイルの確認、検証、プリセット生成、対話型セットアップ
  lock         Show Lock（誤操作防止ロック）の有効化・無効化
  send-artnet  テスト用 Art-Net DMX パケットの送信
  send-sacn    テスト用 sACN (E1.31) DMX パケットの送信
  help         ヘルプ情報の表示
```

---

## 1. `pxtool status`

稼働中の `pixelnoded` デーモンに問い合わせを行い、リアルタイムの稼働状況を表示します。

### オプション
* `-n, --node <URL>`: デーモンの HTTP エンドポイント URL（デフォルト: `http://127.0.0.1:8080`）
* `--json`: スクリプト処理向けの生 JSON 形式で出力

### 実行例

```bash
# ローカルで動作中のノードを確認
pxtool status

# ネットワーク上の Raspberry Pi ノードを確認
pxtool status --node http://192.168.1.50:8080
```

**出力例:**
```
========================================================================
 PIXELNODE STATUS: Stage-WS2811-Node1 (v0.1.0)
========================================================================
 Uptime: 02h 15m 40s | Show Lock: UNLOCKED 🔓 | Failsafe: INACTIVE (OK)
------------------------------------------------------------------------
[RENDERING]
 Target: 50.0 FPS | Actual: 50.0 FPS | Total Frames: 407,000
 Power Draw (ABL): 3.20 A / 15.0 A (21%) | Status: OK
 Active Test Override: None (DMX Console Controlled)

[NETWORK & PROTOCOL]
 Art-Net Packets Rx: 24,500 | sACN Packets Rx: 0
   Universe   1 | Source: ArtNet   | State: ACTIVE (Age: 20ms)

[LIVE CHANNEL VALUES]
   Ch 1 [Dim/Strobe]: 255 [████████████████] 100%
   Ch 2 [Red       ]: 255 [████████████████] 100%
   Ch 3 [Green     ]:   0 [░░░░░░░░░░░░░░░░]   0%
   Ch 4 [Blue      ]: 128 [████████░░░░░░░░]  50%
   Ch 5 [White     ]:   0 [░░░░░░░░░░░░░░░░]   0%
   Ch 6 [Pattern   ]:   5 [░░░░░░░░░░░░░░░░]   2%
   Ch 7 [Audio Sync]:   0 [░░░░░░░░░░░░░░░░]   0%

[AUDIO REACTIVE SYNC]
 Status: CONNECTED | BPM: 128.5 | RMS Energy: 78%
 Beat Triggers: [KICK: ON ] [SNARE: OFF] [HIHAT: ON ]
========================================================================
```

---

## 2. `pxtool monitor`

照明ネットワーク上のパケット（Art-Net: UDP 6454 / sACN: UDP 5568）を受信し、送信元IP・ユニバース番号・シーケンス番号・チャンネル値をリアルタイムに監視します。

### オプション
* `-i, --interface <IP>`: バインドするインターフェースのローカル IP（デフォルト: `0.0.0.0`）
* `-p, --protocol <artnet|sacn|all>`: 対象プロトコル（デフォルト: `all`）
* `-u, --universe <ID>`: 監視対象の特定ユニバース番号（省略時は全ユニバース）
* `-c, --channels <COUNT>`: パケットごとにプレビュー表示するチャンネル数（デフォルト: `8`）
* `-d, --duration <SEC>`: 指定秒数後に自動停止（デフォルト: `0` = 手動停止まで継続）

### 実行例

```bash
# すべての照明パケットを監視
pxtool monitor

# Art-Net の Universe 1 のみを抽出して監視
pxtool monitor --protocol artnet --universe 1

# 10秒間だけ監視し、先頭16チャンネルの値を表示
pxtool monitor --duration 10 --channels 16
```

---

## 3. `pxtool test`

照明卓やネットワークが存在しない状態でも、LED テープの配線や点灯、カラーオーダーを現場で直接テストできる点検機能です。
Arduino Uno の USB シリアルポート（実機検証済み）、Raspberry Pi SPI（今後検証予定）、または稼働中デーモンの REST API 経由で実行できます。

### テストモード一覧
* `rgbw`: 赤 $\rightarrow$ 緑 $\rightarrow$ 青 $\rightarrow$ 白 を1秒おきに巡回（カラーオーダー確認・不良素子チェック）
* `walker`: 1つの白色ピクセルが端から端へ順次移動（ピクセル数・信号伝達方向の確認）
* `soak`: 100% 全白点灯（電源の容量チェック・末端の電圧降下確認）
* `strobe`: 40Hz の高頻度ストロボ（信号の伝送安定性チェック）
* `color`: 任意の色（`--color R,G,B`）で固定点灯
* `off`: テスト発光を停止し、照明卓からの DMX 制御へ復帰

### 自律パターン指定 (`--pattern`)
モード指定の代わりに `--pattern <0..31>` を指定することで、デーモンと同等の点灯パターン（多重リップル、2色ウェーブ、一方向パルス等）を直接出力できます。

### オプション
* `-m, --mode <MODE>`: `rgbw` | `walker` | `soak` | `strobe` | `color` | `off`
* `-p, --pixels <N>`: 制御ピクセル数（デフォルト: `300`）
* `--color-order <ORDER>`: 色順列（`rgb` | `rbg` | `grb` | `gbr` | `brg` | `bgr`、デフォルト: `brg`）
* `--preset <NAME>`: テーププリセット（`btf_fcob_576_24v_10m`, `nobrand_60led_12v_5m`, `ws2812b_60led_5v_5m`）
* `--color <R,G,B>`: `color` モード時の RGB 値（例: `255,128,0`）
* `--pattern <ID>`: 自律パターンスロット番号（`0`〜`31`、例: `24`=多重リップル, `25`=2色ウェーブ, `26`=一方向パルス, `27`=外側集約）
* `--speed <VAL>`: パターンの再生速度（`0`〜`255`、デフォルト: `128` = 等倍正方向）
* `--size <VAL>`: パターンの波長・幅（`0`〜`255`、デフォルト: `128` = 標準幅）
* `-s, --serial <PATH>`: Arduino Uno 等の USB シリアルポート（例: `/dev/ttyUSB0`, `/dev/ttyACM0`）
* `--baud <RATE>`: シリアルボーレート（デフォルト: `115200`）
* `--spi <PATH>`: Linux SPI デバイスパス（例: `/dev/spidev0.0`）
* `-r, --remote`: 稼働中の `pixelnoded` デーモンに対して REST API 経由でテストを指示
* `-n, --node <URL>`: `--remote` 使用時のデーモン URL（デフォルト: `http://127.0.0.1:8080`）
* `-d, --duration <SEC>`: 指定秒数後に自動終了（デフォルト: `0` = 永続実行）

### 実行例

#### Arduino Uno 接続（実機検証済み）
```bash
# 1. BTF-LIGHTING FCOB 24V (160px, RGB) をプリセットで RGBW 点灯テスト
pxtool test --serial /dev/ttyUSB0 --preset btf_fcob_576_24v_10m --mode rgbw

# 2. ノーブランド 12V テープ (100px, RBG) でウォーカーテストを実行
pxtool test --serial /dev/ttyUSB0 --preset nobrand_60led_12v_5m --mode walker

# 3. 60ピクセルのテープに対し、多重センターリップル (Pattern 24) を水色でテスト
pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 24 --color 0,200,255 --duration 5

# 4. 2色ウェーブ (Pattern 25) をマゼンタベースでテスト
pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 25 --color 255,0,128 --speed 140

# 5. アンバー色で 10 秒間点灯
pxtool test --serial /dev/ttyUSB0 --mode color --color 255,120,20 --pixels 60 --duration 10
```

#### Raspberry Pi SPI 接続（※実機未検証・今後テスト予定）
```bash
# BTF FCOB 24V を SPI 経由で RGBW 点灯テスト
pxtool test --preset btf_fcob_576_24v_10m --mode rgbw --spi /dev/spidev0.0

# 電源電圧降下チェックのための 30 秒間 100% 全白点灯
pxtool test --preset btf_fcob_576_24v_10m --mode soak --spi /dev/spidev0.0 --duration 30
```

#### リモート経由（デーモン稼働中のネットワーク点検）
```bash
# 遠隔の Raspberry Pi ノードに RGBW テストを指示
pxtool test --remote --mode rgbw --node http://192.168.1.50:8080

# 固定アンバー色を指示
pxtool test --remote --mode color --color 255,140,20 --node http://192.168.1.50:8080

# テストモードを解除し、照明卓の DMX 制御へ戻す
pxtool test --remote --mode off --node http://192.168.1.50:8080
```

---

## 4. `pxtool config`

設定ファイルの閲覧、検証、プリセット適用、対話型セットアップを行います。

### サブコマンド一覧

#### `pxtool config show`
稼働中ノードまたはローカル TOML ファイルの設定内容を表示します。
```bash
# 稼働中ノードの設定を表示
pxtool config show

# 指定した設定ファイルを表示
pxtool config show --path /etc/pixelnode/config.toml
```

#### `pxtool config validate <PATH>`
設定ファイルの構文エラー、DMX アドレス範囲の超過、電力設定の妥当性を事前チェックします。
```bash
pxtool config validate /etc/pixelnode/config.toml
```

#### `pxtool config preset list`
内蔵されている LED テーププリセット（電圧、長さ、ピクセル数、色順、推奨電流上限）を一覧表示します。
```bash
pxtool config preset list
```

#### `pxtool config preset show <ID>`
特定プリセットの電気特性、計算根拠、電源注入の目安、配線注意事項を表示します。
```bash
pxtool config preset show btf_fcob_576_24v_10m
pxtool config preset show nobrand_60led_12v_5m
```

#### `pxtool config preset apply <ID>`
選択したプリセットと出力方式に応じた、コメント付きの `config.toml` を生成します。
```bash
# BTF FCOB 24V 向け設定を生成 (Arduino Uno 出力)
pxtool config preset apply btf_fcob_576_24v_10m --driver serial_mcu --device /dev/ttyUSB0 --output /etc/pixelnode/config.toml

# ノーブランド 12V 向け設定を生成 (Raspberry Pi SPI 出力)
pxtool config preset apply nobrand_60led_12v_5m --driver spi_ws281x --device /dev/spidev0.0 --output /etc/pixelnode/config.toml
```

#### `pxtool config init`
対話型のセットアップウィザードを起動します。テープ種類、出力ドライバ、DMX アドレスを質問形式で入力し、設定ファイルを出力します。
```bash
pxtool config init
```

#### `pxtool config set`
稼働中のノードの設定を動的に変更し、ディスクへアトミックに保存します。
```bash
# DMX ユニバースと開始アドレスを変更
pxtool config set --universe 2 --address 1

# 色順を RBG に、ピクセル数を 100 に変更
pxtool config set --color-order rbg --pixels 100

# 電源電流制限を 7A (7000mA) に設定
pxtool config set --max-current-ma 7000
```

---

## 5. `pxtool lock`

本番中の誤操作を防止する「Show Lock」の切り替えを行います。Show Lock が有効な間は、Web UI や REST API からの設定変更およびテスト発光の割り込みが拒絶（HTTP 403 Forbidden）されます。

```bash
# 本番開始前にロックを有効化
pxtool lock --enable

# メンテナンス時にロックを解除
pxtool lock --disable
```

---

## 6. `pxtool send-artnet` / `send-sacn`

ネットワーク疎通確認やアドレス確認のために、指定のチャンネル値を持つテストパケットを送信します。

```bash
# Art-Net で Dimmer 100%, Red 100% を Universe 1 宛てに送信
pxtool send-artnet --ip 192.168.1.50 --universe 1 --channels "255,255,0,0,0,0,0"

# sACN で 優先度 150 のパケットを送信
pxtool send-sacn --ip 192.168.1.50 --universe 1 --priority 150 --channels "255,0,255,0,0,0,0"
```

---

## 現場トラブルシューティングの目安

| 症状 | 確認コマンド | 疑われる原因と対処 |
| :--- | :--- | :--- |
| **LEDが全く点灯しない** | `pxtool status` | `Art-Net Packets Rx` が増えているか確認。増えていない場合は照明卓の送信先IP/ポートを確認。`pxtool test` で単体点灯するか確認。 |
| **色が指定と異なる (赤が青になる等)** | `pxtool test --mode rgbw` | 赤 $\rightarrow$ 緑 $\rightarrow$ 青 の順序を目視確認。青が先に光るなら `--color-order brg` や `rbg` へ変更。 |
| **激しく点滅・データ化けする** | `pxtool test --serial /dev/ttyUSB0 --mode color` | コントローラーとLED電源の **GND が共通接続されているか確認**。信号線にダンピング抵抗が入っているか確認。 |
| **設定変更が反映されない** | `pxtool status` | `Show Lock: LOCKED` になっていないか確認。`pxtool lock --disable` で解除。 |
