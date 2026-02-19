# ADR-013: PPKT Protocol and Socket-Based Sink/Source Actors

## Context

ADR-011 で追加された actor runtime timing API（`pipit_now_ns()`, `pipit_iteration_index()`, `pipit_task_rate_hz()`）により、actor は自身の時間軸情報を取得できるようになった。次のステップとして、パイプライン内の信号データを外部プロセス（オシロスコープ GUI、ロガー、テストツール等）へ送受信する仕組みが必要となった。

設計にあたり、以下の選択肢が存在した:

1. **GUI 組み込み**: ImGui/GLFW をランタイムに組み込み、actor 内で描画
2. **共有メモリ**: `mmap` / `shm_open` でプロセス間データ共有
3. **ソケット通信**: UDP/Unix ドメインソケットで独立プロセスへデータ送信

要件:

- actor スレッドをブロックしない（リアルタイム制約）
- GUI クラッシュがパイプラインを停止させない（プロセス分離）
- ランタイムに GUI ライブラリ依存を持ち込まない
- ローカルだけでなくリモート接続も将来対応可能

## Decision

**ソケット通信 + 自前プロトコル (PPKT)** を採用する。

### 1. プロセス分離アーキテクチャ

sink/source actor はソケットの writer/reader として実装する。オシロスコープ等の GUI は完全に別プロセスとし、PPKT パケットを受信して描画する。

```text
Pipit パイプライン           外部プロセス
    |                            |
socket_write("host:port") ─UDP→ pipit-scope (ImGui)
socket_read("host:port")  ←UDP─ signal generator
```

### 2. PPKT (Pipit Packet Protocol)

信号ストリーミング専用の軽量バイナリデータグラムプロトコルを定義した（`doc/spec/ppkt-protocol-spec-v0.2.0.md`）。

- **48 バイト固定ヘッダー** + 可変長ペイロード
- **リトルエンディアン** 固定（x86/ARM ターゲットに合致）
- 自己記述型: dtype, sample_count, sample_rate_hz を含むため、受信側は追加設定不要
- ADR-011 の timing API と連携: `timestamp_ns`, `iteration_index`, `sample_rate_hz` をヘッダーに埋め込み

### 3. トランスポート抽象化

POSIX ソケット API が UDP (`AF_INET`) と Unix ドメインソケット (`AF_UNIX`) を同一 `sendto`/`recvfrom` API で抽象化するため、追加の抽象化レイヤーは不要。

- `"host:port"` → `AF_INET` + `SOCK_DGRAM` (UDP)
- `"unix:///path"` → `AF_UNIX` + `SOCK_DGRAM` (IPC)
- 全ソケット `O_NONBLOCK`（`fcntl(fd, F_SETFL, O_NONBLOCK)`）

### 4. 送信側チャンキング

ペイロードが MTU を超える場合、送信側が自動的に複数の自己完結 PPKT パケットに分割する。

- デフォルト MTU: 1472B (Ethernet 1500 - IP 20 - UDP 8)
- 各チャンクは独立した PPKT パケット（受信側での再組み立て不要）
- `sequence` はチャンクごとにインクリメント
- `iteration_index` はチャンク内のサンプルオフセットで調整

### 5. actor PARAM 順序規約

codegen の `format_actor_params` は PDL 引数を PARAM 宣言順にマッピングする。SDF から推論される `N` は PDL 引数として渡されないため、PARAM リストの末尾に配置する必要がある。

```cpp
// ○ 正しい: ユーザー引数 → SDF 推論 N
PARAM(std::span<const char>, addr) PARAM(int, chan_id) PARAM(int, N)

// × 間違い: N が先頭だと PDL 引数のマッピングがずれる
PARAM(int, N) PARAM(std::span<const char>, addr) PARAM(int, chan_id)
```

この規約は `constant` actor（`RUNTIME_PARAM(float, value) PARAM(int, N)`）の既存パターンと一致する。

## Consequences

- **プロセス分離**: GUI/ロガーのクラッシュ・遅延がパイプラインに影響しない
- **ランタイム依存なし**: pipit_net.h は POSIX ヘッダーのみ依存（`<sys/socket.h>`, `<netinet/in.h>`, `<sys/un.h>`, `<arpa/inet.h>`, `<fcntl.h>`）
- **ネットワーク透過**: 同一マシンでもリモートでも同じ PDL コードで動作
- **非ブロッキング保証**: 送信失敗は無視、受信データなしはゼロ出力（SDF スケジュールを止めない）
- **POSIX 依存**: Linux/macOS のみ。Windows は Winsock2 への適応が別途必要
- **UDP 信頼性なし**: パケットロス・順序逆転は許容。確実な配信が必要な場合は TCP またはアプリケーション層 ACK が必要（Non-goal）
- **static 初期化の制約**: actor 内部の static 変数により、同一プロセス内で同一 actor 型の複数インスタンスは共有状態になる（codegen が 1 actor = 1 インスタンスを保証するため実運用上は問題なし）

## Alternatives

### GUI 組み込み（ImGui をランタイムに統合）

- **利点**: レイテンシ最小（プロセス間通信なし）、デプロイが単一バイナリ
- **欠点**: GUI クラッシュ = パイプライン停止、ImGui/GLFW/OpenGL 依存がランタイムに入る、ヘッドレス環境で動作しない、Windows/macOS で描画バックエンド対応必要

### 共有メモリ（mmap / shm_open）

- **利点**: ゼロコピーで高スループット
- **欠点**: 同一マシン限定、同期プリミティブ（セマフォ等）が必要、プロセスクリーンアップ複雑、ネットワーク透過性なし

### TCP ストリーム

- **利点**: 信頼性あり、順序保証
- **欠点**: ストリーム境界なし（フレーミング必要）、ブロッキングリスク（Nagle, send buffer full）、接続管理の複雑さ

### 既存プロトコル（ZeroMQ, gRPC, Protocol Buffers）

- **利点**: 機能豊富、エコシステム
- **欠点**: 外部ライブラリ依存、ランタイムフットプリント増大、PPKT のような最小限ヘッダーと比較してオーバーヘッド大

## Exit criteria

- [x] `doc/spec/ppkt-protocol-spec-v0.2.0.md` にプロトコル仕様が記載されている
- [x] `runtime/libpipit/include/pipit_net.h` に PpktHeader + DatagramSender/Receiver が実装されている
- [x] `runtime/libpipit/include/std_sink.h` に `socket_write` actor が実装されている
- [x] `runtime/libpipit/include/std_source.h` に `socket_read` actor が実装されている
- [x] codegen が `socket_write` / `socket_read` の正しい C++ コードを生成する
- [x] C++ 単体テスト（17 tests）とループバック統合テスト（2 tests）が通過する
- [x] 言語仕様 §14 に actor インターフェース記述がある
