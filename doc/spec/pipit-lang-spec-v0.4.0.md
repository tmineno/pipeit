# Pipit Language Specification v0.4.0 (Draft)

## 1. Overview

Pipit は、共有メモリ上に SDF (Synchronous Dataflow) セマンティクスでアクターを配置し、クロック駆動のリアルタイムデータパイプラインを簡潔に記述するためのドメイン固有言語である。

### 1.1 設計原則

- **SDFベース**: 各アクターの生産/消費トークン数が静的に決定され、コンパイル時にスケジュールとバッファサイズが確定する
- **CSDF拡張**: control subgraph によるモード切替で条件分岐を表現しつつ、各モード内の静的解析可能性を維持する
- **共有メモリ通信**: タスク間のデータ受け渡しは共有メモリ上のリングバッファを介して行う
- **クロック駆動**: 各タスクは明示的な target rate で駆動される
- **直感的な構文**: 最小限の特殊記号とキーワードにより、低い学習コストで記述可能
- **安全優先の型変換**: 意味保存できる数値拡張のみ暗黙化し、意味変換は明示アクターを要求する
- **接続の遅延束縛**: `bind` 文により外部接続先を宣言し、方向・型・レートは既存パイプラインから推論する
- **静的展開による多チャネル並列化**: `clock` の spawn 句と共有バッファ配列をコンパイル時に展開し、SDF の静的解析可能性を維持する

### 1.2 ターゲット環境

- 通常の OS (Linux / Windows)
- リアルタイム性の保証は CPU 性能に依存（ハードリアルタイム OS を前提としない）
- マルチコア配置はスケジューラに委任（明示的なコアピニングは行わない）

### 1.3 ツールチェイン

Pipit のソースファイル (`.pdl`) は、専用コンパイラ `pcc` に入力され、C++ コードへ変換された後に `libpipit` とリンクして実行形式を生成する。アクターメタデータは `--actor-meta` で `actors.meta.json` を直接指定でき、未指定時は `-I` / `--include` / `--actor-path` で与えたヘッダを `pcc` が直接スキャンして取得する。

```
source.pdl + (actors.meta.json or actors.h)
  → pcc → source_gen.cpp
  → g++/clang++ → executable
```

`pcc` の CLI インターフェース、コンパイル処理フロー、エラー出力形式の詳細は現行仕様 [pcc-spec](pcc-spec-v0.4.0.md) を参照。

### 1.4 用語定義

| 用語 | 定義 |
|------|------|
| **アクター** | SDF グラフ上のノード。固定の消費/生産トークン数を持つ処理単位 |
| **タスク** | 1つ以上のパイプラインを含む実行単位。独立したスレッドで駆動される |
| **イテレーション** | SDF スケジュールの1完全反復。グラフ内の全アクターが repetition vector で定まる回数だけ発火する |
| **ティック** | タスクのクロックタイマーが発生する1周期。1ティック = K イテレーション (K ≥ 1) |
| **イテレーション境界** | イテレーションの完了と次のイテレーション開始の間の論理的な時点 |
| **共有バッファ** | タスク間でデータを受け渡す非同期 FIFO。共有メモリプール上にリングバッファとして静的配置される |
| **共有バッファ配列（family）** | 固定長の共有バッファ集合。`name[idx]` で要素参照し、`name[*]` で全要素を束ねて参照できる |
| **タップ** | パイプライン内のフォークノード。データをコピーして複数の下流に分配する |
| **プローブ** | 非侵入的な観測点。リリースビルドではゼロコストで除去される |
| **バインド** | `bind` 文で宣言される外部接続。共有バッファ名を外部エンドポイントへ遅延束縛する |
| **spawn 句** | `clock` に付与するコンパイル時複製指定。1つのタスク定義から複数タスクを静的生成する |
| **安定ID (`stable_id`)** | バインド対象を一意に識別する決定的 ID。コンパイラが生成し、UI はこの ID を主キーとして使う |
| **アクターマニフェスト** | `ACTOR` 宣言由来のメタデータファイル（`actors.meta.json`）。`--actor-meta` 指定時に `pcc` が読み込む |

---

## 2. 字句構造

### 2.1 文字セット

ソースファイルは UTF-8 でエンコードされる。識別子には ASCII 英数字およびアンダースコアのみを使用する。

### 2.2 コメント

```
# 行末までがコメント
```

### 2.3 特殊記号

| 記号 | 用途 | 意味 |
|------|------|------|
| `\|` | パイプ演算子 | アクター間のデータフロー接続 |
| `->` | 書込み演算子 | 共有メモリバッファへの書込み |
| `@name` | バッファ読出し | 共有メモリバッファからの読出し |
| `:name` | タップ | パイプライン内のフォーク（分岐）ポイント |
| `?name` | プローブ | デバッグ用の非侵入的観測点 |
| `$name` | パラメータ参照 | ランタイムパラメータの参照 |
| `<...>` | 型引数 | polymorphic actor の型実引数 |
| `*` | 全要素参照 | 共有バッファ配列 `name[*]` の全要素を束ねる |
| `..` | 範囲演算子 | spawn 句の半開区間 `[begin..end)` を表す |
| `#` | コメント | 行末コメント |

### 2.4 キーワード

以下の識別子は予約語であり、ユーザー定義の識別子として使用できない。

```
set  const  param  shared  define  clock  mode  control  switch  default  delay  bind
```

### 2.5 リテラル

#### 数値リテラル

```
42          # 整数
3.14        # 浮動小数点数
-1.5        # 負数
1e-3        # 指数表記
```

#### 周波数リテラル

```
100Hz
48kHz
10MHz
2.4GHz
```

周波数リテラルは数値と単位の組であり、内部的には Hz 単位の数値に変換される。

#### サイズリテラル

```
64KB
256MB
1GB
```

サイズリテラルは数値とバイト単位の組であり、内部的にはバイト単位の整数に変換される。

#### 文字列リテラル

```
"output.csv"
```

ダブルクォートで囲む。エスケープシーケンスは `\"` および `\\` のみサポートする。

#### 配列リテラル

```
[0.1, 0.4, 0.1]
[1, 2, 3, 4]
```

配列リテラルはスカラー値のみを含む。ネストは文法レベルで禁止される。要素は同一型でなければならない（型検査で検証）。

---

## 3. 型システム

### 3.1 概要

Pipit の型情報は、`--actor-meta` で指定されたマニフェスト、または `-I` / `--include` / `--actor-path` からのヘッダスキャンで取得されるアクターメタデータから構築される。`pcc` は `IN`/`OUT`/`PARAM`/`RUNTIME_PARAM` 情報を参照し、パイプライン接続の型整合性を静的に検証する。v0.3.0 以降、アクター呼び出しは以下の 2 形式を許容する。

- 明示型引数付き: `actor<float>(...)`
- 型引数省略: `actor(...)`（制約解決で一意に定まる場合のみ）

### 3.2 基本型

| 型名 | 説明 | C++ 対応型 |
|------|------|-----------|
| `int8` | 8ビット符号付き整数 | `int8_t` |
| `int16` | 16ビット符号付き整数 | `int16_t` |
| `int32` | 32ビット符号付き整数 | `int32_t` |
| `float` | 32ビット浮動小数点数 | `float` |
| `double` | 64ビット浮動小数点数 | `double` |
| `cfloat` | 複素浮動小数点数 | `std::complex<float>` |
| `cdouble` | 複素倍精度浮動小数点数 | `std::complex<double>` |

### 3.3 型推論規則（v0.3.0 追補）

型推論は、(1) actor シグネチャ、(2) パイプ接続制約、(3) 引数（literal/const/param）の3系統の制約を統合して行う。

- 一意に解ける場合: 推論結果を採用する
- 解が存在しない場合: 型不整合エラー
- 複数解が残る場合: 曖昧性エラー（明示型引数を要求）

`const` / `param` の数値主型は以下の順序で解く。

```
int8 < int16 < int32 < float < double
cfloat < cdouble
```

- 整数リテラルは `int32` を初期候補とする
- 小数点を含むリテラルは `float` を初期候補とする
- 使用箇所制約で必要なら最小限の上位型に拡張する

```
error: type mismatch at pipe 'fft -> fir'
  fft outputs cfloat[256], but fir expects float[5]
  hint: insert an explicit conversion actor (e.g. c2r)
```

### 3.4 型変換

暗黙変換は「狭窄変換および実数/複素の意味変換を伴わない数値拡張」に限定して許可する。許可される暗黙変換は以下の2系統のみ。

```
int8 -> int16 -> int32 -> float -> double   # 実数系
cfloat -> cdouble                            # 複素数系
```

注: `int32 -> float` は値域上の拡張だが、`|x| > 2^24` では丸めが発生しうる。

各系統内では左から右への変換のみ暗黙的に行われる。系統をまたぐ変換（実数 ↔ 複素数）は暗黙変換の対象外である。

以下は暗黙変換しない（明示変換を要求する）。

- 狭窄変換: `double -> float`, `int32 -> int16`, `float -> int32` 等
- 実数/複素の意味変換: `cfloat -> float`, `float -> cfloat` 等
- それ以外の未定義変換

```
adc(0) | fft(256) | c2r() | fir(coeff) -> signal
#                   ^^^^^ cfloat → float 変換アクター
```

#### 狭窄変換の警告

明示的な変換アクター（例: `f2i()`, `d2f()`）を介して狭窄変換を行う場合でも、コンパイラは情報損失の可能性がある変換に対して警告を発するべきである（SHOULD）。

```
warning: narrowing conversion at pipe 'scale -> quantize'
  double -> int16 may lose precision
  at example.pdl:8:20
```

この警告はデフォルトで有効であり、プログラムの正しさは変更しない。

### 3.5 アクター多相（polymorphism）

同一アルゴリズムに対して複数の入出力型を許容するため、actor 呼び出しは型引数を持てる。

```pdl
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]

clock 1kHz t {
    constant(0.0) | fir<float>(coeff) | stdout()
}
```

型引数省略時は推論を試みる。

```pdl
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]

clock 1kHz t {
    constant(0.0) | fir(coeff) | stdout()   # 型引数は文脈から推論
}
```

推論が曖昧な場合はエラーとし、明示型引数を要求する。

---

## 4. アクター定義（C++ 側）

### 4.1 ACTOR マクロ

アクターは C++ で `ACTOR` マクロを用いて定義する。`IN(type, count)` と `OUT(type, count)` によって消費/生産トークン数を静的に宣言する。

```cpp
#include <pipit.h>

ACTOR(fir, IN(float, 5), OUT(float, 1)) {
    float y = 0;
    for (int i = 0; i < 5; i++)
        y += coeff[i] * in[i];
    out[0] = y;
}
```

#### polymorphic actor 定義（v0.3.0）

`ACTOR` マクロの前に `template <typename T>` を記述することで、型パラメトリックなアクターを定義できる。

```cpp
#include <pipit.h>

template <typename T>
ACTOR(scale, IN(T, N), OUT(T, N), PARAM(T, gain) PARAM(int, N)) {
    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;
    return ACTOR_OK;
}
```

`pcc` は `template <typename T> ACTOR(scale, ...)` パターンを認識し、`T` を型パラメータとして扱う。モノモーフ化時に `pcc` は PDL の使用箇所から具体型を解決し、`IN(T, N)` → `IN(float, N)` 等の型代入を行う。C++ コンパイラは生成コード中の `actor_scale<float>` を通常のテンプレートインスタンス化として処理する。

PDL 側では明示型引数または推論によって呼び出す。

```pdl
clock 1kHz t {
    constant(1.0) | scale<float>(2.0) | stdout()   # 明示
    constant(1.0) | scale(2.0) | stdout()           # 推論
}
```

#### マクロの生成物

`ACTOR` マクロは以下を生成する。

1. **ファンクタクラス**: `IN`/`OUT` インターフェースを持つ `noexcept` な `operator()` を備えるクラス
2. **宣言情報**: `IN`/`OUT`/`PARAM`/`RUNTIME_PARAM` 記述。`pcc` は `--actor-meta` の JSON、またはヘッダの直接スキャンから同等情報を取得する

`pcc` は C++ の完全な構文解析を行わず、ヘッダ中の `ACTOR` 宣言をテキストスキャンしてメタデータを構築する。`--actor-meta` 指定時は JSON マニフェストを優先入力として扱う。

#### ファンクタ内で使用可能なシンボル

| シンボル | 型 | 説明 |
|----------|-----|------|
| `in` | `const T[N]` | 入力バッファ（消費トークン数 N 個） |
| `out` | `T[M]` | 出力バッファ（生産トークン数 M 個） |
| パラメータ名 | 各型 | DSL 側から渡されるパラメータ |

#### 標準実行コンテキスト API（時間軸）

時間軸を必要とする sink アクターは、以下の標準 API を使用して実行コンテキストを取得できる。ランタイム実装はこれらを提供しなければならない（MUST）。

```cpp
uint64_t pipit_now_ns();            // monotonic clock（ns）
uint64_t pipit_iteration_index();   // 当該タスクの論理イテレーション番号（0始まり）
double   pipit_task_rate_hz();      // 当該タスクの target rate（clock <freq>）
```

- `pipit_now_ns()`:
  単調増加クロック（`steady_clock` 相当）の現在時刻を ns 単位で返す。エポックは未規定（実装依存）だが、同一プロセス内で単調性を満たす。
- `pipit_iteration_index()`:
  タスク内の論理イテレーション番号を返す。`K > 1` の場合もイテレーションごとに 1 ずつ増加する（1ティックで K 回進む）。
- `pipit_task_rate_hz()`:
  DSL の `clock <freq>` で指定した target rate を返す。`drop/slip/backlog` による実効レート変動は反映しない。

これらの API は読み取り専用であり、SDF のレート整合、スケジュール、最適化、FIFO 順序を変更してはならない（MUST NOT）。
また、これらは「アクター実行時点の観測値」であり、タスク間 end-to-end 遅延やトークン生成時刻の保証を与えるものではない。

### 4.2 パラメトリックアクター

DSL 側から引数を受け取るアクターは `PARAM` で宣言する。配列引数にはポインタではなく `std::span` を使用する。

```cpp
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    fft_exec(in, out, N);
}

ACTOR(fir, IN(float, N), OUT(float, 1),
      PARAM(std::span<const float>, coeff),
      PARAM(int, N)) {
    float y = 0;
    for (int i = 0; i < N; i++) y += coeff[i] * in[i];
    out[0] = y;
}
```

`fir` の旧引数順（例: `fir(5, coeff)`）は v0.2 系では移行対象であり、`fir(coeff)` または `fir(coeff, 5)` を使用する。

#### パラメータの所有権と寿命

- `PARAM` で受け取る値は、パイプライン全体の寿命にわたって有効な `const` 参照である
- DSL 側の `const` 定義から渡される配列は、コンパイラが生成する静的ストレージに配置される
- アクターはパラメータの所有権を持たず、変更もできない

### 4.3 ランタイムパラメータを受け取るアクター

`param` で宣言されたランタイムパラメータは `RUNTIME_PARAM` で受け取る。

```cpp
ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) {
    out[0] = in[0] * gain;  // gain はイテレーション境界で更新されうる
}
```

`gain` はダブルバッファリングされた読み取り専用参照であり、イテレーション実行中は値が変化しないことが保証される。

### 4.4 エラーハンドリング

アクターの `operator()` は `noexcept` である。アクター内でのエラーはリターンコードで表現する。

```cpp
ACTOR(safe_div, IN(float, 2), OUT(float, 1)) {
    if (in[1] == 0.0f) return ACTOR_ERROR;  // エラーコード
    out[0] = in[0] / in[1];
    return ACTOR_OK;
}
```

| リターンコード | 意味 |
|---------------|------|
| `ACTOR_OK` | 正常完了 |
| `ACTOR_ERROR` | 回復不能エラー。タスク停止を引き起こす |

---

## 5. パイプライン記述言語

### 5.1 グローバル設定

```
set mem = 64MB
set overrun = drop
```

`set` 文はランタイムの動作パラメータを設定する。

| キー | 型 | デフォルト | 説明 |
|------|----|-----------|------|
| `mem` | SIZE | `64MB` | 共有メモリプールの最大サイズ |
| `overrun` | IDENT | `drop` | オーバーラン時のポリシー（§5.4.3 参照） |
| `tick_rate` | FREQ | `10kHz` | OSタイマーのウェイク周波数。K = ceil(タスク周波数 / tick_rate)。高周波タスクのバッチ処理に使用 |
| `timer_spin` | NUMBER or `auto` | `10000` | デッドライン前のスピンウェイト時間（ナノ秒）。`auto` でEWMAベースの適応的スピン調整を有効化。CPU使用量と引き換えにタイマー精度を向上 |
| `wait_timeout` | NUMBER | `50` | タスク間リングバッファの待機タイムアウト（ミリ秒）。1–60000。タイムアウト時はランタイムエラー |

現行実装のスケジュール生成アルゴリズムは固定であり、タスク内では PASS（Periodic Asynchronous Static Schedule）を用いる。`set` によるスケジューリングアルゴリズム選択は v0.2 ではサポートしない。

#### `tick_rate` — K ファクタバッチ処理

`tick_rate` は OS タイマーのウェイク周波数を制御する。タスクの target rate が `tick_rate` を超える場合、コンパイラは `K = ceil(タスク周波数 / tick_rate)` を計算し、1回の OS ウェイクにつき K 回のアクター発火をバッチ実行する。

##### K=1（デフォルト: tick_rate ≥ タスク周波数）

```
set tick_rate = 10kHz    # デフォルト
clock 10kHz task { ... }  # K = ceil(10kHz / 10kHz) = 1

Time ──►
        ├── 100µs ──┤── 100µs ──┤── 100µs ──┤── 100µs ──┤
OS wake ▼           ▼           ▼           ▼           ▼
        ┌──┐        ┌──┐        ┌──┐        ┌──┐        ┌──┐
        │A1│ sleep  │A2│ sleep  │A3│ sleep  │A4│ sleep  │A5│
        └──┘        └──┘        └──┘        └──┘        └──┘
        ◄──►
        ~19ns
        (actor work)

→ 1 tick = 1 firing。OS は 10,000 回/秒ウェイクする。
  フレームワークオーバーヘッド（sleep/wake遷移）が毎回発生。
```

##### K=10（tick_rate = 1kHz、タスク周波数 = 10kHz）

```
set tick_rate = 1kHz
clock 10kHz task { ... }  # K = ceil(10kHz / 1kHz) = 10

Time ──►
        ├────────────── 1ms ──────────────┤────────── 1ms ──────
OS wake ▼                                 ▼
        ┌──┬──┬──┬──┬──┬──┬──┬──┬──┬──┐  ┌──┬──┬──┬──┬──┬──┬──
        │A1│A2│A3│A4│A5│A6│A7│A8│A9│A10│ │A1│A2│A3│A4│A5│A6│A7
        └──┴──┴──┴──┴──┴──┴──┴──┴──┴──┘  └──┴──┴──┴──┴──┴──┴──
        ◄─────── ~190ns ──────────────►
        (10 firings in burst)           sleep ~~~~~~~~~~~

→ 1 tick = 10 firings。OS は 1,000 回/秒ウェイクする。
  sleep/wake オーバーヘッドが 10 発火に分散され、
  1発火あたりのフレームワークコストが約 1/10 に低下。
  トレードオフ: 10 発火がバースト実行されるため、
  最悪応答時間が K × period（= 1ms）に増加。
```

**使用例:**

```
set tick_rate = 1kHz    # 1kHz でウェイク
clock 10kHz fast { adc(0) | fir(coeff) -> signal }   # K=10
clock 100Hz slow { @signal | stdout() }               # K=1（100Hz < 1kHz）
```

#### `timer_spin` — ハイブリッドスリープ/スピンウェイト

`timer_spin` はデッドライン直前のスピンウェイト時間（ナノ秒）を指定する。OS の `sleep_until()` にはジッタ（数十〜数百 µs）があるため、スピンウェイトにより最終区間の精度を向上させる。

##### timer_spin = 0（スピンなし）

```
set timer_spin = 0
clock 10kHz task { ... }

        ├─────────────── 100µs period ─────────────────┤
        │                                               │
        │◄──────── OS sleep_until(deadline) ──────────►│
        │                                     ▲         │
        │                              actual wake-up   │
        │                              (jitter ~76µs)   │
        ▼                                     │         ▼
   ─────┼─────────────────────────────────────┼─────────┼─────
        deadline(N)                    wake    ▼    deadline(N+1)
                                              ┌──┐
                                              │A │ actor work
                                              └──┘
                                              ◄►
                                           latency
                                           ~76µs avg
```

##### timer_spin = 50000（50µs スピン）

```
set timer_spin = 50000    # 50µs = 50,000ns
clock 10kHz task { ... }

        ├─────────────── 100µs period ─────────────────┤
        │                                               │
        │◄── OS sleep_until ──►│◄── busy spin ───────►│
        │  (deadline - 50µs)   │   while(now<deadline) │
        │                      │    ┊  ┊  ┊  ┊  ┊  ┊  │
        ▼                      ▼    ┊  ┊  ┊  ┊  ┊  ┊  ▼
   ─────┼──────────────────────┼────┊──┊──┊──┊──┊──┊──┼─────
        deadline(N)       spin_start ╰──╯  ╰──╯  ╰──╯ deadline(N+1)
                                                  ▲
                                          spin narrows wake
                                          jitter (env-dependent)
                                                  │
                                                  ┌──┐
                                                  │A │
                                                  └──┘
```

##### `timer_spin = auto` — 適応的スピンウェイト（EWMA）

`set timer_spin = auto` を指定すると、ランタイムはウェイクアップジッタを自動的に計測し、スピンウィンドウを動的に調整する。

- **ブートストラップ**: 初期スピン閾値は 10us（固定デフォルトと同一）。
- **EWMA更新**: 各 `sleep_until()` 後、実測ジッタを指数移動平均で追跡（alpha = 1/8、整数演算）。
- **安全マージン**: `spin_threshold = clamp(2 × ewma, 500ns, 100us)`。
- **オーバーヘッド**: 1 ティックあたり約 2ns（整数減算 + 8 による除算）。

プラットフォーム毎のスリープ粒度に自動適応するため、手動チューニングが不要になる。

### 5.2 定数定義

```
const coeff = [0.1, 0.4, 0.1]
const threshold = 0.5
const fs = 48kHz
```

`const` で定義された値はコンパイル時に確定し、変更できない。生成コードにおいて静的ストレージに配置される。

### 5.3 ランタイムパラメータ

```
param gain = 1.0
param enable = 1
```

`param` で定義された値は実行時に外部から変更可能である。初期値は必須。

#### 参照

DSL 内でランタイムパラメータを参照するには `$name` 記法を使う。

```
adc(0) | mul($gain) -> output
```

#### 更新セマンティクス

- 更新は**イテレーション境界**で反映される。1つのイテレーション実行中に値が変化することはない
- 実装はダブルバッファリングにより、外部書込み領域とアクター読取り領域を分離し、イテレーション境界でアトミックにスワップする
- 外部からの更新頻度がタスクのイテレーションレートを超える場合、最新の値のみが採用される（間引き）
- 値の型はアクターの対応する `RUNTIME_PARAM` 型と一致しなければコンパイルエラーとなる

#### 外部制御インターフェース

ランタイムパラメータは CLI 引数で変更可能。

```
$ ./my_app --param gain=2.0 --param enable=0
```

### 5.3.1 共有バッファ配列宣言

固定チャネル数の共有バッファ群を宣言するには `shared` を用いる。

```pdl
const CH = 24
shared in[CH]
shared out[CH]
```

- `shared name[N]` は長さ `N` の共有バッファ配列（family）を定義する
- `N` は正のコンパイル時整数（整数リテラルまたは `const`）でなければならない
- `shared` で宣言された family 要素は `name[idx]` で参照する
- `name[*]` は family 全要素を1つの束として参照する（詳細は §5.7）

### 5.4 タスク定義

タスクはクロック駆動のパイプラインの実行単位である。

```
clock 10MHz capture {
    adc(0) | fft(256) | :raw | fir(coeff) -> signal
    :raw | mag() | stdout()
}
```

#### 5.4.1 構文

```
clock <freq> <name> [<idx>=<begin>..<end>] { <task_body> }
```

- `clock <freq>`: タスクの target rate。省略不可
- `<name>`: タスク名。プログラム内で一意でなければならない
- `[<idx>=<begin>..<end>]`: 任意の spawn 句。半開区間 `[begin, end)` でタスクを複製する
- `<task_body>`: パイプライン行、または mode/control 構成

#### 5.4.2 実行モデル

各タスクはスケジューラにより独立したスレッドとして実行される。`clock freq` はタスクの **target rate** であり、ランタイムはこの周波数に基づくタイマーで**ティック**を生成する。

1ティックあたり K イテレーションを実行する (K ≥ 1)。現行実装での K は次式で決定される。

- `K = ceil(clock_freq / tick_rate)`（§5.1）
- `clock_freq <= tick_rate` の場合は `K = 1`
- `tick_rate` 省略時はデフォルト `10kHz` を用いる

#### 5.4.3 オーバーラン処理

通常 OS 上ではタイマージッタが不可避であり、1ティック内にスケジュールが完了しない場合がありうる。オーバーラン時の動作は `set overrun` で指定する。

| ポリシー | 動作 | 適用場面 |
|---------|------|---------|
| `drop` (デフォルト) | 間に合わなかったティックをスキップし、次のティックから再開する。スキップされたティックのイテレーションは実行されない | リアルタイム信号処理 |
| `slip` | ティック間隔を実行完了まで延長する（実効周波数が低下する） | スループット重視のバッチ処理 |
| `backlog` | 遅れたティック分を蓄積し、後続のティックで追加イテレーションを実行して追いつこうとする | 全データ処理が必須な場合 |

オーバーラン統計は `--stats` フラグで取得できる。

```
$ ./my_app --duration 10s --stats
...
[stats] task 'capture': 100000000 ticks, 12 missed (drop), max_latency=142ns
[stats] task 'drain':   10000 ticks, 0 missed
```

#### 5.4.4 同値スケジュール最適化

コンパイラは、SDF 意味論を保持する範囲で、同値な静的スケジュールへ変換してよい（MAY）。
たとえば、隣接ノード群が同一 repetition count を持つ場合、実装はそれらを1つの反復ループに融合して実行してよい。

ただし最適化の有無にかかわらず、以下は保持されなければならない（MUST）。

- 1イテレーションで各アクターが repetition vector どおり発火すること
- 各エッジ上の FIFO トークン順序
- 観測可能な副作用順序（共有バッファ境界、sink 出力、プローブ出力）

#### 5.4.5 spawn 句（静的タスク複製）

spawn 句付き `clock` は、コンパイル時に複数の独立タスクへ展開される。

```pdl
const CH = 24
shared in[CH]

clock 48kHz capture[ch=0..CH] {
    adc(ch) | fir(coeff) -> in[ch]
}
```

- `clock ... capture[ch=0..CH]` は `capture[0]` 〜 `capture[CH-1]` の `CH` 個へ静的展開される
- 展開後の各タスクは通常の `clock` タスクと同一セマンティクスで実行され、相互に並列である
- `idx` はコンパイル時インデックス変数であり、spawn 本体内で actor 引数と buffer 添字に使用できる
- `begin` / `end` は正のコンパイル時整数でなければならない。`begin < end` を満たさない場合はコンパイルエラー
- spawn 展開は name resolve / 型推論 / SDF 解析の前に実行される

### 5.5 パイプ演算子

```
actor_a() | actor_b() | actor_c()
```

パイプ演算子 `|` はアクター間のデータフロー接続を表す。左辺の出力が右辺の入力に接続される。SDF グラフ上の有向エッジに対応する。

#### 名前解決規則

パイプライン内の裸の識別子（括弧なし）の解決は以下の規則に従う。

| 位置 | 構文 | 解釈 |
|------|------|------|
| 行頭 | `@name` / `@name[idx]` / `@name[*]` | 共有バッファ（単体/配列要素/配列全体）からの読出し |
| パイプ中 | `name(...)` | アクター呼出し（0引数でも括弧必須） |
| パイプ中 | `:name` | タップ（宣言または参照） |
| パイプ中 | `?name` | プローブ |
| パイプ末尾 | `-> name` / `-> name[idx]` / `-> name[*]` | 共有バッファ（単体/配列要素/配列全体）への書込み |

**アクターは必ず括弧付き**で記述する。0引数のアクターも `name()` と書く。これにより、アクター呼出しと他の構文要素（共有バッファ名、タップ名、キーワード）の間に構文上の曖昧さは生じない。

```
# 正しい
adc(0) | fft(256) | mag() | stdout()

# コンパイルエラー: 括弧なしの裸識別子はアクター呼出しとして認識されない
adc(0) | fft(256) | mag | stdout
```

#### レート整合

パイプ演算子の両端でトークンレートが異なる場合（例: `OUT(float, 256)` → `IN(float, 1)`）、SDF バランス方程式に基づいてアクターの発火回数（repetition vector）が自動的に調整される。これは SDF の標準的なセマンティクスである。

### 5.6 タップ（分岐）

```
adc(0) | fft(256) | :raw | fir(coeff) -> signal
:raw | mag() | stdout()
```

`:name` はパイプライン中の**フォークノード**を定義する。

#### セマンティクス

- `:name` は SDF グラフ上の暗黙的なフォークアクターに展開される
- 入力トークンを**コピー**して、すべての下流エッジに同一トークンを出力する
- SDF レート: `IN(T, N) → OUT(T, N) × M`（M = 下流エッジ数）
- 実装は参照カウント付き共有バッファにより物理コピーを回避してよい（最適化の自由）

#### 制約

- タップ名はタスク内で一意でなければならない
- 宣言されたタップは必ず1回以上消費されなければならない

```
adc(0) | :orphan | fir(coeff) -> signal
# error: tap ':orphan' declared but never consumed
```

- パイプライン行頭のタップ参照（`pipe_source` の `:name`）は、宣言より後に記述されなければならない
- アクター引数内のタップ参照（`arg` の `:name`）は前方参照が許容される（フィードバックループ用途、§5.10 参照）

### 5.7 共有メモリバッファ

タスク間のデータ通信は共有メモリバッファを介して行う。共有バッファは非同期 FIFO としてモデル化され、有界性がコンパイル時に静的に保証される。

#### 書込み

```
... | fir(coeff) -> signal
... | fir(coeff) -> out[ch]
```

`-> name` / `-> name[idx]` / `-> name[*]` はパイプラインの末尾に置かれ、共有メモリバッファへデータを書き込む。

#### 読出し

```
@signal | decimate(10000) | csvwrite("out.csv")
@in[ch] | fir(coeff) | stdout()
```

`@name` / `@name[idx]` / `@name[*]` をパイプラインの先頭に置くことで、共有メモリバッファからデータを読み出す。`@` プレフィックスにより、共有バッファの読出しであることが構文上明示される。

#### 要素参照（`name[idx]`）

- `name[idx]` は共有バッファ配列（family）の単一要素を指す
- `idx` は整数リテラル、`const`、または spawn インデックス変数でなければならない
- `idx` はコンパイル時に範囲検査される。`0 <= idx < N` を満たさない場合はコンパイルエラー
- `name[idx]` は独立した共有バッファとして扱われる（型・レート・単一ライター制約も要素単位）

#### 全要素参照（`name[*]`）

`name[*]` は配列全要素をチャンネル次元で束ね、1つの仮想ポートとして扱う。

- 要素数を `C`、各要素のフレーム要素数を `F` とすると、`name[*]` の shape は `[C, F]`（2次元）として扱う
- 実行時レイアウトは従来どおりフラットであり、shape はコンパイル時メタデータである
- `@name[*]` は `name[0], name[1], ..., name[C-1]` を index 昇順で結合する gather と等価
- `-> name[*]` は上流から供給された `[C, F]` を各要素へ分配する scatter と等価
- `name[*]` に接続される全要素は同一 dtype と同一 `F` を満たさなければならない（不一致はコンパイルエラー）

#### 単一ライター制約

一つの共有メモリバッファ（または family 要素）に対して `->` を記述できるのは1タスクのみ。

```
clock 10MHz a { ... -> signal }
clock 10MHz b { ... -> signal }
# error: multiple writers to shared buffer 'signal'
```

`-> name[*]` は family の全要素に対する writer 宣言として扱う。したがって `-> name[*]` と `-> name[idx]` の混在、または複数タスクからの `-> name[*]` は許可されない。

#### 複数リーダー

```
clock 1kHz c { @signal | proc1() | ... }
clock 1kHz d { @signal | proc2() | ... }
# OK: 各リーダーは独立したリードポインタを持つ
```

同一クロックの複数リーダーは、同一イテレーション内で同じデータを観測する（スナップショット読み取り）。同じ規則は `name[idx]` と `name[*]` にも適用される。

#### クロックドメイン境界とレート整合条件

異なるクロック周波数のタスク間で共有メモリバッファを使用する場合、以下のレート整合条件を満たさなければならない。

**整合条件:**

writer タスクが1イテレーションで共有バッファに `Pw` トークンを書き込み、target rate が `fw` であるとする。reader タスクが1イテレーションで `Cr` トークンを読み出し、target rate が `fr` であるとする。このとき、

```
Pw × fw = Cr × fr
```

が成立しなければコンパイルエラーとなる。

```
error: rate mismatch at shared buffer 'buf'
  writer 'fast': 1 token/iteration × 10MHz = 10M tokens/sec
  reader 'slow': 1 token/iteration × 1kHz  = 1K tokens/sec
  hint: insert rate conversion actor (e.g. decimate(10000))
```

`name[*]` を用いる場合の追加条件:

- `@name[*]`（gather）では、全要素 `name[i]` が同一 `Cr_elem × fr` を満たさなければならない
- `-> name[*]`（scatter）では、上流の総トークンレートが全要素へ等分可能でなければならない
- 2次元 shape `[C, F]` の `F` は全要素で一致しなければならない

**バッファサイズ:**

共有バッファのリングバッファサイズは、採用するスケジュール生成アルゴリズムに基づく**安全側上界**として算出される。一般にバッファ最小化は NP 完全であるため、コンパイラは最小サイズを保証するものではない。

算出されたバッファサイズの総計が `set mem` で指定したプールサイズを超える場合はコンパイルエラーとなる。

```
error: shared memory pool exceeded
  required: 78MB (signal: 40MB, ctrl: 2MB, internal: 36MB)
  available: 64MB (set mem = 64MB)
```

#### 遷移時の共有バッファ

CSDF モード遷移（§6参照）時に破棄されるのはタスク内部のエッジ上のトークンのみである。**共有バッファ上のデータは遷移の影響を受けない**。

### 5.8 プローブ

```
adc(0) | fft(256) | ?spec | fir(coeff) -> signal
```

`?name` はパイプライン中に挿入する非侵入的な観測点である。

#### セマンティクス

- データをそのまま通過させつつ、観測用バッファにコピーを出力する
- SDF レート: `IN(T, N) → OUT(T, N)`（パイプラインのレートに影響しない）
- リリースビルド（`pcc --release`）ではデッドコード除去により完全に除去される
- デバッグビルドで有効化するには実行時フラグを使用する

```
$ ./my_app --probe spec --probe raw
```

- プローブデータの出力先は `--probe-output <path>` で指定する（ファイルパスのみ）。標準エラーへ出力したい場合は `/dev/stderr` などの実装依存パスを使用する。

### 5.9 サブパイプライン定義

```
define demod(n) {
    fft(n) | eq() | demap()
}

clock 10MHz rx { adc(0) | demod(256) -> bits }
```

`define` はパイプラインの断片に名前を付けて再利用可能にする。アクターではなく、呼出し箇所にインライン展開される。SDF の階層グラフに対応する。

#### 制約

- 引数は `arg` 文法に従う（`value` / `$param` / `:tap` / `IDENT`）
- 再帰的定義は禁止
- `define` ブロック内でタップを定義できるが、そのスコープは展開先のタスク内に限定される

### 5.10 フィードバックループと初期トークン

SDF グラフにフィードバックループが含まれる場合、`delay` アクターによって初期トークンを明示的に供給しなければならない。

フィードバック接続はアクター引数にタップ参照 (`:name`) を記述することで表現する。

```
clock 10MHz iir {
    input() | add(:fb) | filter() | :out -> output
    :out | delay(1, 0.0) | :fb
}
```

`add(:fb)` はアクター `add` の追加入力ポートにタップ `:fb` を接続することを意味する。上記の例では `:fb` は2行目の末尾で宣言され、1行目の `add(:fb)` で消費される（前方参照）。フィードバックループではタップの前方参照が許容される（§5.6 参照）。

複数の追加入力を持つアクターには、複数のタップ参照を指定できる:

```
clock 10MHz example {
    adc(0) | add(:fb, :fwd) | :out | stdout()
    adc(1) | delay(1, 0.0) | :fwd
    :out | delay(1, 0.0) | :fb
}
```

#### delay アクター

```
delay(N, init)
```

| 引数 | 型 | 説明 |
|------|----|------|
| `N` | 正整数 | 遅延トークン数 |
| `init` | 出力型と同じ | 初期トークンの値 |

- `delay` は SDF エッジ上の初期トークンを表現する組み込みアクターである
- SDF レート: `IN(T, 1) → OUT(T, 1)` + 初期トークン N 個
- フィードバックループ内に `delay` が存在しない場合、コンパイルエラーとなる

```
error: feedback loop detected at ':out -> :fb -> add' with no delay
  hint: insert delay(N, init) to break the cycle
```

### 5.11 `bind` 文（外部接続の遅延束縛）

`bind` は共有バッファ名を外部エンドポイントへ接続する宣言である。型・shape・レート・方向は DSL から推論される。

```
bind iq  = udp("127.0.0.1:9100", chan=10)
bind cmd = udp("127.0.0.1:9200", chan=2)
bind iq2 = shm("rx.iq", slots=1024, slot_bytes=4096)
```

利用可能な endpoint 種別:

- `udp("host:port", chan=<u16>)`
- `unix_dgram("unix:///path", chan=<u16>)`
- `shm("<name>", slots=<int>, slot_bytes=<int>)`

`udp` / `unix_dgram` は [ppkt-protocol-spec-v0.3.0.md](ppkt-protocol-spec-v0.3.0.md) に従う。`shm` は [pshm-protocol-spec-v0.1.0.md](pshm-protocol-spec-v0.1.0.md) に従う。

#### セマンティクス

- `bind <name> = <endpoint>` は共有バッファ `<name>` に対する外部接続を1つ定義する
- `bind` は SDF グラフ構造を変更しない。スケジュール、repetition vector、既存 FIFO 順序を保持しなければならない（MUST）
- 実装は `bind` を非ブロッキングな入出力アダプタへ lower してよい（MAY）。観測可能な意味は `socket_write` / `socket_read` と同等でなければならない（MUST）
- `shm` endpoint は同一ホスト上の複数 PDL プロセス間通信を対象とする。トランスポート層での信頼性再送は行わない（MUST NOT）

#### 方向推論

バインド方向は次で一意に決定する。

1. `-> name` または `-> name[*]` が1つ以上存在する場合: **out-bind**
2. 上記が存在せず `@name` または `@name[*]` が1つ以上存在する場合: **in-bind**
3. 上記いずれにも該当しない場合: コンパイルエラー

#### 契約（contract）推論

- **型/shape**: 共有バッファ `name` の推論済みポート型から決定する
- **レート**:
  - out-bind: writer の `Pw × fw`（tokens/sec）
  - in-bind: 全 reader が要求する `Cr × fr` が同一値に収束しなければならない
- 推論結果はランタイム制御面の `list_bindings` で取得できなければならない（§9.5）
- コンパイル時に静的成果物が必要な場合、実装は interface manifest を出力してよい（§9.4）

#### 安定ID（`stable_id`）生成

- コンパイラは各 `bind` に対して決定的な `stable_id` を生成しなければならない（MUST）
- `stable_id` は span や単純な名前文字列ではなく、意味 ID（タスク/ノード/エッジ由来）を基に生成する（MUST）
- 同一の意味グラフに対しては再コンパイル間で同一 `stable_id` を生成する（MUST）

#### ランタイム再配線

- 外部 UI からの再配線要求は `stable_id` をキーに受け付ける
- 再配線の適用は**イテレーション境界**でのみ行う（MUST）
- 再配線は bind 単位で原子的に適用される（MUST）
- 契約（方向・型・shape・レート）を変更する再配線は拒否しなければならない（MUST）

#### 制約

- `bind` 対象名は少なくとも1つの `@name` / `@name[*]` / `-> name` / `-> name[*]` として参照されなければならない
- 同一 `name` に対する `bind` は最大1つ
- in-bind で reader 契約が一意に定まらない場合はコンパイルエラー

```
error: bind target 'telemetry' cannot infer direction
  hint: add either '@telemetry' or '-> telemetry' in a task
```

---

## 6. CSDF モード切替

### 6.1 概要

タスク内で複数の動作モードを定義し、制御信号に基づいて切り替える。各モードは独立した SDF グラフとして静的解析される。モード遷移の制御は、全モード共通で常時動作する **control subgraph** が担う。

### 6.2 構文

```
clock 10MHz receiver {
    control {
        adc(0) | correlate() | detect() -> ctrl
    }
    mode sync {
        adc(0) | sync_process() -> sync_out
    }
    mode data {
        adc(0) | fft(256) | fir(coeff) -> payload
    }
    switch(ctrl, sync, data)
}
```

#### 構成要素

- `control { ... }`: control subgraph。モード遷移に関係なく常時実行される SDF グラフ。`switch` の制御信号を供給する
- `mode <name> { ... }`: モードブロック。各モードは独立した SDF グラフとして定義される
- `switch(ctrl, mode_a, mode_b, ...)`: モード遷移を制御する構文。`ctrl` は control subgraph 内のアクターが共有バッファまたはタスク内エッジ経由で供給する値
- `switch(... ) default <mode>`: 後方互換のため受理されるが、v0.2 では非推奨。コンパイラは警告を出し、実行時の意味は持たない

### 6.3 ctrl の供給規則

`switch(ctrl, ...)` の `ctrl` は以下のいずれかでなければならない。

1. **control subgraph 内で `-> ctrl` により書き出された共有バッファ**（タスク内通信）
2. **`param $ctrl`** によるランタイムパラメータ

外部からの制御を使う場合も、`switch` の ctrl は本タスク内で `control { ... }` から `-> ctrl` に書き出すか、`param $ctrl` として供給しなければならない（§6.7 の制約に従う）。供給元が存在しない場合はコンパイルエラーとなる。

```
error: switch ctrl 'ctrl' has no supplier
  hint: define 'ctrl' in a control block or as a param
```

ctrl の型は `int32` でなければならない。値は 0, 1, 2, ... がモードリストの順序に対応する。

### 6.4 静的解析

- control subgraph は独立した SDF グラフとして解析・スケジューリングされる
- 各 mode ブロックも独立した SDF グラフとして個別にバランス方程式が解かれる
- モード間でトークンレートやバッファサイズが異なることは許容される
- control subgraph と現在のアクティブモードは、各イテレーションで順次実行される（control → mode の順）

### 6.5 遷移セマンティクス

- モード遷移は**イテレーション境界**でのみ発生する（イテレーション実行中には起きない）
- 遷移時、旧モードの**タスク内エッジ上の残存トークン**は破棄される（クリーンスタート）
- **共有バッファ上のデータは遷移の影響を受けない**（§5.7参照）
- 保持が必要なタスク内データは遷移前に共有バッファ (`-> name`) に退避すること

### 6.6 初期モードと default 句（互換）

起動直後の初期モードは固定ではなく、最初のイテレーションで control subgraph を実行して得られた `ctrl` 値で決定される。

```
switch(ctrl, sync, data) default sync
```

`default` 句は v0.2 ではソフト非推奨であり、構文互換のため受理されるが実行時には無視される。コンパイラは警告を出力する。

### 6.7 制約

- `switch` 文はタスクブロック内に最大1つ
- `mode` ブロックを持つタスクには `control` ブロックまたは `param` による ctrl 供給が必須
- `mode` ブロックを持たないタスクに `switch` / `control` は記述できない
- ctrl の全有効値（0 〜 モード数-1）は網羅的でなければならない。ctrl がこの範囲外の値を取る場合の動作は未定義である（`default` 句の有無にかかわらず、コンパイラは可能な範囲で警告を発する）

---

## 7. エラーセマンティクス

### 7.1 コンパイル時エラー

| カテゴリ | 例 |
|----------|-----|
| 型不整合 | パイプ接続の入出力型が一致しない |
| レート不整合 | クロックドメイン境界で `Pw × fw ≠ Cr × fr` |
| 名前解決失敗 | 存在しないアクター、共有バッファ、タップの参照 |
| 多重書込み | 同一共有バッファへの複数ライター |
| 未消費タップ | 宣言されたタップが消費されていない |
| デッドロック | フィードバックループに `delay` がない |
| ctrl 供給不在 | `switch` の ctrl に供給元がない |
| パラメータ型不整合 | `param` の型とアクターの `RUNTIME_PARAM` 型の不一致 |
| spawn 範囲不正 | `clock ... [ch=begin..end]` で `begin >= end` または非整数 |
| 配列添字範囲外 | `name[idx]` の `idx` が宣言範囲外 |
| family 契約不整合 | `name[*]` で dtype / frame 次元 / 要素レートが一致しない |
| bind 方向推論失敗 | `bind name = ...` で `@name` / `-> name` が存在しない |
| bind 契約曖昧 | in-bind の reader 群から単一レート契約を導けない |
| bind endpoint 不正 | `bind` の endpoint 種別やオプションが未定義/範囲外 |
| SDF バランス不能 | バランス方程式に非負整数解が存在しない |
| メモリプール超過 | 算出バッファサイズの総計が `set mem` を超過 |
| 構文エラー | BNF に適合しないソース |

### 7.2 実行時エラー

アクターが `ACTOR_ERROR` を返した場合、ランタイムは**フェイルファスト**でパイプライン全体を停止する。

1. 当該アクターを含むタスクを即座に停止する
2. 共有バッファ読出し/書込み失敗を含む致命エラーはグローバル停止フラグを設定する
3. 全タスクは停止フラグを観測して終了し、プロセスはエラー終了コードを返す

```
runtime error: actor 'safe_div' in task 'capture' returned ACTOR_ERROR
  task 'capture' stopped
pipit: pipeline terminated with error (exit code 1, fail-fast)
```

エラー情報は stderr に出力される。プローブが有効な場合はプローブにも出力される。

---

## 8. コンパイラ処理フロー

コンパイラ `pcc` の処理フロー（字句解析 → 構文解析 → spawn 展開 → アクターマニフェスト読込み（必要に応じて生成） → 名前解決 → 型制約解決/モノモーフ化 → SDF グラフ構築 → 静的解析 → スケジュール生成 → C++ コード生成 → C++ コンパイル）の詳細は現行仕様 [pcc-spec](pcc-spec-v0.4.0.md) を参照。

polymorphism と暗黙数値拡張を含むプログラムは、実装内部で explicit な lower 形へ書き換えられてもよい。ただしこの書き換えは意味保存でなければならない。少なくとも以下を満たすこと（MUST）。

- 挿入される暗黙拡張は `int8 -> int16 -> int32 -> float -> double` および `cfloat -> cdouble` の範囲に限定
- 書き換えによりトークンレート/shape を変更しない
- 解決不能または曖昧な型は診断エラーとし、暗黙のフォールバック型を採用しない

---

## 9. 実行形式インターフェース

### 9.1 CLI 引数

```
$ ./my_app [OPTIONS]
```

| オプション | 説明 | デフォルト |
|------------|------|-----------|
| `--duration <time>` | 実行時間 (`10s`, `1m`, `inf`) | `inf` |
| `--threads <n>` | スレッドプールサイズ | CPU 論理コア数 |
| `--param <name>=<value>` | ランタイムパラメータの初期値上書き | DSL 定義値 |
| `--bind <name>=<endpoint>` | `bind` で宣言した接続先の初期値上書き（複数指定可） | DSL 定義値 |
| `--probe <name>` | 指定プローブを有効化（複数指定可） | 全無効 |
| `--probe-output <path>` | プローブ出力先（ファイルパス） | `stderr` 相当の実装定義パス |
| `--stats` | 終了時にオーバーラン統計等を表示 | 無効 |

### 9.2 終了コード

| コード | 意味 |
|--------|------|
| `0` | 正常終了（`--duration` 経過 or SIGINT） |
| `1` | ランタイムエラーによる停止 |
| `2` | 起動時エラー（パラメータ不正等） |

### 9.3 統計出力

`--stats` フラグ指定時、終了時に以下の統計が stderr に出力される。

```
[stats] task 'capture': ticks=100000000, missed=12 (drop), max_latency=142ns, avg_latency=87ns
[stats] task 'drain':   ticks=10000, missed=0, max_latency=890us, avg_latency=420us
[stats] shared buffers: signal=4096 tokens (16KB), ctrl=64 tokens (256B)
[stats] memory pool: 64MB allocated, 17KB used
```

### 9.4 バインドインターフェース出力（任意）

コンパイラ実装は、pcc 仕様で定義される方法により、`bind` 対象の推論結果を interface manifest（例: `pipeline.interface.json`）として任意に出力してよい（MAY）。

出力する場合、最低限以下の情報を含むべきである（SHOULD）。

- `stable_id`（決定的 ID）
- `name`（PDL 上の共有バッファ名）
- `direction`（`in` / `out`）
- `dtype` / `shape`
- `rate_hz`（tokens/sec を表す実数）
- `endpoint`（初期値）

### 9.5 ランタイム制御面（Control Plane）

ランタイムは UI/外部ツール向けに、以下の制御操作を提供しなければならない（MUST）。

- `list_bindings`: 現在のバインド一覧を取得
- `rebind(stable_id, endpoint)`: 接続先を更新

`rebind` 要求は即時に I/O スレッドへ反映してはならず、対象タスクのイテレーション境界で原子的に適用しなければならない（MUST）。

---

## 10. 形式文法 (BNF)

```
program         ::= (statement NL)*

statement       ::= set_stmt
                  | const_stmt
                  | param_stmt
                  | shared_stmt
                  | bind_stmt
                  | define_stmt
                  | task_stmt
                  | comment
                  | EMPTY

comment         ::= '#' .*

# ── グローバル文 ──

set_stmt        ::= 'set' IDENT '=' set_value

set_value       ::= NUMBER | SIZE | FREQ | STRING | IDENT

const_stmt      ::= 'const' IDENT '=' value

param_stmt      ::= 'param' IDENT '=' scalar

shared_stmt     ::= 'shared' IDENT '[' shape_dim ']'

bind_stmt       ::= 'bind' IDENT '=' bind_endpoint

bind_endpoint   ::= IDENT '(' bind_args? ')'

bind_args       ::= bind_arg (',' bind_arg)*

bind_arg        ::= scalar
                  | IDENT '=' scalar

# ── サブパイプライン定義 ──

define_stmt     ::= 'define' IDENT '(' params? ')' '{' pipeline_body '}'

# ── タスク定義 ──

task_stmt       ::= 'clock' FREQ IDENT spawn_clause? '{' task_body '}'

spawn_clause    ::= '[' IDENT '=' range_expr ']'
range_expr      ::= shape_dim '..' shape_dim

task_body       ::= pipeline_body
                  | modal_body

modal_body      ::= control_block? mode_block+ switch_stmt

control_block   ::= 'control' '{' pipeline_body '}'

mode_block      ::= 'mode' IDENT '{' pipeline_body '}'

switch_stmt     ::= 'switch' '(' switch_src ',' IDENT (',' IDENT)+ ')'
                     default_clause? NL

switch_src      ::= IDENT              # 共有バッファ名
                  | '$' IDENT           # ランタイムパラメータ

default_clause  ::= 'default' IDENT    # v0.2: 互換のため受理（非推奨・実行時無効）

# ── パイプライン ──

pipeline_body   ::= (pipeline_line NL)*

pipeline_line   ::= pipe_expr
                  | comment

pipe_expr       ::= pipe_source ('|' pipe_elem)* sink?

pipe_source     ::= '@' buffer_ref     # 共有バッファ読出し
                  | ':' IDENT           # タップ参照（消費側）
                  | actor_call          # アクター（ソースアクター）

pipe_elem       ::= actor_call
                  | ':' IDENT           # タップ（宣言側）
                  | '?' IDENT           # プローブ

actor_call      ::= IDENT type_args? '(' args? ')' shape_constraint?
type_args       ::= '<' type_name (',' type_name)* '>'
type_name       ::= IDENT
shape_constraint ::= '[' shape_dims ']'
shape_dims      ::= shape_dim (',' shape_dim)*
shape_dim       ::= NUMBER | IDENT     # IDENT は const 参照

sink            ::= '->' buffer_ref    # 共有バッファ書込み

buffer_ref      ::= IDENT
                  | IDENT '[' index_expr ']'
                  | IDENT '[' '*' ']'

index_expr      ::= NUMBER             # 整数リテラル
                  | IDENT               # const 参照 or spawn index 参照

# ── 引数・パラメータ ──

args            ::= arg (',' arg)*

arg             ::= value
                  | '$' IDENT           # ランタイムパラメータ参照
                  | ':' IDENT           # タップ参照（追加入力ポート）
                  | IDENT               # const 参照

params          ::= IDENT (',' IDENT)*

# ── 値 ──

value           ::= scalar
                  | array

scalar          ::= NUMBER
                  | FREQ
                  | SIZE
                  | STRING
                  | IDENT               # const 参照

array           ::= '[' scalar (',' scalar)* ']'

# ── 字句要素 ──

FREQ            ::= NUMBER FREQ_UNIT
FREQ_UNIT       ::= 'Hz' | 'kHz' | 'MHz' | 'GHz'

SIZE            ::= NUMBER SIZE_UNIT
SIZE_UNIT       ::= 'KB' | 'MB' | 'GB'

NUMBER          ::= '-'? [0-9]+ ('.' [0-9]+)? ([eE] [+-]? [0-9]+)?
STRING          ::= '"' ( [^"\\] | '\\' ["\\] )* '"'
IDENT           ::= [a-zA-Z_] [a-zA-Z0-9_]*
STAR            ::= '*'
RANGE_DOTS      ::= '..'

NL              ::= '\n'+
EMPTY           ::= NL
```

---

## 11. 完全なサンプルプログラム

### 11.1 アクター定義 (actors.h)

```cpp
#include <pipit.h>
#include <span>
#include <complex>

// ── ソースアクター ──

ACTOR(adc, IN(void, 0), OUT(float, 1), PARAM(int, channel)) {
    out[0] = hw_adc_read(channel);
    return ACTOR_OK;
}

// ── 変換アクター ──

ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    fft_exec(in, out, N);
    return ACTOR_OK;
}

ACTOR(c2r, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}

ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}

ACTOR(fir, IN(float, N), OUT(float, 1),
      PARAM(std::span<const float>, coeff),
      PARAM(int, N)) {
    float y = 0;
    for (int i = 0; i < N; i++) y += coeff[i] * in[i];
    out[0] = y;
    return ACTOR_OK;
}

ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) {
    out[0] = in[0] * gain;
    return ACTOR_OK;
}

// ── レート変換アクター ──

ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    out[0] = in[0];
    return ACTOR_OK;
}

// ── 信号処理アクター ──

ACTOR(correlate, IN(float, 64), OUT(float, 1)) {
    out[0] = sync_correlate(in, 64);
    return ACTOR_OK;
}

ACTOR(detect, IN(float, 1), OUT(int32, 1)) {
    out[0] = (in[0] > 0.5f) ? 1 : 0;
    return ACTOR_OK;
}

ACTOR(sync_process, IN(float, 256), OUT(float, 1)) {
    out[0] = sync_demod(in, 256);
    return ACTOR_OK;
}

// ── シンクアクター ──

ACTOR(csvwrite, IN(float, 1), OUT(void, 0), PARAM(std::span<const char>, path)) {
    csv_append(path.data(), in[0]);
    return ACTOR_OK;
}

ACTOR(stdout, IN(float, 1), OUT(void, 0)) {
    printf("%f\n", in[0]);
    return ACTOR_OK;
}
```

### 11.2 パイプライン定義 (example.pdl)

```
# example.pdl — OFDM receiver (Pipit)
# ======================================

# グローバル設定
set mem = 64MB
set overrun = drop

# コンパイル時定数
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]
const fft_size = 256

# ランタイムパラメータ
param gain = 1.0

# サブパイプライン定義
define frontend(n) {
    adc(0) | mul($gain) | fft(n) | c2r()
}

# メイン受信タスク: 10MHz target rate
clock 10MHz capture {
    frontend(fft_size) | :raw | fir(coeff) | ?filtered -> signal
    # FIXME(v0.4.0): frontend() は c2r() で float 化しているため、
    # この枝の mag() 接続は型不整合。サンプル改訂時に修正する。
    :raw | mag() | stdout()
}

# 後段処理: 1kHz target rate
clock 1kHz drain {
    @signal | decimate(10000) | csvwrite("output.csv")
}
```

### 11.3 CSDF モード切替を含む例 (receiver.pdl)

```
# receiver.pdl — Adaptive OFDM receiver with mode switching
# ==========================================================

set mem = 128MB
set overrun = drop

const sync_coeff = [1.0, -1.0, 1.0, -1.0]
const data_coeff = [0.1, 0.2, 0.4, 0.2, 0.1]

clock 10MHz receiver {
    # control subgraph: 全モード共通で常時動作
    # correlate → detect が ctrl 値を供給し続ける
    control {
        adc(0) | correlate() | detect() -> ctrl
    }

    # モード 0: 同期モード — フレーム同期を取得
    mode sync {
        adc(0) | fir(sync_coeff) | ?sync_out -> sync_result
    }

    # モード 1: データモード — OFDM 復調
    mode data {
        adc(0) | fft(256) | c2r() | fir(data_coeff) | ?data_out -> payload
    }

    # ctrl=0 → sync, ctrl=1 → data
    # NOTE: `default sync` は互換のため受理されるが v0.2 では非推奨かつ実行時無効
    switch(ctrl, sync, data) default sync
}

clock 1kHz logger {
    @payload | decimate(2560000) | csvwrite("received.csv")
}
```

### 11.4 ビルドと実行

```bash
# コンパイル
$ pcc example.pdl -I ./actors.h -o receiver

# 実行 (10秒間, filteredプローブ有効, gain初期値上書き)
$ ./receiver --duration 10s --probe filtered --param gain=1.5 --stats

# 終了時の統計出力例
[stats] task 'capture': ticks=100000000, missed=12 (drop), max_latency=142ns
[stats] task 'drain':   ticks=10000, missed=0, max_latency=890us
[stats] shared buffers: signal=4096 tokens (16KB)
[stats] memory pool: 64MB allocated, 17KB used
```

### 11.5 `bind` を用いた外部接続

```pdl
clock 48kHz audio_out {
    sine(1000, 1.0) -> wave
}

clock 1kHz control_in {
    @ctrl | stdout()
}

bind wave = udp("127.0.0.1:9100", chan=0)
bind ctrl = udp("127.0.0.1:9200", chan=2)
```

`wave` は `-> wave` から out-bind と推論され、`ctrl` は `@ctrl` から in-bind と推論される。

### 11.6 多チャネル spawn と配列全体参照（v0.4.0）

```pdl
const CH = 24
const FRAME = 256

shared in[CH]
shared out[CH]

# 24ch を一括生成（各タスクは独立並列実行）
clock 48kHz capture[ch=0..CH] {
    adc(ch) | frame_pack()[FRAME] -> in[ch]
}

# 配列全体参照: in[*] を [CH, FRAME] として扱う
clock 48kHz beam {
    @in[*] | beamform()[CH, FRAME] | distribute()[CH, FRAME] -> out[*]
}

# 要素参照: 個別チャンネルの後段処理
clock 48kHz sink[ch=0..CH] {
    @out[ch] | stdout()
}
```

`@in[*]` / `-> out[*]` は 2 次元 shape `[CH, FRAME]` として接続される。先頭次元は配列（チャンネル）次元、後続は frame 次元である。

---

## 13. フレーム次元推論と多次元ベクトル化（v0.2 ドラフト）

本章は v0.2 の新規仕様ドラフトである。v0.1.0 との後方互換を維持しつつ、フレームベース処理向けに「次元（shape）」を導入する。

### 13.1 目的

- フレーム単位の処理を DSL 上で第一級に扱う
- アクターの入力/出力を多次元 shape で表現する
- shape はコンパイル時に推論し、必要時のみ明示制約する
- SDF の静的スケジューリング可能性（整数レート解）を維持する

### 13.2 基本モデル

#### 13.2.1 shape とトークンレート

- 各ポートは shape ベクトル `S = [d0, d1, ..., dk-1]` を持つ
- 各 `di` は正のコンパイル時整数
- ポートのトークンレートは `|S| = Π di`（要素数）で定義する
- 実行時バッファは従来通り 1 次元連続領域（フラット）であり、shape はコンパイル時メタデータとして扱う

#### 13.2.2 後方互換

- 既存の `IN(T, N)` / `OUT(T, N)` は rank-1 shape の省略記法とみなす
  - `IN(T, N)` ≡ `IN(T, SHAPE(N))`
  - `OUT(T, 1)` はスカラー要素数 1 を意味する
- v0.1.0 のプログラムは意味を変えずに受理される

#### 13.2.3 共有バッファ配列の shape リフト（v0.4.0）

共有バッファ配列 `name[CH]` に対する全要素参照 `name[*]` は、shape を次の規則で 2 次元化する。

- 各要素 `name[i]` のフレーム要素数を `F` とする
- `@name[*]` / `-> name[*]` の shape は `[CH, F]` とする
- `CH` は in/out 配列次元（チャンネル次元）、`F` は frame 次元を表す
- この 2 次元 shape はコンパイル時解析にのみ用いられ、実行時バッファは従来どおりフラット配置を維持する

### 13.3 アクター宣言拡張（C++ 側）

#### 13.3.1 shape 記法

ポート count に `SHAPE(...)` を許可する。

```cpp
ACTOR(frame_gain,
      IN(float, SHAPE(N)),
      OUT(float, SHAPE(N)),
      RUNTIME_PARAM(float, gain),
      PARAM(int, N)) {
    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;
    return ACTOR_OK;
}

ACTOR(img_norm,
      IN(float, SHAPE(H, W, C)),
      OUT(float, SHAPE(H, W, C)),
      PARAM(int, H) PARAM(int, W) PARAM(int, C)) {
    int n = H * W * C;
    for (int i = 0; i < n; ++i) out[i] = in[i];
    return ACTOR_OK;
}
```

#### 13.3.2 次元パラメータ

- shape 内で参照される識別子（例: `N`, `H`, `W`, `C`）は**次元パラメータ**である
- 次元パラメータは `PARAM(int, name)` で受け取る（`RUNTIME_PARAM` は不可）
- 次元パラメータはコンパイル時定数として確定しなければならない
- 次元パラメータは `ACTOR(...)` の `PARAM` 列の**末尾に連続して配置**することを推奨する
  - v0.2 系コンパイラはこの規約違反を warning で通知し、将来バージョンで error 化される可能性がある

#### 13.3.3 次元パラメータの推論（暗黙解決）

`SHAPE(...)` 内で参照される次元パラメータは、DSL 側で明示的に引数として渡す必要がない。コンパイラが呼び出し箇所の `[]` shape 制約または SDF バランス方程式から次元値を推論し、コード生成時にアクターへ自動的に渡す。

```cpp
// C++ 側: PARAM(int, N) を宣言するが、PDL 側で N を引数に書く必要はない
ACTOR(frame_gain,
      IN(float, SHAPE(N)),
      OUT(float, SHAPE(N)),
      RUNTIME_PARAM(float, gain),
      PARAM(int, N)) {
    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;
    return ACTOR_OK;
}
```

```pdl
# N=256 は shape 制約 [256] から推論され、frame_gain の PARAM(int, N) に自動的に渡される
# 引数には gain のみ指定すればよい
frame_gain($gain)[256]
```

**推論ルール:**

- `SHAPE(...)` 内の次元パラメータが DSL 呼び出しの引数リストに対応する位置で省略されている場合、コンパイラは `[]` shape 制約、上流/下流の SDF バランス式、またはパイプライン文脈から値を推論する
- 推論された値は codegen 時にアクターの `PARAM(int, name)` メンバに設定される
- 推論が不可能な場合はコンパイルエラーとなる（§13.6 参照）
- 明示引数と推論値の両方が存在する場合は一致が必須。不一致はコンパイルエラー

### 13.4 DSL 呼び出しでの制約記法

アクター呼び出しに shape 制約を付与できる。

```pdl
fft()[256]
img_norm()[1080, 1920, 3]
```

- `[]` は shape 制約（次元値リスト）を表す
- 制約値は整数リテラルまたは `const` 参照に限る
- 従来どおり引数で次元を渡してもよい（例: `fft(256)`）
- 引数と `[]` の両方で同じ次元を拘束した場合は一致が必須
- shape 制約のみで次元パラメータを解決する場合、アクターの `PARAM(int, name)` は DSL 引数リストから省略可能（§13.3.3 参照）

### 13.5 次元推論ルール

#### 13.5.1 未知数

コンパイラは以下を同時に未知数として扱う。

- 次元パラメータ（`N`, `H`, `W`, ...）
- 各ノードの repetition count（SDF の反復回数）

#### 13.5.2 制約生成

以下から整数制約式を構築する。

- ポート shape から得られるトークンレート式（`|S| = Π di`）
- 各エッジの SDF バランス式
  - `prod(src) * rep(src) = cons_edge(dst) * rep(dst)`
- 呼び出し引数による明示値
- `[]` shape 制約による明示値

複数入力アクターでは v0.1.0 と同様に `cons_edge = total_cons / fan_in` を用いる。割り切れない場合はコンパイルエラー。

#### 13.5.3 解の採択

- 解が一意に定まる場合: その値を採用
- 複数解がある場合: `[]` または明示引数を要求し、曖昧性エラー
- 解が存在しない場合: レート不整合エラー

#### 13.5.4 制約不足の扱い

次元が推論不能な場合（例: ソースアクターで入力がなく、出力次元も拘束されない）はエラー。

### 13.6 エラーセマンティクス（追加）

```text
error: unresolved frame dimension 'N' at actor 'fft'
  hint: add explicit shape constraint, e.g. fft()[256]
```

```text
error: conflicting frame constraints for actor 'img_norm'
  inferred shape: [1080, 1920, 3]
  explicit shape: [720, 1280, 3]
```

```text
error: runtime param '$n' cannot be used as frame dimension
  hint: use const or literal for shape constraints
```

### 13.7 例

```pdl
const frame = 256

clock 10MHz rx {
    # FIXME(v0.4.0): c2r() の後段は float なので mag() へは接続できない。
    # ここはサンプル改訂時に c2r()/mag() の並びを見直す。
    adc(0) | fft()[frame] | c2r() | mag() -> spectrum
}
```

```pdl
const H = 1080
const W = 1920
const C = 3

clock 60Hz vision {
    camera()[H, W, C] | img_norm() | encoder() -> bitstream
}
```

---

## 14. 外部プロセスインターフェース

### 14.1 概要

Pipit パイプラインと外部プロセス（オシロスコープ GUI、ロガー、テストハーネス等）間のリアルタイム信号データストリーミングには、以下の標準プロトコルを使用する。

- datagram transport: **PPKT (Pipit Packet Protocol)** — [ppkt-protocol-spec-v0.3.0.md](ppkt-protocol-spec-v0.3.0.md)
- shared-memory transport: **PSHM (Pipit Shared Memory Bind Protocol)** — [pshm-protocol-spec-v0.1.0.md](pshm-protocol-spec-v0.1.0.md)

設計原則:

- **プロセス分離**: GUI やロガーはパイプラインプロセスの外部で動作する。クラッシュ隔離・言語非依存
- **ノンブロッキング**: 送受信ともに `O_NONBLOCK`。SDF スケジュールを一切阻害しない
- **トランスポート非依存**: UDP / Unix domain datagram / Shared memory を同一 `bind` API で扱う
- **自己記述型パケット**: 各パケットが型・レート・タイムスタンプを含み、受信側に事前設定不要

### 14.2 標準アクター

#### `socket_write` — シンクアクター

```
ACTOR(socket_write, IN(float, N), OUT(void, 0),
      PARAM(std::span<const char>, addr) PARAM(int, chan_id) PARAM(int, N))
```

- 入力サンプルを PPKT パケットとして送信
- 初回ファイアリング時にソケットをオープン（遅延初期化）
- **ノンブロッキング**: `sendto()` が `EAGAIN` を返した場合はドロップして `ACTOR_OK`
- ソケット作成失敗のみ `ACTOR_ERROR`

```pdl
clock 48kHz audio {
    sine(1000, 1.0) | socket_write("localhost:9100", 0)
}
```

#### `socket_read` — ソースアクター

```
ACTOR(socket_read, IN(void, 0), OUT(float, N),
      PARAM(std::span<const char>, addr) PARAM(int, N))
```

- PPKT パケットを受信してサンプルを出力
- 初回ファイアリング時にソケットを bind（遅延初期化）
- **ノンブロッキング**: データ未到着時はゼロ出力で `ACTOR_OK`（SDF スケジュール維持）
- bind 失敗のみ `ACTOR_ERROR`

```pdl
clock 1kHz control {
    socket_read("localhost:9200") | stdout()
}
```

### 14.3 `bind` ベース接続（v0.4.0）

v0.4.0 では外部接続の記述として `bind` を推奨する。`socket_write` / `socket_read` は後方互換のため維持される。

```pdl
clock 48kHz audio {
    sine(1000, 1.0) -> wave
}
clock 1kHz control {
    @cmd | stdout()
}

bind wave = udp("127.0.0.1:9100", chan=0)   # out は推論
bind cmd  = udp("127.0.0.1:9200", chan=2)   # in は推論
```

ランタイムはこの `bind` 記述を基に外部入出力を構成する。SDF スケジュールとタスクの実行意味は変更してはならない（MUST NOT）。

### 14.4 `bind` + `shm` による複数 PDL プロセス通信

2つの独立した PDL プロセスが同一の `shm("<name>")` endpoint を共有することで、低レイテンシなローカル IPC を実現できる。

```pdl
# tx.pdl
clock 1MHz tx {
    source() -> iq
}
bind iq = shm("rx.iq", slots=1024, slot_bytes=4096)
```

```pdl
# rx.pdl
clock 1MHz rx {
    @iq | sink()
}
bind iq = shm("rx.iq", slots=1024, slot_bytes=4096)
```

この場合、writer/reader の契約（dtype/shape/rate）は一致しなければならない（MUST）。不一致時の挙動は protocol spec ではなく language semantics に従い、起動時または再配線時に拒否される（MUST）。

---

## 将来の拡張（v2+ 候補）

以下は v0.4.0 draft でも対象外とする将来候補である。

- 明示的なマルチコアピニング構文
- BDF (Boolean Dataflow) への拡張
- 分散実行（複数ノード間のパイプライン）
- GPU アクターのサポート
- プローブの可視化フロントエンド統合
- ホットリロード（グラフ構造変更を伴うライブ再構成）
- エラー回復戦略（タスク再起動、フォールバックモード）
- `control` ブロックから mode 内アクターへの直接データ供給

---

## 参考文献

1. E. A. Lee, D. G. Messerschmitt, "Static Scheduling of Synchronous Data Flow Programs for Digital Signal Processing," IEEE Trans. Computers, vol. C-36, no. 1, pp. 24–35, Jan. 1987.
2. E. A. Lee, D. G. Messerschmitt, "Synchronous Data Flow," Proceedings of the IEEE, vol. 75, no. 9, pp. 1235–1245, Sep. 1987.
3. G. Bilsen, M. Engels, R. Lauwereins, J. Peperstraete, "Cyclo-Static Dataflow," IEEE Trans. Signal Processing, vol. 44, no. 2, pp. 397–408, Feb. 1996.
4. Ptolemy Project, "SDF Domain," Ptolemy Classic Almagest Documentation.
5. C. Ptolemaeus (ed.), "Dataflow," System Design, Modeling, and Simulation using Ptolemy II, Ptolemy.org, 2014.
6. M. Geilen, T. Basten, S. Stuijk, "Minimising Buffer Requirements of Synchronous Dataflow Graphs with Model Checking," Proc. DAC, pp. 819–824, 2005.
