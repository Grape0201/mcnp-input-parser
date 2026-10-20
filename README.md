# mcnp-input-parser

Rust のパーサコンビネータライブラリ [`nom`](https://github.com/rust-bakery/nom) を用いて構築された、**MCNP (Monte Carlo N-Particle)** 入力ファイル用の型安全なパーサライブラリおよび CLI ツールです。

臨界計算体系から、固定線源・遮蔽計算（Fixed-Source / Shielding）で多用される各種線源定義・分布カード、タリー・線量換算、Weight Window 等の分散低減カードまで幅広く対応しています。

---

## 特長

- **多段パース構造**:
  - 行指向ルール（空行による 3 ブロック分割、`c` コメント行、`$` インラインコメント除去、5 空白または `&` による継続行結合）を前処理で論理カード化。
  - 各ブロック内のカードおよび数式・トークンを `nom` で型安全な AST にパース。
- **MCNP 特有の構文・ショートカット対応**:
  - `e` / `E` を省略した Fortran 科学記法（`1.5-3` → `1.5e-3`, `-2.+4` → `-2.0e4` など）。
  - 入力ショートカット（`nR`/`R` 繰り返し、`nI`/`I` 線形補間、`nILOG`/`ILOG` 対数補間、`xM` 乗算、`nJ`/`J` ジャンプ）のパースと展開。
- **柔軟なブール代数ジオメトリ式**:
  - セルジオメトリの演算子優先順位（単項 `#` 補集合 > 空白区切りの積集合 > `:` 区切りの和集合）やカッコのネスト、マクロボディファセット（`-10.1` など）に対応。
- **Serde による JSON 出力**:
  - すべての AST 構造体が `Serialize` / `Deserialize` を実装しており、パース結果を容易に JSON や他形式へエクスポート可能。
- **未対応カードのフォールバック**:
  - 専用の型が定義されていないカードも `DataCard::Generic` としてトークンを保持するため、パースエラーにならずにロバストに読み込めます。

---

## インストール・ビルド

Rust ツールチェーン（1.70 以上推奨）がインストールされている環境でビルドできます。

```bash
cargo build --release
```

テストの実行:

```bash
cargo test
```

---

## CLI の使い方

引数でファイルパスを指定するか、標準入力から MCNP 入力ファイルを渡すことができます。

```bash
# JSON 形式で AST を出力
cargo run -- /path/to/mcnp_input.inp

# 標準入力からパイプ
cat /path/to/mcnp_input.inp | cargo run

# 各ブロックのカード数サマリーのみを表示
cargo run -- --summary /path/to/mcnp_input.inp
```

---

## Rust API での使い方

```rust
use mcnp_input_parser::parse_mcnp_input;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_text = std::fs::read_to_string("input.inp")?;
    
    // MCNP 入力ファイルをパース
    let ast = parse_mcnp_input(&input_text)?;
    
    println!("Title: {}", ast.title);
    println!("Cells: {}", ast.cells.len());
    println!("Surfaces: {}", ast.surfaces.len());
    println!("Data cards: {}", ast.data_cards.len());
    
    // セルカードの探索
    for cell in &ast.cells {
        println!("Cell {}: {:?}", cell.id, cell.definition);
    }
    
    // JSON にシリアライズ
    let json = serde_json::to_string_pretty(&ast)?;
    println!("{}", json);
    
    Ok(())
}
```

---

## 対応カード一覧

### 前処理・基本構文

| 機能 | 内容 |
| :--- | :--- |
| **Message Block** | 先頭の `MESSAGE:` から最初の空行まで |
| **Title Card** | 1 行目のタイトル文字列 |
| **3 ブロック構造** | 空行による `Cell Cards` / `Surface Cards` / `Data Cards` の分割 |
| **コメント** | 行頭 1〜5 カラム目の `c` / `C` コメント行、および `$` 以降のインラインコメント除去 |
| **継続行結合** | 行頭 5 文字以上のインデント、または行末 `&` による論理カード結合 |
| **数値記法** | 通常の浮動小数点・指数表記に加え、`e` なしの表記（`1.5-3`, `-2.+4`, `.5-2`） |
| **ショートカット** | `nR`/`R` (リピート), `nI`/`I` (線形補間), `nILOG`/`ILOG` (対数補間), `xM` (乗算), `nJ`/`J` (ジャンプ) の展開 |

---

### Block 1: Cell Cards

| 構文・カード | 内容・対応パラメータ |
| :--- | :--- |
| **通常セル定義** | `セルID` + `材料ID` (`0` の場合はボイド) + `密度` (負: 質量密度, 正: 原子数密度) + `ジオメトリ論理式` |
| **`LIKE n BUT`** | 他セル継承構文 (`LIKE n BUT MAT=... RHO=...`) |
| **ジオメトリ論理式** | ・符号付きサーフェス (`-10`, `+10`)<br>・マクロボディファセット (`-10.1`)<br>・セル補集合 (`#20`)<br>・複合補集合 (`#(10 -20)`)<br>・暗黙の積集合 (空白区切り)<br>・和集合 (`:` 区切り)<br>・カッコ `( ... )` ネスト |
| **セルパラメータ** | `IMP:<p>=<val>` (重要度), `VOL=<val>` (体積), `U=<val>` (ユニバース), `LAT=<val>` (格子), `FILL=[*]<val>` (充填), `TMP=<vals>` (温度), `RHO=<val>`, `MAT=<val>`, `TRCL=[*](...)` (セル座標変換), その他任意パラメータ (`CellParam::Other`) |

---

### Block 2: Surface Cards

| 境界・曲面種別 | ニーモニック |
| :--- | :--- |
| **境界条件** | `*` (反射境界), `+` (ホワイト境界), なし (透過境界) |
| **座標変換** | オプションの座標変換番号 / 周期境界サーフェス番号 (`transform_id`) |
| **平面 (Planes)** | `P`, `PX`, `PY`, `PZ` |
| **球面 (Spheres)** | `SO`, `S`, `SX`, `SY`, `SZ` |
| **円柱面 (Cylinders)** | `C/X`, `C/Y`, `C/Z`, `CX`, `CY`, `CZ` |
| **円錐面 (Cones)** | `K/X`, `K/Y`, `K/Z`, `KX`, `KY`, `KZ` |
| **2次曲面・トーラス** | `SQ`, `GQ`, `TX`, `TY`, `TZ` |
| **軸対称点指定曲面** | `X`, `Y`, `Z` |
| **マクロボディ** | `BOX`, `RPP`, `SPH`, `RCC`, `RHP`, `HEX`, `REC`, `TRC`, `ELL`, `WED`, `ARB` |
| **未定義・カスタム** | `SurfaceKind::Custom(String)` |

---

### Block 3: Data Cards

#### 1. 材料・輸送モード
| カード | 説明 |
| :--- | :--- |
| **`m<n>`** | 材料カード (核種 ZAID・比率ペアリスト、`gas=`, `nlib=` 等のキーワードオプション) |
| **`mt<n>`** | 熱中性子散乱カード ($S(\alpha, \beta)$ テーブル識別子リスト) |
| **`mode`** | 粒子輸送モード (例: `mode n p e`) |

#### 2. 線源定義・分布（遮蔽・固定線源）
| カード | 説明 |
| :--- | :--- |
| **`sdef`** | 一般線源定義カード<br>・対応変数: `pos`, `axs`, `vec`, `rad`, `ext`, `erg`, `dir`, `tme`, `par`, `cel`, `sur`, `wgt`, `tr`, `nrm`, `eff`, `x`, `y`, `z`, `ccc`, `ara`, `dat`, `loc`, `bem`, `bap`<br>・対応値形式: スカラー, 3次元ベクトル (`POS=0 0 0`), 分布参照 (`ERG=D1`), 従属変数関数 (`ERG=FPOS D2`), リスト, シンボル/セルパス |
| **`si<n>`** | 線源情報カード (オプション: `H` [ヒストグラム], `L` [離散], `A` [PDF], `S` [分布番号リスト]) |
| **`sp<n>` / `sb<n>`** | 線源確率 / バイアスカード<br>・離散/累積/体積確率配列 (`D`, `C`, `V` オプション)<br>・組み込み解析関数 (`-2` Maxwell, `-3` Watt, `-21` べき乗則 $r^a$, `-31`, `-41` 等) |
| **`ds<n>`** | 従属線源分布カード (`H`, `L`, `S`, `T` [値ペア], `Q` [区間ペア] オプション) |
| **`sc<n>`** | 線源コメントカード |

#### 3. タリー・線量換算・メッシュタリー
| カード | 説明 |
| :--- | :--- |
| **`f<n>:<p>`** | 標準タリー (番号末尾から種別を自動判定: `F1` 電流, `F2` 面フラックス, `F4` セルフラックス, `F6` エネルギー付与, `F7` 核分裂エネルギー付与, `F8` パルス波高)<br>・単一領域, 合算グループ `(1 2 3)`, ラティス式, トータルビン `T`, `ND` |
| **`f5<n>:<p>`** | 点検出器タリー (`(x, y, z, r0)` のリスト、`ND` オプション) |
| **`fx5` / `fy5` / `fz5`** | リング検出器タリー (`(axis_coord, radius, r0)` のリスト、`ND` オプション) |
| **`de<n>` / `df<n>`** | 線量換算エネルギー / 換算係数カード (`LOG` / `LIN` 補間, `IC=99` / `IU=2` / `FAC=-3` 等のオプション) |
| **`fm<n>`** | タリー乗数カード (定数乗数, `(C m (rx1 rx2) (attenuator))` ネスト構造) |
| **`fs<n>`** | タリーセグメントカード (サーフェス番号リスト, `T`) |
| **`sd<n>`** | セグメント除数カード (体積・面積・質量リスト) |
| **`e<n>`** | エネルギービンカード (`NT`, `C`) |
| **`t<n>`** | 時間ビンカード (`NT`, `C`) |
| **`c<n>`** | コサイン・角度ビンカード (`T`, `C`) |
| **`fc<n>`** | タリーコメントカード |
| **`fmesh<n>:<p>`** | 重畳メッシュタリーカード (`geom`, `origin`, `axs`, `vec`, `imesh`/`iints`, `jmesh`/`jints`, `kmesh`/`kints`, `emesh`/`eints`, `tmesh`/`tints`, `factor`, `out` 等) |

#### 4. 分散低減（Weight Window / 重要度）
| カード | 説明 |
| :--- | :--- |
| **`imp:<p>`** | セル重要度カード (粒子別、ショートカット展開対応) |
| **`wwe:<p>`** | Weight Window エネルギー・時間境界 |
| **`wwn<g>:<p>`** | Weight Window セル下限値リスト (グループ番号指定, `-1` 殺処分, `0` スキップ等) |
| **`wwp:<p>`** | Weight Window パラメータ (`wupn`, `wsurvn`, `mxspln`, `mwhere`, `switchn`, `mtime`, `vn`, `etsplit`, `wu`) |
| **`wwg`** | Weight Window ジェネレータカード (`itally`, `icell`, `wlower`) |
| **`wwge:<p>`** | Weight Window ジェネレータ エネルギー境界 |
| **`mesh`** | Weight Window 重畳メッシュ定義カード |

#### 5. 物理・カットオフ・制御・臨界計算
| カード | 説明 |
| :--- | :--- |
| **`phys:<p>`** | 物理モデルパラメータカード |
| **`cut:<p>`** | カットオフカード (時間 `tco`, エネルギー `elpt`, 重み `wc1`, `wc2`, `swtm`) |
| **`tr<n>` / `*tr<n>`** | 座標変換カード (角度単位 `*` 対応) |
| **`nps`** | 計算ヒストリー数 |
| **`kcode`** | 臨界計算カード (`nsrck`, `rkk`, `ikz`, `kct`, `msrk`, `knrm`, `mrkp`, `kc8`) |
| **`ksrc`** | 初期核分裂線源座標リスト (3 つ組 $(x, y, z)$) |
| **`Generic`** | その他未定義カード (カード名・粒子指定子・トークン列を保持) |
