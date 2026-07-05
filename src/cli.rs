//! `vimlrs` command-line driver.
//!
//! Modes:
//! - `vimlrs --expr 'EXPR'` — evaluate one expression, print its value.
//! - `vimlrs --cmd 'echo …'` — run one ex command line.
//! - `vimlrs FILE.vim` — source a script file.
//! - `vimlrs` (no args) — read-eval-print loop on stdin.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser};

use crate::aot;
use crate::fusevm_bridge::{eval_expr, eval_file, eval_source};
use crate::ported::eval::encode::encode_tv2echo;
use crate::ported::message::did_emsg;
use crate::script_cache;
use crate::viml_lexer::VimlError;

// ── MenkeTechnologies house `--help` (cyberpunk style; see `tp -h`) ──────────
// ANSI-Shadow "VIMLRS" wordmark, cyan → magenta → red.
const B1: &str = "██╗   ██╗██╗███╗   ███╗██╗     ██████╗ ███████╗";
const B2: &str = "██║   ██║██║████╗ ████║██║     ██╔══██╗██╔════╝";
const B3: &str = "██║   ██║██║██╔████╔██║██║     ██████╔╝███████╗";
const B4: &str = "╚██╗ ██╔╝██║██║╚██╔╝██║██║     ██╔══██╗╚════██║";
const B5: &str = " ╚████╔╝ ██║██║ ╚═╝ ██║███████╗██║  ██║███████║";
const B6: &str = "  ╚═══╝  ╚═╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝";

/// clap help template: yellow `USAGE:` + cyan section rules around the green-`//`
/// option and positional lists, with the banner/footer supplied at runtime.
const HELP_TEMPLATE: &str = "\n{before-help}\n{about}\n\n\x1b[33m  USAGE:\x1b[0m {usage}\n\n\x1b[36m  ── EVALUATION ─────────────────────────────────────\x1b[0m\n{options}\n\x1b[36m  ── INPUT ──────────────────────────────────────────\x1b[0m\n{positionals}\n{after-help}";

/// Banner + a status box padded at runtime so its right border never drifts as
/// the version grows, closed by the magenta `>> … <<` tagline.
fn banner() -> String {
    const BOX_W: usize = 50;
    let ver = env!("CARGO_PKG_VERSION");
    let status = format!(" STATUS: ONLINE  // SIGNAL: ████████░░ // v{ver}");
    let space = " ".repeat(BOX_W.saturating_sub(status.chars().count()));
    let rule = "─".repeat(BOX_W);
    format!(
        "\n\x1b[36m {B1}\x1b[0m\n\x1b[36m {B2}\x1b[0m\n\x1b[35m {B3}\x1b[0m\n\x1b[35m {B4}\x1b[0m\n\x1b[31m {B5}\x1b[0m\n\x1b[31m {B6}\x1b[0m\n \x1b[36m┌{rule}┐\x1b[0m\n \x1b[36m│\x1b[0m{status}{space}\x1b[36m│\x1b[0m\n \x1b[36m└{rule}┘\x1b[0m\n\x1b[35m  >> VIML INTERPRETER ON FUSEVM // FULL SPECTRUM <<\x1b[0m"
    )
}

/// SYSTEM footer: version + copyright + tagline + block rule.
fn footer() -> String {
    let ver = env!("CARGO_PKG_VERSION");
    format!(
        "\x1b[36m  ── SYSTEM ─────────────────────────────────────────\x1b[0m\n  \x1b[35mv{ver} \x1b[0m// \x1b[33m(c) Jacob Menke and contributors\x1b[0m\n  \x1b[35mThe script is compiled. The runtime is vast.\x1b[0m\n  \x1b[33m>>> JACK IN. SOURCE THE SCRIPT. RUN VIML EVERYWHERE. <<<\x1b[0m\n \x1b[36m░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░\x1b[0m"
    )
}

/// Parsed command line.
#[derive(Parser, Debug)]
#[command(
    name = "vimlrs",
    version,
    about = "VimL (Vimscript) interpreter on the fusevm bytecode VM"
)]
pub struct Cli {
    /// Evaluate a single expression and print its value.
    #[arg(
        short = 'e',
        long = "expr",
        value_name = "EXPR",
        help = "\x1b[32m//\x1b[0m Evaluate one expression and print its value"
    )]
    expr: Option<String>,

    /// Execute a single ex command line (e.g. 'echo 1 + 1').
    #[arg(
        short = 'c',
        long = "cmd",
        value_name = "CMD",
        help = "\x1b[32m//\x1b[0m Run one ex command line (e.g. 'echo 1 + 1')"
    )]
    cmd: Option<String>,

    /// AOT build: bake the given script files into a self-contained executable
    /// at this path.
    #[arg(
        short = 'b',
        long = "build",
        value_name = "OUT",
        help = "\x1b[32m//\x1b[0m Bake the script file(s) into a self-contained executable at OUT"
    )]
    build: Option<PathBuf>,

    /// With --build: AOT-compile to native machine code (Cranelift object
    /// linked standalone) instead of embedding the source.
    #[arg(
        short = 'n',
        long = "native",
        help = "\x1b[32m//\x1b[0m With --build: AOT-compile to native code (Cranelift), not embed source"
    )]
    native: bool,

    /// Delete the rkyv bytecode script cache and exit.
    #[arg(
        long = "clear-cache",
        help = "\x1b[32m//\x1b[0m Delete the rkyv bytecode script cache and exit"
    )]
    clear_cache: bool,

    /// Run the Language Server Protocol server on stdio (for editors).
    #[arg(
        long = "lsp",
        help = "\x1b[32m//\x1b[0m Run the Language Server Protocol server on stdio (for editors)"
    )]
    lsp: bool,

    /// Run the Debug Adapter Protocol server on stdio (for editors).
    #[arg(
        long = "dap",
        help = "\x1b[32m//\x1b[0m Run the Debug Adapter Protocol server on stdio (for editors)"
    )]
    dap: bool,

    /// Print the fusevm bytecode listing before running.
    #[arg(
        long = "disasm",
        help = "\x1b[32m//\x1b[0m Print the fusevm bytecode listing before running"
    )]
    disasm: bool,

    /// VimL script file(s) to source (cached). With --build, the inputs to bake.
    #[arg(
        value_name = "FILE",
        help = "\x1b[32m//\x1b[0m VimL script file(s) to source; with --build, the inputs to bake"
    )]
    files: Vec<PathBuf>,
}

/// Whether any error was raised in the last run (`did_emsg` set).
fn had_error() -> bool {
    did_emsg.with(|d| d.get()) != 0
}

fn exit_for_errors() -> ExitCode {
    if had_error() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Parse args and dispatch. Returns the process exit code.
pub fn run() -> ExitCode {
    // Render `--help`/`-h` in the MenkeTechnologies house style: augment the
    // derived command with the banner, the runtime-padded status box, the
    // cyan section rules, and the SYSTEM footer (see `tp -h`).
    let cmd = Cli::command()
        .help_template(HELP_TEMPLATE)
        .before_help(banner())
        .after_help(footer());
    let cli = match Cli::from_arg_matches(&cmd.get_matches()) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };

    if cli.lsp {
        return match crate::lsp::run_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("vimlrs: lsp: {e}");
                ExitCode::FAILURE
            }
        };
    }

    if cli.dap {
        return match crate::dap::run_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("vimlrs: dap: {e}");
                ExitCode::FAILURE
            }
        };
    }

    crate::fusevm_disasm::set_enabled(cli.disasm);

    if cli.clear_cache {
        if let Some(cache) = script_cache::CACHE.as_ref() {
            if let Err(e) = cache.clear() {
                eprintln!("vimlrs: clear-cache: {e}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if let Some(out) = cli.build {
        let result = if cli.native {
            aot::build_native(&cli.files, &out)
        } else {
            aot::build(&cli.files, &out)
        };
        return match result {
            Ok(p) => {
                println!("{}", p.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(src) = cli.expr {
        return match eval_expr(&src) {
            Ok(v) => {
                // As with :echo, a runtime error during evaluation means there is
                // no valid result to print — emit only the error (already on
                // stderr), not a spurious fallback value.
                if !had_error() {
                    println!("{}", encode_tv2echo(&v));
                }
                exit_for_errors()
            }
            Err(e) => fail(e),
        };
    }

    if let Some(src) = cli.cmd {
        return match eval_source(&src) {
            Ok(_) => exit_for_errors(),
            Err(e) => fail(e),
        };
    }

    if !cli.files.is_empty() {
        // Seed the global argument list (argv()/argc()) with the file args, the
        // standalone counterpart of Vim's file arglist, before sourcing.
        let arglist: Vec<String> = cli
            .files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        crate::ported::eval::funcs::set_arglist(&arglist);
        // Source each file in order through the bytecode cache.
        for path in &cli.files {
            if let Err(e) = eval_file(path) {
                return fail(e);
            }
        }
        return exit_for_errors();
    }

    repl()
}

/// Report a parse/compile error (runtime `emsg`s already printed to stderr).
fn fail(e: VimlError) -> ExitCode {
    eprintln!("{e}");
    ExitCode::FAILURE
}

/// Read-eval-print loop: each input line is one statement; a bare expression
/// prints its value, an `:echo`/`:let` runs for effect.
fn repl() -> ExitCode {
    let stdin = io::stdin();
    let mut out = io::stdout();
    let prompt = |out: &mut io::Stdout| {
        let _ = write!(out, "vimlrs> ");
        let _ = out.flush();
    };
    prompt(&mut out);
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            prompt(&mut out);
            continue;
        }
        match eval_source(&line) {
            Ok(Some(v)) => println!("{}", encode_tv2echo(&v)),
            Ok(None) => {}
            Err(e) => eprintln!("{e}"),
        }
        prompt(&mut out);
    }
    println!();
    ExitCode::SUCCESS
}
