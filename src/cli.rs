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

use clap::Parser;

use crate::aot;
use crate::fusevm_bridge::{eval_expr, eval_file, eval_source};
use crate::ported::eval::encode::encode_tv2echo;
use crate::ported::message::did_emsg;
use crate::script_cache;
use crate::viml_lexer::VimlError;

/// Parsed command line.
#[derive(Parser, Debug)]
#[command(
    name = "vimlrs",
    version,
    about = "VimL (Vimscript) interpreter on the fusevm bytecode VM"
)]
pub struct Cli {
    /// Evaluate a single expression and print its value.
    #[arg(short = 'e', long = "expr", value_name = "EXPR")]
    expr: Option<String>,

    /// Execute a single ex command line (e.g. 'echo 1 + 1').
    #[arg(short = 'c', long = "cmd", value_name = "CMD")]
    cmd: Option<String>,

    /// AOT build: bake the given script files into a self-contained executable
    /// at this path.
    #[arg(short = 'b', long = "build", value_name = "OUT")]
    build: Option<PathBuf>,

    /// Delete the rkyv bytecode script cache and exit.
    #[arg(long = "clear-cache")]
    clear_cache: bool,

    /// Run the Language Server Protocol server on stdio (for editors).
    #[arg(long = "lsp")]
    lsp: bool,

    /// Run the Debug Adapter Protocol server on stdio (for editors).
    #[arg(long = "dap")]
    dap: bool,

    /// Print the fusevm bytecode listing before running.
    #[arg(long = "disasm")]
    disasm: bool,

    /// VimL script file(s) to source (cached). With --build, the inputs to bake.
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
    let cli = Cli::parse();

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
        return match aot::build(&cli.files, &out) {
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
                println!("{}", encode_tv2echo(&v));
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
