//! Port of `src/nvim/os/lang.c` (subset: `init_locale`, `get_locale_val`,
//! `set_lang_var`).

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

/// Port of `get_locale_val()` from `os/lang.c` (vim `locale.c:20`) — the name
/// of locale category `what` as the C library holds it (`setlocale(what,
/// NULL)`). The C's MS-Windows `LC_COLLATE=<name>;…` trimming has no
/// counterpart on the platforms this crate builds for.
pub fn get_locale_val(what: libc::c_int) -> String {
    // SAFETY: a NULL locale only queries; the returned pointer is read before
    // any other setlocale() call can replace it.
    unsafe {
        let loc = libc::setlocale(what, std::ptr::null());
        if loc.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(loc).to_string_lossy().into_owned()
    }
}

/// Port of `set_lang_var()` from `os/lang.c` (vim `locale.c:181`) — seed
/// `v:ctype`, `v:lang`, `v:lc_time` and `v:collate` from the locale
/// [`init_locale`] adopted. vim's build takes `v:lang` from `LC_MESSAGES`
/// (`HAVE_GET_LOCALE_VAL && LC_MESSAGES`), so under `LC_ALL=C.UTF-8` all four
/// read `C.UTF-8`.
pub fn set_lang_var() {
    use crate::ported::eval::vars::{set_vim_var_string, vv};
    set_vim_var_string(vv::VV_CTYPE, &get_locale_val(libc::LC_CTYPE));
    set_vim_var_string(vv::VV_LANG, &get_locale_val(libc::LC_MESSAGES));
    set_vim_var_string(vv::VV_LC_TIME, &get_locale_val(libc::LC_TIME));
    set_vim_var_string(vv::VV_COLLATE, &get_locale_val(libc::LC_COLLATE));
}
