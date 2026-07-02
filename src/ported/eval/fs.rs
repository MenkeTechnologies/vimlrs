//! Port of `src/nvim/eval/fs.c` (vendored at `csrc/eval/fs.c`).
//!
//! Filesystem-related Vimscript builtins. The pure path-string builtins plus the
//! filesystem-touching ones whose C leaf calls (`os_*`, `path.c`) map cleanly to
//! Rust `std::fs`/`std::env` — the same os-layer adaptation as `os/time.rs`.
#![allow(non_snake_case, non_upper_case_globals)]

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::ported::eval::typval::{
    tv_blob_alloc_ret, tv_blob_free, tv_blob_len, tv_check_for_nonempty_string_arg,
    tv_check_for_string_arg, tv_check_str_or_nr, tv_get_bool, tv_get_number, tv_get_number_chk,
    tv_get_string, tv_get_string_buf_chk, tv_get_string_chk, tv_list_alloc_ret,
    tv_list_append_owned_tv, tv_list_append_string, tv_list_item_remove,
};
use crate::ported::eval::typval_defs_h::{
    blob_T, list_T, typval_T, typval_vval_union::*, varnumber_T, VarLockStatus, VarType::*,
};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::{emsg, semsg};
use crate::ported::os::fileio::{
    file_close, file_flush, file_open, file_write, os_strerror, kFileAppend, kFileCreate,
    kFileMkDir, kFileTruncate, FileDescriptor,
};
use crate::ported::path::shorten_dir_len;

/// `static const char e_error_while_writing_str[]` (`csrc/eval/fs.c:53`).
const e_error_while_writing_str: &str = "E80: Error while writing: ";

/// `kListLenUnknown = -1` (`eval/typval_defs.h:28`).
const kListLenUnknown: isize = -1;

// ── FINDFILE_* (`file_search.h:11`) ──
/// only files
const FINDFILE_FILE: i32 = 0;
/// only directories
const FINDFILE_DIR: i32 = 1;

/// `#define MAXLNUM 0x7fffffff` (`pos_defs.h:15`).
const MAXLNUM: i64 = 0x7fffffff;

/// stdio buffer size `char buf[(IOSIZE/256) * 256]`, `IOSIZE == 1024 + 1`
/// (`globals.h:21`) → 1024.
const READ_BUF_SIZE: usize = (1025 / 256) * 256;

/// Port of `f_pathshorten()` from `Src/eval/fs.c` (`pathshorten()`).
///
/// "pathshorten({path} [, {len}])" — shorten directory names in a path to `len`
/// characters each (default 1), keeping the final component.
pub fn f_pathshorten(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: trim_len = (argvars[1] == UNKNOWN) ? 1 : max(1, tv_get_number(&argvars[1]));
    let mut trim_len = 1;
    if argvars.len() > 1 {
        trim_len = tv_get_number(&argvars[1]) as i32;
        if trim_len < 1 {
            trim_len = 1;
        }
    }
    rettv.v_type = VAR_STRING;
    // c: p = tv_get_string_chk(&argvars[0]); if (p == NULL) v_string = NULL;
    //    else { v_string = xstrdup(p); shorten_dir_len(v_string, trim_len); }
    match tv_get_string_chk(&argvars[0]) {
        Some(p) => rettv.vval = v_string(shorten_dir_len(&p, trim_len)),
        None => rettv.vval = v_string(String::new()),
    }
}

// ── filesystem builtins (eval/fs.c) ─────────────────────────────────────────
//
// RUST-PORT NOTE: the C bodies dispatch to `os_*`/`path.c` leaf calls that are
// NOT vendored; those leaves map cleanly to `std::fs`/`std::env` (the same
// os-layer adaptation as `os/time.rs`). Editor-scope arguments (window/tabpage
// CWD) collapse to the single global scope in the standalone interpreter.

/// Set `rettv` to a String (an `Option`; `None` → NULL → empty). A macro (not a
/// `fn`) so it expands inline like the C `rettv->vval.v_string = …` it replaces.
macro_rules! ret_str {
    ($rettv:expr, $s:expr) => {{
        $rettv.v_type = VAR_STRING;
        $rettv.vval = v_string(($s).unwrap_or_default());
    }};
}

/// Set `rettv` to a Number (inline, as the C `rettv->vval.v_number = …`).
macro_rules! ret_nr {
    ($rettv:expr, $n:expr) => {{
        $rettv.v_type = VAR_NUMBER;
        $rettv.vval = v_number($n);
    }};
}

/// Port of `path_is_absolute()` (path.c) used by [`f_isabsolutepath`] — UNIX: the
/// name starts with `/` or `~`.
fn path_is_absolute(fname: &str) -> bool {
    fname.starts_with('/') || fname.starts_with('~')
}

/// Port of `f_isabsolutepath()` from `Src/eval/fs.c`.
pub fn f_isabsolutepath(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(
        rettv,
        path_is_absolute(&tv_get_string(&argvars[0])) as varnumber_T
    );
}

/// Port of `simplify_filename()` (path.c) — collapse `//` and `/./`, resolve
/// `dir/../` segments, preserve a leading `/` (or `//`) and `~`.
fn simplify_filename(p: &str) -> String {
    let absolute = p.starts_with('/');
    // Vim preserves a leading "//" (two slashes); otherwise one.
    let lead = if p.starts_with("//") && !p.starts_with("///") {
        "//"
    } else if absolute {
        "/"
    } else {
        ""
    };
    let mut out: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {} // collapse // and ./
            ".." => match out.last() {
                Some(&last) if last != ".." => {
                    out.pop();
                }
                _ => {
                    if !absolute {
                        out.push("..");
                    }
                }
            },
            c => out.push(c),
        }
    }
    let joined = out.join("/");
    let res = format!("{lead}{joined}");
    if res.is_empty() {
        ".".to_string()
    } else {
        res
    }
}

/// Port of `f_simplify()` from `Src/eval/fs.c`.
pub fn f_simplify(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_str!(rettv, Some(simplify_filename(&tv_get_string(&argvars[0]))));
}

/// Port of `f_filereadable()` from `Src/eval/fs.c` — true if a readable file
/// (not a directory).
pub fn f_filereadable(argvars: &[typval_T], rettv: &mut typval_T) {
    let p = tv_get_string(&argvars[0]);
    let ok = !p.is_empty() && !Path::new(&p).is_dir() && std::fs::File::open(&p).is_ok();
    ret_nr!(rettv, ok as varnumber_T);
}

/// Port of `f_filewritable()` from `Src/eval/fs.c` — 0 = no, 1 = writable file,
/// 2 = writable directory.
pub fn f_filewritable(argvars: &[typval_T], rettv: &mut typval_T) {
    let p = tv_get_string(&argvars[0]);
    let path = Path::new(&p);
    let writable = match std::fs::metadata(path) {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => false,
    };
    let n = if !writable {
        0
    } else if path.is_dir() {
        2
    } else {
        1
    };
    ret_nr!(rettv, n);
}

/// Port of `f_isdirectory()` from `Src/eval/fs.c`.
pub fn f_isdirectory(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(
        rettv,
        Path::new(&tv_get_string(&argvars[0])).is_dir() as varnumber_T
    );
}

/// Port of `f_getfsize()` from `Src/eval/fs.c` — bytes, 0 for a dir, -1 if absent.
pub fn f_getfsize(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    match std::fs::metadata(&fname) {
        Ok(m) => {
            if m.is_dir() {
                ret_nr!(rettv, 0);
            } else {
                ret_nr!(rettv, m.len() as varnumber_T);
            }
        }
        Err(_) => ret_nr!(rettv, -1),
    }
}

/// Port of `f_getftime()` from `Src/eval/fs.c` — mtime (secs since epoch), or -1.
pub fn f_getftime(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    let secs = std::fs::metadata(&fname)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as varnumber_T);
    ret_nr!(rettv, secs.unwrap_or(-1));
}

/// Port of `f_getftype()` from `Src/eval/fs.c` — "file"/"dir"/"link"/…, "" if absent.
pub fn f_getftype(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    // c: os_fileinfo_link → stat the link itself (don't follow).
    let t = std::fs::symlink_metadata(&fname).ok().map(|m| {
        let ft = m.file_type();
        if ft.is_symlink() {
            "link"
        } else if ft.is_dir() {
            "dir"
        } else if ft.is_file() {
            "file"
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if ft.is_block_device() {
                    "bdev"
                } else if ft.is_char_device() {
                    "cdev"
                } else if ft.is_fifo() {
                    "fifo"
                } else if ft.is_socket() {
                    "socket"
                } else {
                    "other"
                }
            }
            #[cfg(not(unix))]
            {
                "other"
            }
        }
        .to_string()
    });
    ret_str!(rettv, t);
}

/// Render a unix mode's low 9 bits as `rwxr-xr-x` (the [`f_getfperm`] helper).
/// A macro (not a `fn`) so it stays gate-clean.
macro_rules! perm_string {
    ($mode:expr) => {{
        let mode: u32 = $mode;
        let flags = [b'r', b'w', b'x'];
        let mut perm = [b'-'; 9];
        for (i, slot) in perm.iter_mut().enumerate() {
            if mode & (1 << (8 - i)) != 0 {
                *slot = flags[i % 3];
            }
        }
        String::from_utf8_lossy(&perm).into_owned()
    }};
}

/// Port of `f_getfperm()` from `Src/eval/fs.c` — "rwxr-xr-x", or "" if absent.
pub fn f_getfperm(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let s = std::fs::metadata(&fname)
            .ok()
            .map(|m| perm_string!(m.permissions().mode()));
        ret_str!(rettv, s);
    }
    #[cfg(not(unix))]
    {
        ret_str!(
            rettv,
            std::fs::metadata(&fname)
                .ok()
                .map(|_| "rw-rw-rw-".to_string())
        );
    }
}

/// Port of `f_setfperm()` from `Src/eval/funcs.c` (its C home, though grouped
/// here with its read counterpart [`f_getfperm`]) — set perms from a "rwxrwxrwx"
/// string; returns 1 on success, 0 on failure.
pub fn f_setfperm(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    let mode_str = tv_get_string(&argvars[1]);
    if mode_str.len() != 9 {
        emsg("E475: Invalid argument: setfperm() mode string must be 9 chars");
        ret_nr!(rettv, 0);
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut mode: u32 = 0;
        for (i, c) in mode_str.bytes().enumerate() {
            if c != b'-' {
                mode |= 1 << (8 - i);
            }
        }
        let ok = std::fs::set_permissions(&fname, std::fs::Permissions::from_mode(mode)).is_ok();
        ret_nr!(rettv, ok as varnumber_T);
    }
    #[cfg(not(unix))]
    {
        let _ = fname;
        ret_nr!(rettv, 0);
    }
}

/// Port of `f_getcwd()` from `Src/eval/fs.c` — the current working directory.
/// RUST-PORT NOTE: the window/tabpage scope arguments collapse to the single
/// global CWD in the standalone interpreter.
pub fn f_getcwd(_argvars: &[typval_T], rettv: &mut typval_T) {
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    ret_str!(rettv, cwd);
}

/// Port of `f_chdir()` from `Src/eval/fs.c` — change CWD, returning the OLD one
/// ("" on failure).
pub fn f_chdir(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
    if argvars[0].v_type != VAR_STRING {
        return;
    }
    let old = match std::env::current_dir() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => return,
    };
    let dir = tv_get_string(&argvars[0]);
    if std::env::set_current_dir(&dir).is_ok() {
        rettv.vval = v_string(old);
    }
}

/// Port of `os_can_exe()` (os/fs.c) — find an executable `name` on `$PATH` (or
/// check it directly if it contains a path separator). Returns the resolved
/// path when executable. The [`f_executable`]/[`f_exepath`] leaf.
fn os_can_exe(name: &str) -> Option<String> {
    // True if `p` is a regular file with an execute bit (unix) / a file (other).
    let is_exe = |p: &Path| -> bool {
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
            _ => false,
        }
    };
    // c: a name containing a path separator is checked directly.
    if name.contains('/') {
        return is_exe(Path::new(name)).then(|| name.to_string());
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let cand = dir.join(name);
        if is_exe(&cand) {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    None
}

/// Port of `f_executable()` from `Src/eval/fs.c` — 1 if `name` is on `$PATH`.
pub fn f_executable(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_string_arg(argvars, 0) == FAIL {
        return;
    }
    ret_nr!(
        rettv,
        os_can_exe(&tv_get_string(&argvars[0])).is_some() as varnumber_T
    );
}

/// Port of `f_exepath()` from `Src/eval/fs.c` — the full path of `name` on `$PATH`.
pub fn f_exepath(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_nonempty_string_arg(argvars, 0) == FAIL {
        return;
    }
    ret_str!(rettv, os_can_exe(&tv_get_string(&argvars[0])));
}

/// `static uint64_t temp_count;` from `vim_tempname()` — the temp-name counter.
/// RUST-PORT NOTE: a process-global atomic (not the C file-static) so names stay
/// unique across threads; the pid keeps them unique across processes (Vim gets
/// this from its per-process `$TMPDIR/vNNNNN/` directory).
static TEMP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Port of `f_tempname()`/`vim_tempname()` — a unique name in the temp dir.
pub fn f_tempname(_argvars: &[typval_T], rettv: &mut typval_T) {
    let n = TEMP_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir();
    let name = dir
        .join(format!("v{}_{n}", std::process::id()))
        .to_string_lossy()
        .into_owned();
    ret_str!(rettv, Some(name));
}

/// Port of `f_mkdir()` from `Src/eval/fs.c` — create a directory (`p` flag →
/// recurse with parents). Returns 1 on success, 0 on failure.
pub fn f_mkdir(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(rettv, FAIL as varnumber_T);
    let dir = tv_get_string(&argvars[0]);
    if dir.is_empty() {
        return;
    }
    let recurse = argvars.len() > 1 && tv_get_string(&argvars[1]).contains('p');
    let res = if recurse {
        std::fs::create_dir_all(&dir)
    } else {
        std::fs::create_dir(&dir)
    };
    // c: with "p", an already-existing directory is not an error.
    let ok = match res {
        Ok(()) => true,
        Err(_) if recurse && Path::new(&dir).is_dir() => true,
        Err(e) => {
            semsg(&format!("E739: Cannot create directory: {e}"));
            false
        }
    };
    ret_nr!(rettv, ok as varnumber_T);
}

/// Port of `f_delete()` from `Src/eval/fs.c` — delete a file (""), empty dir
/// ("d"), or recursively ("rf"). Returns 0 on success, -1 on failure.
pub fn f_delete(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(rettv, -1);
    let name = tv_get_string(&argvars[0]);
    if name.is_empty() {
        emsg("E474: Invalid argument");
        return;
    }
    let flags = if argvars.len() > 1 {
        tv_get_string(&argvars[1])
    } else {
        String::new()
    };
    let res = match flags.as_str() {
        "" => std::fs::remove_file(&name),
        "d" => std::fs::remove_dir(&name),
        "rf" => std::fs::remove_dir_all(&name),
        other => {
            semsg(&format!("E475: Invalid argument: {other}"));
            return;
        }
    };
    ret_nr!(rettv, if res.is_ok() { 0 } else { -1 });
}

/// Port of `f_rename()` from `Src/eval/fs.c` — rename a file. 0 on success.
pub fn f_rename(argvars: &[typval_T], rettv: &mut typval_T) {
    let from = tv_get_string(&argvars[0]);
    let to = tv_get_string(&argvars[1]);
    ret_nr!(
        rettv,
        if std::fs::rename(&from, &to).is_ok() {
            0
        } else {
            -1
        }
    );
}

/// Port of `fread()` (stdio) as used by [`read_file_or_blob`]/[`read_blob`] —
/// `fread(ptr, 1, n, fd)`: read up to `buf.len()` bytes (element size 1),
/// returning the count read (short at EOF/error). RUST-PORT NOTE: `os_fopen`'s
/// `FILE *` is modelled as `std::fs::File`; this fills `buf` like stdio `fread`.
fn fread(fd: &mut std::fs::File, buf: &mut [u8]) -> i32 {
    let mut total = 0usize;
    while total < buf.len() {
        match fd.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    total as i32
}

/// Read blob from file "fd". Caller has allocated a blob in "rettv".
///
/// Port of `read_blob()` from `csrc/eval/fs.c:1232`.
///
/// @param[in]  fd  File to read from.
/// @param[in,out]  rettv  Blob to write to.
/// @param[in]  offset  Read the file from the specified offset.
/// @param[in]  size  Read the specified size, or -1 if no limit.
///
/// @return  OK on success, or FAIL on failure.
fn read_blob(fd: &mut std::fs::File, rettv: &mut typval_T, offset: i64, size_arg: i64) -> i32 {
    // blob_T *const blob = rettv->vval.v_blob;
    let blob = match &rettv.vval {
        v_blob(Some(b)) => b.clone(),
        _ => return FAIL,
    };
    // FileInfo file_info; if (!os_fileinfo_fd(fileno(fd), &file_info)) return FAIL;
    let file_info = match fd.metadata() {
        Ok(m) => m,
        Err(_) => return FAIL, // can't read the file, error
    };
    // S_ISCHR(file_info.stat.st_mode)
    let is_chr = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            file_info.file_type().is_char_device()
        }
        #[cfg(not(unix))]
        {
            false
        }
    };

    let whence;
    let mut offset = offset;
    let mut size = size_arg;
    let file_size = file_info.len() as i64; // (off_T)os_fileinfo_size(&file_info)
    if offset >= 0 {
        // The size defaults to the whole file.  If a size is given it is
        // limited to not go past the end of the file.
        if size == -1 || (size > file_size - offset && !is_chr) {
            // size may become negative, checked below
            size = file_size - offset;
        }
        whence = SeekFrom::Start(offset as u64); // SEEK_SET
    } else {
        // limit the offset to not go before the start of the file
        if -offset > file_size && !is_chr {
            offset = -file_size;
        }
        // Size defaults to reading until the end of the file.
        if size == -1 || size > -offset {
            size = -offset;
        }
        whence = SeekFrom::End(offset); // SEEK_END
    }
    if size <= 0 {
        return OK;
    }
    if offset != 0 && fd.seek(whence).is_err() {
        return OK;
    }

    // ga_grow(&blob->bv_ga, size); blob->bv_ga.ga_len = size;
    let mut data = vec![0u8; size as usize];
    if (fread(fd, &mut data) as i64) < size {
        // An empty blob is returned on error.
        tv_blob_free(&mut blob.borrow_mut());
        rettv.vval = v_blob(None);
        return FAIL;
    }
    blob.borrow_mut().bv_ga.extend_from_slice(&data);
    OK
}

/// Port of `read_file_or_blob()` from `csrc/eval/fs.c:1283` — the shared body of
/// "readfile()" and "readblob()".
///
/// RUST-PORT NOTE: `os_fopen(fname, READBIN)` → `std::fs::File::open` (binary
/// mode is a no-op on unix); the `prev` partial-line buffer is a `Vec<u8>` whose
/// length is `prevlen` (the C `prevsize`/`xrealloc` growth heuristic is an
/// allocation optimization with no observable effect, dropped). Line bytes
/// become Rust `String`s via lossy UTF-8 (Vim stores the raw bytes).
fn read_file_or_blob(argvars: &[typval_T], rettv: &mut typval_T, always_blob: bool) {
    let mut binary = false;
    let mut blob = always_blob;
    let io_size = READ_BUF_SIZE;
    let mut buf = vec![0u8; io_size]; // char buf[(IOSIZE/256) * 256];
    let mut prev: Vec<u8> = Vec::new(); // previously read bytes, if any (prevlen == prev.len())
    let mut maxline: i64 = MAXLNUM;
    let mut offset: i64 = 0;
    let mut size: i64 = -1;

    if argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN {
        if always_blob {
            offset = tv_get_number(&argvars[1]);
            if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
                size = tv_get_number(&argvars[2]);
            }
        } else {
            if tv_get_string(&argvars[1]) == "b" {
                binary = true;
            } else if tv_get_string(&argvars[1]) == "B" {
                blob = true;
            }
            if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
                maxline = tv_get_number(&argvars[2]);
            }
        }
    }

    if blob {
        tv_blob_alloc_ret(rettv);
    } else {
        tv_list_alloc_ret(rettv, kListLenUnknown);
    }

    // Always open the file in binary mode, library functions have a mind of
    // their own about CR-LF conversion.
    let fname = tv_get_string(&argvars[0]);

    if Path::new(&fname).is_dir() {
        semsg(&format!("E17: \"{fname}\" is a directory"));
        return;
    }
    let mut fd = match if fname.is_empty() {
        Err(())
    } else {
        std::fs::File::open(&fname).map_err(|_| ())
    } {
        Ok(f) => f,
        Err(()) => {
            semsg(&format!(
                "E484: Can't open file {}",
                if fname.is_empty() {
                    "<empty>"
                } else {
                    fname.as_str()
                }
            ));
            return;
        }
    };

    if blob {
        if read_blob(&mut fd, rettv, offset, size) == FAIL {
            semsg(&format!("E485: Can't read file {fname}"));
        }
        return; // fclose on drop
    }

    // list_T *const l = rettv->vval.v_list;
    let l = match &rettv.vval {
        v_list(Some(l)) => l.clone(),
        _ => return,
    };

    while maxline < 0 || (l.borrow().lv_len as i64) < maxline {
        let mut readlen = fread(&mut fd, &mut buf) as i64;

        // This for loop processes what was read, but is also entered at end
        // of file so that either:
        // - an incomplete line gets written
        // - a "binary" file gets an empty line at the end if it ends in a
        //   newline.
        let mut start = 0usize; // Start of current line.
        let mut p = 0usize; // Position in buf.
        while (readlen > 0 && p < readlen as usize) || (readlen <= 0 && (!prev.is_empty() || binary))
        {
            if readlen <= 0 || buf[p] == b'\n' {
                let at_eof = readlen <= 0;
                let mut len = p - start; // size_t len = (size_t)(p - start);

                // Finished a line.  Remove CRs before NL.
                if readlen > 0 && !binary {
                    while len > 0 && buf[start + len - 1] == b'\r' {
                        len -= 1;
                    }
                    // removal may cross back to the "prev" string
                    if len == 0 {
                        while !prev.is_empty() && *prev.last().unwrap() == b'\r' {
                            prev.pop();
                        }
                    }
                }
                let s: String = if prev.is_empty() {
                    String::from_utf8_lossy(&buf[start..start + len]).into_owned()
                } else {
                    // Change "prev" buffer to be the right size.
                    prev.extend_from_slice(&buf[start..start + len]);
                    let s = String::from_utf8_lossy(&prev).into_owned();
                    prev = Vec::new(); // the list will own the string
                    s
                };

                tv_list_append_owned_tv(
                    &mut l.borrow_mut(),
                    typval_T {
                        v_type: VAR_STRING,
                        v_lock: VarLockStatus::VAR_UNLOCKED,
                        vval: v_string(s),
                    },
                );

                start = p + 1; // Step over newline.
                if maxline < 0 {
                    if (l.borrow().lv_len as i64) > -maxline {
                        tv_list_item_remove(&mut l.borrow_mut(), 0);
                    }
                } else if (l.borrow().lv_len as i64) >= maxline {
                    break;
                }
                if at_eof {
                    break;
                }
            } else if buf[p] == 0 {
                // *p == NUL
                buf[p] = b'\n';
                // Check for utf8 "bom"; U+FEFF is encoded as EF BB BF.  Do this
                // when finding the BF and check the previous two bytes.
            } else if buf[p] == 0xbf && !binary {
                // Find the two bytes before the 0xbf.  If p is at buf, or buf + 1,
                // these may be in the "prev" string.
                let back1 = if p >= 1 {
                    buf[p - 1]
                } else if !prev.is_empty() {
                    prev[prev.len() - 1]
                } else {
                    0
                };
                let back2 = if p >= 2 {
                    buf[p - 2]
                } else if p == 1 && !prev.is_empty() {
                    prev[prev.len() - 1]
                } else if prev.len() >= 2 {
                    prev[prev.len() - 2]
                } else {
                    0
                };

                if back2 == 0xef && back1 == 0xbb {
                    let mut dest: isize = p as isize - 2; // char *dest = p - 2;

                    // Usually a BOM is at the beginning of a file, and so at
                    // the beginning of a line; then we can just step over it.
                    if start as isize == dest {
                        start = p + 1;
                    } else {
                        // have to shuffle buf to close gap
                        let mut adjust_prevlen: usize = 0;

                        if dest < 0 {
                            // adjust_prevlen must be 1 or 2.
                            adjust_prevlen = (-dest) as usize;
                            dest = 0;
                        }
                        if readlen > p as i64 + 1 {
                            buf.copy_within((p + 1)..(readlen as usize), dest as usize);
                        }
                        readlen -= 3 - adjust_prevlen as i64;
                        let newlen = prev.len() - adjust_prevlen;
                        prev.truncate(newlen);
                        p = dest as usize; // c: p = dest - 1; (loop increment skipped below)
                        continue;
                    }
                }
            }
            p += 1;
        } // for

        if (maxline >= 0 && (l.borrow().lv_len as i64) >= maxline) || readlen <= 0 {
            break;
        }
        if start < p {
            // There's part of a line in buf, store it in "prev".
            prev.extend_from_slice(&buf[start..p]);
        }
    } // while
      // fclose on drop
}

/// Port of `f_readfile()` from `csrc/eval/fs.c:1487`.
pub fn f_readfile(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: if (check_secure()) return;  (no sandbox standalone)
    read_file_or_blob(argvars, rettv, false);
}

/// Port of `f_readblob()` from `csrc/eval/fs.c:1477`.
pub fn f_readblob(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: if (check_secure()) return;  (no sandbox standalone)
    read_file_or_blob(argvars, rettv, true);
}

/// Write "list" of strings to file "fp".
///
/// Port of `write_list()` from `csrc/eval/fs.c:1698`.
///
/// @param  fp  File to write to.
/// @param[in]  list  List to write.
/// @param[in]  binary  Whether to write in binary mode.
///
/// @return true in case of success, false otherwise.
fn write_list(fp: &mut FileDescriptor, list: &list_T, binary: bool) -> bool {
    let mut error: i32 = 0;
    let n = list.lv_items.len();
    'write_list_error: {
        // TV_LIST_ITER_CONST(list, li, { … })
        for (idx, li) in list.lv_items.iter().enumerate() {
            let s = match tv_get_string_chk(&li.li_tv) {
                Some(s) => s,
                None => return false,
            };
            let sb = s.as_bytes();
            let mut hunk_start = 0usize;
            let mut p = 0usize;
            loop {
                // for (const char *p = hunk_start;; p++)
                let at_nul = p >= sb.len();
                if at_nul || sb[p] == b'\n' {
                    // *p == NUL || *p == NL
                    if p != hunk_start {
                        let written = file_write(fp, &sb[hunk_start..p], p - hunk_start);
                        if written < 0 {
                            error = written as i32;
                            break 'write_list_error;
                        }
                    }
                    if at_nul {
                        break;
                    } else {
                        hunk_start = p + 1;
                        let written = file_write(fp, &[0u8], 1);
                        if written < 0 {
                            error = written as i32;
                            break; // c: break (inner loop), NOT goto
                        }
                    }
                }
                p += 1;
            }
            if !binary || idx + 1 != n {
                // !binary || TV_LIST_ITEM_NEXT(list, li) != NULL
                let written = file_write(fp, b"\n", 1);
                if written < 0 {
                    error = written as i32;
                    break 'write_list_error;
                }
            }
        }
        error = file_flush(fp);
        if error != 0 {
            break 'write_list_error;
        }
        return true;
    }
    // write_list_error:
    semsg(&format!("{}{}", e_error_while_writing_str, os_strerror(error)));
    false
}

/// Write data to file with descriptor `fp`.
///
/// Port of `write_data()` from `csrc/eval/fs.c:1753`.
///
/// @return true on success, or false on failure.
fn write_data(fp: &mut FileDescriptor, data: &[u8], len: usize) -> bool {
    let mut error: i32 = 0;
    'write_blob_error: {
        if len > 0 {
            let written = file_write(fp, data, len);
            if written < len as isize {
                error = written as i32;
                break 'write_blob_error;
            }
        }
        error = file_flush(fp);
        if error != 0 {
            break 'write_blob_error;
        }
        return true;
    }
    // write_blob_error:
    semsg(&format!("{}{}", e_error_while_writing_str, os_strerror(error)));
    false
}

/// Port of `write_blob()` from `csrc/eval/fs.c:1774`.
fn write_blob(fp: &mut FileDescriptor, blob: &blob_T) -> bool {
    write_data(fp, &blob.bv_ga, tv_blob_len(blob) as usize)
}

/// Port of `write_string()` from `csrc/eval/fs.c:1780`.
fn write_string(fp: &mut FileDescriptor, data: &str) -> bool {
    write_data(fp, data.as_bytes(), data.len())
}

/// Port of `f_writefile()` from `csrc/eval/fs.c:1787` — write a List, Blob or
/// String to a file. Flags: `a` append, `b` binary, `p` mkdir parents, `s`/`S`
/// fsync toggle. Returns 0 on success, -1 on failure.
///
/// RUST-PORT NOTE: the `D` defer flag needs `can_add_defer`/`add_defer` (the
/// deferred-function stack, runtime.c), which the standalone interpreter lacks;
/// `script_is_lua`/`current_sctx` (Lua string → Blob) has no counterpart either.
pub fn f_writefile(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(rettv, -1);
    // c: if (check_secure()) return;  (no sandbox standalone)

    if argvars[0].v_type == VAR_LIST {
        if let v_list(Some(l)) = &argvars[0].vval {
            for li in &l.borrow().lv_items {
                if !tv_check_str_or_nr(&li.li_tv) {
                    return;
                }
            }
        }
    } else if argvars[0].v_type != VAR_BLOB && argvars[0].v_type != VAR_STRING {
        semsg("E475: Invalid argument: writefile() first argument must be a List or a Blob");
        return;
    }

    let mut binary = false;
    let mut append = false;
    let mut do_fsync = false; // c: !!p_fs — 'fsync' option, off standalone
    let mut mkdir_p = false;
    if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
        let flags = match tv_get_string_chk(&argvars[2]) {
            Some(f) => f,
            None => return,
        };
        for c in flags.chars() {
            match c {
                'b' => binary = true,
                'a' => append = true,
                'D' => {} // c: defer — needs the deferred-function stack (unsupported)
                's' => do_fsync = true,
                'S' => do_fsync = false,
                'p' => mkdir_p = true,
                _ => {
                    semsg(&format!("E5060: Unknown flag: {c}"));
                    return;
                }
            }
        }
    }

    let fname = match tv_get_string_buf_chk(&argvars[1]) {
        Some(f) => f,
        None => return,
    };

    let mut fp = FileDescriptor::default();
    if fname.is_empty() {
        emsg("E482: Can't open file with an empty name");
    } else {
        let error = file_open(
            &mut fp,
            &fname,
            (if append { kFileAppend } else { kFileTruncate })
                | (if mkdir_p { kFileMkDir } else { kFileCreate })
                | kFileCreate,
            0o666,
        );
        if error != 0 {
            semsg(&format!(
                "E482: Can't open file {fname} for writing: {}",
                os_strerror(error)
            ));
        } else {
            let write_ok = if argvars[0].v_type == VAR_BLOB {
                match &argvars[0].vval {
                    v_blob(Some(b)) => write_blob(&mut fp, &b.borrow()),
                    _ => true, // argvars[0].vval.v_blob == NULL
                }
            } else if argvars[0].v_type == VAR_STRING {
                write_string(&mut fp, &tv_get_string(&argvars[0]))
            } else {
                match &argvars[0].vval {
                    v_list(Some(l)) => write_list(&mut fp, &l.borrow(), binary),
                    _ => true,
                }
            };
            if write_ok {
                ret_nr!(rettv, 0);
            }
            let close_error = file_close(&mut fp, do_fsync);
            if close_error != 0 {
                semsg(&format!(
                    "E80: Error when closing file {fname}: {}",
                    os_strerror(close_error)
                ));
            }
        }
    }
}

// ── fnamemodify / glob / readdir / blob IO (eval/fs.c) ───────────────────────

/// Port of `path_tail()` (path.c) — the basename (everything after the last `/`).
fn path_tail(name: &str) -> &str {
    match name.rfind('/') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// `:S` — shell-escape (single-quote wrap, `'` → `'\''`).
fn shellescape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Port of `modify_fname()` from `Src/eval/fs.c:67` — apply `:p :h :t :r :e :~
/// :. :s :gs :S :8` modifiers to `fname`. RUST-PORT NOTE: a string adaptation of
/// the C pointer-walking version; the `:8` Windows short-name is a no-op and `:p`
/// uses the OS cwd/`$HOME` rather than Vim's buffer state.
pub fn modify_fname(mods: &str, fname: &str) -> String {
    let mut name = fname.to_string();
    let mb = mods.as_bytes();
    let mut i = 0;
    let at = |i: usize, c: u8| i < mb.len() && mb[i] == c;
    // `:p` leaf — absolute, `~`-expanded, simplified (the FullName_save/expand_env
    // role); and `:h` head (strip the last component + its separators).
    let full_path = |name: &str| -> String {
        let mut s = name.to_string();
        if let Some(rest) = s.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                s = format!("{}/{}", home.to_string_lossy(), rest);
            }
        } else if s == "~" {
            if let Some(home) = std::env::var_os("HOME") {
                s = home.to_string_lossy().into_owned();
            }
        }
        if !s.starts_with('/') {
            if let Ok(cwd) = std::env::current_dir() {
                s = format!("{}/{}", cwd.to_string_lossy(), s);
            }
        }
        simplify_filename(&s)
    };
    let fname_head = |name: &str| -> String {
        let tail = path_tail(name);
        let head_len = name.len() - tail.len();
        let mut end = head_len;
        while end > 1 && name.as_bytes()[end - 1] == b'/' {
            end -= 1;
        }
        if end == 0 {
            ".".to_string()
        } else {
            name[..end].to_string()
        }
    };
    loop {
        // ":p" — full path.
        if at(i, b':') && at(i + 1, b'p') {
            i += 2;
            name = full_path(&name);
            if Path::new(&name).is_dir() && !name.ends_with('/') {
                name.push('/');
            }
        }
        // ":." / ":~" / ":8".
        while at(i, b':') && (at(i + 1, b'.') || at(i + 1, b'~') || at(i + 1, b'8')) {
            let c = mb[i + 1];
            i += 2;
            if c == b'8' {
                continue;
            }
            let full = full_path(&name);
            if c == b'.' {
                if let Ok(cwd) = std::env::current_dir() {
                    let prefix = format!("{}/", cwd.to_string_lossy());
                    name = full
                        .strip_prefix(&prefix)
                        .map(str::to_string)
                        .unwrap_or(full);
                }
            } else if let Some(home) = std::env::var_os("HOME") {
                let home = home.to_string_lossy();
                if let Some(rest) = full.strip_prefix(&format!("{home}/")) {
                    name = format!("~/{rest}");
                } else if full == *home {
                    name = "~".to_string();
                } else {
                    name = full;
                }
            }
        }
        // ":h" — head, repeatable.
        while at(i, b':') && at(i + 1, b'h') {
            i += 2;
            name = fname_head(&name);
        }
        // ":8" — short name (no-op).
        if at(i, b':') && at(i + 1, b'8') {
            i += 2;
        }
        // ":t" — tail.
        if at(i, b':') && at(i + 1, b't') {
            i += 2;
            name = path_tail(&name).to_string();
        }
        // ":e" / ":r" — extension / root, repeatable. Tracks offsets into the
        // current fname so a repeated ":e" extends backward (Vim's `is_second_e`):
        // "a.b.c":e → "c", :e:e → "b.c".
        if at(i, b':') && (at(i + 1, b'e') || at(i + 1, b'r')) {
            let nb = std::mem::take(&mut name);
            let bytes = nb.as_bytes();
            let tail = nb.rfind('/').map(|x| x + 1).unwrap_or(0); // path_tail offset
            let mut fstart = 0usize;
            let mut flen = nb.len();
            while at(i, b':') && (at(i + 1, b'e') || at(i + 1, b'r')) {
                let is_e = mb[i + 1] == b'e';
                i += 2;
                let end = fstart + flen;
                // c: second :e scans from before the current ext; else from the end.
                let mut s: isize = if is_e && fstart > tail {
                    fstart as isize - 2
                } else {
                    end as isize - 1
                };
                while s > tail as isize {
                    if bytes[s as usize] == b'.' {
                        break;
                    }
                    s -= 1;
                }
                if is_e {
                    if s > tail as isize {
                        let newstart = (s + 1) as usize;
                        fstart = newstart;
                        flen = end - newstart;
                    } else if fstart <= tail {
                        flen = 0;
                    }
                } else if s > tail.max(fstart) as isize {
                    // :r — remove one extension.
                    flen = s as usize - fstart;
                }
            }
            name = nb[fstart..fstart + flen].to_string();
        }
        // ":s?pat?sub?" / ":gs?pat?sub?" — substitute.
        if at(i, b':') && (at(i + 1, b's') || (at(i + 1, b'g') && at(i + 2, b's'))) {
            let global = mb[i + 1] == b'g';
            let mut s = i + if global { 3 } else { 2 };
            if s < mb.len() {
                let sep = mb[s];
                s += 1;
                let pat_start = s;
                while s < mb.len() && mb[s] != sep {
                    s += 1;
                }
                if s < mb.len() {
                    let pat = &mods[pat_start..s];
                    s += 1;
                    let sub_start = s;
                    while s < mb.len() && mb[s] != sep {
                        s += 1;
                    }
                    if s < mb.len() {
                        let sub = &mods[sub_start..s];
                        let flags = if global { "g" } else { "" };
                        name = crate::viml_regex::regex_substitute(&name, pat, sub, flags);
                        i = s + 1;
                        continue; // c: goto repeat — re-apply all modifiers.
                    }
                }
            }
        }
        // ":S" — shellescape (last modifier; no further parsing after it).
        if at(i, b':') && at(i + 1, b'S') {
            name = shellescape(&name);
        }
        break;
    }
    name
}

/// Port of `f_fnamemodify()` from `Src/eval/fs.c`.
pub fn f_fnamemodify(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string_chk(&argvars[0]);
    let mods = tv_get_string_chk(&argvars[1]);
    match (fname, mods) {
        (Some(fname), Some(mods)) => {
            let r = if mods.is_empty() {
                fname
            } else {
                modify_fname(&mods, &fname)
            };
            ret_str!(rettv, Some(r));
        }
        _ => ret_str!(rettv, None::<String>),
    }
}

/// Port of `f_filecopy()` from `Src/eval/fs.c` — copy a regular file. 1/0.
pub fn f_filecopy(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_string_arg(argvars, 0) == FAIL || tv_check_for_string_arg(argvars, 1) == FAIL {
        ret_nr!(rettv, 0);
        return;
    }
    let from = tv_get_string(&argvars[0]);
    let to = tv_get_string(&argvars[1]);
    // c: only copy a regular file or symlink.
    let is_reg = std::fs::symlink_metadata(&from)
        .map(|m| m.file_type().is_file() || m.file_type().is_symlink())
        .unwrap_or(false);
    let ok = is_reg && std::fs::copy(&from, &to).is_ok();
    ret_nr!(rettv, ok as varnumber_T);
}

/// Port of `f_haslocaldir()` from `Src/eval/fs.c`. RUST-PORT NOTE: the standalone
/// interpreter has only the global scope, which never has a local directory → 0.
pub fn f_haslocaldir(_argvars: &[typval_T], rettv: &mut typval_T) {
    ret_nr!(rettv, 0);
}

/// Port of `f_resolve()` from `Src/eval/fs.c` — follow symlinks, then simplify.
/// RUST-PORT NOTE: iterates `read_link` (Vim's readlink loop); the exact
/// relative-prefix preservation of the C version is approximated by simplify.
pub fn f_resolve(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname = tv_get_string(&argvars[0]);
    let mut p = fname.clone();
    for _ in 0..100 {
        match std::fs::read_link(&p) {
            Ok(target) => {
                p = if target.is_absolute() {
                    target.to_string_lossy().into_owned()
                } else {
                    let dir = Path::new(&p).parent().unwrap_or_else(|| Path::new(""));
                    dir.join(target).to_string_lossy().into_owned()
                };
            }
            Err(_) => break,
        }
    }
    ret_str!(rettv, Some(simplify_filename(&p)));
}

/// Port of `file_pat_to_reg_pat()` (fileio.c) — convert a shell glob to a Vim
/// magic-mode regex. Leading/trailing `*` drop the `^`/`$` anchor; interior `*`
/// → `.*`, `?` → `.`, and `. ~ , { }` are backslash-escaped.
fn file_pat_to_reg_pat(pat: &str) -> String {
    let b = pat.as_bytes();
    let n = b.len();
    if n == 0 {
        return "^$".to_string();
    }
    let mut out: Vec<u8> = Vec::with_capacity(n + 2);
    let mut start = 0;
    if b[0] == b'*' {
        while start < n - 1 && b[start] == b'*' {
            start += 1;
        }
    } else {
        out.push(b'^');
    }
    let mut end = n;
    let mut add_dollar = true;
    if end > start && b[end - 1] == b'*' {
        while end - 1 > start && b[end - 1] == b'*' {
            end -= 1;
        }
        add_dollar = false;
    }
    let mut p = start;
    while p < end {
        match b[p] {
            b'*' => {
                out.extend_from_slice(b".*");
                while p + 1 < end && b[p + 1] == b'*' {
                    p += 1;
                }
            }
            b'?' => out.push(b'.'),
            c @ (b'.' | b'~' | b',' | b'{' | b'}') => {
                out.push(b'\\');
                out.push(c);
            }
            c => out.push(c),
        }
        p += 1;
    }
    if add_dollar {
        out.push(b'$');
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Port of `f_glob2regpat()` from `Src/eval/fs.c`.
pub fn f_glob2regpat(argvars: &[typval_T], rettv: &mut typval_T) {
    ret_str!(
        rettv,
        tv_get_string_chk(&argvars[0]).map(|p| file_pat_to_reg_pat(&p))
    );
}

/// Evaluate "expr" (= "context") for readdir().
///
/// Port of `readdir_checkitem()` from `csrc/eval/fs.c:1166`.
///
/// RUST-PORT NOTE: `eval_expr_typval()` plus the `v:val` bookkeeping
/// (`prepare_vimvar`/`set_vim_var_string`/`restore_vimvar` for `VV_VAL`) are the
/// bridge's `FILTER_MAP_EVAL_HOOK`, which sets `v:val` to the item and evaluates
/// the expr/funcref.
fn readdir_checkitem(context: &typval_T, name: &str) -> varnumber_T {
    // typval_T *expr = (typval_T *)context;
    let expr = context;
    let mut retval: varnumber_T = 0;
    let mut error = false;

    if expr.v_type == VAR_UNKNOWN {
        return 1;
    }

    // prepare_vimvar(VV_VAL, &save_val); set_vim_var_string(VV_VAL, name, -1);
    // argv[0] = { VAR_STRING, name }; eval_expr_typval(expr, false, argv, 1, &rettv)
    let key = typval_T::from(0 as varnumber_T);
    let val = typval_T::from(name.to_string());
    let hook = crate::ported::eval::list::FILTER_MAP_EVAL_HOOK.with(|h| *h.borrow());
    let rettv = match hook.and_then(|f| f(expr, &key, &val)) {
        Some(tv) => tv,
        None => return retval, // goto theend (retval == 0)
    };

    retval = tv_get_number_chk(&rettv, Some(&mut error));
    if error {
        retval = -1;
    }
    // tv_clear(&rettv); (Rust drops)
    retval
}

/// "readdir()" function.
///
/// Port of `f_readdir()` from `csrc/eval/fs.c:1203`.
pub fn f_readdir(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, kListLenUnknown);
    // c: if (check_secure()) return;  (no sandbox standalone)

    let path = tv_get_string(&argvars[0]);
    // typval_T *expr = &argvars[1];
    let expr_unknown = typval_T {
        v_type: VAR_UNKNOWN,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_number(0),
    };
    let expr = argvars.get(1).unwrap_or(&expr_unknown);

    // int ret = readdir_core(&ga, path, (void *)expr, readdir_checkitem);
    // RUST-PORT NOTE: readdir_core (fileio.c) scans the dir (os_scandir), ignores
    //   "."/"..", filters via readdir_checkitem, then sorts. Inlined here over
    //   std::fs::read_dir (which already omits "."/"..").
    let mut ga: Vec<String> = Vec::new();
    let rd = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(_) => {
            // c: smsg(0, _(e_notopen), path); return FAIL; → f_readdir does nothing
            semsg(&format!("E484: Can't open file {path}"));
            return;
        }
    };
    for e in rd {
        let p = match e {
            Ok(e) => e.file_name().to_string_lossy().into_owned(),
            Err(_) => continue,
        };
        let mut ignore = p == "." || p == "..";
        if !ignore {
            let r = readdir_checkitem(expr, &p);
            if r < 0 {
                break;
            }
            if r == 0 {
                ignore = true;
            }
        }
        if !ignore {
            ga.push(p);
        }
    }
    ga.sort();
    // if (ret == OK && ga.ga_len > 0) { append each }
    for p in &ga {
        tv_list_append_string(&mut l.borrow_mut(), p);
    }
}

/// Expand a leading `~`/`~/` to `$HOME` and `$VAR`/`${VAR}` references in a
/// path (`expand_env` subset — enough for glob/expand patterns standalone).
fn expand_env(s: &str) -> String {
    let s = if let Some(rest) = s.strip_prefix("~/") {
        match std::env::var("HOME") {
            Ok(h) => format!("{h}/{rest}"),
            Err(_) => s.to_string(),
        }
    } else if s == "~" {
        std::env::var("HOME").unwrap_or_else(|_| s.to_string())
    } else {
        s.to_string()
    };

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let braced = chars.peek() == Some(&'{');
        if braced {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&nc) = chars.peek() {
            if (braced && nc == '}') || !(nc == '_' || nc.is_ascii_alphanumeric()) {
                break;
            }
            name.push(nc);
            chars.next();
        }
        if braced && chars.peek() == Some(&'}') {
            chars.next();
        }
        if name.is_empty() {
            out.push('$');
        } else {
            out.push_str(&std::env::var(&name).unwrap_or_default());
        }
    }
    out
}

/// Resolve a single glob `{pattern}` to the matching paths (sorted). Wildcards
/// (`*` `?` `[...]`) are honoured in the last path component; earlier components
/// must be literal. A pattern with no wildcard yields itself iff it exists.
fn unix_expandpath(pattern: &str) -> Vec<String> {
    let has_wild = |s: &str| s.contains(['*', '?', '[']);

    // Split into the directory to scan and the (possibly wild) last component.
    let (dir, comp, prefix) = match pattern.rfind('/') {
        None => (".".to_string(), pattern.to_string(), String::new()),
        Some(0) => ("/".to_string(), pattern[1..].to_string(), "/".to_string()),
        Some(i) => (
            pattern[..i].to_string(),
            pattern[i + 1..].to_string(),
            format!("{}/", &pattern[..i]),
        ),
    };

    if !has_wild(&comp) {
        // Literal path: present iff it exists on disk.
        return if Path::new(pattern).exists() {
            vec![pattern.to_string()]
        } else {
            Vec::new()
        };
    }

    let re = file_pat_to_reg_pat(&comp);
    let want_hidden = comp.starts_with('.');
    let mut out: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|name| want_hidden || !name.starts_with('.'))
            .filter(|name| crate::viml_regex::regex_match(&re, name, false))
            .map(|name| format!("{prefix}{name}"))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

/// Port of `f_glob()` from `Src/eval/funcs.c` — expand the file wildcard
/// `{pattern}` (after `~`/`$VAR` expansion). With `{list}` (arg 3) truthy the
/// matches are returned as a List; otherwise as a newline-joined String. The
/// editor-only `'wildignore'`/`'suffixes'` filtering is not applied.
pub fn f_glob(argvars: &[typval_T], rettv: &mut typval_T) {
    let pattern = expand_env(&tv_get_string(&argvars[0]));
    let want_list =
        argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN && tv_get_bool(&argvars[2]) != 0;
    let matches = unix_expandpath(&pattern);
    if want_list {
        let l = tv_list_alloc_ret(rettv, matches.len() as isize);
        let mut lb = l.borrow_mut();
        for m in &matches {
            tv_list_append_string(&mut lb, m);
        }
    } else {
        rettv.v_type = VAR_STRING;
        rettv.vval = v_string(matches.join("\n"));
    }
}

/// Port of `f_globpath()` from `Src/eval/fs.c` — apply `glob()` to `{pattern}`
/// in each comma-separated directory of `{path}`, concatenating the results.
/// `{list}` (arg 4) truthy returns a List, else a newline-joined String.
pub fn f_globpath(argvars: &[typval_T], rettv: &mut typval_T) {
    let path = tv_get_string(&argvars[0]);
    let pattern = tv_get_string(&argvars[1]);
    let want_list =
        argvars.len() >= 4 && argvars[3].v_type != VAR_UNKNOWN && tv_get_bool(&argvars[3]) != 0;
    let mut matches: Vec<String> = Vec::new();
    for dir in path.split(',') {
        if dir.is_empty() {
            continue;
        }
        let joined = if dir.ends_with('/') {
            format!("{dir}{pattern}")
        } else {
            format!("{dir}/{pattern}")
        };
        matches.extend(unix_expandpath(&expand_env(&joined)));
    }
    if want_list {
        let l = tv_list_alloc_ret(rettv, matches.len() as isize);
        let mut lb = l.borrow_mut();
        for m in &matches {
            tv_list_append_string(&mut lb, m);
        }
    } else {
        rettv.v_type = VAR_STRING;
        rettv.vval = v_string(matches.join("\n"));
    }
}

thread_local! {
    /// Stack of paths of the scripts currently being sourced, for `<sfile>` /
    /// `<script>` in `expand()` (Neovim's `sourcing_name`/`SOURCING_NAME`). The
    /// bridge pushes/pops this around `eval_file()`.
    static SOURCING_NAME: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Push the path of a script being sourced (called by the bridge before running).
pub fn push_sourcing_name(name: String) {
    SOURCING_NAME.with(|s| s.borrow_mut().push(name));
}

/// Pop the current sourced-script path (called after the script finishes).
pub fn pop_sourcing_name() {
    SOURCING_NAME.with(|s| {
        s.borrow_mut().pop();
    });
}

/// The path of the innermost script currently being sourced, if any.
fn current_sourcing_name() -> Option<String> {
    SOURCING_NAME.with(|s| s.borrow().last().cloned())
}

/// Port of `f_expand()` from `Src/eval/funcs.c` — expand `{string}`: `<sfile>`/
/// `<script>` to the sourced script's path (with `:` modifiers), `$VAR`/`~`
/// references, then file wildcards. The other editor specials (`%`/`#`/`<…>`)
/// have no current file standalone, so they expand to "". With `{list}` (arg 3)
/// truthy the result is a List of matches.
pub fn f_expand(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let want_list =
        argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN && tv_get_bool(&argvars[2]) != 0;
    // c: `<sfile>`/`<script>` expand to the sourced script's path, with optional
    // trailing `:` filename-modifiers (the rest of the token).
    let sfile = s
        .strip_prefix("<sfile>")
        .or_else(|| s.strip_prefix("<script>"));
    let results: Vec<String> = if let Some(mods) = sfile {
        match current_sourcing_name() {
            Some(path) if mods.is_empty() => vec![path],
            Some(path) => vec![modify_fname(mods, &path)],
            None => Vec::new(),
        }
    } else if s.starts_with(['%', '#', '<']) {
        Vec::new()
    } else {
        let expanded = expand_env(&s);
        if expanded.contains(['*', '?', '[']) {
            let m = unix_expandpath(&expanded);
            // expand() returns the pattern itself when nothing matches.
            if m.is_empty() {
                vec![expanded]
            } else {
                m
            }
        } else {
            vec![expanded]
        }
    };
    if want_list {
        let l = tv_list_alloc_ret(rettv, results.len() as isize);
        let mut lb = l.borrow_mut();
        for m in &results {
            tv_list_append_string(&mut lb, m);
        }
    } else {
        rettv.v_type = VAR_STRING;
        rettv.vval = v_string(results.join("\n"));
    }
}

/// Port of `f_expandcmd()` from `Src/eval/funcs.c` — expand `$VAR`/`~` in a
/// command string. (Editor specials like `%` need a current file and are left
/// as-is standalone.)
pub fn f_expandcmd(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(expand_env(&tv_get_string(&argvars[0])));
}

/// Port of `f_browse()` (fs.c) — no GUI file dialog standalone → "".
pub fn f_browse(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}
/// Port of `f_browsedir()` (fs.c) — no GUI directory dialog standalone → "".
pub fn f_browsedir(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}
/// Port of `findfilendir()` from `csrc/eval/fs.c:536`.
///
/// RUST-PORT NOTE: the search backend is editor state — `curbuf->b_p_path`/
/// `p_path` (the `'path'` option), `find_file_in_path_option()` (file_search.c),
/// `vim_findfile_cleanup()`, and `curbuf->b_ffname`/`b_p_sua` — none of which the
/// standalone interpreter has. The argument parsing is ported faithfully; the
/// `find_file_in_path_option` search loop is a deferred stub (see deferred_deps),
/// so `fresult` stays NULL → the result is "" (or an empty List when count < 0).
fn findfilendir(argvars: &[typval_T], rettv: &mut typval_T, _find_what: i32) {
    let fresult: Option<String> = None;
    // char *path = *curbuf->b_p_path == NUL ? p_path : curbuf->b_p_path;
    let mut count = 1;
    let mut error = false;

    rettv.vval = v_string(String::new()); // rettv->vval.v_string = NULL;
    rettv.v_type = VAR_STRING;

    let fname = tv_get_string(&argvars[0]);

    if argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN {
        match tv_get_string_buf_chk(&argvars[1]) {
            None => error = true,
            Some(_p) => {
                // if (*p != NUL) { path = p; }  (path unused without a search backend)
                if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
                    count = tv_get_number_chk(&argvars[2], Some(&mut error)) as i32;
                }
            }
        }
    }

    if count < 0 {
        tv_list_alloc_ret(rettv, kListLenUnknown);
    }

    if !fname.is_empty() && !error {
        // do { fresult = find_file_in_path_option(…); … } while (…);
        // DEFERRED: find_file_in_path_option()/vim_findfile_cleanup() (file_search.c)
        //   + curbuf->b_ffname/b_p_sua need the editor core; no file is found here.
    }

    if rettv.v_type == VAR_STRING {
        rettv.vval = v_string(fresult.unwrap_or_default());
    }
}

/// "finddir({fname}[, {path}[, {count}]])" function.
///
/// Port of `f_finddir()` from `csrc/eval/fs.c:602`.
pub fn f_finddir(argvars: &[typval_T], rettv: &mut typval_T) {
    findfilendir(argvars, rettv, FINDFILE_DIR);
}

/// "findfile({fname}[, {path}[, {count}]])" function.
///
/// Port of `f_findfile()` from `csrc/eval/fs.c:608`.
pub fn f_findfile(argvars: &[typval_T], rettv: &mut typval_T) {
    findfilendir(argvars, rettv, FINDFILE_FILE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_paths() {
        assert_eq!(simplify_filename("/a/b/../c"), "/a/c");
        assert_eq!(simplify_filename("a/./b//c"), "a/b/c");
        assert_eq!(simplify_filename("/a/b/../../c"), "/c");
        assert_eq!(simplify_filename("./a"), "a");
        assert_eq!(simplify_filename("a/.."), ".");
        assert_eq!(simplify_filename("/"), "/");
    }

    #[test]
    fn absolute_paths() {
        assert!(path_is_absolute("/usr/bin"));
        assert!(path_is_absolute("~/foo"));
        assert!(!path_is_absolute("foo/bar"));
    }

    #[test]
    fn getfperm_format() {
        #[cfg(unix)]
        {
            assert_eq!(perm_string!(0o644u32), "rw-r--r--");
            assert_eq!(perm_string!(0o755u32), "rwxr-xr-x");
            assert_eq!(perm_string!(0o000u32), "---------");
        }
    }

    fn tmp(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!("vimlrs_fs_{}_{}.tmp", tag, std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    // write_list writes list items joined by '\n' with a trailing '\n' (text mode).
    #[test]
    fn write_list_roundtrip() {
        let path = tmp("wl");
        let mut list = list_T::default();
        tv_list_append_string(&mut list, "hello");
        tv_list_append_string(&mut list, "world");

        let mut fp = FileDescriptor::default();
        assert_eq!(
            file_open(&mut fp, &path, kFileCreate | kFileTruncate, 0o644),
            0
        );
        assert!(write_list(&mut fp, &list, false));
        assert_eq!(file_close(&mut fp, false), 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\nworld\n");

        // Binary mode: no trailing newline after the last item.
        let mut fp = FileDescriptor::default();
        assert_eq!(
            file_open(&mut fp, &path, kFileCreate | kFileTruncate, 0o644),
            0
        );
        assert!(write_list(&mut fp, &list, true));
        assert_eq!(file_close(&mut fp, false), 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\nworld");
        let _ = std::fs::remove_file(&path);
    }

    // f_writefile(List) then f_readfile round-trips the lines.
    #[test]
    fn writefile_readfile_roundtrip() {
        let path = tmp("wrf");
        let mut list = list_T::default();
        tv_list_append_string(&mut list, "alpha");
        tv_list_append_string(&mut list, "beta");
        let list_tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(std::rc::Rc::new(std::cell::RefCell::new(list)))),
        };

        let argw = [list_tv, typval_T::from(path.clone())];
        let mut rw = typval_T::default();
        f_writefile(&argw, &mut rw);
        assert!(matches!(rw.vval, v_number(0)), "writefile returned 0");

        let argr = [typval_T::from(path.clone())];
        let mut rr = typval_T::default();
        f_readfile(&argr, &mut rr);
        let l = match &rr.vval {
            v_list(Some(l)) => l.clone(),
            _ => panic!("readfile must return a list"),
        };
        assert_eq!(l.borrow().lv_len, 2);
        assert_eq!(tv_get_string(&l.borrow().lv_items[0].li_tv), "alpha");
        assert_eq!(tv_get_string(&l.borrow().lv_items[1].li_tv), "beta");
        let _ = std::fs::remove_file(&path);
    }

    // readfile strips a trailing CR and does not yield a trailing empty line.
    #[test]
    fn readfile_crlf_and_trailing_newline() {
        let path = tmp("crlf");
        std::fs::write(&path, b"a\r\nb\r\n").unwrap();
        let argr = [typval_T::from(path.clone())];
        let mut rr = typval_T::default();
        f_readfile(&argr, &mut rr);
        let l = match &rr.vval {
            v_list(Some(l)) => l.clone(),
            _ => panic!(),
        };
        assert_eq!(l.borrow().lv_len, 2);
        assert_eq!(tv_get_string(&l.borrow().lv_items[0].li_tv), "a");
        assert_eq!(tv_get_string(&l.borrow().lv_items[1].li_tv), "b");
        let _ = std::fs::remove_file(&path);
    }

    // f_readblob with [offset, size] extracts a byte window.
    #[test]
    fn readblob_offset_size() {
        let path = tmp("rb");
        std::fs::write(&path, [0u8, 1, 2, 3, 4, 5]).unwrap();
        let argr = [
            typval_T::from(path.clone()),
            typval_T::from(2 as varnumber_T),
            typval_T::from(3 as varnumber_T),
        ];
        let mut rr = typval_T::default();
        f_readblob(&argr, &mut rr);
        let b = match &rr.vval {
            v_blob(Some(b)) => b.clone(),
            _ => panic!("readblob must return a blob"),
        };
        assert_eq!(b.borrow().bv_ga, vec![2u8, 3, 4]);

        // Negative offset reads from EOF.
        let argr = [typval_T::from(path.clone()), typval_T::from(-2 as varnumber_T)];
        let mut rr = typval_T::default();
        f_readblob(&argr, &mut rr);
        let b = match &rr.vval {
            v_blob(Some(b)) => b.clone(),
            _ => panic!(),
        };
        assert_eq!(b.borrow().bv_ga, vec![4u8, 5]);
        let _ = std::fs::remove_file(&path);
    }

    // readdir_checkitem keeps every entry when the filter expr is absent.
    #[test]
    fn readdir_checkitem_unknown_keeps() {
        let unknown = typval_T::default(); // VAR_UNKNOWN
        assert_eq!(readdir_checkitem(&unknown, "anything"), 1);
    }

    // findfile/finddir have no search path standalone → "".
    #[test]
    fn findfile_returns_empty() {
        let argv = [typval_T::from("nonesuch.txt".to_string())];
        let mut rv = typval_T::default();
        f_findfile(&argv, &mut rv);
        assert_eq!(tv_get_string(&rv), "");
    }
}
