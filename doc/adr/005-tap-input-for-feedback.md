# ADR-005: Tap-input syntax for feedback loops

## Context

SDF グラフ構築（§8 step 4）において、フィードバックループの接続表現に問題が発生した。

元の spec §5.10 では、同名アクターの重複によりフィードバック接続を暗黙的に表現していた:

```
input() | add() | filter() | :fb -> output
:fb | delay(1, 0.0) | add()
```

コンパイラはこれを「同名アクター (`add`) のうち dead-end ノードを primary ノードにマージする」ヒューリスティック (`merge_multi_input_actors`) で処理していたが、同名アクターが 3 個以上存在する場合にどの dead-end をどの primary に接続すべきか判別できない根本的欠陥があった。

## Decision

アクター引数にタップ参照 (`:name`) を許可し、追加入力ポートへの接続を構文レベルで明示化する。

```
input() | add(:fb) | filter() | :out -> output
:out | delay(1, 0.0) | :fb
```

- `add(:fb)` — アクター `add` の追加入力としてタップ `:fb` を接続
- フィードバックでは消費がタップ宣言より前に出現するため、`Arg::TapRef` に限り前方参照を許容
- 複数の追加入力も `add(:fb, :fwd)` のように対応可能
- `merge_multi_input_actors` ヒューリスティックは完全に削除

## Consequences

- 接続先が構文レベルで確定するため、同名アクター 3+ 個でも曖昧性なし
- BNF 変更: `arg ::= ... | ':' IDENT`
- AST: `Arg::TapRef(Ident)` variant 追加
- Parser: actor arg 内の `:name` パース追加
- Resolve: `Arg::TapRef` の deferred tap validation（前方参照対応）
- Graph builder: `pending_tap_inputs` による遅延エッジ作成
- Spec §5.6 / §5.10 / §10 更新

## Alternatives

- **名前ベースマージの改良**: primary/dead-end のペアリングを改善する方法。3+ 個で曖昧性が残る上、暗黙的な接続は可読性が低い
- **専用構文 (`feedback` ブロック等)**: より大きな言語変更が必要。タップの仕組みを再利用する方が最小限の変更で済む

## Exit criteria

Revisit if:

- 前方参照の許容範囲が広がりすぎてエラー検出が困難になった場合
- フィードバック以外の用途で `Arg::TapRef` の意味が曖昧になった場合
