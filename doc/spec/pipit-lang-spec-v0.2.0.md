# Pipit Language Specification v0.2.0 (Draft)

## 1. Overview

Pipit は、共有メモリ上に SDF (Synchronous Dataflow) セマンティクスでアクターを配置し、クロック駆動のリアルタイムデータパイプラインを簡潔に記述するためのドメイン固有言語である。

### 1.1 設計原則

- **SDFベース**: 各アクターの生産/消費トークン数が静的に決定され、コンパイル時にスケジュールとバッファサイズが確定する
- **CSDF拡張**: control subgraph によるモード切替で条件分岐を表現しつつ、各モード内の静的解析可能性を維持する
- **共有メモリ通信**: タスク間のデータ受け渡しは共有メモリ上のリングバッファを介して行う
- **クロック駆動**: 各タスクは明示的な target rate で駆動される
- **直感的な構文**: 最小限の特殊記号とキーワードにより、低い学習コストで記述可能
- **暗黙変換なし**: 型変換・レート変換はすべて明示的なアクター挿入を要求する

### 1.2 ターゲット環境

- 通常の OS (Linux / Windows)
- リアルタイム性の保証は CPU 性能に依存（ハードリアルタイム OS を前提としない）
- マルチコア配置はスケジューラに委任（明示的なコアピニングは行わない）

### 1.3 ツールチェイン

Pipit のソースファイル (`.pdl`) は専用コンパイラ `pcc` によって C++ コードに変換され、`libpipit` とリンクして実行形式を生成する。

```
source.pdl → pcc → source_gen.cpp → g++/clang++ → executable
```

`pcc` の CLI インターフェース、コンパイル処理フロー、エラー出力形式の詳細は現行仕様 [pcc-spec-v0.1.0](pcc-spec-v0.1.0.md) を参照。

### 1.4 用語定義

| 用語 | 定義 |
|------|------|
| **アクター** | SDF グラフ上のノード。固定の消費/生産トークン数を持つ処理単位 |
| **タスク** | 1つ以上のパイプラインを含む実行単位。独立したスレッドで駆動される |
| **イテレーション** | SDF スケジュールの1完全反復。グラフ内の全アクターが repetition vector で定まる回数だけ発火する |
| **ティック** | タスクのクロックタイマーが発生する1周期。1ティック = K イテレーション (K ≥ 1) |
| **イテレーション境界** | イテレーションの完了と次のイテレーション開始の間の論理的な時点 |
| **共有バッファ** | タスク間でデータを受け渡す非同期 FIFO。共有メモリプール上にリングバッファとして静的配置される |
| **タップ** | パイプライン内のフォークノード。データをコピーして複数の下流に分配する |
| **プローブ** | 非侵入的な観測点。リリースビルドではゼロコストで除去される |

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
| `#` | コメント | 行末コメント |

### 2.4 キーワード

以下の識別子は予約語であり、ユーザー定義の識別子として使用できない。

```
set  const  param  define  clock  mode  control  switch  default  delay
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

Pipit はパイプライン記述側に型注釈を持たない。型情報はアクター定義（C++ 側）の `ACTOR` マクロが生成する `constexpr` 登録関数から取得される。コンパイラ `pcc` はこの登録情報を参照し、パイプライン接続の型整合性を静的に検証する。

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

### 3.3 型推論規則

パイプ `A | B` において、アクター `A` の出力型とアクター `B` の入力型が一致しなければコンパイルエラーとなる。

```
error: type mismatch at pipe 'fft -> fir'
  fft outputs cfloat[256], but fir expects float[5]
  hint: insert a conversion actor (e.g. c2r)
```

### 3.4 型変換

暗黙の型変換は一切行わない。型が異なる場合、ユーザーは明示的に変換アクターを挿入しなければならない。

```
adc(0) | fft(256) | c2r() | fir(coeff) -> signal
#                   ^^^^^ cfloat → float 変換アクター
```

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

#### マクロの生成物

`ACTOR` マクロは以下を生成する。

1. **ファンクタクラス**: `IN`/`OUT` インターフェースを持つ `noexcept` な `operator()` を備えるクラス
2. **`constexpr` 登録関数**: アクター名、入力型、入力トークン数、出力型、出力トークン数、パラメータ型リストをコンパイル時定数として公開する関数

`pcc` は登録関数の情報のみを使用し、C++ ソースのパースは行わない。

#### ファンクタ内で使用可能なシンボル

| シンボル | 型 | 説明 |
|----------|-----|------|
| `in` | `const T[N]` | 入力バッファ（消費トークン数 N 個） |
| `out` | `T[M]` | 出力バッファ（生産トークン数 M 個） |
| パラメータ名 | 各型 | DSL 側から渡されるパラメータ |

### 4.2 パラメトリックアクター

DSL 側から引数を受け取るアクターは `PARAM` で宣言する。配列引数にはポインタではなく `std::span` を使用する。

```cpp
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    fft_exec(in, out, N);
}

ACTOR(fir, IN(float, N), OUT(float, 1),
      PARAM(int, N),
      PARAM(std::span<const float>, coeff)) {
    float y = 0;
    for (int i = 0; i < N; i++) y += coeff[i] * in[i];
    out[0] = y;
}
```

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
set scheduler = round_robin
set overrun = drop
```

`set` 文はスケジューラおよびランタイムの動作パラメータを設定する。

| キー | 型 | デフォルト | 説明 |
|------|----|-----------|------|
| `mem` | SIZE | `64MB` | 共有メモリプールの最大サイズ |
| `scheduler` | IDENT | `static` | スケジューリングアルゴリズム (`round_robin`, `static`) |
| `overrun` | IDENT | `drop` | オーバーラン時のポリシー（§5.4.3 参照） |

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
clock <freq> <name> { <task_body> }
```

- `clock <freq>`: タスクの target rate。省略不可
- `<name>`: タスク名。プログラム内で一意でなければならない
- `<task_body>`: パイプライン行、または mode/control 構成

#### 5.4.2 実行モデル

各タスクはスケジューラにより独立したスレッドとして実行される。`clock freq` はタスクの **target rate** であり、ランタイムはこの周波数に基づくタイマーで**ティック**を生成する。

1ティックあたり K イテレーションを実行する (K ≥ 1)。K の値は以下により決定される。

- デフォルトでは K = 1
- コンパイラが target rate と1イテレーションの推定処理時間から K > 1 が必要と判断した場合、K を自動調整してよい
- 高い target rate（例: 10MHz）の場合、コンパイラは複数イテレーションをバッチ化して1回のティックで実行するスケジュールを生成する

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

### 5.5 パイプ演算子

```
actor_a() | actor_b() | actor_c()
```

パイプ演算子 `|` はアクター間のデータフロー接続を表す。左辺の出力が右辺の入力に接続される。SDF グラフ上の有向エッジに対応する。

#### 名前解決規則

パイプライン内の裸の識別子（括弧なし）の解決は以下の規則に従う。

| 位置 | 構文 | 解釈 |
|------|------|------|
| 行頭 | `@name` | 共有バッファからの読出し |
| パイプ中 | `name(...)` | アクター呼出し（0引数でも括弧必須） |
| パイプ中 | `:name` | タップ（宣言または参照） |
| パイプ中 | `?name` | プローブ |
| パイプ末尾 | `-> name` | 共有バッファへの書込み |

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
```

`-> name` はパイプラインの末尾に置かれ、共有メモリバッファ `signal` へデータを書き込む。

#### 読出し

```
@signal | decimate(10000) | csvwrite("out.csv")
```

`@name` をパイプラインの先頭に置くことで、共有メモリバッファからデータを読み出す。`@` プレフィックスにより、共有バッファの読出しであることが構文上明示される。

#### 単一ライター制約

一つの共有メモリバッファに対して `->` を記述できるのは1タスクのみ。

```
clock 10MHz a { ... -> signal }
clock 10MHz b { ... -> signal }
# error: multiple writers to shared buffer 'signal'
```

#### 複数リーダー

```
clock 1kHz c { @signal | proc1() | ... }
clock 1kHz d { @signal | proc2() | ... }
# OK: 各リーダーは独立したリードポインタを持つ
```

同一クロックの複数リーダーは、同一イテレーション内で同じデータを観測する（スナップショット読み取り）。

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

- プローブデータの出力先は `--probe-output` で指定する（stderr, ファイルパス, またはネットワーク）

### 5.9 サブパイプライン定義

```
define demod(n) {
    fft(n) | eq() | demap()
}

clock 10MHz rx { adc(0) | demod(256) -> bits }
```

`define` はパイプラインの断片に名前を付けて再利用可能にする。アクターではなく、呼出し箇所にインライン展開される。SDF の階層グラフに対応する。

#### 制約

- 引数はコンパイル時定数のみ（`param` による実行時値は不可）
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

### 6.3 ctrl の供給規則

`switch(ctrl, ...)` の `ctrl` は以下のいずれかでなければならない。

1. **control subgraph 内で `-> ctrl` により書き出された共有バッファ**（タスク内通信）
2. **`param $ctrl`** によるランタイムパラメータ

外部からの制御（例: 外部タスクから共有バッファ経由）も許容されるが、ctrl の供給元が存在しない場合はコンパイルエラーとなる。

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

### 6.6 初期モードとデフォルト

ブロック内で最初に宣言されたモードが初期モードとなる。明示的に指定する場合は `default` キーワードを使う。

```
switch(ctrl, sync, data) default sync
```

### 6.7 制約

- `switch` 文はタスクブロック内に最大1つ
- `mode` ブロックを持つタスクには `control` ブロックまたは `param` による ctrl 供給が必須
- `mode` ブロックを持たないタスクに `switch` / `control` は記述できない
- ctrl の全有効値（0 〜 モード数-1）は網羅的でなければならない。ctrl がこの範囲外の値を取る場合の動作は未定義である（コンパイラは可能な範囲で警告を発する）

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
| SDF バランス不能 | バランス方程式に非負整数解が存在しない |
| メモリプール超過 | 算出バッファサイズの総計が `set mem` を超過 |
| 構文エラー | BNF に適合しないソース |

### 7.2 実行時エラー

アクターが `ACTOR_ERROR` を返した場合、以下の連鎖が発生する。

1. 当該アクターを含む**タスク全体を即座に停止**する
2. 停止したタスクに `->` で接続されている下流タスクは、データ枯渇により**タイムアウトで停止**する
3. 上流タスクはバックプレッシャーにより**ブロック → タイムアウトで停止**する
4. 結果としてエラーはグラフ全体に伝搬し、最終的にパイプライン全体が停止する

```
runtime error: actor 'safe_div' in task 'capture' returned ACTOR_ERROR
  task 'capture' stopped
  task 'drain' stopped (timeout: no data from 'signal' for 100ms)
pipit: pipeline terminated with error (exit code 1)
```

エラー情報は stderr に出力される。プローブが有効な場合はプローブにも出力される。

---

## 8. コンパイラ処理フロー

コンパイラ `pcc` の処理フロー（字句解析 → 構文解析 → アクター登録情報読込み → 名前解決 → SDF グラフ構築 → 静的解析 → スケジュール生成 → C++ コード生成 → C++ コンパイル）の詳細は現行仕様 [pcc-spec-v0.1.0](pcc-spec-v0.1.0.md) を参照。

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
| `--probe <name>` | 指定プローブを有効化（複数指定可） | 全無効 |
| `--probe-output <path>` | プローブ出力先 (`stderr`, ファイルパス) | `stderr` |
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

---

## 10. 形式文法 (BNF)

```
program         ::= (statement NL)*

statement       ::= set_stmt
                  | const_stmt
                  | param_stmt
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

# ── サブパイプライン定義 ──

define_stmt     ::= 'define' IDENT '(' params? ')' '{' pipeline_body '}'

# ── タスク定義 ──

task_stmt       ::= 'clock' FREQ IDENT '{' task_body '}'

task_body       ::= pipeline_body
                  | modal_body

modal_body      ::= control_block mode_block+ switch_stmt

control_block   ::= 'control' '{' pipeline_body '}'

mode_block      ::= 'mode' IDENT '{' pipeline_body '}'

switch_stmt     ::= 'switch' '(' switch_src ',' IDENT (',' IDENT)+ ')'
                     default_clause? NL

switch_src      ::= IDENT              # 共有バッファ名
                  | '$' IDENT           # ランタイムパラメータ

default_clause  ::= 'default' IDENT

# ── パイプライン ──

pipeline_body   ::= (pipeline_line NL)*

pipeline_line   ::= pipe_expr
                  | comment

pipe_expr       ::= pipe_source ('|' pipe_elem)* sink?

pipe_source     ::= '@' IDENT          # 共有バッファ読出し
                  | ':' IDENT           # タップ参照（消費側）
                  | actor_call          # アクター（ソースアクター）

pipe_elem       ::= actor_call
                  | ':' IDENT           # タップ（宣言側）
                  | '?' IDENT           # プローブ

actor_call      ::= IDENT '(' args? ')'

sink            ::= '->' IDENT         # 共有バッファ書込み

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
      PARAM(int, N),
      PARAM(std::span<const float>, coeff)) {
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
    switch(ctrl, sync, data) default sync
}

clock 1kHz logger {
    @payload | decimate(10000) | csvwrite("received.csv")
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

---

## 12. 参考文献

1. E. A. Lee, D. G. Messerschmitt, "Static Scheduling of Synchronous Data Flow Programs for Digital Signal Processing," IEEE Trans. Computers, vol. C-36, no. 1, pp. 24–35, Jan. 1987.
2. E. A. Lee, D. G. Messerschmitt, "Synchronous Data Flow," Proceedings of the IEEE, vol. 75, no. 9, pp. 1235–1245, Sep. 1987.
3. G. Bilsen, M. Engels, R. Lauwereins, J. Peperstraete, "Cyclo-Static Dataflow," IEEE Trans. Signal Processing, vol. 44, no. 2, pp. 397–408, Feb. 1996.
4. Ptolemy Project, "SDF Domain," Ptolemy Classic Almagest Documentation.
5. C. Ptolemaeus (ed.), "Dataflow," System Design, Modeling, and Simulation using Ptolemy II, Ptolemy.org, 2014.
6. M. Geilen, T. Basten, S. Stuijk, "Minimising Buffer Requirements of Synchronous Dataflow Graphs with Model Checking," Proc. DAC, pp. 819–824, 2005.

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

### 13.3 アクター宣言拡張（C++ 側）

#### 13.3.1 shape 記法

ポート count に `SHAPE(...)` を許可する。

```cpp
ACTOR(frame_gain,
      IN(float, SHAPE(N)),
      OUT(float, SHAPE(N)),
      PARAM(int, N),
      RUNTIME_PARAM(float, gain)) {
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

### 13.7 文法差分（v0.2）

本章は §10 の差分のみを示す。

```bnf
actor_call       ::= IDENT '(' args? ')' shape_constraint?
shape_constraint ::= '[' shape_dims ']'
shape_dims       ::= shape_dim (',' shape_dim)*
shape_dim        ::= NUMBER | IDENT   # IDENT は const 参照
```

### 13.8 例

```pdl
const frame = 256

clock 10MHz rx {
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

## 14. 将来の拡張（v2+ 候補）

以下は v0.2.0 draft でも対象外とする将来候補である。

- 明示的なマルチコアピニング構文
- BDF (Boolean Dataflow) への拡張
- 分散実行（複数ノード間のパイプライン）
- GPU アクターのサポート
- プローブの可視化フロントエンド統合
- ホットリロード（パイプライン再構成のライブ適用）
- エラー回復戦略（タスク再起動、フォールバックモード）
- `control` ブロックから mode 内アクターへの直接データ供給
