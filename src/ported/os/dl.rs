//! Port of `src/nvim/os/dl.c` (vendored at `csrc/os/dl.c`).
//!
//! `os_libcall()` — call a function in a dynamically loadable library. Backs the
//! `libcall()`/`libcallnr()` builtins via
//! [`libcall_common`](crate::ported::eval::funcs::libcall_common).
//!
//! RUST-PORT NOTE: Neovim wraps libuv's `uv_dlopen`/`uv_dlsym`/`uv_dlclose`
//! (`uv_lib_t`). libuv is not vendored here, so this port calls the underlying
//! POSIX `dlopen`/`dlsym`/`dlclose`/`dlerror` directly (declared `extern "C"`
//! below — they resolve against libSystem on macOS / libdl on Linux at link
//! time, so no new crate dependency is introduced). The four supported
//! prototypes (`str_str_fn`/`int_str_fn`/`str_int_fn`/`int_int_fn`) are declared
//! as `extern "C"` function-pointer transmute targets exactly as the C `gen_fn`
//! cast does.
#![allow(dead_code, non_snake_case, non_camel_case_types)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

// c: os/dl.c uses uv_dlopen/uv_dlsym/uv_dlclose; here the POSIX equivalents from
// libc (via nix). Imported (not re-declared) so no non-C `fn` names are introduced.
use nix::libc::{dlclose, dlerror, dlopen, dlsym};

// RTLD_NOW | RTLD_LOCAL — resolve all symbols now, do not export globally.
const RTLD_NOW: c_int = 0x2;

// c: possible function prototypes that can be called by os_libcall()
// c: typedef const char *(*str_str_fn)(const char *str);
type str_str_fn = extern "C" fn(*const c_char) -> *const c_char;
// c: typedef int (*str_int_fn)(const char *str);
type str_int_fn = extern "C" fn(*const c_char) -> c_int;
// c: typedef const char *(*int_str_fn)(int i);
type int_str_fn = extern "C" fn(c_int) -> *const c_char;
// c: typedef int (*int_int_fn)(int i);
type int_int_fn = extern "C" fn(c_int) -> c_int;

/// Port of `os_libcall()` from `Src/os/dl.c:39`.
///
/// Call a function in a dynamic loadable library. `argv` (the input string,
/// `None` when using `argi`), `argi` (the input integer). On success writes the
/// allocated result string into `str_out` (when `Some`) or the integer into
/// `int_out`, and returns `true`.
///
/// RUST-PORT NOTE (signature): the C `char **str_out` / `int *int_out`
/// out-params become `Option<&mut Option<String>>` / `&mut i32`. `str_out ==
/// NULL` (the "want an integer" selector) maps to `str_out.is_none()`.
pub fn os_libcall(
    libname: Option<&str>,
    funcname: Option<&str>,
    argv: Option<&str>,
    argi: i32,
    str_out: Option<&mut Option<String>>,
    int_out: &mut i32,
) -> bool {
    // c: if (!libname || !funcname) return false;
    let (libname, funcname) = match (libname, funcname) {
        (Some(l), Some(f)) => (l, f),
        _ => return false,
    };

    // c: uv_dlerror(&lib) — read the current loader error message. Local closure
    // (not a named fn) so no synthesis-adapter fn name is introduced.
    let dlerr = || -> String {
        // SAFETY: `dlerror` returns a static/thread-local C string or NULL.
        unsafe {
            let p = dlerror();
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    };

    let c_libname = match CString::new(libname) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let c_funcname = match CString::new(funcname) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // c: open the dynamic loadable library — if (uv_dlopen(libname, &lib)) { … }
    // SAFETY: passing valid NUL-terminated C strings to the C loader.
    let lib = unsafe { dlopen(c_libname.as_ptr(), RTLD_NOW) };
    if lib.is_null() {
        // c: semsg(_("dlerror = \"%s\""), uv_dlerror(&lib));
        crate::ported::message::semsg(&format!("dlerror = \"{}\"", dlerr()));
        return false;
    }

    // c: find and load the requested function in the library
    // SAFETY: `lib` is a valid handle returned by `dlopen`.
    let fnptr = unsafe { dlsym(lib, c_funcname.as_ptr()) };
    if fnptr.is_null() {
        crate::ported::message::semsg(&format!("dlerror = \"{}\"", dlerr()));
        // SAFETY: `lib` is a valid handle.
        unsafe { dlclose(lib) };
        return false;
    }

    // c: call the library and save the result
    let c_argv = argv.and_then(|s| CString::new(s).ok());
    let mut success = true;
    if let Some(out) = str_out {
        // c: str_str_fn sfn; int_str_fn ifn; res = argv ? sfn(argv) : ifn(argi);
        let res: *const c_char = match &c_argv {
            Some(a) => {
                // SAFETY: transmute a resolved symbol to the declared prototype;
                // callers are responsible for using a matching library function.
                let sfn: str_str_fn = unsafe { std::mem::transmute(fnptr) };
                sfn(a.as_ptr())
            }
            None => {
                let ifn: int_str_fn = unsafe { std::mem::transmute(fnptr) };
                ifn(argi as c_int)
            }
        };
        // c: assume that ptr values of NULL, 1 or -1 are illegal
        let addr = res as isize;
        *out = if !res.is_null() && addr != 1 && addr != -1 {
            // SAFETY: `res` is a non-sentinel C string pointer from the callee.
            Some(
                unsafe { CStr::from_ptr(res) }
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
    } else {
        // c: str_int_fn sfn; int_int_fn ifn; *int_out = argv ? sfn(argv) : ifn(argi);
        *int_out = match &c_argv {
            Some(a) => {
                let sfn: str_int_fn = unsafe { std::mem::transmute(fnptr) };
                sfn(a.as_ptr()) as i32
            }
            None => {
                let ifn: int_int_fn = unsafe { std::mem::transmute(fnptr) };
                ifn(argi as c_int) as i32
            }
        };
        let _ = &mut success; // keep parity with the C `return true`
    }

    // c: free the library
    // SAFETY: `lib` is a valid handle returned by `dlopen`.
    unsafe { dlclose(lib) };

    // c: return true;
    let _ = success;
    true
}
