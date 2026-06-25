//! `vimlrs` binary entry point. All logic lives in the library so zemacs can
//! embed the same interpreter; this dispatches the AOT trailer (if any) then
//! the CLI.

use std::process::ExitCode;

use vimlrs::aot;
use vimlrs::fusevm_bridge::eval_source;
use vimlrs::ported::message::did_emsg;

fn main() -> ExitCode {
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
