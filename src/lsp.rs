//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. Language Server Protocol (stdio) for
//! editors — `vimlrs --lsp`. Self-contained, reusing the synthesis
//! lexer/parser: diagnostics come from per-line [`crate::viml_parser::parse_stmt`];
//! completion / hover draw on the Phase-3 builtin set, the ex-command words, and
//! the predefined `v:` constants. No output reaches the terminal — JSON-RPC on
//! stdio only. Structure ported from awkrs's `lsp.rs`.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::collections::HashMap;

use lsp_server::{Connection, ErrorCode, ExtractError, Message, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, DocumentSymbolRequest, HoverRequest, Request as _};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, MarkupContent, MarkupKind, OneOf,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};

use crate::builtin_docs::{hover_markdown, BUILTIN_DOCS, EX_COMMANDS, V_VARS};
use crate::viml_parser::{parse_stmt, PHASE3_BUILTINS};

/// Open-document store: uri string → full buffer text (FULL text sync).
type Docs = HashMap<String, String>;

/// Guard against leaking as an orphan LSP process. Editors / GUI apps reap us via
/// kill-on-drop, which only fires on a *graceful* client exit; a hard client
/// death (SIGKILL/crash/force-quit) skips it, and a leaked pipe fd can keep our
/// stdin open so we never see EOF either. Watch for reparenting to pid 1 (our
/// client died) and exit — nothing this read-only server holds is worth leaking.
fn spawn_orphan_guard() {
    std::thread::spawn(|| {
        // Linux: also ask the kernel to SIGKILL us the instant the parent dies.
        // Best-effort; the getppid poll below is the portable guarantee (macOS
        // has no PDEATHSIG).
        #[cfg(target_os = "linux")]
        // SAFETY: prctl(PR_SET_PDEATHSIG, ...) only registers a signal disposition.
        unsafe {
            libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0,
                0,
                0,
            );
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // SAFETY: getppid takes no arguments and never fails.
            if unsafe { libc::getppid() } == 1 {
                std::process::exit(0);
            }
        }
    });
}

/// Entry point for `vimlrs --lsp`. Serves JSON-RPC on stdio until `shutdown`.
pub fn run_stdio() -> Result<(), String> {
    spawn_orphan_guard();
    let (conn, io_threads) = Connection::stdio();
    let (init_id, _params) = conn
        .initialize_start()
        .map_err(|e| format!("lsp initialize: {e}"))?;
    let init_result = serde_json::json!({
        "capabilities": server_capabilities(),
        "serverInfo": { "name": "vimlrs", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.sender
        .send(Response::new_ok(init_id, init_result).into())
        .map_err(|e| format!("lsp send: {e}"))?;

    let mut docs: Docs = HashMap::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn
                    .handle_shutdown(&req)
                    .map_err(|e| format!("lsp shutdown: {e}"))?
                {
                    break;
                }
                dispatch_request(&conn, &docs, req);
            }
            Message::Notification(not) => dispatch_notification(&conn, &mut docs, not),
            Message::Response(_) => {}
        }
    }
    drop(conn);
    io_threads.join().map_err(|_| "lsp io join".to_string())?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

fn handle<P, R>(conn: &Connection, req: Request, f: impl FnOnce(P) -> R)
where
    P: serde::de::DeserializeOwned,
    R: serde::Serialize,
{
    let method = req.method.clone();
    let id = req.id.clone();
    match req.extract::<P>(&method) {
        Ok((id, params)) => {
            let value = serde_json::to_value(f(params)).unwrap_or(serde_json::Value::Null);
            let _ = conn.sender.send(Response::new_ok(id, value).into());
        }
        Err(ExtractError::JsonError { error, .. }) => {
            let _ = conn.sender.send(
                Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string()).into(),
            );
        }
        Err(ExtractError::MethodMismatch(_)) => unreachable!("method matched before extract"),
    }
}

fn dispatch_request(conn: &Connection, docs: &Docs, req: Request) {
    match req.method.as_str() {
        Completion::METHOD => handle(conn, req, |p: CompletionParams| completions(docs, p)),
        HoverRequest::METHOD => handle(conn, req, |p: HoverParams| hover(docs, p)),
        DocumentSymbolRequest::METHOD => handle(conn, req, |p: DocumentSymbolParams| {
            document_symbols(docs, p)
        }),
        _ => {
            let _ = conn.sender.send(
                Response::new_err(req.id, ErrorCode::MethodNotFound as i32, "unhandled".into())
                    .into(),
            );
        }
    }
}

fn dispatch_notification(conn: &Connection, docs: &mut Docs, not: lsp_server::Notification) {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.insert(uri.as_str().to_string(), p.text_document.text.clone());
                publish_diagnostics(conn, &uri, &p.text_document.text);
            }
        }
        DidChangeTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri;
                    docs.insert(uri.as_str().to_string(), change.text.clone());
                    publish_diagnostics(conn, &uri, &change.text);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.remove(uri.as_str());
                publish_diagnostics(conn, &uri, "");
            }
        }
        _ => {}
    }
}

// ── diagnostics ──

fn publish_diagnostics(conn: &Connection, uri: &Uri, text: &str) {
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: compute_diagnostics(text),
        version: None,
    };
    let not = lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = conn.sender.send(not.into());
}

/// Parse each statement line independently; a parse error becomes a diagnostic
/// spanning that line. (Whole-program control-flow errors arrive with the
/// `ex_eval.c` port.)
fn compute_diagnostics(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('"') {
            continue;
        }
        if let Err(e) = parse_stmt(line) {
            let l = i as u32;
            out.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: l,
                        character: 0,
                    },
                    end: Position {
                        line: l,
                        character: line.chars().count() as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("vimlrs".into()),
                message: e.to_string(),
                ..Default::default()
            });
        }
    }
    out
}

// ── completion ──

/// Flat, sorted, deduped list of every completion word — the union of the four
/// completion sources ([`BUILTIN_DOCS`] names, [`EX_COMMANDS`], [`V_VARS`], and
/// the parser's [`PHASE3_BUILTINS`]). This is the single source of truth for
/// bare names; the LSP's [`completions`] enriches the same union with
/// signatures/docs/kinds, while the interactive REPL (`crate::repl`) feeds this
/// flat list straight into its completion menu. Keeping both on this union
/// means the editor and the REPL always offer the same surface.
pub fn completion_words() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(BUILTIN_DOCS.len() + 32);
    for (name, _, _) in BUILTIN_DOCS {
        v.push((*name).to_string());
    }
    for (cmd, _) in EX_COMMANDS {
        v.push((*cmd).to_string());
    }
    for (var, _) in V_VARS {
        v.push((*var).to_string());
    }
    for name in PHASE3_BUILTINS {
        v.push((*name).to_string());
    }
    v.sort();
    v.dedup();
    v
}

/// LSP `textDocument/completion`. Builds rich `CompletionItem`s from the same
/// four sources as [`completion_words`] (builtins + ex-commands + `v:` vars +
/// Phase-3 builtins), preserving per-item signature/doc/kind detail. The set of
/// names offered here is identical to [`completion_words`].
fn completions(_docs: &Docs, _params: CompletionParams) -> CompletionResponse {
    let mut items = Vec::new();
    for (name, sig, doc) in BUILTIN_DOCS {
        items.push(CompletionItem {
            label: format!("{name}()"),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some((*sig).to_string()),
            documentation: Some(lsp_types::Documentation::String((*doc).to_string())),
            insert_text: Some(format!("{name}(")),
            ..Default::default()
        });
    }
    // Builtins without a doc entry (kept in sync with the parser's table).
    for name in PHASE3_BUILTINS {
        if !BUILTIN_DOCS.iter().any(|(n, _, _)| n == name) {
            items.push(CompletionItem {
                label: format!("{name}()"),
                kind: Some(CompletionItemKind::FUNCTION),
                ..Default::default()
            });
        }
    }
    for (cmd, doc) in EX_COMMANDS {
        items.push(CompletionItem {
            label: (*cmd).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some((*doc).to_string()),
            ..Default::default()
        });
    }
    for (v, doc) in V_VARS {
        items.push(CompletionItem {
            label: (*v).to_string(),
            kind: Some(CompletionItemKind::CONSTANT),
            detail: Some((*doc).to_string()),
            ..Default::default()
        });
    }
    CompletionResponse::Array(items)
}

// ── hover ──

fn hover(docs: &Docs, params: HoverParams) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let text = docs.get(uri.as_str())?;
    let word = word_at(text, pos)?;
    let value = hover_markdown(&word)?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    })
}

// ── document symbols ──

#[allow(deprecated)]
fn document_symbols(docs: &Docs, params: DocumentSymbolParams) -> DocumentSymbolResponse {
    let Some(text) = docs.get(params.text_document.uri.as_str()) else {
        return DocumentSymbolResponse::Nested(Vec::new());
    };
    let mut syms = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        // `let <name> = …` and `function <name>(…)`.
        let (kind, name) = if let Some(rest) = t.strip_prefix("let ") {
            (
                SymbolKind::VARIABLE,
                rest.split(['=', ' ']).next().unwrap_or("").trim(),
            )
        } else if let Some(rest) = t.strip_prefix("function ") {
            (
                SymbolKind::FUNCTION,
                rest.split('(').next().unwrap_or("").trim(),
            )
        } else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let l = i as u32;
        let range = Range {
            start: Position {
                line: l,
                character: 0,
            },
            end: Position {
                line: l,
                character: line.chars().count() as u32,
            },
        };
        syms.push(DocumentSymbol {
            name: name.to_string(),
            detail: None,
            kind,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    DocumentSymbolResponse::Nested(syms)
}

/// The identifier-ish word at `pos` (for hover). Letters, digits, `_`, `:`, `#`.
fn word_at(text: &str, pos: Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = (pos.character as usize).min(chars.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == ':' || c == '#';
    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(chars[start..end].iter().collect())
}
