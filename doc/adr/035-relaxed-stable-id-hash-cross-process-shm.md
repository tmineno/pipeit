# ADR-035: Relaxed stable_id_hash Validation for Cross-Process SHM

## Context

v0.4.3 (ADR-021) で導入された `stable_id` は、bind エンドポイントの同一性を検証するためのセマンティックハッシュである。`compute_stable_id()` は以下の要素からハッシュを生成する:

- bind 方向 (IN/OUT)
- actor チェーン（接続された actor 名の列）
- トランスポート種別とパラメータ

PPKT (ADR-013) では、同一プロセス内の writer/reader が同じ PDL プログラムからコンパイルされるため、`stable_id_hash` の完全一致による検証が可能だった。

しかし PSHM の主要ユースケースである **クロスプロセス通信** では:

- writer と reader は **異なる PDL プログラム** からコンパイルされる
- bind 方向が逆 (writer=OUT, reader=IN)
- actor チェーンが異なる (e.g. `sine → sig` vs `sig → dump_block`)

このため `stable_id_hash` は構造的に不一致となり、厳格な検証は正当な使用パターンを拒否してしまう。

## Decision

**`stable_id_hash` の不一致を警告として記録し、attach を拒否しない。** データ互換性は他のコントラクトフィールド (dtype, rank, dims, slot_count, slot_payload_bytes) で十分に保証する。

### 検証戦略

| フィールド | 検証 | 不一致時の動作 |
|---|---|---|
| magic, version | 厳格 | reject (破損/非互換) |
| dtype | 厳格 | reject |
| rank, dims | 厳格 | reject |
| slot_count, slot_payload_bytes | 厳格 | reject |
| **stable_id_hash** | **警告のみ** | **log + continue** |

```cpp
// stable_id_hash: log mismatch as a warning but do not reject.
if (expected_stable_id_hash != 0 && sb->stable_id_hash != expected_stable_id_hash) {
    std::fprintf(stderr,
        "pshm reader: note: stable_id_hash mismatch in '%s' "
        "(reader=%lu, writer=%lu) — normal for cross-process SHM\n",
        name, (unsigned long)expected_stable_id_hash,
        (unsigned long)sb->stable_id_hash);
}
```

### PPKT (ADR-013) との違い

PPKT は UDP パケットレベルのプロトコルであり、`stable_id` 検証は行わない（パケットは自己記述型で dtype/sample_count をヘッダーに含む）。PSHM は共有メモリ上の永続的なコントラクトであるため、attach 時の検証が可能かつ有用だが、`stable_id_hash` のみ緩和する。

## Consequences

- **クロスプロセス互換性**: 独立した PDL プログラム間で PSHM 通信が可能
- **開発時デバッグ支援**: mismatch は stderr に警告として出力され、意図しない接続のデバッグに役立つ
- **データ安全性維持**: dtype/rank/dims/geometry の厳格検証により、型不一致によるメモリ破損を防止
- **将来の拡張性**: `stable_id_hash` をオプショナルなセマンティック検証として保持（将来 `--strict-bind` フラグで厳格モードを追加可能）

## Alternatives

### 厳格な拒否 (PPKT と同じ方針)

- **利点**: 意図しない接続を完全に防止
- **欠点**: クロスプロセス通信が不可能になる（writer/reader の stable_id が構造的に異なるため）

### stable_id_hash を完全に無視

- **利点**: 実装がシンプル
- **欠点**: 開発時の接続ミスを検出できない。将来セマンティック検証を追加する余地がなくなる

### 方向独立な stable_id の再設計

- **利点**: writer/reader で同じハッシュを生成可能
- **欠点**: `compute_stable_id()` の意味論変更が必要。既存の bind 検証 (v0.4.3) との後方互換性が複雑になる

## Exit criteria

- [x] `ShmReader::attach()` が `stable_id_hash` mismatch で拒否せず警告のみ出力する
- [x] `examples/shm/` でクロスプロセス通信が正常動作する
- [x] dtype/rank/dims の厳格検証は維持されている
