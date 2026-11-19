# アーキテクチャ・モジュール構成ドキュメント (`mcnp-input-parser`)

本ドキュメントは、`mcnp-input-parser` クレートの全体設計と、`src/` 配下の各ファイルの責務・主要な型・関数をまとめたリファレンスです。

関連ドキュメント:
- [VALIDATION_ERRORS.md](./VALIDATION_ERRORS.md) — 検出可能な構文・セマンティックエラー／警告コード一覧

---

## 1. 全体処理パイプライン

MCNP 入力テキストは、前処理から構文解析・検証・解析・LSP/CLI 出力まで、次の単方向データフローで処理されます。

```text
MCNP入力テキスト (&str)
  │
  ▼  preprocess.rs
PreprocessedInput (message / title / cell_cards / surface_cards / data_cards / 空行情報)
  │
  ▼  parser/*  (+ lexer.rs, shortcut.rs)
McnpInput (AST: ast.rs) + CardLineSpans (行番号マップ: lib.rs)
  │
  ├─▶ validate/*   ──▶ Vec<ValidationError> (セマンティック検証・クロスリファレンス)
  │
  └─▶ analysis/*   ──▶ ValidationReport / SymbolsReport / InspectReport / ResolvedCursorTarget
         │
         ├─▶ main.rs        (CLI: --validate, --symbols, --inspect, --template, --lsp 等)
         └─▶ lsp.rs         (LSPサーバ: diagnostics, hover, goto-definition, references, symbols, completion)
```

---

## 2. コア・共通基盤 (`src/*.rs`)

| ファイル | 役割 | 主な公開型・関数 |
| :--- | :--- | :--- |
| [`src/lib.rs`](../src/lib.rs) | クレートのエントリポイント。各モジュールの公開・再エクスポートと、トップレベルのパース関数を提供。 | `ParseError`, `CardLineSpans`, `LossyParseResult`, `parse_mcnp_input`, `parse_mcnp_input_lossy` |
| [`src/ast.rs`](../src/ast.rs) | MCNP 入力全体および Block 1〜3 の各カードを表す AST 型定義（`Serialize` / `Deserialize` 対応）。 | `McnpInput`, `Entry`, `Particle`, `CellCard`, `CellDefinition`, `GeometryExpr`, `CellParam`, `SurfaceCard`, `SurfaceKind`, `BoundaryCondition`, `SdefParam`, `SiCard`, `SpSbCard`, `DsCard`, `DoseCard`, `FmCard`, `FmeshCard`, `DataCard` |
| [`src/preprocess.rs`](../src/preprocess.rs) | 行指向前処理。空行による 3 ブロック分割、`c` コメント行・`$` インラインコメント除去、5 空白 / `&` 継続行の結合、余分な空行や第 3 空行以降の無視カードを検出。 | `LogicalCard`, `PreprocessedInput`, `is_comment_line`, `is_indented_continuation`, `strip_inline_comment`, `extract_logical_cards`, `preprocess_mcnp` |
| [`src/lexer.rs`](../src/lexer.rs) | `nom` ベースのトークン・数値プリミティブパーサ。`e`/`E` 省略の Fortran 科学記法（例: `1.5-3`）や粒子指定子 `:n,p` を解析。 | `ws0`, `ws1`, `sep0`, `sep1`, `is_token_boundary`, `mcnp_u32`, `mcnp_u64`, `mcnp_i32`, `mcnp_float`, `particle_designators`, `raw_token`, `identifier`, `kw` |
| [`src/shortcut.rs`](../src/shortcut.rs) | MCNP 入力ショートカット（`nR`/`R`, `nI`/`I`, `nILOG`/`ILOG`, `xM`, `nJ`/`J`）のパースと展開。 | `ShortcutItem`, `shortcut_item`, `expand_shortcut_items`, `parse_expanded_entries`, `parse_expanded_floats` |
| [`src/templates.rs`](../src/templates.rs) | CLI `--template` およびエージェント向けの最小構成スケルトン（`skeleton-sdef`, `skeleton-kcode`）とカード別スニペットを保持。 | `TemplateTopic`, `list_templates`, `get_template` |

---

## 3. 構文解析 (`src/parser/`)

Block 1〜3 の論理カード文字列を `nom` コンビネータで AST へ変換します。

| ファイル | 役割 | 主な関数 |
| :--- | :--- | :--- |
| [`src/parser/mod.rs`](../src/parser/mod.rs) | `parser` サブモジュール群の公開。 | `cell`, `data`, `source`, `surface`, `tally` |
| [`src/parser/cell.rs`](../src/parser/cell.rs) | Block 1（Cell カード）のパーサ。`#` 補集合・空白積・`:` 和の演算子優先順位を持つブールジオメトリ式、`LIKE n BUT`、マクロボディファセット（`-10.1`）、セルパラメータを解析。 | `parse_union_expr`, `parse_cell_card`, `parse_cell_card_complete` |
| [`src/parser/surface.rs`](../src/parser/surface.rs) | Block 2（Surface カード）のパーサ。`*`/`+` 境界条件、座標変換／周期境界 ID、平面・球・円柱・円錐・トーラス・マクロボディ・`Custom` ニーモニックを解析。 | `parse_surface_card`, `parse_surface_card_complete` |
| [`src/parser/data.rs`](../src/parser/data.rs) | Block 3（Data カード）のディスパッチャおよび材料・分散低減・物理・制御・臨界カードのパーサ。未対応カードは `DataCard::Generic` へフォールバック。 | `parse_data_card`, `parse_data_card_complete`（内部で `parse_material_body`, `parse_wwg_body`, `parse_ww_mesh_body`, `parse_cut_body`, `parse_kcode_body`, `parse_ksrc_body`, `parse_nps_body` 等を呼び出し） |
| [`src/parser/source.rs`](../src/parser/source.rs) | 固定線源・分布カード群のパーサ。 | `parse_sdef_body`, `parse_si_body`, `parse_sp_sb_body`, `parse_ds_body`, `parse_sc_body` |
| [`src/parser/tally.rs`](../src/parser/tally.rs) | 標準タリー・検出器タリー・線量換算・タリー修飾カード・メッシュタリーのパーサ。 | `parse_standard_tally_body`, `parse_detector_tally_body`, `parse_dose_body`, `parse_fm_body`, `parse_fs_body`, `parse_sd_body`, `parse_tally_energy_body`, `parse_tally_time_body`, `parse_tally_cosine_body`, `parse_fc_body`, `parse_fmesh_body` |

---

## 4. セマンティック検証 (`src/validate/`)

パース済み `McnpInput` と `CardLineSpans` を受け取り、6 つの検証フェーズを順に実行して `Vec<ValidationError>` を返します。共有状態は `ValidationContext` に集約されており、[`src/lib.rs`](../src/lib.rs) の公開 API は分割前と互換です。

| ファイル | 役割 | 主な型・関数 |
| :--- | :--- | :--- |
| [`src/validate/mod.rs`](../src/validate/mod.rs) | 検証全体のオーケストレータ。6 フェーズを順次実行し、単体テスト群（`validate::tests`）を保持。 | `validate_mcnp_input`, `ErrorCode`, `Severity`, `ValidationCategory`, `ValidationError`（再エクスポート） |
| [`src/validate/types.rs`](../src/validate/types.rs) | 診断レベル・カテゴリ・エラーコード（`E0xx`〜`W9xx`）および MCNP 予約キーワード定数を定義。 | `Severity`, `ValidationCategory`, `ErrorCode`, `ValidationError`, `MAX_MCNP_ID`, `KNOWN_MCNP_DATA_PREFIXES`, `KNOWN_MCNP_CELL_PARAMETERS` |
| [`src/validate/context.rs`](../src/validate/context.rs) | フェーズ間で共有するルックアップマップ・使用済みセット・診断結果と、行番号解決・粒子モード確認などの共通ヘルパを提供。 | `ValidationContext`, `CellFillRecord`, `cell_line`, `surf_line`, `data_line`, `check_particle_in_mode`, `is_strictly_increasing` |
| [`src/validate/structure.rs`](../src/validate/structure.rs) | **Phase 1 & 2**: 3 ブロック構造・余分な空行（`E001`〜`E004`）の検査、全カードの ID 範囲／重複検査、ルックアップマップおよび `MODE` 活性粒子セットの構築。 | `check_block_structure`, `build_lookup_maps` |
| [`src/validate/surface.rs`](../src/validate/surface.rs) | **Phase 3**: サーフェスの座標変換参照・周期境界制約の検査、各サーフェス／マクロボディ種別ごとのパラメータ数・幾何条件検証。 | `validate_surfaces`, `validate_surface_kind_and_params`, `macrobody_max_facets` |
| [`src/validate/cell.rs`](../src/validate/cell.rs) | **Phase 4**: セルの材料／密度整合（Void・非 Void・`LIKE n BUT`）、ジオメトリ参照（サーフェス・ファセット・`#n` 補集合）、セルパラメータ値、循環参照（`#n` / `U`-`FILL`）の DFS 検査。 | `validate_cells`, `validate_geometry_expr`, `check_complement_cell_cycles`, `extract_fill_universes`, `check_universe_cycles` |
| [`src/validate/data.rs`](../src/validate/data.rs) | **Phase 5**: 材料（ZAID・質量/原子分率符号混在）、熱中性子散乱 `MT`、`SDEF`、`SI`/`SP`/`SB`/`DS`、タリー群、`IMP`/`WW*`、`TR`/`NPS`/`KCODE`、`Generic` カードの単体検証。 | `validate_data_cards`, `is_valid_zaid` |
| [`src/validate/fmesh.rs`](../src/validate/fmesh.rs) | `FMESH` メッシュタリーの専用検証（番号末尾 `4`、単一粒子、未認識キーワード、空間・エネルギー・時間メッシュの単調性・区間数、`OUT`/`TYPE`/`TALLY`/`TR` 等）。 | `validate_fmesh` |
| [`src/validate/cross_ref.rs`](../src/validate/cross_ref.rs) | **Phase 6**: 複数カードに跨る整合検査（`SDEF`/`KCODE`/`SSR` の有無、3D ベクトル分布伝搬を含む `SI`/`SP`/`SB` ペアリングと長さ、`DE`/`DF` ペアリング、全セルの `IMP` カバレッジ、未使用 Surface/Material 警告）。 | `validate_cross_references` |

---

## 5. 静的解析・診断・LSP支援 (`src/analysis/`)

CLI（`--validate` / `--symbols` / `--inspect`）および LSP サーバの共通バックエンドを提供します。[`src/analysis/mod.rs`](../src/analysis/mod.rs) から全公開シンボルを再エクスポートしています。

| ファイル | 役割 | 主な型・関数 |
| :--- | :--- | :--- |
| [`src/analysis/mod.rs`](../src/analysis/mod.rs) | サブモジュールの公開・再エクスポート、AST 走査の内部共通ヘルパ、統合テスト（`analysis::tests`）を保持。 | `extract_fill_universes`, `collect_geometry_refs`（いずれも `pub(super)`） |
| [`src/analysis/diagnostics.rs`](../src/analysis/diagnostics.rs) | 構文エラー・セマンティック検証エラーを統合し、修正ヒント付きのレポート生成とテキスト整形を行う。余分な空行（`E004`）がある場合は連鎖誤検知を抑止して `E004` を最優先で返す。 | `DiagnosticItem`, `ValidationReport`, `hint_for_validation_error`, `analyze_diagnostics`, `format_validation_report` |
| [`src/analysis/symbols.rs`](../src/analysis/symbols.rs) | 全定義シンボル（Cell / Surface / Material / Universe / Transform / Tally / Distribution）の抽出、参照関係集計、衝突しない次の空き ID 提案（`NextAvailableIds`）の生成。 | `SymbolsReport`, `CellSymbol`, `SurfaceSymbol`, `MaterialSymbol`, `UniverseSymbol`, `TransformSymbol`, `TallySymbol`, `DistributionSymbol`, `NextAvailableIds`, `analyze_symbols`, `build_symbols_report`, `format_symbols_report` |
| [`src/analysis/inspect.rs`](../src/analysis/inspect.rs) | 入力全体のクロスリファレンス索引（`ReferenceIndex`）の構築と、特定ターゲット（例: `c1`, `s10`, `m1`, `u1`, `tr1`, `f4`, `d1`）の定義・参照箇所の検索・整形。 | `DefinitionLocation`, `ReferenceLocation`, `InspectReport`, `InspectTarget`, `parse_inspect_target`, `inspect_target`, `inspect_target_entity`, `format_inspect_report`, `collect_all_references` (`pub(super)`) |
| [`src/analysis/cursor.rs`](../src/analysis/cursor.rs) | エディタの 0-based 行・列位置からブロック種別とトークン文脈を解決し、対応する `InspectTarget`、MCNP サーフェス解説、キーワードドキュメントを返す。 | `McnpBlockKind`, `ResolvedCursorTarget`, `detect_block_at_line`, `resolve_symbol_at_position` |

---

## 6. CLI / LSP サーバ (`src/lsp.rs`, `src/main.rs`, `src/bin/mcnp-lsp.rs`)

| ファイル | 役割 | 主な公開型・関数 |
| :--- | :--- | :--- |
| [`src/lsp.rs`](../src/lsp.rs) | `tower-lsp` を用いた Language Server 実装。`analysis` および `templates` の結果を LSP の Diagnostics / Hover / Go-to-Definition / Find References / Document Symbols / Completion に変換し、stdio でサービスを提供する。 | `McnpLanguageServer`, `run_lsp_stdio`, `line_code_range`, `build_lsp_diagnostics`, `compute_hover`, `compute_definitions`, `compute_references`, `compute_document_symbols`, `compute_completions` |
| [`src/main.rs`](../src/main.rs) | CLI バイナリ（`mcnp-input-parser`）のエントリポイント。`--validate`, `--symbols`, `--inspect`, `--template`, `--summary`, `--json`, `--lsp` の各オプションを処理。 | `main` |
| [`src/bin/mcnp-lsp.rs`](../src/bin/mcnp-lsp.rs) | LSP 専用のスタンドアロンバイナリ（`mcnp-lsp`）。`mcnp_input_parser::lsp::run_lsp_stdio()` を起動する。 | `main` |
