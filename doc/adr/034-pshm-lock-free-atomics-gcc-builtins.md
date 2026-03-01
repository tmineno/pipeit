# ADR-034: PSHM Lock-Free Atomics via GCC Builtins

## Context

PSHM ではプロセス間共有メモリ上の `Superblock` と `SlotHeader` をアトミックに読み書きする必要がある。通常の C++ では `std::atomic<T>` を使うが、以下の制約がある:

1. **`std::atomic<T>` は trivially copyable ではない** — `memcpy` や `mmap` で共有する POD 構造体のフィールドに使えない
2. **`std::atomic` の内部ロックはプロセスローカル** — `is_lock_free()` が `false` のプラットフォームでは cross-process で動作しない
3. **バイナリレイアウトの安定性** — `std::atomic<T>` の内部表現は実装依存であり、異なるコンパイラ/バージョン間での互換性が保証されない

設計にあたり、以下の選択肢が存在した:

1. **GCC builtins (`__atomic_load_n` / `__atomic_store_n`)**: POD フィールドに直接適用、コンパイル時にロックフリー保証を検証
2. **`std::atomic<T>` メンバー**: 標準的だが上記の制約あり
3. **`volatile` + メモリバリア**: ポータビリティ低、意味論が不明確
4. **C++20 `std::atomic_ref<T>`**: クリーンだが C++20 必須、コンパイラバージョン依存

## Decision

**GCC builtins + コンパイル時ロックフリー検証** を採用する。

### 1. POD 構造体 + 明示的アトミックヘルパー

`Superblock` と `SlotHeader` は `#pragma pack(push, 1)` の plain POD として定義し、論理的にアトミックなフィールド (`write_seq`, `epoch`, `seq` 等) には専用ヘルパー関数を通してアクセスする:

```cpp
inline uint64_t shm_load_acquire(const uint64_t *p) {
    return __atomic_load_n(p, __ATOMIC_ACQUIRE);
}
inline void shm_store_release(uint64_t *p, uint64_t v) {
    __atomic_store_n(p, v, __ATOMIC_RELEASE);
}
```

### 2. コンパイル時ロックフリー保証

`__atomic_always_lock_free()` を `static_assert` で検証し、ロックフリーでないプラットフォームではビルドを停止する:

```cpp
static_assert(__atomic_always_lock_free(sizeof(uint64_t), &probe_u64),
              "PSHM requires lock-free 64-bit atomics");
```

### 3. 非 POSIX プラットフォーム対応

`#if defined(__unix__)` ガードで POSIX 固有コード (shm_open, mmap) を囲み、非対応プラットフォームでは `ShmIoAdapter` がエラーログ + no-op にフォールバックする。

## Consequences

- **バイナリレイアウト安定**: 構造体は plain POD のため `static_assert(sizeof(...))` で厳密に検証可能
- **ゼロオーバーヘッド**: x86/ARM64 では `__atomic_load_n` はフェンス付き通常 load に展開される
- **コンパイル時安全性**: ロックフリー不可のプラットフォームではビルドが失敗し、サイレントな破損を防止
- **ポータビリティ制約**: GCC/Clang 必須。MSVC では `_InterlockedCompareExchange` 等への差し替えが必要
- **C++20 移行パス**: 将来 `std::atomic_ref<T>` が広く利用可能になれば、ヘルパー関数を置換可能（API 変更なし）

## Alternatives

### `std::atomic<T>` メンバー

- **利点**: 標準 C++、IDE サポート良好
- **欠点**: trivially copyable でないため mmap POD に使えない。プロセスローカル mutex フォールバックのリスク

### `volatile` + 明示的メモリバリア

- **利点**: C89 互換、最小依存
- **欠点**: `volatile` はアトミック性を保証しない。メモリオーダリングの意味論が曖昧

### C++20 `std::atomic_ref<T>`

- **利点**: 標準的、POD フィールドにも適用可能
- **欠点**: C++20 必須（現在のプロジェクト標準は C++20 だが、一部ターゲット環境で完全サポートが不確実）

## Exit criteria

- [x] `pipit_shm.h` で `__atomic_always_lock_free` の `static_assert` が通過する
- [x] `shm_load_acquire` / `shm_store_release` ヘルパーが全アトミックアクセスに使用されている
- [x] SHM ベンチマーク (`shm_bench.cpp`) でデータ破損なく正常動作する
