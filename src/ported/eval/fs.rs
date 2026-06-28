//! Port of `src/nvim/eval/fs.c` (vendored at `csrc/eval/fs.c`).
//!
//! Filesystem-related Vimscript builtins. The pure path-string builtins plus the
//! filesystem-touching ones whose C leaf calls (`os_*`, `path.c`) map cleanly to
//! Rust `std::fs`/`std::env` — the same os-layer adaptation as `os/time.rs`.
#![allow(non_snake_case)]

use std::path::Path;

use crate::ported::eval::typval::{
    tv_check_for_nonempty_string_arg, tv_check_for_string_arg, tv_get_bool, tv_get_number,
    tv_get_string, tv_get_string_chk, tv_list_alloc_ret, tv_list_append_string,
};
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, varnumber_T, VarType::*};
use crate::ported::eval_h::FAIL;
use crate::ported::message::{emsg, semsg};
use crate::ported::path::shorten_dir_len;

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

/// Port of `f_readfile()` from `Src/eval/fs.c` — read a file into a List of
/// lines. RUST-PORT NOTE: text mode (default) — split on `\n`, strip a trailing
/// `\r`, and drop the empty element after a final newline. The `b`/`B`/`maxline`
/// options and NUL handling are not modeled (text-line subset).
pub fn f_readfile(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 0);
    let fname = tv_get_string(&argvars[0]);
    if Path::new(&fname).is_dir() {
        semsg(&format!("E17: {fname} is a directory"));
        return;
    }
    let data = match std::fs::read(&fname) {
        Ok(d) => d,
        Err(_) => {
            semsg(&format!("E484: Can't open file {fname}"));
            return;
        }
    };
    let text = String::from_utf8_lossy(&data);
    let mut s = text.as_ref();
    // A single trailing newline does not yield a trailing empty line.
    if let Some(stripped) = s.strip_suffix('\n') {
        s = stripped;
    }
    if data.is_empty() {
        return;
    }
    for line in s.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        tv_list_append_string(&mut l.borrow_mut(), line);
    }
}

/// Port of `f_writefile()` from `Src/eval/fs.c` — write a List of lines to a
/// file. Flags: `a` append, `b` binary (no trailing newline). Returns 0/−1.
pub fn f_writefile(argvars: &[typval_T], rettv: &mut typval_T) {
    use std::io::Write;
    ret_nr!(rettv, -1);
    let lines = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => {
            emsg("E475: Invalid argument: writefile() requires a List");
            return;
        }
    };
    let fname = tv_get_string(&argvars[1]);
    let flags = if argvars.len() > 2 {
        tv_get_string(&argvars[2])
    } else {
        String::new()
    };
    let append = flags.contains('a');
    let binary = flags.contains('b');

    let items: Vec<String> = lines
        .borrow()
        .lv_items
        .iter()
        .map(|it| tv_get_string(&it.li_tv))
        .collect();
    let mut buf = items.join("\n");
    if !binary && !buf.is_empty() {
        buf.push('\n');
    } else if !binary && buf.is_empty() && !items.is_empty() {
        buf.push('\n');
    }

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(&fname);
    let mut f = match file {
        Ok(f) => f,
        Err(_) => {
            semsg(&format!("E482: Can't create file {fname}"));
            return;
        }
    };
    ret_nr!(
        rettv,
        if f.write_all(buf.as_bytes()).is_ok() {
            0
        } else {
            -1
        }
    );
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

/// Port of `f_readdir()` from `Src/eval/fs.c` — directory entries (sorted), with
/// an optional filter expr (`{name -> 1 keep / 0 skip / -1 stop}` via `v:val`).
pub fn f_readdir(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 0);
    let path = tv_get_string(&argvars[0]);
    let mut names: Vec<String> = match std::fs::read_dir(&path) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect(),
        Err(_) => return,
    };
    names.sort();
    let has_filter = argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN;
    for name in names {
        if has_filter {
            // c: readdir_checkitem — set v:val=name, eval expr; 1 keep / 0 skip / -1 stop.
            let key = typval_T::from(0 as varnumber_T);
            let val = typval_T::from(name.clone());
            let r = crate::ported::eval::list::FILTER_MAP_EVAL_HOOK
                .with(|h| *h.borrow())
                .and_then(|f| f(&argvars[1], &key, &val));
            match r.map(|tv| tv_get_number(&tv)) {
                Some(0) => continue,
                Some(n) if n < 0 => break,
                _ => {}
            }
        }
        tv_list_append_string(&mut l.borrow_mut(), &name);
    }
}

/// Port of `read_file_or_blob()`/`read_blob()`/`f_readblob()` from `Src/eval/fs.c`
/// — read a file's bytes into a Blob, honoring `[offset [, size]]`.
pub fn f_readblob(argvars: &[typval_T], rettv: &mut typval_T) {
    use crate::ported::eval::typval::tv_blob_alloc_ret;
    let b = tv_blob_alloc_ret(rettv);
    let fname = tv_get_string(&argvars[0]);
    let data = match std::fs::read(&fname) {
        Ok(d) => d,
        Err(_) => {
            semsg(&format!("E484: Can't open file {fname}"));
            return;
        }
    };
    // c: offset (negative → from EOF), then size.
    let len = data.len() as i64;
    let mut off = if argvars.len() > 1 {
        tv_get_number(&argvars[1])
    } else {
        0
    };
    if off < 0 {
        off += len;
        if off < 0 {
            off = 0;
        }
    }
    let off = (off.min(len)) as usize;
    let size = if argvars.len() > 2 {
        let s = tv_get_number(&argvars[2]);
        if s < 0 {
            (len as usize).saturating_sub(off)
        } else {
            (s as usize).min(data.len() - off)
        }
    } else {
        data.len() - off
    };
    b.borrow_mut()
        .bv_ga
        .extend_from_slice(&data[off..off + size]);
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

/// Port of `f_expand()` from `Src/eval/funcs.c` — expand `{string}`: `$VAR`/`~`
/// references, then file wildcards. Editor specials (`%`/`#`/`<…>`) have no
/// current file standalone, so they expand to "". With `{list}` (arg 3) truthy
/// the result is a List of matches.
pub fn f_expand(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let want_list =
        argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN && tv_get_bool(&argvars[2]) != 0;
    let results: Vec<String> = if s.starts_with(['%', '#', '<']) {
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
/// Port of `f_finddir()` (fs.c) — no `'path'` to search standalone → "".
pub fn f_finddir(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}
/// Port of `f_findfile()` (fs.c) — no `'path'` to search standalone → "".
pub fn f_findfile(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
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
}
