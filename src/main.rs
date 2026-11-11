//! `vimlrs` binary entry point. All logic lives in the library so zmax can
//! embed the same interpreter; this dispatches the AOT trailer (if any) then
//! the CLI.

use std::process::ExitCode;

use vimlrs::aot;
use vimlrs::fusevm_bridge::eval_source;
use vimlrs::ported::message::ex_exitval;
use vimlrs::WORKER_STACK;

/// Run, then close the message line. `:echo` under Vim's message-column model
/// puts the newline *before* the text, so the last line of a run has none yet
/// (see `fusevm_bridge::msg_flush_line`).
fn run() -> ExitCode {
    let code = run_inner();
    vimlrs::fusevm_bridge::msg_flush_line();
    code
}

fn run_inner() -> ExitCode {
    // AOT: if this binary has scripts baked in, run them in build order and
    // exit (the self-contained-executable path).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(files) = aot::try_load_embedded(&exe) {
            for f in files.0 {
                if eval_source(&f.source).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            // c: `ex_exitval` (`vendor/message.c:855`), not `did_emsg` — `:catch`
            // resets the latter but never the former. Same rule as `cli::had_error`.
            let failed = ex_exitval.with(|e| e.get()) != 0;
            return if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    }
    vimlrs::cli::run()
}

fn main() -> ExitCode {
    // Run everything on a worker thread with a large stack so deep-but-valid
    // recursion (see `WORKER_STACK`) does not overflow the default main stack.
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(run)
        .expect("spawn worker thread")
        .join()
        .expect("worker thread panicked")
}
