# PixelNode

> **Linux & Raspberry Pi 向け Art-Net / sACN ピクセルLEDコントローラー（ベストエフォート実装）**  
> *Developed by MotoYucchi*

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#ライセンス--著作権)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Target: Raspberry Pi 3B / x86_64](https://img.shields.io/badge/Platform-Raspberry%20Pi%20%7C%20Linux%20x86__64-lightgrey.svg)](#対象ハードウェア)

言語: [English](README.md) | [日本語 (Japanese)](README.ja.md)

---

## 概要

**PixelNode** は、イーサネット／IPネットワーク経由で受信した **Art-Net 4** および **sACN (E1.31)** の照明制御パケットを解釈し、Linux（主として Raspberry Pi 3B、および x86_64 Linux）のハードウェア SPI からデジタルピクセルLED（WS2811 / WS2812B 等）を駆動するために開発された Rust 製のノードソフトウェアです。

省DMXアドレス運用に適した「Preset 7ch モード」、リアルタイムパラメータ拡張用の「Standard 12ch モード」、メディアサーバー向け「Pixel Direct RGB モード」のほか、ヘッドレス環境で稼働する Web 管理ダッシュボード、現場診断用 CLI ツール（`pxtool`）、および生演奏向けの音響同期（オーディオリアクティブ）機能を備えています。

> [!NOTE]
> **本ソフトウェアの設計範囲と現実的な制約について**  
> PixelNode は、一般的な Linux カーネル（Raspberry Pi OS または汎用 Linux）上のソフトウェアタイマーおよび非同期 I/O をベースに動作しています。ジッタや遅延を可能な限り低減するための配慮・設計を施していますが、**ハードウェア RTOS（リアルタイムOS）と同等の厳密なマイクロ秒単位の決定論性を保証するものではありません**。伝送遅延やフレームタイミングは、OS のスケジューラ負荷、割り込み処理、SPI バス調停、ネットワーク環境の影響を受ける場合があります。本番運用に投入される前には、必ず現場の想定環境での実機リグチェック（事前検証）を行ってください。

---

## 設計方針

PixelNode は、以下の現実的な目標を掲げて設計されています：

* **ホットパスでのゼロアロケーション指向**: リアルタイム描画ループ（`RenderEngine`）内部ではヒープ確保（`Vec` や `Box`、動的文字列生成）を行わず、スライスのインプレース演算に徹することで、アロケータやGCに起因する突発的な遅延スパイクの抑制を図っています。
* **データプレーンと制御プレーンの分離**: DMX 受信から LED 出力に至るメイン描画スレッド（データプレーン）は、Web ダッシュボードや CLI による監視処理（制御プレーン）とは独立して動作し、相互の影響を低減しています。
* **段階的フェイルセーフ**: ネットワーク信号の瞬断時に突然ステージが真っ暗になる事故を防ぐため、最後のフレームを維持する `hold` や、緩やかに減光する `hold_then_fade` などのフェイルセーフ挙動を設定可能です。
* **電力制限保護（ABL: Automatic Brightness Limiter）**: LEDの推定消費電流を常時積算し、電源容量（PSU Cap）を超える場合には自動で全体を比例減光して、電源の電圧降下や落ち込みの緩和を試みます。
* **スタンドアロン現場点検性**: 照明卓やLAN環境がまだ仕込まれていない現場環境でも、Raspberry Pi 単体で結線・点灯・電源チェックを実施可能です。

---

## 技術的制約と現場導入における留意点

プロの舞台演出、コンサート、イベント設備等で安全かつ意図通りに運用していただくために、以下のハードウェア的・電気的な留意点をご確認ください。

### 1. タイミングの揺らぎ（ジッタ）とソフトウェアタイマー
* **汎用 Linux と RTOS の違い**: Linux カーネルは完全なハードウェア・リアルタイム性を保証しません。Linux 標準の `spidev` ドライバと DMA を用いることで CPU 負荷は大幅に抑えられますが、カーネルスレッドのプリエンプションやハードウェア割り込みによって、フレーム周期に微小なジッタが生じる可能性があります。
* **VPU コアクロックの固定（Raspberry Pi 3B）**: Pi 3B では SPI クロックが VideoCore VPU クロックから分周されているため、CPU 負荷変動に伴う周波数スケーリングで信号タイミングが狂い、LED のチラつき（フリッカー）の原因となります。そのため、設定レシピ（`core_freq=250`）による周波数固定を推奨していますが、これもソフトウェア制御の範疇である点をご留意ください。

### 2. 電気的ノイズと信号レベル変換
* **5V ロジックレベル変換の必須性**: Raspberry Pi の GPIO 出力は 3.3V ロジックです。一方、24V / 5V 駆動の WS2811 IC の推奨入力電圧仕様は $V_{IH} \ge 0.7 \times V_{DD}$（約 3.5V 以上）とされています。GPIO を直接 LED のデータ線に接続した場合、ノイズマージンが極めて小さくなり、電源ノイズや舞台照明機材の誘導ノイズでデータ化け（激しい点滅）が発生する原因となります。**74AHCT125 等の高速 CMOS レベルシフタを用い、出力直後に 33〜100Ω のダンピング抵抗を挿入することを強く推奨します。**
* **電源供給とグラウンド共通接続**: 24V ストリップは 5V ストリップに比べて電圧降下に強い設計ですが、適切な間隔での電源注入（Power Injection）と、Raspberry Pi と LED 電源間のクリーンな共通グラウンド（GND）接続が不可欠です。

---

## システム構成

PixelNode は、以下の 3 つの主要コンポーネントで構成されています：

```
+-------------------------------------------------------------------------+
|                              PixelNode                                  |
|                                                                         |
|  +---------------------+   +---------------------+   +---------------+  |
|  |     pixelnoded      |   |       pxtool        |   |audio-analyzerd|  |
|  |    (メインデーモン)   |   |   (現場診断・設定CLI) |   |  (音響解析)   |  |
|  |                     |   |                     |   |               |  |
|  | - Art-Net 4 / sACN  |   | - リアルタイム監視  |   | - デュアル    |  |
|  | - RenderEngine      |   | - SPI 単体リグ点検  |   |   解像度 FFT  |  |
|  | - Web ダッシュボード|   | - 動的設定変更      |   | - UDP 特徴量  |  |
|  | - REST API & RDM    |   | - DMX パケット監視  |   |   ブロードキャスト
|  +----------+----------+   +----------+----------+   +-------+-------+  |
|             |                         |                      |          |
+-------------|-------------------------|----------------------|----------+
              | /dev/spidev0.0          | HTTP / REST          | UDP 8765
              v                         v                      v
     +-----------------+       +-----------------+    +-----------------+
     |  WS2811 Strip   |       | Web UI / 卓     |    | 音声キャプチャ   |
     | (要 74AHCT125)  |       | (ブラウザ / CLI)|    | (DAW / ライン入力)
     +-----------------+       +-----------------+    +-----------------+
```

* **`pixelnoded`**: ネットワーク DMX を受信し、パーソナリティ解釈、パターン描画、電力制限、SPI 送信、および Web UI / REST API を司るコアデーモン。
* **`pxtool`**: 稼働中のノード状態の確認、スタンドアロンでの点検パターン発光、ネットワークパケット監視、設定変更を行う現場作業用 CLI。
* **`audio-analyzerd`**: Raspberry Pi 単体運用時、USB ライン入力音声をデュアル解像度 FFT 解析して UDP 経由で特徴量フレームを配信する補助デーモン。

---

## 制御パーソナリティ（DMX モード）

用途に応じて 3 つの制御モードを用意しています：

### 1. Preset 7ch（推奨・基本モード）
わずか **7チャンネル** でマスター調光、ストロボ、RGBW ベース色調色、自律点灯パターン、および音響自動同期を網羅する省アドレス設計です：

| ch | 機能 | DMX値範囲 | 動作概要 |
|:---:|---|:---:|---|
| **1** | Dimmer / Strobe | 0–255 | 0–7: 消灯 / 8–134: マスター調光 / 135–239: ストロボ (1–25Hz) / 240–255: 全開 |
| **2** | Red | 0–255 | ベース色 赤 (0%–100%) |
| **3** | Green | 0–255 | ベース色 緑 (0%–100%) |
| **4** | Blue | 0–255 | ベース色 青 (0%–100%) |
| **5** | White | 0–255 | ベース色 白（WS2811時はRGB合成または無視を選択可） |
| **6** | 点灯 Pattern | 0–255 | 24種類の自律点灯パターン（等間隔8値幅スロット規格） |
| **7** | 音声同期 (Audio Sync) | 0–255 | 0–7: Off (ch 6に従う) / 8–255: 24種類の音声同期パターン（ch 6をオーバーライド） |

*各スロットの割り当てと詳細なアルゴリズムは、[DMX チャンネル・アドレススロット仕様書](docs/dmx_channel_slot_specification.md) をご参照ください。*

### 2. Standard 12ch（パラメータ拡張モード）
パターンの速度（Speed）やサイズ（Size）、音声ゲイン、コントロールを卓のフェーダーで手動微調整したい現場向けのモードです。

### 3. Pixel Direct RGB（メディアサーバー・ピクセルマッピング）
メディアサーバー等から映像信号を直接流し込むモード（1ピクセルあたり3ch）。170px/ユニバースの標準境界跨ぎおよび Packed 境界跨ぎに対応しています。

---

## 照明卓（Console）プロファイル同梱

主要な照明ソフトウェア・規格向けのフィクスチャ定義を [`docs/fixtures/`](docs/fixtures/) に同梱しています：
* **ChamSys MagicQ**: `MagicQ_PixelNode_Preset7ch.hed`
* **QLC+**: `QLC+_PixelNode_Preset7ch.qxf`
* **Daslight 4/5**: `Daslight_PixelNode_Preset7ch.ssl2`
* **GDTF (grandMA3 等)**: `gdtf_description.xml`

---

## 導入とクイックスタート

### 必要要件
* Rust 1.75 以降 (`cargo`, `rustc`)
* Linux 環境（Raspberry Pi OS 64-bit または Ubuntu/Debian x86_64）
* ALSA 開発用ライブラリ（音声解析用）：
  ```bash
  sudo apt-get install -y libasound2-dev
  ```

### 1. ビルド
```bash
git clone https://github.com/motoyucchi/rasPi_Art-Net_Lighting-Ctrl.git pixelnode
cd pixelnode

# リリースバイナリの最適化ビルド
cargo build --release
```

### 2. 設定ファイルの配置
設定サンプルをコピーし、環境に合わせて編集します：
```bash
sudo mkdir -p /etc/pixelnode
sudo cp deploy/config.example.toml /etc/pixelnode/config.toml
```

### 3. デーモンの起動
```bash
./target/release/pixelnoded /etc/pixelnode/config.toml
```

### 4. `pxtool` による現場点検
デーモン起動中、別ターミナルやリモート PC からノードの状態を確認できます：
```bash
# 稼働ステータス、FPS、DMXチャンネルインジケーターの確認
./target/release/pxtool status

# ネットワーク経由で 10秒間の RGBW 順次点灯テストを実行
./target/release/pxtool test --mode rgbw --remote --duration 10

# テストを終了し、照明卓の制御へ復旧
./target/release/pxtool test --mode off --remote
```

### 5. Web 管理ダッシュボード
ブラウザから `http://<ノードのIP>:8080/` にアクセスします。ダッシュボードは外部 CDN に一切依存せず、完全オフラインの照明 LAN 環境でも正常に表示・操作可能です。

---

## ドキュメント一覧

詳細な技術仕様や手順書は [`docs/`](docs/) ディレクトリにまとめています：

* **[アーキテクチャ & 設計仕様書](docs/Architecture&Design.md)**: システム全体の詳細仕様、FMEA（障害モード解析）、タイミングモデル、FFT 仕様。
* **[DMX チャンネル・アドレススロット仕様書 (Preset 7ch)](docs/dmx_channel_slot_specification.md)**: 全24パターンの点灯アニメーションおよび全24パターンの音声同期マトリクス完全対照表。
* **[CLI リファレンスマニュアル (`pxtool`)](docs/cli_reference.md)**: コマンドライン構文、パケットスニッファー、現場活用レシピ。
* **[Web 管理インターフェース リファレンス](docs/web_interface.md)**: Web ダッシュボードの機能と REST API 仕様。
* **[Raspberry Pi 3B セットアップガイド](deploy/raspi3b_setup.md)**: ハードウェア結線図、レベル変換回路、OS 安定化設定手順。

---

## 開発者・連絡先

* **Author**: MotoYucchi ([@motoyucchi](https://github.com/motoyucchi))
* **想定用途**: ピクセル照明演出、ライブハウス・フェス等のイベント照明、実験的ステージオートメーションのプロトタイピングなど。

---

## ライセンス / 著作権

本プロジェクトは、以下のいずれかのライセンスの下で利用可能です：

* Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0))
* MIT License ([LICENSE-MIT](http://opensource.org/licenses/MIT))

利用者の選択により、いずれかの条件に従うことができます。
