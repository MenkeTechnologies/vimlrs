//! Embedding entry point: run a VimL script with a given stdin and collect its
//! stdout, the way `viml FILE` runs one (`vim -es -c 'verbose source FILE'`).
//!
//! Interpreter state (variables, functions, options, the message column) is
//! thread-local, so each call runs on a thread of its own and starts from a
//! fresh interpreter; concurrent calls share nothing.

use crate::fusevm_bridge::{capture_begin, capture_take, set_input, set_silent_ex, source_text};
use crate::ported::eval::vars::{get_vim_var_str, vv::VV_ERRMSG};
use crate::ported::message::ex_exitval;

/// Run `src` as a complete program with `input` as its stdin and return
/// everything it wrote to stdout.
///
/// `src` is sourced as `viml FILE` sources a file: a legacy script, every line
/// an Ex command, under `:verbose`. `input()`, `inputlist()` and `confirm()`
/// read their lines from `input`, and their prompts are part of the output.
/// `:echo` output is collected one message per line, each ending in `\n`.
///
/// Returns `Err` with the parse error, or with the last error message (`v:errmsg`)
/// when the script reported an error that was not caught — the condition on which
/// `viml FILE` exits 1. Error messages are also written to stderr, as the CLI
/// writes them. A panic in the interpreter is returned as `Err`. Nothing is
/// written to the process stdout, and the process stdin is not read.
pub fn run_program(src: &str, input: &str) -> Result<String, String> {
    let (src, input) = (src.to_owned(), input.to_owned());
    std::thread::Builder::new()
        .name("vimlrs-embed".into())
        .stack_size(crate::WORKER_STACK)
        .spawn(move || run_here(&src, &input))
        .map_err(|e| format!("vimlrs: cannot start the interpreter thread: {e}"))?
        .join()
        .unwrap_or_else(|panic| Err(panic_message(panic.as_ref())))
}

/// [`run_program`]'s body, on the fresh interpreter thread.
fn run_here(src: &str, input: &str) -> Result<String, String> {
    set_input(Some(input));
    set_silent_ex(true);
    capture_begin();
    let result = source_text(src);
    let output = capture_take();
    result.map_err(|e| e.to_string())?;
    if ex_exitval.with(|e| e.get()) != 0 {
        return Err(get_vim_var_str(VV_ERRMSG));
    }
    Ok(output)
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    format!("vimlrs: interpreter panicked: {what}")
}
