# PixelNode Web 管理ダッシュボード & REST API 仕様書

`pixelnoded` デーモンには、外部 CDN やインターネット接続を必要としない、軽量な Web 管理ダッシュボードと REST API が組み込まれています。

現場の照明 LAN に接続されたスマートフォン、タブレット、PC のブラウザから、ノードの稼働状態の確認、単体点灯テスト、設定の調整が可能です。

> [!NOTE]
> **実機検証ステータス**:  
> Web UI / REST API の通信機能、設定変更、および Arduino Uno (USB-Serial) を経由した点灯テスト・制御は実機検証済みです。Raspberry Pi ハードウェア SPI からの直接出力検証は今後実施予定です。

---

## 1. Web ダッシュボードへのアクセス

`pixelnoded` が起動しているマシンに対し、同一ネットワーク内のブラウザからアクセスします：

```
http://<ノードのIPアドレス>:8080/
```

例: `http://192.168.1.50:8080/`（ローカル環境なら `http://127.0.0.1:8080/`）

* **完全オフライン対応**: 外部の Google Fonts や CDN スクリプト等は一切参照せず、すべてバイナリ内に内蔵された静的リソースから配信されます。インターネット回線のない現場 LAN でも問題なく動作します。

---

## 2. ダッシュボードの主な機能

### 1. ステータス・監視ビュー
* **レンダリング性能**: 目標フレームレート（50.0 FPS）と現在の実測 FPS、積算レンダリングフレーム数を表示。
* **ネットワーク DMX 受信**: Art-Net および sACN (E1.31) の受信パケット数、最終受信からの経過時間（Age）。
* **電力監視（ABL: Automatic Brightness Limiter）**: 現在の推定総消費電流（A）と、設定された電源上限値に対する利用比率（%）。
* **リアルタイム・チャンネルインジケーター**: 受信中の DMX チャンネル（調光、RGBW、パターンスロット、音声同期）の値をプログレスバーと数値で可視化。
* **音響同期ステータス**: 推定 BPM、RMS 音声エネルギー、およびキック／スネア／ハイハットのトリガー状態のリアルタイム点滅表示。

### 2. リグチェック・点検発光ビュー
照明卓がまだ稼働していない状況でも、ブラウザから直接テストパターンを発光させて結線・電源を点検できます：
* **RGBW 巡回**: 赤 $\rightarrow$ 緑 $\rightarrow$ 青 $\rightarrow$ 白 の順次点灯で色順と LED 素子の健全性を確認。
* **アドレスウォーカー**: 1 つの白色ピクセルが端から端へ順次移動し、ピクセル数と信号方向を確認。
* **100% 全白点灯 (Power Soak)**: 全ピクセルを最大輝度で点灯させ、電源の容量不足や末端の電圧降下（赤変）を確認。
* **高頻度ストロボ**: 40Hz の高速点滅により、信号のクロック安定性を確認。
* **カラーパレット / カラーピッカー**: 任意の色を指定して常時点灯。
* **停止 / DMX 制御へ復帰**: テスト発光を終了し、照明卓からの制御へ戻します。

### 3. 設定管理ビュー
* ノード名、バインド IP、Art-Net / sACN 有効無効の切り替え。
* パーソナリティ（`preset_7ch`, `standard`, `pixel_direct`）の選択。
* カラーオーダー（`RGB`, `RBG`, `GRB`, `BRG`, `BGR`, `GBR`）の選択。
* 開始ユニバース、開始チャンネル、制御ピクセル数の変更。
* 電源容量上限（mA）、フェイルセーフ動作（`hold`, `hold_then_fade`, `blackout`）の設定。
* **設定の保存と即時適用**: 変更内容はディスク上の設定ファイル（TOML）へアトミックに書き込まれ、デーモンの描画ループにも即座に反映されます。

### 4. Show Lock（誤操作防止ロック）
* 画面右上のロックボタン（`🔒 Locked` / `🔓 Unlocked`）。
* Show Lock が有効な間は、Web UI や REST API からの設定変更およびテスト発光の割り込みが拒否され、本番中の誤タップによる事故を防止します。

---

## 3. REST API 仕様

外部の管理スクリプトや監視ツールから利用可能な REST エンドポイントです。CORS に対応し、JSON 形式でデータをやり取りします。

### `GET /api/status`
ノードのリアルタイム稼働状況を JSON で取得します。

```bash
curl http://192.168.1.50:8080/api/status
```

**レスポンス例:**
```json
{
  "node_name": "Stage-WS2811-Node1",
  "version": "0.1.0",
  "uptime_sec": 3612,
  "show_lock": false,
  "fps_x10": 500,
  "frame_count": 180600,
  "artnet_rx": 72200,
  "sacn_rx": 0,
  "current_ma": 3400,
  "max_psu_ma": 15000,
  "failsafe_active": false,
  "test_mode": 0,
  "channels": [255, 255, 0, 128, 0, 5, 0, 0, 0, 0],
  "universes": [
    {
      "universe": 1,
      "source": "ArtNet",
      "healthy": true,
      "age_ms": 18
    }
  ],
  "audio": {
    "connected": true,
    "bpm": 128.5,
    "energy": 0.76,
    "kick": true,
    "snare": false,
    "hihat": true
  }
}
```

---

### `GET /api/config`
現在の設定内容（ノード設定、ネットワーク、フェイルセーフ、出力ドライバ、フィクスチャ割り当て）を JSON で取得します。

```bash
curl http://192.168.1.50:8080/api/config
```

---

### `POST /api/config`
ノード設定を更新し、ディスク（TOML）へ保存して稼働中のループへ反映します。

* **HTTP ステータスコード:**
  * `200 OK`: 設定が正常に更新・保存された
  * `400 Bad Request`: JSON 構造が不正
  * `403 Forbidden`: Show Lock が有効化されている

```bash
curl -X POST http://192.168.1.50:8080/api/config \
  -H "Content-Type: application/json" \
  -d '{
    "node": { "name": "Stage-Left-WS2811", "show_lock": false },
    "network": { "bind_ip": "0.0.0.0", "artnet": true, "sacn": true },
    "failsafe": { "on_data_loss": "hold", "hold_timeout_sec": 10, "fade_duration_sec": 3 },
    "output": [{
      "id": "out1",
      "driver": "serial_mcu",
      "device": "/dev/ttyUSB0",
      "chip": "ws2811",
      "color_order": "rgb",
      "pixels": 160,
      "psu_max_current_ma": 7000
    }],
    "fixture": [{
      "output": "out1",
      "pixel_range": [0, 159],
      "personality": "preset_7ch",
      "white_mode": "rgb_blend",
      "universe_span": "per_170px",
      "universe": 1,
      "address": 1
    }]
  }'
```

---

### `POST /api/test`
点検用のテストパターン発光を指示します。

```bash
# RGBW 巡回テスト開始
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "rgbw"}'

# 任意の色（例: アンバー）で常時点灯
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "color", "color": [255, 140, 20]}'

# テストを解除し、照明卓の制御へ復帰
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "off"}'
```

---

### `POST /api/lock`
Show Lock の状態を切り替えます。

```bash
# 本番ロックの有効化
curl -X POST http://192.168.1.50:8080/api/lock \
  -H "Content-Type: application/json" \
  -d '{"show_lock": true}'

# 本番ロックの解除
curl -X POST http://192.168.1.50:8080/api/lock \
  -H "Content-Type: application/json" \
  -d '{"show_lock": false}'
```
