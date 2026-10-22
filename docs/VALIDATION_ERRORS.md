# MCNP 入力ファイル 検出エラー・警告一覧 (`mcnp-input-parser`)

本ドキュメントは、[`mcnp-input-parser`](../src/lib.rs) の構文解析（[`parse_mcnp_input`](../src/lib.rs) / [`parse_mcnp_input_lossy`](../src/lib.rs)）およびセマンティック検証（[`validate_mcnp_input`](../src/validate.rs)）によって検出されるエラーおよび警告の一覧をまとめたリファレンスです。

---

## 1. 使い方

```rust
use mcnp_input_parser::{parse_mcnp_input_lossy, validate_mcnp_input, Severity};

let res = parse_mcnp_input_lossy(mcnp_text);

// 1. カード単体の構文エラー (ParseError)
for syntax_err in &res.errors {
    eprintln!("[SYNTAX] {}", syntax_err);
}

// 2. AST 全体のセマンティック検証・クロスリファレンスエラー (ValidationError)
let diagnostics = validate_mcnp_input(&res.input, &res.spans);
for diag in &diagnostics {
    match diag.severity {
        Severity::Error => eprintln!("{}", diag),
        Severity::Warning => println!("{}", diag),
    }
}
```

CLI からは `--validate` オプションで実行できます：

```bash
cargo run -- --validate path/to/input.i
```

---

## 2. 構文解析エラー (`ParseError`)

[`parse_mcnp_input`](../src/lib.rs) および [`parse_mcnp_input_lossy`](../src/lib.rs) で検出されるカードレベルの構文エラーです。

| バリアント | 発生箇所 | 検出内容・具体例 |
| :--- | :--- | :--- |
| `ParseError::CellError` | Block 1 (Cell) | ・セル番号や材料番号が整数でない（例: `bad_id 1 -1.0 -10`）<br>・非 Void セル（`mat > 0`）で密度が欠落または数値でない<br>・ジオメトリ論理式の括弧 `(` `)` が閉じていない、または不正なトークンがある<br>・`LIKE n BUT` 構文に `BUT` または後続パラメータがない<br>・ショートカット展開エラー（先行値のない `R`/`M`、非正値に対する `ILOG` など） |
| `ParseError::SurfaceError` | Block 2 (Surface) | ・サーフェス番号が正の整数でない<br>・サーフェスニーモニックが欠落している<br>・パラメータに数値またはショートカット以外の文字列が含まれる（例: `20 px not_a_float`）<br>・サーフェスカード内で `J`（Jump デフォルト）が使用されている |
| `ParseError::DataError` | Block 3 (Data) | ・カードヘッダ（ニーモニック）が認識できない形式<br>・`KSRC` の座標数が 3 の倍数でない<br>・`F5` ポイント検出器のパラメータ数が 4 の倍数でない / リング検出器（`FX5`/`FY5`/`FZ5`）が 3 の倍数でない<br>・`WWG` の必須パラメータ（3個）不足、`NPS` のヒストリ数欠落<br>・データカード内のショートカット展開エラー（`R`, `I`, `ILOG`, `M` の前後関係不正） |

---

## 3. セマンティック検証エラー・警告一覧 (`ValidationError` / `ErrorCode`)

[`validate_mcnp_input(&input, &spans)`](../src/validate.rs) が返す診断結果（[`ValidationError`](../src/validate.rs)）の一覧です。

### 3.1 ブロック構造 (`ValidationCategory::Structure`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E001`** | `EmptyCellsBlock` | `Error` | Block 1（セルカード）が1枚も存在しない | 空ファイル、またはブロック区切り空行のミス |
| **`E002`** | `EmptySurfacesBlock` | `Error` | Block 2（サーフェスカード）が1枚も存在しない | セルブロックの後にサーフェスブロックがない |
| **`E003`** | `EmptyDataBlock` | `Error` | Block 3（データカード）が1枚も存在しない | サーフェスブロックの後にデータブロックがない |

---

### 3.2 ID 範囲・重複定義 (`ValidationCategory::IdRange` / `DuplicateId`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E101`** | `CellIdOutOfRange` | `Error` | セル ID が `1..=99,999,999` の範囲外 | `100000000 0 -1` |
| **`E102`** | `SurfaceIdOutOfRange` | `Error` | サーフェス ID が `1..=99,999,999` の範囲外 | `100000000 so 5.0` |
| **`E103`** | `DataCardIdOutOfRange` | `Error` | データカードの番号が MCNP の上限を超過 | `TRn` / `SIn` / `SPn` / `SBn` / `DSn` が `1..=999` の範囲外、または `Mm` / `Fn` が `99,999,999` 超過 |
| **`E104`** | `DuplicateCellId` | `Error` | 同一の Cell ID が複数回定義されている | 最初に定義された行番号と重複行番号の両方を報告 |
| **`E105`** | `DuplicateSurfaceId` | `Error` | 同一の Surface ID が複数回定義されている | 最初に定義された行番号と重複行番号の両方を報告 |
| **`E106`** | `DuplicateDataCard` | `Error` | 同一番号のデータカード、または単一であるべきカードが複数回定義されている | `M1`, `MT1`, `TR1`, `F4`, `FMESH4`, `SI1`, `SP1`, `SB1`, `DS1`, `SC1`, `DE4`, `DF4`, `FM4`, `FS4`, `SD4`, `E4`, `T4`, `C4`, `FC4` の重複、または複数の `MODE`, `SDEF`, `KCODE`, `KSRC`, `NPS` |

---

### 3.3 サーフェス・マクロボディ検証 (`ValidationCategory::Surface`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E201`** | `UnknownSurfaceMnemonic` | `Error` | 未知のサーフェス種別ニーモニック（`SurfaceKind::Custom`） | `10 sph_typo 0 0 0 5` |
| **`E202`** | `InvalidSurfaceParamCount` | `Error` | サーフェス種別に対してパラメータの個数が一致しない | ・`PX`/`PY`/`PZ`/`SO`/`CX`/`CY`/`CZ`: 1個<br>・`SX`/`SY`/`SZ`: 2個、`C/X`/`C/Y`/`C/Z`: 3個、`S`/`SPH`: 4個<br>・`KX`/`KY`/`KZ`: 2または3個、`K/X`/`K/Y`/`K/Z`: 4または5個<br>・`P`: 4または9個、`TX`/`TY`/`TZ`/`RPP`: 6個<br>・`RCC`/`ELL`: 7個、`TRC`: 8個、`RHP`/`HEX`: 9または15個<br>・`SQ`/`GQ`: 10個、`REC`: 10または12個、`BOX`/`WED`: 12個、`ARB`: 30個<br>・`X`/`Y`/`Z`: 2, 4, または 6個 |
| **`E203`** | `InvalidSurfaceGeometry` | `Error` | 幾何学的に不正なパラメータ値 | ・球・円柱・円錐台・トーラス等の半径 $R \le 0$、円錐の勾配二乗 $t^2 \le 0$<br>・`RPP` で $x_{\min} \ge x_{\max}$, $y_{\min} \ge y_{\max}$, $z_{\min} \ge z_{\max}$<br>・平面 `P` の法線ベクトル $(A, B, C) = (0, 0, 0)$<br>・`RCC`, `TRC`, `REC`, `RHP`, `HEX` の高さベクトル $\mathbf{h} = \mathbf{0}$<br>・`BOX`, `WED` の辺ベクトルがゼロベクトル<br>・`ELL` の長短軸パラメータ $R_m = 0$<br>・軸対称面 `X`/`Y`/`Z` の半径座標 $r < 0$ |
| **`E204`** | `InvalidPeriodicBoundary` | `Error` | 周期境界条件の指定が不正 | ・周期境界（`transform_id < 0`）と反射境界（`*`）/白色境界（`+`）を併用している<br>・平面（`P`, `PX`, `PY`, `PZ`）以外の曲面に周期境界を指定している |

---

### 3.4 クロスリファレンス・循環参照検証 (`ValidationCategory::Reference`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E301`** | `UndefinedSurface` | `Error` | 定義されていない Surface ID を参照している | ・セルのジオメトリ式（`-99`, `+99.1` 等）<br>・周期境界の相手サーフェス<br>・面タリー（`F1`, `F2`）、セグメント（`FSn`）、線源（`SDEF SUR=n`） |
| **`E302`** | `InvalidMacrobodyFacet` | `Error` | マクロボディのファセット参照（`±s.f`）が不正 | ・通常サーフェス（`SO`, `PX` 等）に対する `.f` 指定<br>・マクロボディの面数上限超過（`RPP`/`BOX`/`ARB`: `1..=6`, `RCC`/`REC`/`TRC`: `1..=3`, `SPH`/`ELL`: `1`, `RHP`/`HEX`: `1..=8`, `WED`: `1..=5`） |
| **`E303`** | `UndefinedCell` | `Error` | 定義されていない Cell ID を参照している | ・セル補集合 `#n`<br>・`LIKE n BUT` の基底セル `n`<br>・セルタリー（`F4`, `F6`, `F7`, `F8`）、線源（`SDEF CEL=n`）、Weight Window Generator（`WWG`） |
| **`E304`** | `CircularCellReference` | `Error` | セル参照が自己参照または循環参照している | ・`#1 -> #2 -> #1` のような補集合のサイクル<br>・`LIKE n BUT` の自己参照、または `LIKE` セルをさらに `LIKE` している多段参照 |
| **`E305`** | `UndefinedMaterial` | `Error` | 定義されていない材料番号 `Mm` を参照している | ・セルの `material_id > 0` または `MAT=m`<br>・熱散乱カード `MTm`<br>・タリー乗数カード `FMn` 内の材料番号 |
| **`E306`** | `UndefinedTransform` | `Error` | 定義されていない座標変換カード `TRn` を参照している | ・サーフェスの `transform_id > 0`<br>・セルの `TRCL=n`（変換ID指定時）<br>・線源の `SDEF TR=n` |
| **`E307`** | `UndefinedUniverse` | `Error` | `FILL` で指定された Universe ID `u` に属するセル（`U=u`）が存在しない | `1 0 -1 fill=10` だが `u=10` のセルが未定義 |
| **`E308`** | `CircularUniverseFill` | `Error` | Universe と `FILL` の階層構造が循環している | `U=1` のセルが `FILL=2` を持ち、`U=2` のセルが `FILL=1` を持つ |
| **`W309`** | `UnusedSurface` | `Warning` | 定義されているがどこからも参照されていないサーフェス | セル、タリー、`SDEF`、周期境界のいずれからも未使用 |
| **`W310`** | `UnusedMaterial` | `Warning` | 定義されているがどこからも参照されていない材料 `Mm` | セルおよび `FMn` カードのいずれからも未使用 |

---

### 3.5 セル定義・パラメータ検証 (`ValidationCategory::Cell`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E401`** | `ZeroDensityNonVoidCell` | `Error` | 非 Void セル（`material_id > 0`）の密度が `0.0` | `1 1 0.0 -10` または `RHO=0` |
| **`E402`** | `DuplicateCellParameter` | `Error` | 同一セル内で同じパラメータが重複指定されている | `1 0 -10 vol=5.0 vol=10.0` や同一粒子の `imp:n=1 imp:n=2` |
| **`E403`** | `InvalidCellParameterValue` | `Error` | セルパラメータの値が制約外 | ・`LAT` が `1` または `2` 以外<br>・`VOL <= 0`、`TMP <= 0`、`IMP < 0`<br>・インライン `TRCL=(...)` の要素数が `1, 3, 5, 6, 9, 12, 13` 以外 |
| **`E404`** | `MissingCellImportance` | `Error` | 輸送対象粒子に対するセルの Importance (`IMP`) が欠落、または全セルで `IMP=0` | 一部のセルに `IMP:n` の指定漏れがある、または全セルが `IMP:n=0` |
| **`E405`** | `ImportanceCardLengthMismatch` | `Error` | Block 3 の `IMP` または `WWNn` カードの要素数が全セル数と一致しない | セルが 5 個なのに `imp:n 1 1 0`（3個）しかない |

---

### 3.6 線源定義検証 (`ValidationCategory::Source`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E501`** | `MissingSourceDefinition` | `Error` | 線源定義カード（`SDEF`, `KCODE`, `SSR`）が1つも存在しない | 固定線源も臨界線源も未定義 |
| **`E502`** | `ConflictingSourceDefinitions` | `Error` | `SDEF`（固定線源）と `KCODE`（臨界計算）が同時に指定されている | 排他的な線源モードの競合 |
| **`W503`** | `MissingKsrcForKcode` | `Warning` | `KCODE` が指定されているが初期線源点 `KSRC` が存在しない | `SRCTP` ファイルを使用しない場合はエラーになるため警告 |
| **`E504`** | `UndefinedSourceDistribution` | `Error` | `SDEF`, `SIn`, `DSn` から未定義の分布番号 `Dn` を参照している | `sdef erg=d5` だが `SI5`/`SP5`/`DS5` が未定義 |
| **`E505`** | `IncompleteSourceDistribution` | `Error` | `SIn`, `SPn`, `SBn` のペアが欠けている | ・`SIn` があるが `SPn` がない<br>・関数でない `SPn` があるが `SIn` がない<br>・`SBn` があるが `SPn` がない |
| **`E506`** | `SourceDistributionLengthMismatch` | `Error` | `SIn` と `SPn` / `SBn` の要素数が一致しない、またはヒストグラム先頭確率が `0.0` でない | ・`SI1 H` が 4 要素なのに `SP1` が 3 要素<br>・`SI1 H` に対する `SP1 D` の第1要素が `0.0` でない |
| **`E507`** | `NonMonotonicSourceBins` | `Error` | `SIn`（`H` または `A` オプション）のビン境界が狭義単調増加していない | `si1 h 0.0 5.0 2.0 10.0` |
| **`E508`** | `InvalidSourceProbability` | `Error` | `SPn` / `SBn` の確率値に負数が含まれる、または正の確率が1つもない | `sp1 d 0.0 -0.5 0.5` または全要素 `0.0` |

---

### 3.7 タリー・メッシュタリー検証 (`ValidationCategory::Tally`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E601`** | `OrphanTallyModifierCard` | `Error` | 修飾カード（`En`, `Tn`, `Cn`, `FMn`, `FSn`, `SDn`, `DEn`, `DFn`, `FCn`）や `WWG` が未定義のタリー `Fn` を参照している | `f4:n` しか定義していないのに `e14` や `de14` がある（`n=0` のグローバル既定は除く） |
| **`E602`** | `TallyTargetKindMismatch` | `Error` | タリー番号の末尾桁が有効なタリー種別（`1, 2, 4, 5, 6, 7, 8`）でない | `f3:n` や `f9:n` |
| **`E603`** | `IncompleteDoseCards` | `Error` | `DEn` と `DFn` の片方が欠けている、または要素数が一致しない | `de4` が 5 要素なのに `df4` が 4 要素（`DFn` で `IC=` 標準関数を使う場合を除く） |
| **`E604`** | `NonMonotonicTallyBins` | `Error` | `En`, `Tn`, `Cn`, `DEn` のビン境界が狭義単調増加していない | `e4 10.0 1.0 20.0` |
| **`E605`** | `InvalidCosineBinRange` | `Error` | 角度ビン `Cn` の余弦値が $[-1.0, 1.0]$ の範囲外 | `c1 -1.5 0.0 1.0` |
| **`E606`** | `InvalidDetectorRadius` | `Error` | `F5` ポイント/リング検出器の排除球半径 $R_0 < 0$ | `f5:n 0 0 10 -0.5` |
| **`E607`** | `InvalidFmeshConfiguration` | `Error` | `FMESH` の設定不備 | ・`IMESH`/`IINTS`（および `J`, `K`, `E`）の要素数が対応していない<br>・メッシュ座標が単調増加していない<br>・分割数（`IINTS`, `JINTS`, `KINTS`）に `0` が含まれる |

---

### 3.8 材料・粒子モード・その他データカード検証 (`Material` / `Mode` / `DataCard`)

| コード | `ErrorCode` バリアント | 重要度 | 検出条件 | 具体例 / 備考 |
| :--- | :--- | :---: | :--- | :--- |
| **`E701`** | `EmptyMaterialCard` | `Error` | `Mm` に核種が1つもない、または `MTm` にテーブル指定がない | `m1 gas=0`（核種リストなし） |
| **`E702`** | `MixedMaterialFractionSigns` | `Error` | 同一の `Mm` カード内で原子分率（正）と重量分率（負）が混在している、または分率が `0.0` | `m1 1001.80c 0.5 8016.80c -0.5` |
| **`E703`** | `InvalidZaidFormat` | `Error` | 核種の ZAID 文字列が数字（`ZZZAAA[.nnX]`）で始まっていない | `m1 h1.80c 1.0` |
| **`E801`** | `EmptyModeCard` | `Error` | `MODE` カードに粒子識別子が1つも指定されていない | `mode` |
| **`W802`** | `ParticleNotInMode` | `Warning` | 各カードの `:p` で指定された粒子が `MODE` カードに含まれていない | `mode n` なのに `f4:p` や `imp:p` が指定されている |
| **`E901`** | `InvalidTransformCardLength` | `Error` | `TRn` カードの要素数が `3, 5, 6, 9, 12, 13` のいずれでもない | `tr1 0 0 10 1`（4要素） |
| **`E902`** | `InvalidNpsCount` | `Error` | `NPS` のヒストリ数が `0` | `nps 0` |
| **`E903`** | `InvalidKcodeParameters` | `Error` | `KCODE` のサイクルあたり粒子数 `nsrck == 0`、またはスキップサイクル数 `ikz >= kct` | `kcode 1000 1.0 50 50`（有効サイクル数が 0 以下） |
| **`W904`** | `UnrecognizedDataCard` | `Warning` | MCNP の標準データカード一覧にない未知のカード名（タイポの可能性） | `mde n` や `sdeff pos=0 0 0` |
