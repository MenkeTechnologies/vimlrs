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
/// `Once`-guarded and invoked lazily by the locale-dependent callers
/// (`item_compare()` before `strcoll`), so every entry point — the CLI, the
/// library, the test harnesses — gets the same locale state. The C's
/// gettext/bindtextdomain setup is not mirrored (no message translation).
pub fn init_locale() {
    INIT.call_once(|| unsafe {
        libc::setlocale(libc::LC_ALL, c"".as_ptr());
        // c: "Make sure strtod() uses a decimal point, not a comma."
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    });
}
