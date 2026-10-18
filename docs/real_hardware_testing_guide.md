# PixelNode 実機ハードウェア検証ガイド

> [!IMPORTANT]
> **実機テスト・検証ステータス（2026年10月現在）**
> - **Arduino Uno (USB-Serial) + LEDテープ（Pin D6）**: **実機接続テスト完了・点灯確認済み**
> - **Raspberry Pi 3B (SPI0 / GPIO10)**: **実機テスト未完了（未検証・今後テスト予定）**

---

## 1. 概要とテストの目的

ソフトウェア上のシミュレータテストやユニットテストは、メモリ上での計算ロジック（アルゴリズムや境界値）を検証するのに有効ですが、**実際の現場運用では物理的な配線・給電・ノイズ・シリアル通信・タイミングの壁**が存在します。

本ガイドでは、接続された実機の Arduino Uno および LED テープに対して、以下の項目を実際に動作させて点検する手順を説明します：
1. USB シリアル通信（CH340 / ATmega16U2）でのパケット損失および CRC-8 エラーの有無
2. 接続する LED テープ（BTF 24V、ノーブランド 12V、汎用 5V 等）に応じた色順（カラーオーダー）の正常性
3. ピクセル数スケーリング（10球〜最大170球）時のマイコン SRAM 安定性
4. DMX チャンネル操作による自律パターンの正常な発光・アニメーション
5. 電源容量制限（ABL）による過電流保護の動作

---

## 2. 実機テストツールの種類と使い分け

現在、用途に合わせて 3 つのテスト方法が用意されています：

| テスト方法 | 実行コマンド / スクリプト | 用途・特徴 |
|---|---|---|
| **① 単体ダイレクト点灯** | `pxtool test --serial ...` | **最速・最も確実**。デーモンやネットワークを介さず、指定した色やパターンを直接Arduinoへ送って目視点検。マイコン側の受信ステータスもリアルタイム表示。 |
| **② 新パターン一括点検** | `tests/test_new_patterns.sh` | Slot 24〜27 の 4 つの新パターン（多重リップル、2色ウェーブ、一方向パルス、外側集約）を各 5 秒間、順次実機で点灯させるスクリプト。 |
| **③ 包括的自動診断スイート** | `tests/test_real_hardware.sh` | シリアル、ピクセル長、カラーオーダー、パーソナリティ、ストロボ、電源リミッターの全項目を自動実行する診断スクリプト。 |

---

## 3. 各テストの実行手順

### 方法 ①: `pxtool test` による直接点灯点検（推奨）

照明卓やデーモンを起動することなく、コマンドラインから直接 Arduino Uno へ信号を送出して点灯状態を確認します。

```bash
# 【赤色で 3 秒間点灯】 (結線と色順の確認)
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --mode color --color "255,0,0" --duration 3

# 【多重センターリップル (Slot 24) をシアン色で 5 秒間点灯】
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 24 --color "0,220,255" --duration 5

# 【2色ツートーン波 (Slot 25) をマゼンタベースで 5 秒間点灯】
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 25 --color "255,0,180" --duration 5

# 【一方向パルス流 (Slot 26) を右から左（speed: 192）で 5 秒間点灯】
./target/debug/pxtool test --serial /dev/ttyUSB0 --pixels 60 --color-order grb --pattern 26 --speed 192 --color "255,160,10" --duration 5
```

実行中、画面に以下のようなマイコン側からのステータスが表示され、**実際にデータがマイコンに届いて処理されているか**を数値で確認できます：
```
Connected to Arduino Uno on '/dev/ttyUSB0' (baud: 115200)
Synchronized MCU configuration: 60 pixels (order: grb)
  [MCU Status] Rx: 1 frames, CRC err: 0, Pixels: 60, Failsafe: false
  [MCU Status] Rx: 28 frames, CRC err: 0, Pixels: 60, Failsafe: false
  [MCU Status] Rx: 80 frames, CRC err: 0, Pixels: 60, Failsafe: false
```

---

### 方法 ②: `tests/test_new_patterns.sh` による新パターン一括点検

4つの新パターン（Slot 24〜27）が正しく表示されるかを連続で確認したい場合に使用します。

```bash
./tests/test_new_patterns.sh /dev/ttyUSB0 60 grb 115200
```

- 各パターンが 5 秒ずつ切り替わりながら点灯します。
- センターから外側への波の広がり、2色のグラデーション変化、左右の流れなどを目視で確認してください。

---

### 方法 ③: `tests/test_real_hardware.sh` による総合ハードウェア診断

接続された Arduino Uno の通信健全性、各種カラーオーダーでのフレーム生成、パーソナリティ切り替え、ストロボ、電力制限（ABL）を一括でテストします。

```bash
# クイック診断モード (約 30 秒)
./tests/test_real_hardware.sh --mode quick --device /dev/ttyUSB0

# フル網羅診断モード (ピクセル数を 10, 60, 100, 160, 170 と順次テスト)
./tests/test_real_hardware.sh --mode full --device /dev/ttyUSB0

# 特定テープのプロファイルを適用してテスト (BTF 24V 160px の例)
./tests/test_real_hardware.sh --preset btf --device /dev/ttyUSB0
```

実行結果の例：
```
========================================================================
                   REAL HARDWARE TEST SUMMARY REPORT                    
========================================================================
 Total Tests Run : 17
 Passed Tests    : 17
 Failed Tests    : 0
 Target Device   : /dev/ttyUSB0 (115200 baud)
------------------------------------------------------------------------
Hardware verification completed successfully.
All test items passed on connected device /dev/ttyUSB0.
========================================================================
```

---

## 4. 実機テスト時の重要な注意点

1. **Arduino の DTR オートリセット待ち（2.2 秒）**:  
   Arduino Uno は、PC 側でシリアルポートを開いた瞬間に DTR 信号により自動リセットされます。Optiboot ブートローダ待機と `setup()` の完了までに **約 2.2 秒** を要するため、テストスクリプト等で接続直後にパケットを送信する場合は、必ず 2.2 秒以上の待機時間を設けてください。
2. **GND 共通接続の徹底**:  
   LED テープが勝手に点滅したり、白いノイズが走る場合の大半は、**Arduino の GND と LED 外部電源の GND が直結されていない（グラウンド浮き）** ことが原因です。
3. **過信の防止と目視確認の重要性**:  
   スクリプト上の `[PASS]` 表示は「通信プロトコルとデータ送出が正常に完了した」ことを意味します。LED の物理的な発色や輝度、色の並び順が意図通りであるかは、必ず**人間の目で直接確認**してください。
