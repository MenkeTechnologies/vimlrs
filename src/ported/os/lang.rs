//! Port of `src/nvim/os/lang.c` (subset: `init_locale`).

use std::sync::Once;

static INIT: Once = Once::new();

/// Port of `init_locale()` from `Src/os/lang.c` — adopt the environment's
/// locale (`setlocale(LC_ALL, "")`), then force `LC_NUMERIC` back to `"C"` so
/// `strtod()` always parses a `.` decimal point. This is what makes
/// `strcoll()` (the `sort()` `'l'` flag) collate by the user's locale instead
/// of byte order.
///
/// RUST-PORT NOTE: the C calls this once at startup (`main.c`); here it is
/// `Once`-guarded and called from the once-per-thread startup block in
/// [`crate::fusevm_bridge::install`], next to `eval_init()`, so every entry
/// point — the CLI, the library, the test harnesses — reaches it before
/// anything is evaluated. The C's gettext/bindtextdomain setup is not mirrored
/// (no message translation), so vim's translated diagnostics under a non-English
/// `LC_MESSAGES` have no counterpart here.
///
/// It was previously called ONLY from `item_compare()` before `strcoll`, i.e.
/// only from `sort(…, 'l')`. Every other locale-dependent libc call — chiefly
/// `strftime()` and `strptime()` — therefore ran in the process's default `"C"`
/// locale until a locale-collating sort happened to occur, which made a
/// `strftime()` result depend on what had run before it in the same script.
/// The `Once` is what keeps the state stable once set; the call site is what
/// decides when it is set, and lazily was the wrong answer.
pub fn init_locale() {
    INIT.call_once(|| unsafe {
        libc::setlocale(libc::LC_ALL, c"".as_ptr());
        // c: "Make sure strtod() uses a decimal point, not a comma."
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    });
}
