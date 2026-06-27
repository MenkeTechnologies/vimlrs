//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. fusevm bytecode listing to stdout when
//! `vimlrs` is invoked with `--disasm` (ported from zshrs's `fusevm_disasm.rs`).
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use fusevm::Chunk;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Set from the CLI after arg parsing.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// If `--disasm` was passed, print a listing to stdout before `VM::run`.
pub fn maybe_print_stdout(context: &str, chunk: &Chunk) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut buf = String::new();
    let _ = writeln!(buf, "; vimlrs fusevm — {context}");
    append_chunk(&mut buf, chunk, "");
    print!("{buf}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

fn append_chunk(out: &mut String, chunk: &Chunk, indent: &str) {
    if !chunk.source.is_empty() {
        let _ = writeln!(out, "{indent}; source: {}", chunk.source);
    }
    for (i, n) in chunk.names.iter().enumerate() {
        let _ = writeln!(out, "{indent}; name[{i}] = {n}");
    }
    if !chunk.sub_entries.is_empty() {
        let _ = writeln!(out, "{indent}; sub_entries:");
        for (ni, ip) in &chunk.sub_entries {
            let name = chunk
                .names
                .get(*ni as usize)
                .map(String::as_str)
                .unwrap_or("?");
            let _ = writeln!(out, "{indent};   {name} @ {ip}");
        }
    }
    for (i, op) in chunk.ops.iter().enumerate() {
        let line = chunk.lines.get(i).copied().unwrap_or(0);
        let _ = writeln!(out, "{indent}{i:04} {line:>5}     {op:?}");
    }
    for (si, sub) in chunk.sub_chunks.iter().enumerate() {
        let _ = writeln!(out, "{indent}; --- sub_chunk[{si}] ---");
        let sub_indent = format!("{indent}  ");
        append_chunk(out, sub, &sub_indent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chunk_writes_nothing() {
        let mut buf = String::new();
        append_chunk(&mut buf, &Chunk::default(), "");
        assert_eq!(buf, "");
    }

    #[test]
    fn ops_numbered_four_digit_width() {
        let mut c = Chunk::default();
        c.ops = vec![fusevm::Op::Nop, fusevm::Op::Nop];
        c.lines = vec![1, 2];
        let mut buf = String::new();
        append_chunk(&mut buf, &c, "");
        assert!(buf.contains("0000     1     Nop"));
        assert!(buf.contains("0001     2     Nop"));
    }

    #[test]
    fn set_enabled_toggles() {
        let prev = ENABLED.load(Ordering::Relaxed);
        set_enabled(true);
        assert!(ENABLED.load(Ordering::Relaxed));
        set_enabled(false);
        assert!(!ENABLED.load(Ordering::Relaxed));
        ENABLED.store(prev, Ordering::Relaxed);
    }
}
