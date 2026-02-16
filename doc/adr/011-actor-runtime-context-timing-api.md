# ADR-011: Actor Runtime Context Timing API for Sink Time Axis

## Context

`pipit-lang-spec-v0.2.0` では、sink アクター（例: oscilloscope）が時間軸付きで表示するための標準的な時刻取得手段が未定義だった。既存仕様にはタスク統計（tick/missed/latency）はあるが、これは終了時の集計であり、actor 実行中に参照できる API ではない。

同時に、以下の制約がある:

1. DSL/BNF を変更せず後方互換を維持したい
2. 既存アクターシグネチャ（`operator()(in, out)`）を壊したくない
3. 全エッジに timestamp を埋め込む方式はデータパス・メモリ・互換性への影響が大きい

## Decision

ランタイム標準 API として、actor から参照可能な時間軸コンテキストを追加する。DSL 文法は変更しない。

### 1. 標準 API を `pipit.h` に追加

- `uint64_t pipit_now_ns()`
- `uint64_t pipit_iteration_index()`
- `double pipit_task_rate_hz()`

`pipit_now_ns()` は `steady_clock` ベースの単調時刻（ns）を返す。

### 2. 実装は thread-local context

ランタイム内部に `thread_local` な actor runtime context を保持し、各 task thread が自分の値を更新する。

- `iteration_index`
- `task_rate_hz`

### 3. codegen で task ループに context 更新を挿入

- タスク開始時に `task_rate_hz` を設定
- 論理イテレーションごとに `iteration_index` をインクリメントして設定
- `K > 1` の場合でも 1 論理イテレーションにつき 1 増加

## Consequences

- DSL/BNF/アクター呼び出しシグネチャは変更不要（後方互換維持）
- sink アクターは論理時間軸（`iteration_index / task_rate_hz`）と実時間観測（`now_ns`）の両方を構成できる
- 全トークン timestamp 化を避けるため、データパスのレイアウト変更は不要
- これは「actor 実行時点の観測値」であり、タスク間 end-to-end 遅延やトークン生成時刻を保証するものではない

## Alternatives

- **全 in/out データフローへの timestamp 埋め込み**:
  計測表現力は高いが、型・バッファサイズ・コード生成・既存アクター互換に大きな影響
- **ACTOR シグネチャに context 引数を追加**:
  API と既存アクター実装を破壊的変更
- **実装依存の private API として運用**:
  仕様として移植性・互換性が担保されない

## Exit criteria

- [ ] 言語仕様に actor 標準 API とセマンティクスが記載されている
- [ ] `runtime/libpipit/include/pipit.h` に API が公開されている
- [ ] codegen が task rate / iteration index を適切に設定する
- [ ] 既存テストと追加テストが通過する
