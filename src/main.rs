//! `vimlrs` binary entry point. All logic lives in the library so zemacs can
//! embed the same interpreter; this dispatches the AOT trailer (if any) then
//! the CLI.

use std::process::ExitCode;

use vimlrs::aot;
use vimlrs::fusevm_bridge::eval_source;
use vimlrs::ported::message::did_emsg;

/// Worker-thread stack size. The parser, bytecode compiler and value `Drop` all
/// walk the expression tree recursively, so a pathologically deep-but-valid
/// script (`let x = ` + 500000 `-`, which real vim evaluates without error) would
/// overflow the default 8 MiB main-thread stack and abort with SIGABRT. Vim
/// tolerates such depth because its C frames are tiny; a debug Rust build's
/// frames are far larger, so give the worker a 1 GiB stack (virtual reservation,
/// pages commit only when touched). This keeps vimlrs matching vim's behaviour —
/// succeed on deep linear chains, and raise `E1169` on deep *bracket* nesting via
/// the parser's depth guard — instead of crashing. Verified: 500000 nested `-`
/// now succeed; 1000 nested `(` raise E1169 like vim.
const WORKER_STACK: usize = 1 << 30;

fn run() -> ExitCode {
    // AOT: if this binary has scripts baked in, run them in build order and
    // exit (the self-contained-executable path).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(files) = aot::try_load_embedded(&exe) {
            for f in files.0 {
                if eval_source(&f.source).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            let failed = did_emsg.with(|d| d.get()) != 0;
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
