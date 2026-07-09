//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. Interactive REPL for `vimlrs` — a
//! utop-style line editor backed by `reedline`. Adapted from strykelang's
//! `repl.rs` (same reedline 0.47 API), trimmed for vimlrs's thread-local
//! interpreter model.
//!
//! Layout per turn:
//!
//! ```text
//! ─( HH:MM:SS )──< command N >─────────────────────────────{ vimlrs 0.1.2 }─
//! vimlrs❯ <buffer>
//!         abs           add           and           append        …
//! ```
//!
//! * The top "modeline" is rendered as part of `Prompt::render_prompt_left`, so
//!   it repaints with the buffer (no scroll-off, no flicker).
//! * Tab pops a `ColumnarMenu` of suggestions sourced from
//!   [`crate::lsp::completion_words`] — the same wordlist the LSP serves.
//! * History is `~/.vimlrs/history` via `FileBackedHistory`.
//! * Edit mode (emacs/vi) comes from `~/.vimlrs/config.toml` `[repl] mode`, with
//!   a `VIMLRS_REPL_MODE` env override and a `VIMLRS_NO_CONFIG` guard.
//!
//! ## Single-line evaluation (no multi-line block validator)
//!
//! Each `Enter` submits one line to [`crate::eval_source`], exactly like the
//! plain non-TTY `cli::repl` fallback. A reedline `Validator` that continued the
//! buffer on an open `function`/`if`/`while`/`for`/`try` block was considered
//! and deliberately **not** shipped: doing it correctly requires replicating the
//! parser's `|`-separator + string/comment awareness (an inline
//! `if x | echo 1 | endif` is a complete line; `echo "endif"` must not decrement
//! block depth). A naive counter hangs the REPL on those valid single-line
//! inputs, which is worse than not having continuation. Interactive block
//! definitions are therefore entered one line at a time, matching the existing
//! stdin REPL. Reedline does not include a file-path completer either; bare-path
//! completion is intentionally dropped — the LSP word list is the high-value
//! surface (commands, not paths), the same choice utop makes.

use std::borrow::Cow;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use nu_ansi_term::{Color as NuColor, Style};
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, Completer, EditMode, Emacs, FileBackedHistory, KeyCode, KeyModifiers,
    Keybindings, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion, Vi,
};

use crate::ported::eval::encode::encode_tv2echo;
use crate::ported::message::did_emsg;

const VIMLRS_VERSION: &str = env!("CARGO_PKG_VERSION");

fn vimlrs_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".vimlrs"))
        .unwrap_or_else(|| std::path::PathBuf::from(".vimlrs"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn history_path() -> std::path::PathBuf {
    vimlrs_dir().join("history")
}

fn config_path() -> std::path::PathBuf {
    vimlrs_dir().join("config.toml")
}

/// Contents of the auto-seeded `~/.vimlrs/config.toml`. Every setting is
/// commented out so the seeded file documents the schema without changing
/// behavior — uncomment + edit a line to override the in-code default.
const DEFAULT_CONFIG_TOML: &str = r#"# vimlrs REPL config — auto-generated on first launch.
# Lines starting with `#` are comments. Uncomment + edit a line to
# override the in-code default. Delete this file and vimlrs will
# regenerate it on the next run.

[repl]
# Edit mode for the interactive REPL. Defaults to emacs.
#
#   "emacs" — Ctrl-A/Ctrl-E/Ctrl-K/etc., readline-style (default)
#   "vi"    — modal editing; Esc → normal mode, i/a → insert,
#             h/j/k/l navigation, dd/cc/yy/x, /-search, etc.
#
# Tab + Shift+Tab cycle the completion menu in either mode.
# Override per-session with `VIMLRS_REPL_MODE=vi viml`.
# mode = "emacs"
"#;

/// First-run seed: write `~/.vimlrs/config.toml` if it does not exist. Safe to
/// call on every REPL launch — no-op when the file is already there (and silent
/// if the home directory is read-only). Honors `VIMLRS_NO_CONFIG=1` for CI /
/// sandbox environments that should not touch the user's home dir.
pub fn ensure_default_config_seeded() {
    if std::env::var_os("VIMLRS_NO_CONFIG").is_some() {
        return;
    }
    let path = config_path();
    if path.exists() {
        return;
    }
    // `vimlrs_dir()` already created the directory; ignore write failures.
    let _ = std::fs::write(&path, DEFAULT_CONFIG_TOML);
}

/// REPL edit-mode selector. `Emacs` is the default; `Vi` enables reedline's
/// two-mode insert/normal keybinding set with the standard `Esc` toggle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ReplMode {
    Emacs,
    Vi,
}

/// Resolve the REPL edit mode in this precedence:
/// 1. `VIMLRS_REPL_MODE=emacs|vi` env var (overrides everything).
/// 2. `~/.vimlrs/config.toml` `[repl] mode = "vi"`.
/// 3. Default `Emacs`.
fn resolve_repl_mode() -> ReplMode {
    if let Some(env) = std::env::var_os("VIMLRS_REPL_MODE") {
        let s = env.to_string_lossy().to_ascii_lowercase();
        if s == "vi" || s == "vim" {
            return ReplMode::Vi;
        }
        if s == "emacs" {
            return ReplMode::Emacs;
        }
    }
    let raw = match std::fs::read_to_string(config_path()) {
        Ok(s) => s,
        Err(_) => return ReplMode::Emacs,
    };
    let parsed: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return ReplMode::Emacs,
    };
    let mode = parsed
        .get("repl")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("emacs");
    match mode.to_ascii_lowercase().as_str() {
        "vi" | "vim" => ReplMode::Vi,
        _ => ReplMode::Emacs,
    }
}

/// Apply the completion-menu Tab / Shift+Tab bindings to a keybinding set — so
/// the bindings live on the emacs map AND the vi insert map.
fn install_menu_bindings(keybindings: &mut Keybindings) {
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
}

/// True for characters that belong to a VimL completion token. Includes `:` so
/// scoped names (`g:foo`, `v:true`, `s:count`) and `#` so autoload names
/// (`pack#init`) complete as a single unit; letters, digits and `_` are the
/// bare identifier chars.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | ':' | '#')
}

/// Byte index `start` and the incomplete word before `pos` (for prefix matching).
/// The start snaps back over consecutive [`is_word_char`] characters, so a
/// scoped name like `g:foo` completes as one token.
fn completion_word_start(line: &str, pos: usize) -> (usize, &str) {
    let pos = pos.min(line.len());
    let before = line.get(..pos).unwrap_or("");
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(pos);
    (start, line.get(start..pos).unwrap_or(""))
}

struct VimlCompleter {
    static_words: Vec<String>,
}

impl Completer for VimlCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let (start, prefix) = completion_word_start(line, pos);
        let span = Span::new(start, pos);
        let mut out: Vec<Suggestion> = self
            .static_words
            .iter()
            .filter(|w| w.starts_with(prefix))
            .map(|w| Suggestion {
                value: w.clone(),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: false,
                display_override: None,
                match_indices: None,
            })
            .collect();
        out.sort_by(|a, b| a.value.cmp(&b.value));
        out
    }
}

struct VimlPrompt {
    cmd_count: Arc<Mutex<u64>>,
}

fn now_hms() -> String {
    // Local time via `libc::localtime_r` — no chrono / time crate. Works on
    // macOS aarch64 + Linux. On failure, falls back to UTC modulo math so the
    // status bar always shows something.
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    if ok {
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    } else {
        let s = (secs as u64) % 86_400;
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

fn term_cols() -> usize {
    use std::os::unix::io::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let fd = std::io::stdout().as_raw_fd();
    let cols = if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
        ws.ws_col as usize
    } else {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(80)
    };
    cols.max(40)
}

fn render_status_bar(cmd_count: u64) -> String {
    let cols = term_cols();
    let dim = NuColor::DarkGray;
    let accent = NuColor::Cyan;
    let label = NuColor::LightYellow;

    let left = format!(" {} ", now_hms());
    let mid = format!(" command {} ", cmd_count);
    let right = format!(" viml {} ", VIMLRS_VERSION);

    // Plain-text widths for layout math (segments carry no ANSI yet).
    // `frame_chars` = display width of every literal frame char emitted below
    // (`─(`, `)──<`, `>`, `{`, `}─`). `chars().count()` isn't `const fn`, so
    // this runs once per repaint.
    let frame_chars = "─()──<>{}─".chars().count();
    let visible = left.chars().count() + mid.chars().count() + right.chars().count() + frame_chars;
    let dashes = cols.saturating_sub(visible);
    // Need at least 1 dash per side for the frame look; if the terminal is
    // genuinely too narrow, drop the right segment instead of wrapping.
    if dashes < 2 {
        return format!(
            "{lp}{l}{rp}{ml}{m}{mr}",
            lp = Style::new().fg(dim).paint("─("),
            l = Style::new().fg(accent).paint(left),
            rp = Style::new().fg(dim).paint(")"),
            ml = Style::new().fg(dim).paint("──<"),
            m = Style::new().fg(label).bold().paint(mid),
            mr = Style::new().fg(dim).paint(">"),
        );
    }
    let left_dash = dashes / 2;
    let right_dash = dashes - left_dash;

    let bar_l = "─".repeat(left_dash);
    let bar_r = "─".repeat(right_dash);

    format!(
        "{lp}{l}{rp}{ml}{m}{mr}{bar}{rl}{r}{rr}",
        lp = Style::new().fg(dim).paint("─("),
        l = Style::new().fg(accent).paint(left),
        rp = Style::new().fg(dim).paint(")"),
        ml = Style::new().fg(dim).paint("──<"),
        m = Style::new().fg(label).bold().paint(mid),
        mr = Style::new().fg(dim).paint(">"),
        bar = Style::new().fg(dim).paint(format!("{}{}", bar_l, bar_r)),
        rl = Style::new().fg(dim).paint("{"),
        r = Style::new().fg(NuColor::Magenta).paint(right),
        rr = Style::new().fg(dim).paint("}─"),
    )
}

impl Prompt for VimlPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let count = self.cmd_count.lock().map(|g| *g).unwrap_or(0);
        let bar = render_status_bar(count);
        let prompt = Style::new()
            .fg(NuColor::Cyan)
            .bold()
            .paint("viml")
            .to_string();
        Cow::Owned(format!("{}\n{}", bar, prompt))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        let s = Style::new()
            .fg(NuColor::LightCyan)
            .bold()
            .paint("❯ ")
            .to_string();
        Cow::Owned(s)
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        let s = Style::new()
            .fg(NuColor::DarkGray)
            .paint("····❯ ")
            .to_string();
        Cow::Owned(s)
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

/// Cumulative `did_emsg` counter. Compared before/after each eval so we can tell
/// whether *this* line raised a runtime error (the counter is process-wide and
/// never resets between lines, so a raw `!= 0` check would suppress all output
/// after the first error — see `fusevm_bridge.rs:2092`'s ERR_MARK checkpoint).
fn emsg_count() -> u64 {
    did_emsg.with(|d| d.get())
}

/// Run the interactive reedline REPL. Must be called on the CLI worker thread
/// (interpreter globals — `g:`/`v:` vars, the last-result slot — are
/// thread-local, so evaluating inline here preserves state across turns).
pub fn run() -> ExitCode {
    ensure_default_config_seeded();

    // Same wordmark + live-stats banner as `vimlrs --help`, so a fresh REPL
    // session looks like the rest of the CLI surface. Followed by one hint line.
    crate::banner::print_banner(true);
    println!();
    println!("\x1b[2m  type `exit`/`quit` or Ctrl-D to leave — Tab for completion\x1b[0m");
    println!();

    let static_words = crate::lsp::completion_words();
    let cmd_count = Arc::new(Mutex::new(0u64));

    let completer = VimlCompleter { static_words };

    let menu = ColumnarMenu::default()
        .with_name("completion_menu")
        .with_columns(4)
        .with_column_padding(2);

    // Mode (emacs/vi) from `~/.vimlrs/config.toml` or `VIMLRS_REPL_MODE`. Menu
    // navigation bindings attach to the active insert-mode keymap so completion
    // behaves the same in either edit mode.
    let edit_mode: Box<dyn EditMode> = match resolve_repl_mode() {
        ReplMode::Emacs => {
            let mut kb = default_emacs_keybindings();
            install_menu_bindings(&mut kb);
            Box::new(Emacs::new(kb))
        }
        ReplMode::Vi => {
            let mut insert_kb = default_vi_insert_keybindings();
            install_menu_bindings(&mut insert_kb);
            let normal_kb = default_vi_normal_keybindings();
            Box::new(Vi::new(insert_kb, normal_kb))
        }
    };

    let history = match FileBackedHistory::with_file(5_000, history_path()) {
        Ok(h) => Box::new(h) as Box<dyn reedline::History>,
        Err(e) => {
            eprintln!("vimlrs: repl: history unavailable: {}", e);
            match FileBackedHistory::new(5_000) {
                Ok(h) => Box::new(h) as Box<dyn reedline::History>,
                Err(_) => {
                    eprintln!("vimlrs: repl: cannot create in-memory history");
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    let mut line_editor = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(edit_mode)
        .with_history(history);

    let prompt = VimlPrompt {
        cmd_count: Arc::clone(&cmd_count),
    };

    loop {
        let sig = match line_editor.read_line(&prompt) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("vimlrs: repl: {}", e);
                break;
            }
        };

        match sig {
            Signal::Success(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let low = trimmed.to_lowercase();
                if low == "exit" || low == "quit" {
                    break;
                }

                if let Ok(mut g) = cmd_count.lock() {
                    *g += 1;
                }

                // Evaluate on this (worker) thread so thread-local interpreter
                // state persists across turns. Checkpoint `did_emsg`: only print
                // the result value when no runtime error was raised for THIS
                // line (mirrors `cli::run`'s `--expr` path).
                let before = emsg_count();
                match crate::eval_source(trimmed) {
                    Ok(opt) => {
                        let errored = emsg_count() > before;
                        if let Some(v) = opt {
                            if !errored {
                                println!("{}", encode_tv2echo(&v));
                            }
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
            Signal::CtrlC => continue,
            Signal::CtrlD => break,
            #[allow(unreachable_patterns)]
            _ => break,
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_word_at_cursor_is_bare_name() {
        let s = "echo len";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(st, 5);
        assert_eq!(pre, "len");
    }

    #[test]
    fn completion_word_keeps_scope_colon_as_one_token() {
        let s = "call g:foo";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(st, 5);
        assert_eq!(pre, "g:foo");
    }

    #[test]
    fn completion_word_keeps_autoload_hash() {
        let s = "call pack#in";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(st, 5);
        assert_eq!(pre, "pack#in");
    }

    #[test]
    fn completion_word_empty_after_space() {
        let s = "let ";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(st, 4);
        assert_eq!(pre, "");
    }

    #[test]
    fn completer_offers_lsp_words_for_prefix() {
        let mut c = VimlCompleter {
            static_words: crate::lsp::completion_words(),
        };
        let line = "ab";
        let out = c.complete(line, line.len());
        // `abs` is a documented builtin; every suggestion must share the prefix.
        assert!(out.iter().any(|s| s.value == "abs"));
        assert!(out.iter().all(|s| s.value.starts_with("ab")));
    }

    #[test]
    fn completer_offers_scoped_vvars() {
        let mut c = VimlCompleter {
            static_words: crate::lsp::completion_words(),
        };
        let line = "echo v:";
        let out = c.complete(line, line.len());
        assert!(out.iter().any(|s| s.value == "v:true"));
    }
}
