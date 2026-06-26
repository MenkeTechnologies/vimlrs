//! Port of `src/nvim/path.c` (not vendored under `csrc/`; most names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them —
//! `vim_ispathsep_nocolon` is the exception, see `fake_fn_allowlist.txt`).
//!
//! Only the path-component helpers backing `pathshorten()` are ported here. The
//! Unix path model is taken (separators are `/`); the Windows `#ifdef`
//! (`BACKSLASH_IN_FILENAME` / `MSWIN`) branches are not compiled.
#![allow(non_snake_case)]

/// Port of `vim_ispathsep()` from `Src/path.c:261`.
///
/// Whether `c` is a path separator. (Unix: `/` only — `:` is valid in names.)
pub fn vim_ispathsep(c: char) -> bool {
    // c: #ifdef UNIX return c == '/';
    c == '/'
}

/// Port of `vim_ispathsep_nocolon()` from `Src/path.c:275`.
///
/// Like `vim_ispathsep` but never treats `:` as a separator. On Unix the extra
/// `&& c != ':'` guard is `BACKSLASH_IN_FILENAME`-only, so this equals
/// `vim_ispathsep`.
pub fn vim_ispathsep_nocolon(c: char) -> bool {
    vim_ispathsep(c)
}

/// Port of `get_past_head()` from `Src/path.c:240`.
///
/// Returns the byte offset of the path proper, past a leading run of separators
/// (and, on Windows, a `c:` drive — not compiled here).
pub fn get_past_head(path: &str) -> usize {
    // c: while (vim_ispathsep(*retval)) retval++;
    let bytes = path.as_bytes();
    let mut retval = 0;
    while retval < bytes.len() && vim_ispathsep(bytes[retval] as char) {
        retval += 1;
    }
    retval
}

/// Port of `path_tail()` from `Src/path.c:102`.
///
/// Returns the byte offset of the last path component (the name after the final
/// separator). The C returns a `char *` into `fname`; here it is that pointer's
/// offset.
pub fn path_tail(fname: &str) -> usize {
    // c: tail = get_past_head(fname); for (p=tail; *p; MB_PTR_ADV(p)) if (sep) tail = p+1;
    let start = get_past_head(fname);
    let mut tail = start;
    for (i, c) in fname[start..].char_indices() {
        if vim_ispathsep_nocolon(c) {
            tail = start + i + c.len_utf8();
        }
    }
    tail
}

/// Port of `shorten_dir_len()` from `Src/path.c:298`.
///
/// Shorten each directory component of `str` to `trim_len` characters (keeping a
/// leading `~`/`.`), leaving the final component (the tail) intact. The C edits
/// the buffer in place via a `char *` write cursor; the Rust port builds the
/// result by char, which is the same algorithm over UTF-8 char boundaries
/// (`utfc_ptr2len`'s job).
pub fn shorten_dir_len(str: &str, trim_len: i32) -> String {
    let tail = path_tail(str);
    let mut d = String::with_capacity(str.len());
    let mut skip = false;
    let mut dirchunk_len = 0i32;
    for (i, c) in str.char_indices() {
        if i >= tail {
            // c: copy the whole tail.
            d.push_str(&str[i..]);
            break;
        } else if vim_ispathsep(c) {
            // c: copy '/' and reset the per-component counter.
            d.push(c);
            skip = false;
            dirchunk_len = 0;
        } else if !skip {
            // c: copy the next char; count only non-leading word chars.
            d.push(c);
            if c != '~' && c != '.' {
                dirchunk_len += 1;
                if dirchunk_len >= trim_len {
                    skip = true;
                }
            }
        }
        // else: skipping the rest of this component — drop the char.
    }
    d
}
