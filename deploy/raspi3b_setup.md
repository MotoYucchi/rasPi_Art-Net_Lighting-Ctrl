# PixelNode Raspberry Pi 3B セットアップガイド（動作安定化設定）

Raspberry Pi 3B で WS2811 / WS2812B を SPI0（GPIO10）から駆動する際の、クロック変動やバッファ詰まりによるタイミングの乱れ・チラつきを低減するためのハードウェアおよびOS設定手順です。

> [!IMPORTANT]
> **実機テスト・検証ステータス（実機の事前確認のお願い）**  
> 現行バージョンにおいて、**Raspberry Pi 3B 実機での SPI ハードウェア接続テストは未実施（未検証・今後テスト予定）** です。
> ソフトウェア上のドライバ実装およびシミュレーションテストは完了していますが、物理的な実機点灯テストが完了しているのは「PC / Linux + Arduino Uno (USB-Serial)」環境のみです。
> Raspberry Pi 実機を現場で使用される際は、必ず事前にオシロスコープやロジックアナライザ等での信号タイミング確認および目視点灯テストを行ってください。

> [!NOTE]
> **OSタイマーとジッターについての留意点**:
> Raspberry Pi や一般的な Linux PC では、OS のプロセススケジューリングや割り込み処理、CPU負荷変動の影響を受けるため、ハードウェアタイマー専用のリアルタイムコントローラ（専用マイコン等）と比較して信号ジッターをゼロにすることは構造上保証できません。本ガイドの設定は、クロックの固定やカーネルバッファ拡張により、それらの変動要因を可能な限り低減することを目的としています。

---

## 1. ハードウェア結線

### ピンアサイン（Raspberry Pi 40pin GPIO）
- **GPIO10 (SPI0 MOSI)**: **物理 19 番ピン** （データ出力）
- **GND**: **物理 20 番ピン** （グラウンド共通接続）

```
[Pi 3B (3.3V)]                     [74AHCT125 (5V駆動)]                  [24V WS2811 LED]
GPIO10 (Pin 19) ----> 1A (Pin 2)  --> 1Y (Pin 3) ----[ 47Ω 抵抗 ]----> DATA IN
GND (Pin 20)    --------------------> GND (Pin 7)  --------------------> GND (共通)
                      1OE (Pin 1) --> GND
                      VCC (Pin 14) -> 5V (高品質電源)
```

> [!IMPORTANT]
> **レベル変換（74AHCT125等）は必須**:
> Pi の GPIO 出力は 3.3V ですが、WS2811 の推奨入力電圧は 5V ロジックです。直結するとノイズマージンが極小になり、電源ノイズや周囲の照明機材の電磁ノイズで化け（チラつき）が発生します。
> また、LED用の 24V 電源と Pi の 5V 電源は別系統とし、**GND のみ共通接続** してください。

---

## 2. Raspberry Pi OS 設定

### A. `/boot/firmware/config.txt` の編集
Pi 3B の SPI クロックは VPU コアクロックから分周されているため、CPU負荷による周波数変動でクロックがブレると **WS2811 の信号タイミングが狂って激しいフリッカーが発生します**。

`/boot/firmware/config.txt` の末尾に以下を追記してください：

```ini
# SPI クロックの安定化（コアクロック固定）
core_freq=250

# SPI0 ペリフェラル有効化
dtparam=spi=on

# 不要な無線を停止（熱低減・リアルタイム性向上・PL011 UART解放）
dtoverlay=disable-bt
dtoverlay=disable-wifi
```

### B. `/boot/firmware/cmdline.txt` の編集
Linux カーネル標準の `spidev` バッファサイズ（4096バイト）を拡張します。

行の末尾（1行のまま、改行せずスペース区切りで追加）に以下を追記：
```
spidev.bufsiz=65536
```

---

## 3. アプリケーションのビルドと配備

```bash
# ビルド
cargo build --release

# インストール
sudo cp target/release/pixelnoded /usr/local/bin/
sudo cp target/release/pxtool /usr/local/bin/

# 設定ファイル配置（以下のいずれかを選択）
# 方式A: 対話型セットアップウィザードで初期設定
sudo mkdir -p /etc/pixelnode
pxtool config init -o /etc/pixelnode/config.toml

# 方式B: プリセットTOMLから直接コピー
# 例: BTF 24V 10m FCOB (SPI) の場合
sudo cp deploy/presets/btf_fcob_24v_10m_spi.toml /etc/pixelnode/config.toml
# 例: ノーブランド 12V 5m (SPI) の場合
# sudo cp deploy/presets/generic_12v_5m_spi.toml /etc/pixelnode/config.toml

# 設定ファイルの構文検証
pxtool config validate /etc/pixelnode/config.toml

# systemd サービス登録
sudo cp deploy/pixelnoded.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now pixelnoded
```

---

## 4. 現場でのセルフテスト（リグチェック）

照明卓やネットワークがまだ接続されていない状態でも、`pxtool` で単体テストを行えます。
プリセット指定（`--preset`）または個別パラメータ指定（`--pixels`, `--color-order` 等）のどちらでも実行可能です：

```bash
# プリセットを使用したテスト（ピクセル数・色順が自動適用されます）
# 例: BTF 24V 10m FCOB (160 px, RGB)
sudo pxtool test --preset btf --mode rgbw --spi /dev/spidev0.0
sudo pxtool test --preset btf --mode soak --spi /dev/spidev0.0

# 例: ノーブランド 12V 5m (100 px, RBG)
sudo pxtool test --preset generic --mode rgbw --spi /dev/spidev0.0
sudo pxtool test --preset generic --mode soak --spi /dev/spidev0.0

# 手動指定でのテスト
# 1. 色順・断線チェック（赤→緑→青→白の順次点灯）
sudo pxtool test --mode rgbw --pixels 160 --color-order rgb --spi /dev/spidev0.0

# 2. アドレス・ピクセル数・向きチェック（白い光が1px走る）
sudo pxtool test --mode walker --pixels 160 --spi /dev/spidev0.0

# 3. 電源容量・電圧降下チェック（全白フル点灯）
sudo pxtool test --mode soak --pixels 160 --spi /dev/spidev0.0

# 4. 信号線ノイズ耐性チェック（高速ストロボ）
sudo pxtool test --mode strobe --pixels 160 --spi /dev/spidev0.0
```
