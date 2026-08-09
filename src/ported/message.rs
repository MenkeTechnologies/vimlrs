//! Port of the `emsg()` / `did_emsg` error path and the `msg_outtrans` display
//! path from `vendor/message.c`.
//!
//! The eval functions signal failure by calling `emsg()`, which sets the global
//! `did_emsg` and writes to the message area; callers checkpoint `did_emsg`
//! before an operation and compare afterward.
//!
//! `msg_multiline`/`msg_outtrans_len` are the other half: the transform every
//! message goes through on its way to the screen, which shows a control byte as
//! `^A` and a byte that is not part of a valid character as `<ff>`. `:echo`
//! reaches it through `ex_echo` → `msg_multiline` (`vendor/eval.c:6181`).
#![allow(non_upper_case_globals)]

use crate::ported::charset::{transchar_buf, transchar_byte_buf, vim_isprintc};
use crate::ported::mbyte::{utf_ptr2char, utfc_ptr2len};
use crate::vimstr::VimStr;
use std::cell::{Cell, RefCell};

thread_local! {
    /// C global `int did_emsg` (`message.c`): set/bumped on each error message.
    /// Read with `did_emsg.with(|d| d.get())`.
    ///
    /// RUST-PORT NOTE: a per-thread `Cell` counter stands in for the C global,
    /// since the eval engine runs single-threaded per evaluation.
    pub static did_emsg: Cell<u64> = const { Cell::new(0) };

    /// Every error raised, including the ones `:silent!` suppresses and the ones
    /// `assert_fails()` captures — unlike `did_emsg`, which those two paths skip
    /// and `:catch` resets.
    ///
    /// RUST-PORT NOTE: the C propagates "this command failed" as a `FAIL` return
    /// value up the call chain, which unwinds the command. This VM has no such
    /// unwind, so a statement asks "did an error happen while I evaluated my
    /// arguments?" by comparing this counter against a mark taken at its start
    /// (`VIML_ERR_MARK`). `did_emsg` cannot answer that: `:silent!` deliberately
    /// leaves it alone, and an erroring `silent! echo` would then print the
    /// recovered value that Vim never prints.
    pub static err_count: Cell<u64> = const { Cell::new(0) };

    /// C global `int msg_silent` (`globals.h`) — while non-zero, ordinary message
    /// output is suppressed. `:silent` raises it, which is why `silent echo 'x'`
    /// prints nothing. (`emsg_silent`, in `ex_eval`, is the error-message
    /// counterpart that `:silent!` raises.)
    pub static msg_silent: Cell<i32> = const { Cell::new(0) };

    /// When `Some`, each `emsg` text is captured here instead of printed —
    /// modelling `emsg_silent` + the saved error list that `assert_fails()`
    /// inspects in `message.c`/`testing.c`. `None` is the normal (print) path.
    static ERROR_CAPTURE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };

}

/// Begin capturing `emsg` text (suppressing stderr output), as `assert_fails()`
/// does while running the command under test.
pub fn capture_errors_begin() {
    ERROR_CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

/// Stop capturing and return the messages collected since `capture_errors_begin`.
pub fn capture_errors_take() -> Vec<String> {
    ERROR_CAPTURE.with(|c| c.borrow_mut().take().unwrap_or_default())
}

/// Port of `emsg(const char *s)` from `vendor/message.c:900`.
///
/// Record an error message and set `did_emsg`. C writes to the message area;
/// the faithful sink here is stderr (Vim prints errors to the user) — unless a
/// capture is active (`assert_fails`), in which case the text is collected and
/// not printed, like Vim's `emsg_silent` path.
pub fn emsg(s: &str) {
    // Counted first: this one tracks *every* error, whatever happens to it next.
    err_count.with(|d| d.set(d.get() + 1));
    // Observed second, before any of the paths that suppress or divert the message.
    // The observer lives in the synthesis zone (it has no C counterpart) and, unlike
    // a capture, changes nothing about what happens next.
    crate::fusevm_bridge::observe_error(s);
    let captured = ERROR_CAPTURE.with(|c| {
        if let Some(list) = c.borrow_mut().as_mut() {
            list.push(s.to_string());
            true
        } else {
            false
        }
    });
    if captured {
        did_emsg.with(|d| d.set(d.get() + 1));
        return;
    }
    // c: `emsg_silent` (raised by `:silent!`) returns before the message is shown
    // *or* counted. The command still fails, but the error neither prints nor marks
    // the script as having errored — which is why `silent! call Foo()` on a missing
    // function leaves a sourced script exiting 0.
    if crate::ported::ex_eval::emsg_silent.with(|e| e.get()) != 0 {
        return;
    }
    did_emsg.with(|d| d.set(d.get() + 1));
    // c: `emsg_multiline` → `cause_errthrow()` → the message becomes a *catchable
    // exception* when a `:try` is active, instead of being printed:
    //
    //   try | echo [1] . 'x' | catch | echo v:exception | endtry
    //   → "Vim(echo):E730: Using a List as a String"
    //
    // The pending-exception slot lives in the VM bridge (the synthesis zone), so the
    // throw itself does: `errthrow` returns true when it took ownership of the
    // message, in which case nothing is printed here — the exception carries it.
    // Outside a `:try` it declines and the error prints as before.
    if !crate::fusevm_bridge::errthrow(s) {
        // c: `msg_start()` — an error message begins on a fresh line, so a
        // half-written `:echon` line is terminated first rather than having the
        // error run onto the end of it.
        crate::fusevm_bridge::msg_flush_line();
        eprintln!("{s}");
    }
}

/// Port of `semsg(const char *fmt, ...)` from `vendor/message.c:913`.
///
/// The variadic C form is reduced to a pre-formatted message (callers format
/// with Rust's `format!` at the call site, then pass the result).
pub fn semsg(s: &str) {
    emsg(s);
}

// ─── the display path: msg_multiline / msg_outtrans_len ─────────────────────

/// Port of `msg_outtrans_len()` from `vendor/message.c:1866` — write `msgstr`
/// with unprintable characters translated.
///
/// Normal characters are passed through in runs (the C's `plain_start` window,
/// flushed by `msg_puts_len`); an unprintable character is replaced by its
/// `transchar` text. Which of the two `transchar` entry points applies is the
/// whole point: a byte that starts no valid UTF-8 sequence goes through
/// `transchar_byte_buf` and comes out `<ff>`, while a *decoded* character goes
/// through `transchar_buf` and comes out `<200b>` — or is left alone when
/// `vim_isprintc` accepts it.
///
/// RUST-PORT NOTE: the C returns the number of screen CELLS written, for callers
/// doing cursor arithmetic against a real grid. This port's sink is a byte
/// stream — `MSG_COL` in the bridge is a boolean "line is dirty" flag, not a
/// column — so there is no consumer for that count, and producing it would need
/// `utf_char2cells`, whose width data comes from utf8proc's property tables (not
/// vendored). The translation itself, which is what has an observable effect
/// here, is complete.
pub fn msg_outtrans_len(msgstr: &[u8], out: &mut VimStr) {
    let mut str_ = 0usize; // c: const char *str
    let mut plain_start = 0usize; // c: const char *plain_start
    while str_ < msgstr.len() {
        // c: utfc_ptr2len_len(str, len + 1) — bounded by the bytes that remain.
        // Passing the remaining slice is the same bound: this port's
        // `utfc_ptr2len` reads a past-the-end byte as the C's NUL, so an
        // incomplete trailing sequence answers 1 either way.
        let mb_l = utfc_ptr2len(&msgstr[str_..]) as usize;
        if mb_l > 1 {
            let c = utf_ptr2char(&msgstr[str_..]);
            if vim_isprintc(c) {
                // c: printable multi-byte char: count the cells (see the note
                // above) and leave the bytes in the plain run.
            } else {
                // c: unprintable multi-byte char: print the printable chars so
                // far and the translation of the unprintable char.
                if str_ > plain_start {
                    out.push_bytes(&msgstr[plain_start..str_]);
                }
                plain_start = str_ + mb_l;
                out.push_bytes(&transchar_buf(c));
            }
            str_ += mb_l;
        } else {
            let s = transchar_byte_buf(msgstr[str_] as i32);
            if s.len() > 1 {
                // c: `if (s[1] != NUL)` — an unprintable byte translates to more
                // than one character; a printable one translates to itself.
                if str_ > plain_start {
                    out.push_bytes(&msgstr[plain_start..str_]);
                }
                plain_start = str_ + 1;
                out.push_bytes(&s);
            }
            str_ += 1;
        }
    }
    // c: print the printable chars at the end (or emit empty string).
    if str_ > plain_start || plain_start == 0 {
        out.push_bytes(&msgstr[plain_start..str_]);
    }
}

/// Port of `msg_outtrans()` from `vendor/message.c:1845` — [`msg_outtrans_len`]
/// over a whole NUL-terminated string.
pub fn msg_outtrans(s: &[u8], out: &mut VimStr) {
    msg_outtrans_len(s, out);
}

/// Port of `msg_multiline()` from `vendor/message.c:279` — like
/// [`msg_outtrans_len`], but `\n`, TAB, `\r` and BELL are delimiters rather than
/// text: the run before each is translated, then the delimiter itself is written
/// RAW (`msg_putchar_hl`). That is why `echo "a\tb"` keeps a real tab while
/// `strtrans("a\tb")` — which does not go through here — is `a^Ib`.
///
/// RUST-PORT NOTE: BELL does not reach the output at all; the C calls
/// `vim_beep()` instead of writing it. `hl_id`/`hist`/`need_clear` are the
/// highlight id, the `:messages` history and the "still need to clear to end of
/// screen" flag — three things a stdout sink does not have.
pub fn msg_multiline(str_: &[u8], out: &mut VimStr) {
    const TAB: u8 = 0x09;
    const BELL: u8 = 0x07;
    let mut s = 0usize;
    let mut chunk = 0usize;
    while s < str_.len() {
        let b = str_[s];
        if b == b'\n' || b == TAB || b == b'\r' || b == BELL {
            // c: print all chars before the delimiter
            msg_outtrans_len(&str_[chunk..s], out);
            if b == BELL {
                // c: vim_beep(kOptBoFlagShell) — the byte is consumed, not shown.
            } else {
                out.push_bytes(&[b]);
            }
            chunk = s + 1;
        }
        s += 1;
    }
    // c: print the remainder or emit empty event if entire message is empty.
    if chunk < str_.len() || chunk == 0 {
        msg_outtrans_len(&str_[chunk..], out);
    }
}
