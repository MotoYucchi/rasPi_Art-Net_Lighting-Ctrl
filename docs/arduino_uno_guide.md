# PixelNode Arduino Uno (ATmega328P) USBシリアル接続ガイド

> [!IMPORTANT]
> **実機テスト・検証ステータス（2026年10月現在）**
> - **Arduino Uno (USB-Serial) + LEDテープ（Pin D6）**: **実機接続テスト完了・点灯確認済み**
>   - CH340（互換機）および 純正 ATmega16U2 経由での 115200bps 通信、双方向テレメトリ、自律パターン点灯を実機で確認しています。
> - **Raspberry Pi (SPI0 / GPIO10)**: **実機テスト未完了（未検証・今後テスト予定）**

---

## 1. 概要と構成

**PixelNode** を運用する際、Raspberry Pi の GPIO/SPI を直接使用できない環境や、LED信号の生成を専用マイコンに分離させたい場合に、**Arduino Uno (ATmega328P @ 16 MHz)** を Linux PC または Raspberry Pi の USB ポートに接続して使用できます。

Arduino Uno を用いる主なメリット：
1. **5V ロジック出力**: Arduino Uno のデジタルピンは 5V 出力のため、Raspberry Pi で必須となるレベルシフタ（3.3V $\rightarrow$ 5V 変換）が不要となり、D6 ピンから直接 WS2811 / WS2812B のデータ線へ接続できます。
2. **ハードウェアタイマー専用駆動**: 汎用 OS（Linux）の割り込みやスケジューラ遅延から LED 信号生成を物理的に切り離せます。
3. **低コスト・入手性**: 現場での予備機の確保が容易です。

```
+-------------------------------------------------------------------------------+
|                       ホストシステム (Linux PC / Raspberry Pi)               |
|                                                                               |
|  [Art-Net / sACN] ---> [DMX 受信]                                             |
|  [音声ライン入力] ---> [audio-analyzerd] (FFT / ビート検出)                   |
|                               |                                               |
|                               v                                               |
|                    [RenderEngine (Rust)]                                      |
|                    - 28種類の自律照明パターン (Ch 6: Slot 0〜27)              |
|                    - 24種類の音響同期パターン (Ch 7)                          |
|                    - マスターディマー & 1〜25Hz ストロボ                      |
|                    - ガンマ 2.2 補正 & 電源リミッター (ABL)                   |
|                               |                                               |
|                               v                                               |
|                    [SerialMcuSink (Rust)]                                     |
|                    - フレーミング: Magic 'PX', Sequence, Length, CRC-8        |
|                    - デバイス: /dev/ttyACM0 または /dev/ttyUSB0 (115200 bps)  |
+-------------------------------------------------------------------------------+
                                        |  USB ケーブル (CDC / UART)
                                        v
+-------------------------------------------------------------------------------+
|                       Arduino Uno (ATmega328P @ 16 MHz)                       |
|                                                                               |
|  [ハードウェア UART] ---> [パケットパーサー (状態遷移マシン + CRC-8 検証)]    |
|                               |                                               |
|            +------------------+------------------+                            |
|            | (ストリーミング受信)                | (DMXパラメータ自律描画)    |
|            v                                     v                            |
|  [OP_STREAM_PIXELS]                     [内蔵パターンエンジン]                |
|  受信バッファをそのままLEDへ出力        - 同一の自律パターン (Ch 6)           |
|                                         - 同一の音響同期 (Ch 7)               |
|                                         - フェイルセーフ保持 / 自動復旧       |
|                                                  |                            |
|            +-------------------------------------+                            |
|            |                                                                  |
|            v                                                                  |
|  [AVR アセンブラ ドライバ] ------------> Pin D6 ----> [WS2811 / WS2812B LED] |
|                                                                               |
|  [保護機能]:                                                                  |
|  - ハードウェアウォッチドッグタイマー (2.0s WDT)                               |
|  - リンク途絶タイマー監視 (Hold / Fallback / Blackout)                        |
|  - 動的メモリ確保ゼロ (2KB SRAM 制約内で静的バッファ運用)                     |
|  - 双方向テレメトリ返送 (ホストへ受信フレーム数・CRCエラー数を 1Hz 返信)      |
+-------------------------------------------------------------------------------+
```

---

## 2. 動作モード

### モード 1: ホスト側レンダリング・ストリーミングモード (`OP_STREAM_PIXELS`)
- **標準モード**: ホスト側（`pixelnoded` または `pxtool`）の Rust `RenderEngine` が全ピクセルの色を計算し、パックされた RGB データをシリアル送信します。
- **Arduinoの役割**: 受信したフレームの CRC-8 を検証し、エラーがなければ Pin D6 から LED へパルス列を出力します。

### モード 2: DMX パラメータ送信・マイコン自律描画モード (`OP_DMX_CHANNELS`)
- ホストから 10 バイトの軽量な DMX パラメータ（Dimmer, R, G, B, Strobe, Pattern, Audio, Speed, Size）を送信します。
- Arduino 内部のパターンエンジンが、時間 $t$ に基づいて自律的にアニメーションを 50 FPS でローカル描画します。
- ホストとの通信頻度を抑えたい場合や、帯域を節約したい場合に適しています。

---

## 3. 配線方法と電気的注意事項

> [!CAUTION]
> **電圧の分離（最重要）**:  
> 24V や 12V の外部電源（V+）を、Arduino Uno の 5V、3.3V、VIN、またはデジタルピンに絶対に接続しないでください。Arduino Uno が瞬時に破損します。  
> Arduino と LED 電源の間で共有するのは **DC マイナス（GND）のみ** です。

### 結線図（24V FCOB または 12V テープの場合）:

```
[外部 LED 電源 (24V または 12V)]
  +V (24V/12V) -----------------------------------> LEDテープの +V (赤線)
  GND (0V)     ---+-------------------------------> LEDテープの GND (白/黒線)
                  |
                  | （※GND同士を必ず直結して共通化する）
                  |
[Arduino Uno]     |
  GND -----------+
  Pin D6 ------------------[ 47Ω〜100Ω 抵抗 ]-----> LEDテープの DIN (緑線)
  USB端子 -----------------------------------------> PC / ラズパイの USBポート
```

> **直列抵抗の推奨**:  
> Arduino の Pin D6 と LED テープのデータ入力（DIN）の間に、33Ω〜100Ω 程度の抵抗を入れると、信号の反射（オーバーシュート）を抑制し、長時間の安定性が向上します。

---

## 4. ファームウェアの書き込み手順

### 必要なツール
- `arduino-cli`（推奨）または Arduino IDE (2.x)
- AVR ボードパッケージ: `arduino:avr`

### arduino-cli による書き込み
```bash
# ボードパッケージのインストール（初回のみ）
arduino-cli core install arduino:avr

# コンパイル
arduino-cli compile --fqbn arduino:avr:uno firmware/pixelnode_uno

# 書き込み (/dev/ttyUSB0 の場合)
arduino-cli upload -p /dev/ttyUSB0 --fqbn arduino:avr:uno firmware/pixelnode_uno
```

> **書き込み直後の自己診断点滅**:  
> 書き込みまたは電源投入直後、Pin D6 に接続された LED の先頭 5 球が **赤 → 緑 → 青 → 橙（アンバー）** の順に各 100ms 点滅します。PC と通信する前でも、LEDテープの電源・GND・信号線の導通を視覚的に確認できます。

---

## 5. 実機点灯テスト

照明卓やデーモンを立ち上げる前に、`pxtool test` コマンドで直接点灯を確認できます。

```bash
# 1. 赤色で 3 秒間点灯（60ピクセル、GRB色順）
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --mode color --color "255,0,0" --duration 3

# 2. 多重センターリップル（Slot 24）をシアン色で 5 秒間点灯
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 24 --color "0,220,255" --duration 5

# 3. 4つの拡張パターンを一括順次テスト
./tests/test_new_patterns.sh /dev/ttyUSB0 60 grb 115200
```

正常に通信できると、ターミナルに以下のようなマイコン側受信ステータスが表示されます：
```
Connected to Arduino Uno on '/dev/ttyUSB0' (baud: 115200)
Synchronized MCU configuration: 60 pixels (order: grb)
  [MCU Status] Rx: 1 frames, CRC err: 0, Pixels: 60, Failsafe: false
  [MCU Status] Rx: 28 frames, CRC err: 0, Pixels: 60, Failsafe: false
  [MCU Status] Rx: 80 frames, CRC err: 0, Pixels: 60, Failsafe: false
```

---

## 6. トラブルシューティング

| 現象 | 主な原因 | 対処方法 |
|---|---|---|
| **LEDが一切点灯しない** | 電源未投入、GND未共通化、DTRリセット待ち不足 | ① LED電源が入っているか確認。<br>② ArduinoのGNDとLED電源のGNDが直結されているか確認。<br>③ 起動直後に先頭5球が点滅するか確認。点滅もしない場合は配線（D6ピン）を確認。 |
| **色が指定と違う（赤と緑が逆など）** | テープの色順（カラーオーダー）不一致 | 12Vテープは `rbg`、汎用WS2812Bは `grb`、24V FCOBは `rgb` を指定してください。 |
| **激しくチラつく・勝手に白く光る** | GND浮き、信号線ノイズ、長すぎる信号線 | ① コントローラーとLED電源のGND共通接続を再確認。<br>② D6ピン直後に 47Ω 抵抗を入れる。<br>③ ArduinoからLEDテープまでのデータ線を短く（1m以内推奨）する。 |
| **起動直後の最初のパケットが届かない** | ArduinoのDTRオートリセット待ち（2.2s） | Arduino Uno はポートを開いた直後に自動リセットがかかり、起動完了まで約 2.2 秒かかります。スクリプト等で開く際は、接続直後に 2.2 秒以上の待機時間を設けてください。 |
