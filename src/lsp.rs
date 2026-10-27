use crate::analysis::{
    analyze_diagnostics, analyze_symbols, detect_block_at_line, inspect_target_entity,
    resolve_symbol_at_position, McnpBlockKind, ResolvedCursorTarget,
};
use crate::preprocess::strip_inline_comment;
use crate::templates::list_templates;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// Computes a 0-based LSP [`Range`] covering the non-comment, non-whitespace code span of a 1-based line.
pub fn line_code_range(content: &str, line_1based: Option<usize>) -> Range {
    let lines: Vec<&str> = content.lines().collect();
    let idx = line_1based.unwrap_or(1).saturating_sub(1);
    if let Some(&raw_line) = lines.get(idx) {
        let without_comment = strip_inline_comment(raw_line).trim_end();
        let total_chars = without_comment.chars().count() as u32;
        let leading_ws = without_comment
            .chars()
            .take_while(|c| c.is_whitespace())
            .count() as u32;
        let start_col = if leading_ws < total_chars {
            leading_ws
        } else {
            0
        };
        let end_col = if total_chars > start_col {
            total_chars
        } else {
            raw_line.chars().count() as u32
        };
        let line_u32 = idx as u32;
        Range {
            start: Position::new(line_u32, start_col),
            end: Position::new(line_u32, end_col),
        }
    } else {
        Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        }
    }
}

/// Converts MCNP validation diagnostics for `content` into LSP [`Diagnostic`] items.
pub fn build_lsp_diagnostics(content: &str) -> Vec<Diagnostic> {
    let report = analyze_diagnostics(content);
    report
        .diagnostics
        .into_iter()
        .map(|item| {
            let range = line_code_range(content, item.line_number);
            let severity = match item.severity.as_str() {
                "Warning" => DiagnosticSeverity::WARNING,
                _ => DiagnosticSeverity::ERROR,
            };
            Diagnostic {
                range,
                severity: Some(severity),
                code: Some(NumberOrString::String(item.code)),
                code_description: None,
                source: Some("mcnp".to_string()),
                message: format!("{}\nHint: {}", item.message, item.hint),
                related_information: None,
                tags: None,
                data: None,
            }
        })
        .collect()
}

/// Builds an LSP [`Hover`] response for a cursor position in `content`.
pub fn compute_hover(content: &str, pos: Position) -> Option<Hover> {
    let resolved = resolve_symbol_at_position(content, pos.line, pos.character)?;
    let markdown = match resolved {
        ResolvedCursorTarget::Entity { target, token } => {
            let rep = inspect_target_entity(content, target, &token);
            let mut md = format!("### {}\n\n", rep.canonical_target);
            if rep.definitions.is_empty() {
                md.push_str("_Not defined in this input file._\n\n");
            } else {
                md.push_str("**Definitions:**\n");
                for def in &rep.definitions {
                    let loc = def
                        .line_number
                        .map(|l| format!("Line {}", l))
                        .unwrap_or_else(|| "Line ?".to_string());
                    md.push_str(&format!("- **[{}]**: `{}`\n", loc, def.summary));
                }
                md.push('\n');
            }

            if rep.references.is_empty() {
                md.push_str("**References:** _None_\n");
            } else {
                md.push_str(&format!("**References ({}):**\n", rep.references.len()));
                for r in rep.references.iter().take(15) {
                    let loc = r
                        .line_number
                        .map(|l| format!("Line {}", l))
                        .unwrap_or_else(|| "Line ?".to_string());
                    md.push_str(&format!(
                        "- **[{}]** `{}` — {}\n",
                        loc, r.source_card, r.context
                    ));
                }
                if rep.references.len() > 15 {
                    md.push_str(&format!(
                        "- _...and {} more reference(s)_\n",
                        rep.references.len() - 15
                    ));
                }
            }
            md
        }
        ResolvedCursorTarget::Keyword {
            summary,
            documentation,
            ..
        } => {
            format!("### {}\n\n{}", summary, documentation)
        }
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: None,
    })
}

/// Computes Go-to-Definition locations for a cursor position in `content`.
pub fn compute_definitions(uri: &Url, content: &str, pos: Position) -> Option<GotoDefinitionResponse> {
    let resolved = resolve_symbol_at_position(content, pos.line, pos.character)?;
    match resolved {
        ResolvedCursorTarget::Entity { target, token } => {
            let rep = inspect_target_entity(content, target, &token);
            let locations: Vec<Location> = rep
                .definitions
                .into_iter()
                .filter_map(|def| {
                    def.line_number.map(|line_1based| Location {
                        uri: uri.clone(),
                        range: line_code_range(content, Some(line_1based)),
                    })
                })
                .collect();
            if locations.is_empty() {
                None
            } else if locations.len() == 1 {
                Some(GotoDefinitionResponse::Scalar(
                    locations.into_iter().next().unwrap(),
                ))
            } else {
                Some(GotoDefinitionResponse::Array(locations))
            }
        }
        ResolvedCursorTarget::Keyword { .. } => None,
    }
}

/// Computes Find-References locations for a cursor position in `content`.
pub fn compute_references(
    uri: &Url,
    content: &str,
    pos: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let resolved = resolve_symbol_at_position(content, pos.line, pos.character)?;
    match resolved {
        ResolvedCursorTarget::Entity { target, token } => {
            let rep = inspect_target_entity(content, target, &token);
            let mut locations = Vec::new();
            if include_declaration {
                for def in &rep.definitions {
                    if let Some(l) = def.line_number {
                        locations.push(Location {
                            uri: uri.clone(),
                            range: line_code_range(content, Some(l)),
                        });
                    }
                }
            }
            for r in &rep.references {
                if let Some(l) = r.line_number {
                    locations.push(Location {
                        uri: uri.clone(),
                        range: line_code_range(content, Some(l)),
                    });
                }
            }
            if locations.is_empty() {
                None
            } else {
                Some(locations)
            }
        }
        ResolvedCursorTarget::Keyword { .. } => None,
    }
}

#[allow(deprecated)]
fn make_doc_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children,
    }
}

/// Computes hierarchical [`DocumentSymbol`]s for `content`.
pub fn compute_document_symbols(content: &str) -> Vec<DocumentSymbol> {
    let rep = analyze_symbols(content);
    let mut groups = Vec::new();

    // 1. Cells
    if !rep.cells.is_empty() {
        let children: Vec<DocumentSymbol> = rep
            .cells
            .iter()
            .map(|c| {
                let range = line_code_range(content, c.line_number);
                let detail = match (c.like_base_cell, c.material_id, c.density) {
                    (Some(base), _, _) => format!("LIKE cell {}", base),
                    (None, Some(0), _) => "void (mat=0)".to_string(),
                    (None, Some(m), Some(d)) => format!("M{} (rho={})", m, d),
                    (None, Some(m), None) => format!("M{}", m),
                    _ => String::new(),
                };
                make_doc_symbol(
                    format!("Cell {}", c.id),
                    Some(detail),
                    SymbolKind::OBJECT,
                    range,
                    None,
                )
            })
            .collect();
        let group_range = children
            .first()
            .map(|f| Range {
                start: f.range.start,
                end: children.last().unwrap().range.end,
            })
            .unwrap_or_default();
        groups.push(make_doc_symbol(
            format!("Block 1: Cells ({})", children.len()),
            Some(format!("next id: {}", rep.next_available_ids.cell)),
            SymbolKind::NAMESPACE,
            group_range,
            Some(children),
        ));
    }

    // 2. Surfaces
    if !rep.surfaces.is_empty() {
        let children: Vec<DocumentSymbol> = rep
            .surfaces
            .iter()
            .map(|s| {
                let range = line_code_range(content, s.line_number);
                let detail = format!("{:?} {:?}", s.kind, s.params);
                make_doc_symbol(
                    format!("Surface {}", s.id),
                    Some(detail),
                    SymbolKind::INTERFACE,
                    range,
                    None,
                )
            })
            .collect();
        let group_range = children
            .first()
            .map(|f| Range {
                start: f.range.start,
                end: children.last().unwrap().range.end,
            })
            .unwrap_or_default();
        groups.push(make_doc_symbol(
            format!("Block 2: Surfaces ({})", children.len()),
            Some(format!("next id: {}", rep.next_available_ids.surface)),
            SymbolKind::NAMESPACE,
            group_range,
            Some(children),
        ));
    }

    // 3. Materials
    if !rep.materials.is_empty() {
        let children: Vec<DocumentSymbol> = rep
            .materials
            .iter()
            .map(|m| {
                let range = line_code_range(content, m.line_number);
                let detail = format!("{} nuclide(s): {}", m.nuclide_count, m.zaids.join(", "));
                make_doc_symbol(
                    format!("M{}", m.id),
                    Some(detail),
                    SymbolKind::MODULE,
                    range,
                    None,
                )
            })
            .collect();
        let group_range = children
            .first()
            .map(|f| Range {
                start: f.range.start,
                end: children.last().unwrap().range.end,
            })
            .unwrap_or_default();
        groups.push(make_doc_symbol(
            format!("Materials ({})", children.len()),
            Some(format!("next: M{}", rep.next_available_ids.material)),
            SymbolKind::NAMESPACE,
            group_range,
            Some(children),
        ));
    }

    // 4. Tallies
    if !rep.tallies.is_empty() {
        let children: Vec<DocumentSymbol> = rep
            .tallies
            .iter()
            .map(|t| {
                let range = line_code_range(content, t.line_number);
                let label = if t.tally_type == "FMESH" {
                    format!("FMESH{}", t.id)
                } else {
                    format!("F{}", t.id)
                };
                let detail = format!("{}:{}", t.tally_type, t.particles.join(","));
                make_doc_symbol(label, Some(detail), SymbolKind::EVENT, range, None)
            })
            .collect();
        let group_range = children
            .first()
            .map(|f| Range {
                start: f.range.start,
                end: children.last().unwrap().range.end,
            })
            .unwrap_or_default();
        groups.push(make_doc_symbol(
            format!("Tallies ({})", children.len()),
            Some(format!("next F4: F{}", rep.next_available_ids.tally_f4)),
            SymbolKind::NAMESPACE,
            group_range,
            Some(children),
        ));
    }

    // 5. Distributions
    if !rep.distributions.is_empty() {
        let children: Vec<DocumentSymbol> = rep
            .distributions
            .iter()
            .map(|d| {
                make_doc_symbol(
                    format!("D{}", d.id),
                    Some(d.defined_cards.join(", ")),
                    SymbolKind::CONSTANT,
                     line_code_range(content, Some(1)),
                    None,
                )
            })
            .collect();
        groups.push(make_doc_symbol(
            format!("Distributions ({})", children.len()),
            Some(format!("next: D{}", rep.next_available_ids.distribution)),
            SymbolKind::NAMESPACE,
            line_code_range(content, Some(1)),
            Some(children),
        ));
    }

    groups
}

/// Computes context-aware [`CompletionItem`]s for a cursor position in `content`.
pub fn compute_completions(content: &str, pos: Position) -> Vec<CompletionItem> {
    let block = detect_block_at_line(content, pos.line);
    let symbols = analyze_symbols(content);
    let next = &symbols.next_available_ids;
    let mut items = Vec::new();

    match block {
        McnpBlockKind::Header => {
            for tpl in list_templates() {
                if tpl.name.starts_with("skeleton-") {
                    items.push(CompletionItem {
                        label: tpl.name.to_string(),
                        kind: Some(CompletionItemKind::SNIPPET),
                        detail: Some(tpl.summary.to_string()),
                        insert_text: Some(tpl.content.to_string()),
                        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                        ..Default::default()
                    });
                }
            }
        }
        McnpBlockKind::Cells => {
            // Next conflict-free Cell snippets
            items.push(CompletionItem {
                label: format!("cell {} (material cell)", next.cell),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New material cell with next available ID ({})", next.cell)),
                insert_text: Some(format!(
                    "{} ${{1:1}} -${{2:1.0}} -${{3:10}} imp:n=1.0",
                    next.cell
                )),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: format!("cell {} (void cell)", next.cell),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New void cell with next available ID ({})", next.cell)),
                insert_text: Some(format!("{} 0 -${{1:10}} imp:n=1.0", next.cell)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });

            // Defined Surfaces for geometry expressions
            for s in &symbols.surfaces {
                items.push(CompletionItem {
                    label: format!("-{}", s.id),
                    kind: Some(CompletionItemKind::REFERENCE),
                    detail: Some(format!("Inside/negative sense of Surface {} ({:?})", s.id, s.kind)),
                    ..Default::default()
                });
                items.push(CompletionItem {
                    label: format!("{}", s.id),
                    kind: Some(CompletionItemKind::REFERENCE),
                    detail: Some(format!("Outside/positive sense of Surface {} ({:?})", s.id, s.kind)),
                    ..Default::default()
                });
            }

            // Cell parameters
            for (label, detail, snippet) in [
                ("imp:n=1.0", "Neutron cell importance", "imp:n=${1:1.0}"),
                ("imp:n,p=1.0", "Neutron & photon importance", "imp:n,p=${1:1.0}"),
                ("vol=", "Cell volume [cm³]", "vol=${1:1.0}"),
                ("tmp=", "Cell thermal temperature [MeV]", "tmp=${1:2.53-8}"),
                ("u=", "Assign cell to Universe", &format!("u=${{1:{}}}", next.universe)),
                ("fill=", "Fill cell with Universe", "fill=${1:1}"),
                ("lat=1", "Hexahedral (rectangular) lattice", "lat=1"),
                ("lat=2", "Hexagonal prism lattice", "lat=2"),
                ("trcl=", "Cell coordinate transformation", "trcl=${1:1}"),
            ] {
                items.push(CompletionItem {
                    label: label.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(detail.to_string()),
                    insert_text: Some(snippet.to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
        }
        McnpBlockKind::Surfaces => {
            items.push(CompletionItem {
                label: format!("{} so (next surface ID)", next.surface),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("Sphere at origin with next available Surface ID ({})", next.surface)),
                insert_text: Some(format!("{} so ${{1:10.0}}", next.surface)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });

            for (mnem, summary, snippet) in [
                ("so", "Sphere centered at origin (R)", "so ${1:10.0}"),
                ("s", "General sphere (x0 y0 z0 R)", "s ${1:0.0} ${2:0.0} ${3:0.0} ${4:10.0}"),
                ("px", "Plane normal to X axis (D)", "px ${1:0.0}"),
                ("py", "Plane normal to Y axis (D)", "py ${1:0.0}"),
                ("pz", "Plane normal to Z axis (D)", "pz ${1:0.0}"),
                ("p", "General plane (A B C D)", "p ${1:1.0} ${2:0.0} ${3:0.0} ${4:0.0}"),
                ("cx", "Cylinder along X axis (R)", "cx ${1:5.0}"),
                ("cy", "Cylinder along Y axis (R)", "cy ${1:5.0}"),
                ("cz", "Cylinder along Z axis (R)", "cz ${1:5.0}"),
                ("c/x", "Cylinder parallel to X axis (y0 z0 R)", "c/x ${1:0.0} ${2:0.0} ${3:5.0}"),
                ("c/y", "Cylinder parallel to Y axis (x0 z0 R)", "c/y ${1:0.0} ${2:0.0} ${3:5.0}"),
                ("c/z", "Cylinder parallel to Z axis (x0 y0 R)", "c/z ${1:0.0} ${2:0.0} ${3:5.0}"),
                ("rpp", "Axis-aligned box macrobody (xmin xmax ymin ymax zmin zmax)", "rpp ${1:-10.0} ${2:10.0} ${3:-10.0} ${4:10.0} ${5:-10.0} ${6:10.0}"),
                ("rcc", "Right circular cylinder macrobody (vx vy vz hx hy hz R)", "rcc ${1:0.0} ${2:0.0} ${3:0.0} ${4:0.0} ${5:0.0} ${6:10.0} ${7:5.0}"),
                ("sph", "Sphere macrobody (vx vy vz R)", "sph ${1:0.0} ${2:0.0} ${3:0.0} ${4:10.0}"),
                ("box", "Orthogonal box macrobody (12 params)", "box ${1:0.0} ${2:0.0} ${3:0.0}  ${4:10.0} 0 0  0 ${5:10.0} 0  0 0 ${6:10.0}"),
                ("rhp", "Right hexagonal prism macrobody", "rhp ${1:0.0} ${2:0.0} ${3:0.0}  0 0 ${4:10.0}  ${5:5.0} 0 0"),
            ] {
                items.push(CompletionItem {
                    label: mnem.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(summary.to_string()),
                    insert_text: Some(snippet.to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
        }
        McnpBlockKind::Data | McnpBlockKind::Trailing => {
            // Next available IDs in Block 3
            items.push(CompletionItem {
                label: format!("m{}", next.material),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New Material card (next available ID: M{})", next.material)),
                insert_text: Some(format!("m{} ${{1:1001.80c}} ${{2:1.0}}", next.material)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: format!("f{}:n", next.tally_f4),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New F4 cell flux tally (next available ID: F{})", next.tally_f4)),
                insert_text: Some(format!("f{}:n ${{1:1}}", next.tally_f4)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: format!("fmesh{}:n", next.fmesh),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New superimposed mesh tally (next available ID: FMESH{})", next.fmesh)),
                insert_text: Some(format!(
                    "fmesh{}:n geom=xyz origin=${{1:-10.0}} ${{2:-10.0}} ${{3:-10.0}} imesh=${{4:10.0}} iints=${{5:10}} jmesh=${{6:10.0}} jints=${{7:10}} kmesh=${{8:10.0}} kints=${{9:10}}",
                    next.fmesh
                )),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: format!("si{} / sp{}", next.distribution, next.distribution),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New source distribution pair D{}", next.distribution)),
                insert_text: Some(format!(
                    "si{} ${{1:l}} ${{2:14.1}}\nsp{} d ${{3:1.0}}",
                    next.distribution, next.distribution
                )),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: format!("tr{}", next.transform),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(format!("New coordinate transformation (TR{})", next.transform)),
                insert_text: Some(format!("tr{} ${{1:0.0}} ${{2:0.0}} ${{3:0.0}}", next.transform)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });

            for (kw, detail, snippet) in [
                ("mode", "Particle transport mode", "mode ${1:n}"),
                ("sdef", "Fixed-source definition", "sdef pos=${1:0.0 0.0 0.0} erg=d${2:1} par=${3:n}"),
                ("kcode", "Criticality calculation parameters", "kcode ${1:1000} ${2:1.0} ${3:20} ${4:120}"),
                ("ksrc", "Initial fission source coordinates", "ksrc ${1:0.0} ${2:0.0} ${3:0.0}"),
                ("nps", "History cutoff count", "nps ${1:100000}"),
                ("wwg", "Weight window generator", "wwg ${1:4} ${2:1} ${3:1.0}"),
            ] {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(detail.to_string()),
                    insert_text: Some(snippet.to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
        }
    }

    items
}

/// MCNP Language Server implementation powered by `tower-lsp`.
pub struct McnpLanguageServer {
    client: Client,
    documents: RwLock<HashMap<Url, String>>,
}

impl McnpLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
        }
    }

    async fn update_and_publish(&self, uri: Url, text: String) {
        let diagnostics = build_lsp_diagnostics(&text);
        self.documents.write().await.insert(uri.clone(), text);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn get_document(&self, uri: &Url) -> Option<String> {
        self.documents.read().await.get(uri).cloned()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for McnpLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "mcnp-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        "=".to_string(),
                        ":".to_string(),
                        ".".to_string(),
                        "#".to_string(),
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "MCNP Language Server initialized.")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_and_publish(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update_and_publish(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.update_and_publish(params.text_document.uri, text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(content) = self.get_document(uri).await else {
            return Ok(None);
        };
        Ok(compute_hover(&content, pos))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(content) = self.get_document(uri).await else {
            return Ok(None);
        };
        Ok(compute_definitions(uri, &content, pos))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let include_decl = params.context.include_declaration;
        let Some(content) = self.get_document(uri).await else {
            return Ok(None);
        };
        Ok(compute_references(uri, &content, pos, include_decl))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let Some(content) = self.get_document(uri).await else {
            return Ok(None);
        };
        let symbols = compute_document_symbols(&content);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(content) = self.get_document(uri).await else {
            return Ok(None);
        };
        let items = compute_completions(&content, pos);
        Ok(Some(CompletionResponse::Array(items)))
    }
}

/// Starts the MCNP Language Server over standard input/output (`stdio`).
pub async fn run_lsp_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(McnpLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::get_template;

    #[test]
    fn test_lsp_diagnostics_generation() {
        let bad_input = "\
Test Invalid Input
1 99 -1.0 -999 imp:n=1.0

10 so 5.0

mode n
";
        let diags = build_lsp_diagnostics(bad_input);
        assert!(!diags.is_empty());
        // Undefined surface 999 and undefined material M99 on line 2 (0-based line 1)
        assert!(diags.iter().any(|d| d.range.start.line == 1 && d.message.contains("Hint:")));
    }

    #[test]
    fn test_lsp_goto_definition_and_references_and_hover() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");
        let uri = Url::parse("file:///test/skeleton.i").unwrap();

        // Line 7 (0-based): `2  1 -7.87     10 -20  imp:n,p=1.0            $ Iron shield shell`
        // Column 3 is `1` (Material M1), Column 15 is `10` (Surface 10)
        let mat_def = compute_definitions(&uri, tpl.content, Position::new(7, 3))
            .expect("Material 1 definition should resolve");
        match mat_def {
            GotoDefinitionResponse::Scalar(loc) => {
                // `m1   26056.80c 1.0` is on 1-based line 29 -> 0-based line 28
                assert_eq!(loc.range.start.line, 28);
            }
            other => panic!("Expected scalar location, got {:?}", other),
        }

        let surf_def = compute_definitions(&uri, tpl.content, Position::new(7, 15))
            .expect("Surface 10 definition should resolve");
        match surf_def {
            GotoDefinitionResponse::Scalar(loc) => {
                // `10 so  5.0` is on 1-based line 16 -> 0-based line 15
                assert_eq!(loc.range.start.line, 15);
            }
            other => panic!("Expected scalar location, got {:?}", other),
        }

        // References for Surface 10 (including declaration = 1 def + 2 cell refs = 3 locations)
        let refs = compute_references(&uri, tpl.content, Position::new(7, 15), true)
            .expect("Surface 10 references should resolve");
        assert_eq!(refs.len(), 3);

        // Hover on `so` mnemonic at Line 15 (0-based), Col 3
        let hover = compute_hover(tpl.content, Position::new(15, 3))
            .expect("Hover on `so` mnemonic should resolve");
        match hover.contents {
            HoverContents::Markup(m) => {
                assert!(m.value.contains("SO — Sphere Centered at Origin"));
            }
            _ => panic!("Expected markdown hover"),
        }
    }

    #[test]
    fn test_lsp_document_symbols_and_completions() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");
        let doc_symbols = compute_document_symbols(tpl.content);
        assert_eq!(doc_symbols.len(), 5); // Cells, Surfaces, Materials, Tallies, Distributions

        let cell_completions = compute_completions(tpl.content, Position::new(7, 0));
        assert!(cell_completions.iter().any(|c| c.label.contains("cell 4")));

        let data_completions = compute_completions(tpl.content, Position::new(28, 0));
        assert!(data_completions.iter().any(|c| c.label == "m3"));
        assert!(data_completions.iter().any(|c| c.label == "f14:n"));
    }

    #[tokio::test]
    async fn test_lsp_service_lifecycle_and_context_resolution() {
        let sample = "\
LSP Context Resolution Test
1 1 -10.0  -10     u=1 imp:n=1.0
2 0         10     u=1 imp:n=1.0
3 0        -20     fill=1 trcl=1 imp:n=1.0
4 LIKE 1 BUT trcl=1 imp:n=1.0
99 0        20 #3  imp:n=0.0

10 so 2.0
20 1 rpp -10 10 -10 10 -10 10

mode n
sdef pos=0 0 0 erg=d1 cel=3
si1 l 14.1
sp1 d 1.0
m1 92235.80c 1.0
tr1 1.0 2.0 3.0
f4:n 1 3
";
        let (service, _socket) = LspService::new(McnpLanguageServer::new);
        let server = service.inner();

        let init_res = server
            .initialize(InitializeParams::default())
            .await
            .expect("initialize should succeed");
        assert!(init_res.capabilities.hover_provider.is_some());
        assert!(init_res.capabilities.definition_provider.is_some());
        assert!(init_res.capabilities.references_provider.is_some());

        let uri = Url::parse("file:///test/context.inp").unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "mcnp".to_string(),
                    version: 1,
                    text: sample.to_string(),
                },
            })
            .await;

        // 1. `fill=1` on line 3 (0-based) -> should jump to cells with `u=1` (lines 1 and 2)
        let fill_defs = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(3, 21), // `fill=1`
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .expect("fill=1 should resolve to U=1 definitions");
        match fill_defs {
            GotoDefinitionResponse::Array(locs) => {
                assert_eq!(locs.len(), 2);
                assert_eq!(locs[0].range.start.line, 1);
                assert_eq!(locs[1].range.start.line, 2);
            }
            other => panic!("Expected 2 universe member cells, got {:?}", other),
        }

        // 2. `trcl=1` on line 3 -> should jump to `tr1` on line 15 (0-based)
        let tr_def = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(3, 28), // `trcl=1`
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .expect("trcl=1 should resolve to TR1");
        match tr_def {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 15);
            }
            other => panic!("Expected scalar location for TR1, got {:?}", other),
        }

        // 3. `erg=d1` on line 11 (0-based) -> should jump to `si1` and `sp1` (lines 12 and 13)
        let dist_defs = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(11, 18), // `erg=d1`
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .expect("erg=d1 should resolve to SI1/SP1");
        match dist_defs {
            GotoDefinitionResponse::Array(locs) => {
                assert_eq!(locs.len(), 2);
                assert_eq!(locs[0].range.start.line, 12);
                assert_eq!(locs[1].range.start.line, 13);
            }
            other => panic!("Expected 2 distribution cards for D1, got {:?}", other),
        }

        // 4. `f4:n 1 3` on line 16 (0-based), col 7 (`3`) -> should jump to Cell 3 on line 3
        let f4_cell_def = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(16, 7),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .expect("Target `3` in `f4:n 1 3` should resolve to Cell 3");
        match f4_cell_def {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 3);
            }
            other => panic!("Expected scalar location for Cell 3, got {:?}", other),
        }
    }
}
