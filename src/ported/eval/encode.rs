//! Port of `src/nvim/eval/encode.c` (vendored at `vendor/eval/encode.c`) — the
//! `string()` / `:echo` value-rendering entry points and the recursive
//! converter the `typval_encode.c.h` macro template generates.
//!
//! RUST-PORT NOTE: C generates `encode_vim_to_string`/`encode_vim_to_echo` by
//! instantiating the `typval_encode.c.h` template twice. The two instantiations
//! render identically for nested values (both quote nested strings); they differ
//! only at the outermost string, which the `encode_tv2*` wrappers handle. The
//! recursive walk is ported once as `encode_vim_to_string`; `encode_vim_to_echo`
//! delegates to it (the bodies the macro emits are equivalent).
#![allow(non_snake_case)]

use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, BoolVarValue::*, VarType::*,
};
use crate::vimstr::VimStr;

/// Render a float for `"%g"` the way neovim's `vim_vsnprintf_typval()`
/// (`src/nvim/strings.c`) does — which is *not* libc `%g`. Unlike libc, neovim
/// picks fixed (`%f`) vs exponential (`%e`) form by the fixed magnitude range
/// `[0.001, 1e7)` (`abs_f >= 0.001 && abs_f < 10000000.0`), prints the exponent
/// with neither a `+` nor leading zeroes (`e8`, `e-4`), and — only when the
/// precision was *not* specified — strips trailing mantissa zeroes, keeping one
/// digit after the `.`. `precision` is `Some(p)` for an explicit `%.<p>g`,
/// `None` for a bare `%g` (which uses libc's default precision of 6). The caller
/// appends `.0` when there is no `.`/`e`, so `1.0` prints as `1.0`.
///
/// Rust's `{:e}` already renders the exponent the way neovim does *after* it
/// strips libc's `+`/leading-zero padding, so no exponent fixup is needed here.
pub(crate) fn vim_float_g(f: f64, precision: Option<i32>) -> String {
    // c: `vim_snprintf` prints the non-finite values as lowercase words, and Rust's
    // `{}`/`{:.6}` render NaN as `NaN` — so `string(0.0/0.0)` came out `NaN` where
    // Vim says `nan`. (printf's `%F`/`%E`/`%G` uppercase them; that lives in
    // `f_printf`'s own non-finite path, not here.)
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    // c: double abs_f = f < 0 ? -f : f;
    let abs_f = f.abs();
    // c (g/G branch): fixed form when abs_f is in [0.001, 1e7) or zero, else exp.
    let use_exp = !((abs_f >= 0.001 && abs_f < 10000000.0) || abs_f == 0.0);
    let prec_specified = precision.is_some();
    // c: precision defaults to libc's 6 for a bare "%g"; else the given value.
    let p = precision.unwrap_or(6).max(0) as usize;
    let mut s: Vec<u8> = if use_exp {
        format!("{f:.p$e}").into_bytes()
    } else {
        format!("{f:.p$}").into_bytes()
    };
    // c: remove_trailing_zeroes — only when no precision was specified, strip
    // trailing '0's of the mantissa, keeping the digit that follows the '.'.
    // c: while (tp > tmp + 2 && *tp == '0' && tp[-1] != '.') { … tp--; }
    if !prec_specified {
        let mut i = match s.iter().position(|&b| b == b'e') {
            Some(epos) => epos - 1,
            None => s.len() - 1,
        };
        while i > 2 && s[i] == b'0' && s[i - 1] != b'.' {
            s.remove(i);
            i -= 1;
        }
    }
    String::from_utf8(s).unwrap()
}

/// Port of `encode_special_var_names[]` from `Src/eval/encode.c:41` — the name a
/// `VAR_SPECIAL` value prints under, indexed by its `SpecialVarValue`.
///
/// c: `const char *const encode_special_var_names[] = { [kSpecialVarNull] =
/// "v:null" };` — one entry, because Neovim has only `v:null`. Vim carries
/// `v:none` as a second special and prints it under its own name
/// (`get_var_special_name()`), which is the second entry. A table, not a
/// function, exactly as the C is: both readers INDEX it.
///
/// Those readers are `encode_vim_to_string()` here and `tv_get_string_buf_chk()`
/// (c: `STRCPY(buf, encode_special_var_names[tv->vval.v_special])`,
/// `Src/eval/typval.c:4602`), so `v:none` in string context and `string(v:none)`
/// cannot disagree. They did while the latter had `"v:null"` written into it.
#[allow(non_upper_case_globals)]
pub const encode_special_var_names: [&str; 2] = ["v:null", "v:none"];

/// Port of `encode_blob_write()` from `Src/eval/encode.c:48`.
///
/// Append the raw bytes `buf` to blob `blob`, returning the number written
/// (used as the readfile/channel-output sink for Blob mode).
pub fn encode_blob_write(blob: &mut crate::ported::eval::typval_defs_h::blob_T, buf: &[u8]) -> i32 {
    blob.bv_ga.extend_from_slice(buf);
    buf.len() as i32
}

/// Port of `conv_error()` from `vendor/eval/encode.c:113`.
///
/// Show an error message when converting to a msgpack value, building the path
/// to the failed value by walking `mpstack`. `msg` must contain exactly two
/// `%s` (replaced with `objname` and the path). Returns
/// [`FAIL`](crate::ported::eval_h::FAIL).
///
/// RUST-PORT NOTE: nothing in vimlrs builds an `MPConvStack` at runtime (the
/// encoders recurse directly), so this is a faithful dead-reference port —
/// exercised only by the `#[cfg(test)]` stack built by hand below. C's
/// `garray_T msg_ga` byte buffer becomes a `String`; `vim_snprintf(IObuff, …,
/// fmt, …)` becomes formatting the verbatim C `%s`/`%i` template via `replacen`;
/// the variadic `semsg(msg, a, b)` becomes sequential `%s` substitution.
pub fn conv_error(
    msg: &str,
    mpstack: &crate::ported::eval::typval_encode_h::MPConvStack,
    objname: &str,
) -> i32 {
    use crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED;
    use crate::ported::eval::typval_encode_h::{
        kv_A, kv_size, MPConvPartialStage::*, MPConvStackValData, MPConvStackValType::*,
    };
    use crate::ported::eval_h::FAIL;
    use crate::ported::message::semsg;

    // c:118 ga_init(&msg_ga, sizeof(char), 80) — the object-path accumulator.
    let mut msg_ga = String::new();
    // c:119-124 localized message templates (verbatim C printf formats).
    let key_msg = "key %s";
    let key_pair_msg = "key %s at index %i from special map";
    let idx_msg = "index %i";
    let partial_arg_msg = "partial";
    let partial_arg_i_msg = "argument %i";
    let partial_self_msg = "partial self dictionary";
    // RUST-PORT NOTE: vim_snprintf(IObuff, IOSIZE, fmt, one-arg) — fill a single
    // %s (string) or %i (int) in a C template.
    let snprintf_s = |fmt: &str, s: &str| -> String { fmt.replacen("%s", s, 1) };
    let snprintf_i = |fmt: &str, i: i32| -> String { fmt.replacen("%i", &i.to_string(), 1) };

    // c:125 for (size_t i = 0; i < kv_size(*mpstack); i++)
    for i in 0..kv_size(mpstack) {
        // c:126-127 if (i != 0) { GA_CONCAT_LITERAL(&msg_ga, ", "); }
        if i != 0 {
            msg_ga.push_str(", ");
        }
        // c:129 MPConvStackVal v = kv_A(*mpstack, i);
        let v = kv_A(mpstack, i);
        // c:130 switch (v.type)
        match v.r#type {
            // c:131 case kMPConvDict:
            kMPConvDict => {
                let (dict, hi) = match &v.data {
                    MPConvStackValData::d { dict, hi, .. } => (dict, *hi),
                    _ => continue,
                };
                let dict = dict.borrow();
                // c:132-138 key_tv.vval.v_string = (hi == NULL ? ht_array
                //   : (hi - 1))->hi_key — first entry when not advanced, else the
                //   entry preceding the next-to-process slot.
                let key_idx = match hi {
                    None => 0,
                    Some(h) => h - 1,
                };
                let hi_key = dict
                    .dv_hashtab
                    .get_index(key_idx)
                    .map(|(k, _)| k.clone())
                    .unwrap_or_default();
                let key_tv = typval_T {
                    v_type: VAR_STRING,
                    v_lock: VAR_UNLOCKED,
                    vval: v_string(hi_key.into()),
                };
                // c:139 char *const key = encode_tv2string(&key_tv, NULL);
                let key = encode_tv2string(&key_tv);
                // c:140-141 vim_snprintf(IObuff, IOSIZE, key_msg, key);
                //   ga_concat(&msg_ga, IObuff);
                msg_ga.push_str(&snprintf_s(key_msg, &key.to_string_lossy()));
            }
            // c:145-146 case kMPConvPairs: case kMPConvList:
            kMPConvPairs | kMPConvList => {
                let (list, li) = match &v.data {
                    MPConvStackValData::l { list, li } => (list, *li),
                    _ => continue,
                };
                let list = list.borrow();
                let len = list.lv_items.len() as i32;
                // c:147-153 const int idx = (li == first ? 0 : (li == NULL ?
                //   len-1 : tv_list_idx_of_item(list, PREV(li)))).
                let idx = match li {
                    Some(0) => 0,            // li == tv_list_first(list)
                    None => len - 1,         // li == NULL
                    Some(l) => l as i32 - 1, // idx of PREV(li)
                };
                // c:154-157 const listitem_T *const li = (li == NULL ? last
                //   : PREV(li)).
                let li_cur: Option<usize> = match li {
                    None => {
                        if len >= 1 {
                            Some((len - 1) as usize)
                        } else {
                            None
                        }
                    }
                    Some(0) => None, // PREV(first) == NULL
                    Some(l) => Some(l - 1),
                };
                // c:158-174 idx_msg unless the current item is a non-empty pair
                //   sublist, in which case its first item is the key.
                // RUST-PORT NOTE: C reads `->vval.v_list` regardless of v_type;
                // a non-VAR_LIST (or empty-list) item can only take the idx_msg
                // branch here — the pair branch needs the first item of an actual
                // sublist — so the combined `(v_type != VAR_LIST && len <= 0)`
                // condition reduces to "not a non-empty VAR_LIST".
                let pair_first: Option<typval_T> = if v.r#type == kMPConvList {
                    None
                } else {
                    match li_cur {
                        None => None,
                        Some(ci) => match &list.lv_items[ci].li_tv.vval {
                            v_list(Some(sub)) => {
                                sub.borrow().lv_items.first().map(|it| it.li_tv.clone())
                            }
                            _ => None,
                        },
                    }
                };
                match pair_first {
                    // c:162-163 vim_snprintf(IObuff, IOSIZE, idx_msg, idx);
                    None => msg_ga.push_str(&snprintf_i(idx_msg, idx)),
                    // c:165-173 key from the pair's first item; key_pair_msg.
                    Some(key_tv) => {
                        // c:170 char *const key = encode_tv2echo(&key_tv, NULL);
                        let key = encode_tv2echo(&key_tv);
                        // c:171 vim_snprintf(IObuff, IOSIZE, key_pair_msg, key, idx);
                        msg_ga.push_str(&snprintf_i(
                            &snprintf_s(key_pair_msg, &key.to_string_lossy()),
                            idx,
                        ));
                    }
                }
            }
            // c:177 case kMPConvPartial:
            kMPConvPartial => {
                let stage = match &v.data {
                    MPConvStackValData::p { stage, .. } => *stage,
                    _ => continue,
                };
                // c:178 switch (v.data.p.stage)
                match stage {
                    // c:179-181 case kMPConvPartialArgs: abort();
                    kMPConvPartialArgs => panic!("conv_error: kMPConvPartialArgs"),
                    // c:182-184 case kMPConvPartialSelf: ga_concat(partial_arg_msg);
                    kMPConvPartialSelf => msg_ga.push_str(partial_arg_msg),
                    // c:185-187 case kMPConvPartialEnd: ga_concat(partial_self_msg);
                    kMPConvPartialEnd => msg_ga.push_str(partial_self_msg),
                }
            }
            // c:190 case kMPConvPartialList:
            kMPConvPartialList => {
                let arg = match &v.data {
                    MPConvStackValData::a { arg, .. } => *arg,
                    _ => continue,
                };
                // c:191 const int idx = (int)(v.data.a.arg - v.data.a.argv) - 1;
                let idx = arg as i32 - 1;
                // c:192-193 vim_snprintf(IObuff, IOSIZE, partial_arg_i_msg, idx);
                msg_ga.push_str(&snprintf_i(partial_arg_i_msg, idx));
            }
        }
    }
    // c:198-200 semsg(msg, _(objname), (kv_size(*mpstack) == 0 ? _("itself")
    //   : msg_ga.ga_data));
    let path = if kv_size(mpstack) == 0 {
        "itself"
    } else {
        msg_ga.as_str()
    };
    // RUST-PORT NOTE: variadic C semsg(fmt, a, b) → sequential %s substitution.
    let out = {
        let first = msg.replacen("%s", objname, 1);
        first.replacen("%s", path, 1)
    };
    semsg(&out);
    // c:202 return FAIL;
    FAIL
}

/// Port of `encode_vim_list_to_buf()` from `Src/eval/encode.c:213`.
///
/// Serialize a List of strings to the `writefile()` byte form: items joined by
/// `NL`, with each item's embedded `NL` mapped to `NUL`. Returns `None` (the C
/// `false`) if any item is not a String.
pub fn encode_vim_list_to_buf(list: &crate::ported::eval::typval_defs_h::list_T) -> Option<String> {
    let mut parts: Vec<String> = Vec::with_capacity(list.lv_items.len());
    for it in &list.lv_items {
        if it.li_tv.v_type != VAR_STRING {
            return None;
        }
        match &it.li_tv.vval {
            v_string(s) => parts.push(s.to_string_lossy().replace('\n', "\0")),
            _ => parts.push(String::new()),
        }
    }
    Some(parts.join("\n"))
}

/// Port of `ListReaderState` (`Src/eval/encode.h:28`) — position state for
/// reading a List's joined byte stream. RUST-PORT NOTE: the C holds the `list`
/// and a `listitem_T *li` pointer; here `li` is an item index and the list is
/// passed to [`encode_read_from_list`].
#[derive(Debug, Clone, Copy)]
pub struct ListReaderState {
    /// Index of the item currently being read.
    pub li: usize,
    /// Byte offset inside the current item's string.
    pub offset: usize,
    /// Byte length of the current item's string.
    pub li_length: usize,
}

/// Port of `encode_init_lrstate()` from `Src/eval/encode.c:1053`.
///
/// Initialize a [`ListReaderState`] at the start of `list`.
pub fn encode_init_lrstate(list: &crate::ported::eval::typval_defs_h::list_T) -> ListReaderState {
    let li_length = list.lv_items.first().map_or(0, |it| match &it.li_tv.vval {
        v_string(s) => s.len(),
        _ => 0,
    });
    ListReaderState {
        li: 0,
        offset: 0,
        li_length,
    }
}

/// Port of `encode_read_from_list()` from `Src/eval/encode.c:257`.
///
/// Read up to `buf.len()` bytes of `list`'s joined byte form into `buf` (items
/// separated by `NL`, embedded `NL` → `NUL`), advancing `state`. Returns
/// `(status, read_bytes)` where status is [`OK`](crate::ported::eval_h::OK)
/// (finished), `2` (NOTDONE — more remains), or
/// [`FAIL`](crate::ported::eval_h::FAIL) (a non-String item).
pub fn encode_read_from_list(
    state: &mut ListReaderState,
    list: &crate::ported::eval::typval_defs_h::list_T,
    buf: &mut [u8],
) -> (i32, usize) {
    use crate::ported::eval_h::{FAIL, OK};
    const NOTDONE: i32 = 2; // c: Src/macros_defs.h
    let nbuf = buf.len();
    let mut p = 0;
    while p < nbuf {
        if let Some(bytes) = list
            .lv_items
            .get(state.li)
            .and_then(|it| match &it.li_tv.vval {
                v_string(s) => Some(s.as_bytes()),
                _ => None,
            })
        {
            while state.offset < state.li_length && p < nbuf {
                let ch = bytes[state.offset];
                state.offset += 1;
                buf[p] = if ch == b'\n' { 0 } else { ch };
                p += 1;
            }
        }
        if p < nbuf {
            state.li += 1;
            if state.li >= list.lv_items.len() {
                return (OK, p);
            }
            buf[p] = b'\n';
            p += 1;
            match list.lv_items.get(state.li).map(|it| &it.li_tv) {
                Some(tv) if tv.v_type == VAR_STRING => {
                    state.offset = 0;
                    state.li_length = match &tv.vval {
                        v_string(s) => s.len(),
                        _ => 0,
                    };
                }
                _ => return (FAIL, p),
            }
        }
    }
    let more = state.offset < state.li_length || state.li + 1 < list.lv_items.len();
    (if more { NOTDONE } else { OK }, nbuf)
}

/// Port of `encode_list_write()` from `Src/eval/encode.c:56`.
///
/// Append the lines of `buf` to `list`, splitting on `NL` and mapping embedded
/// `NUL` → `NL` (the `readfile()`/channel-output representation). The first
/// line continues the list's last item (so streamed chunks join), and a buffer
/// ending in `NL` yields a trailing empty item. RUST-PORT NOTE: the C's NULL
/// (never-set) string item is an empty string here.
///
/// The C signature is `(void *data, const char *buf, size_t len)` — a BYTE
/// buffer, and it must stay one here. This took a `&str` and so could not carry
/// the one payload its main caller produces: `msgpackdump()` writes MessagePack,
/// which is binary (`msgpackdump([v:true])` is the single byte `0xc3`), and
/// routing it through a `str` destroyed it. The `NUL` → `NL` substitution is the
/// C's `memchrsub(str, NUL, NL, line_length)` (c:78, c:90) and is what makes
/// `msgpackparse()`'s `encode_read_from_list()` an exact inverse.
pub fn encode_list_write(list: &mut crate::ported::eval::typval_defs_h::list_T, buf: &[u8]) {
    use crate::ported::eval::typval::tv_list_append_allocated_string;
    use crate::ported::eval::typval_defs_h::typval_vval_union::v_string;
    // c:59 if (len == 0) return;
    if buf.is_empty() {
        return;
    }
    // c: memchrsub(str, NUL, NL, line_length) — a NUL byte in the stream is
    // stored as NL inside a line, the readfile() convention.
    let subst = |seg: &[u8]| -> crate::vimstr::VimStr {
        let mut v = seg.to_vec();
        for b in v.iter_mut() {
            if *b == 0 {
                *b = b'\n';
            }
        }
        v.into()
    };
    let mut segments = buf.split(|&b| b == b'\n');
    // c:68 "Continue the last list element" with the first (partial) line.
    if !list.lv_items.is_empty() {
        if let Some(first) = segments.next() {
            if let v_string(s) = &mut list.lv_items.last_mut().unwrap().li_tv.vval {
                s.push_bytes(subst(first).as_bytes());
            }
        }
    }
    // c:83 each remaining NL-delimited run becomes its own item.
    for seg in segments {
        tv_list_append_allocated_string(list, subst(seg));
    }
}

/// Port of `encode_tv2string()` from `Src/eval/encode.c:869`.
///
/// String representation of a value with quotes around strings (parseable back
/// by `eval()`). This is `string()`.
pub fn encode_tv2string(tv: &typval_T) -> VimStr {
    // c: encode_vim_to_string(&ga, tv, ...)
    encode_vim_to_string(tv)
}

/// Port of `encode_tv2echo()` from `Src/eval/encode.c:893`.
///
/// String representation without quotes around the outermost string, as `:echo`
/// displays values.
pub fn encode_tv2echo(tv: &typval_T) -> VimStr {
    // c: if (tv->v_type == VAR_STRING || tv->v_type == VAR_FUNC) { ga_concat(v_string) }
    match (tv.v_type, &tv.vval) {
        (VAR_STRING | VAR_FUNC, v_string(s)) => s.clone(),
        // c: else encode_vim_to_echo(&ga, tv, ...)
        _ => encode_vim_to_echo(tv),
    }
}

/// Port of the `encode_vim_to_string` instantiation of the `typval_encode.c.h`
/// template — recursive render with every string quoted.
///
/// The C builds into a `garray_T` of bytes and hands back `ga.ga_data`, a
/// `char *`. It is a byte builder here for the same reason: a VimL string may
/// hold bytes that are not valid UTF-8 (`string(list2str([-1]))` quotes the
/// single byte `0xff`), and those bytes are spliced in as they are.
pub fn encode_vim_to_string(tv: &typval_T) -> VimStr {
    match (tv.v_type, &tv.vval) {
        // TYPVAL_ENCODE_CONV_NUMBER
        (VAR_NUMBER, v_number(n)) => n.to_string().into(),
        // TYPVAL_ENCODE_CONV_FLOAT (encode.c:351) — FP_NAN → "str2float('nan')",
        // FP_INFINITE → "[-]str2float('inf')", else "%g" then append ".0" if no
        // '.'/'e' (so string(3.0) is "3.0", not "3").
        (VAR_FLOAT, v_float(f)) => {
            if f.is_nan() {
                "str2float('nan')".into()
            } else if f.is_infinite() {
                if *f < 0.0 {
                    "-str2float('inf')"
                } else {
                    "str2float('inf')"
                }
                .into()
            } else {
                let s = vim_float_g(*f, None);
                if s.contains(['.', 'e', 'E']) {
                    s.into()
                } else {
                    format!("{s}.0").into()
                }
            }
        }
        // TYPVAL_ENCODE_CONV_STRING (encode.c:295) — single-quoted, embedded
        // quotes doubled. The C is a macro with the loop written out at each
        // use, so it is written out here too:
        //
        //   ga_append(gap, '\'');
        //   for (size_t i_ = 0; i_ < len_; i_++) {
        //     if (buf_[i_] == '\'') { ga_append(gap, '\''); }
        //     ga_append(gap, (uint8_t)buf_[i_]);
        //   }
        //   ga_append(gap, '\'');
        //
        // It walks BYTES. `'` is ASCII and a UTF-8 trail byte is always >= 0x80,
        // so the scan can never fire inside a multibyte sequence — no decode is
        // needed, and a string whose bytes are not valid UTF-8 keeps every one.
        (VAR_STRING, v_string(s)) => {
            let mut out = VimStr::from("'");
            for &b in s.as_bytes() {
                if b == b'\'' {
                    out.push_char('\'');
                }
                out.as_mut_vec().push(b);
            }
            out.push_char('\'');
            out
        }
        // TYPVAL_ENCODE_CONV_FUNC_START — function('name'), the same macro over
        // the function name.
        (VAR_FUNC, v_string(s)) => {
            let mut out = VimStr::from("function('");
            for &b in s.as_bytes() {
                if b == b'\'' {
                    out.push_char('\'');
                }
                out.as_mut_vec().push(b);
            }
            out.push_str("')");
            out
        }
        // A Partial — function('name'[, [args]][, {self}]).
        //
        // c: TYPVAL_ENCODE_CONV_FUNC_BEFORE_ARGS writes ", " when there are
        // bound args and TYPVAL_ENCODE_CONV_FUNC_BEFORE_SELF writes ", " when
        // `pt_dict` is not NULL (encode.c:393-405), so the two suffixes are
        // independent: `function('F', {…})` is what a dict-bound partial with no
        // bound arguments prints. Both oracles agree — vim 9.2 and nvim 0.12
        // print `function('P', [1], {'n': 7})`.
        (VAR_PARTIAL, v_partial(Some(p))) => {
            let name = p.pt_name.replace('\'', "''");
            let mut out = VimStr::from(format!("function('{name}'"));
            if !p.pt_argv.is_empty() {
                out.push_str(", [");
                for (i, a) in p.pt_argv.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_bytes(&encode_tv2string(a));
                }
                out.push_char(']');
            }
            if let Some(d) = &p.pt_dict {
                out.push_str(", ");
                out.push_bytes(&encode_tv2string(&typval_T {
                    v_type: VAR_DICT,
                    v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
                    vval: crate::ported::eval::typval_defs_h::typval_vval_union::v_dict(Some(
                        d.clone(),
                    )),
                }));
            }
            out.push_char(')');
            out
        }
        (VAR_BOOL, v_bool(b)) => if *b == kBoolVarTrue {
            "v:true"
        } else {
            "v:false"
        }
        .into(),
        (VAR_SPECIAL, v_special(v)) => encode_special_var_names[*v as usize].into(),
        (VAR_SPECIAL, _) => "v:null".into(),
        // TYPVAL_ENCODE_CONV_LIST_START / _BETWEEN_ITEMS / _END
        (VAR_LIST, v_list(l)) => match l {
            None => "[]".into(),
            Some(l) => {
                let l = l.borrow();
                let mut out = VimStr::from("[");
                for (i, it) in l.lv_items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_bytes(&encode_vim_to_string(&it.li_tv));
                }
                out.push_char(']');
                out
            }
        },
        // TYPVAL_ENCODE_CONV_DICT_START / _KEY / _AFTER_KEY / _BETWEEN_ITEMS / _END
        (VAR_DICT, v_dict(d)) => match d {
            None => "{}".into(),
            Some(d) => {
                let d = d.borrow();
                let mut out = VimStr::from("{");
                for (i, (k, v)) in d.dv_hashtab.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("'{}'", k.replace('\'', "''")));
                    out.push_str(": ");
                    out.push_bytes(&encode_vim_to_string(v));
                }
                out.push_char('}');
                out
            }
        },
        // TYPVAL_ENCODE_CONV_BLOB — 0z followed by hex, grouped in 4-byte runs.
        (VAR_BLOB, v_blob(b)) => match b {
            None => "0z".into(),
            Some(b) => {
                let b = b.borrow();
                let mut out = VimStr::from("0z");
                for (i, byte) in b.bv_ga.iter().enumerate() {
                    if i > 0 && i % 4 == 0 {
                        out.push_char('.');
                    }
                    out.push_str(&format!("{byte:02X}"));
                }
                out
            }
        },
        _ => VimStr::new(),
    }
}

/// Port of the `encode_vim_to_echo` instantiation. Equivalent to
/// [`encode_vim_to_string`] for all nested values (see file-header note).
pub fn encode_vim_to_echo(tv: &typval_T) -> VimStr {
    encode_vim_to_string(tv)
}

/// Port of `encode_tv2json()` from `Src/eval/encode.c:921` — the `json_encode()`
/// rendering of a value.
pub fn encode_tv2json(tv: &typval_T) -> String {
    encode_vim_to_json(tv)
}

/// Port of `convert_to_json_string()` from `Src/eval/encode.c:621` — a
/// double-quoted, JSON-escaped string.
fn convert_to_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Port of the `encode_vim_to_json` instantiation of the encode template — JSON
/// render. Strings/keys are double-quoted+escaped, `v:true`/`v:false`/`v:null`
/// become `true`/`false`/`null`.
pub fn encode_vim_to_json(tv: &typval_T) -> String {
    match (tv.v_type, &tv.vval) {
        (VAR_NUMBER, v_number(n)) => n.to_string(),
        (VAR_FLOAT, v_float(f)) => {
            if f.is_finite() {
                let s = vim_float_g(*f, None);
                if s.contains(['.', 'e', 'E']) {
                    s
                } else {
                    format!("{s}.0")
                }
            } else {
                "null".to_string() // JSON has no NaN/Inf
            }
        }
        (VAR_STRING, v_string(s)) => convert_to_json_string(&s.to_string_lossy()),
        (VAR_BOOL, v_bool(b)) => if *b == kBoolVarTrue { "true" } else { "false" }.to_string(),
        (VAR_SPECIAL, _) => "null".to_string(),
        (VAR_LIST, v_list(l)) => match l {
            None => "[]".to_string(),
            Some(l) => {
                let l = l.borrow();
                let mut out = String::from("[");
                for (i, it) in l.lv_items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&encode_vim_to_json(&it.li_tv));
                }
                out.push(']');
                out
            }
        },
        (VAR_DICT, v_dict(d)) => match d {
            None => "{}".to_string(),
            Some(d) => {
                let d = d.borrow();
                let mut out = String::from("{");
                for (i, (k, v)) in d.dv_hashtab.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&convert_to_json_string(k));
                    out.push(':');
                    out.push_str(&encode_vim_to_json(v));
                }
                out.push('}');
                out
            }
        },
        // c: TYPVAL_ENCODE_CONV_BLOB (encode.c:751) — a Blob is a JSON *array of
        // byte values*, `[0, 17, 34]`, not `null`. (Note the ", " separator: the
        // JSON encoder spaces list items, unlike `string()`.)
        (VAR_BLOB, v_blob(b)) => match b {
            None => "[]".to_string(),
            Some(b) => {
                let b = b.borrow();
                let items: Vec<String> = b.bv_ga.iter().map(|byte| byte.to_string()).collect();
                format!("[{}]", items.join(", "))
            }
        },
        _ => "null".to_string(),
    }
}

/// Port of `encode_check_json_key()` from `vendor/eval/encode.c:781`.
///
/// Check whether given key can be used in `json_encode()`: either a plain
/// String, or a MessagePack string special dictionary
/// (`{'_TYPE': v:msgpack_types.string, '_VAL': [strings]}`).
///
/// RUST-PORT NOTE: the special-dict `_TYPE` identity check (`c:798`) compares
/// against `eval_msgpack_type_lists[kMPString]`. In C that array is the single
/// global shared with the decoder (`vars.c` `evalvars_init()`); in the vimlrs
/// port it lives in [`crate::ported::eval::decode`], so this reads the same
/// per-run lists the decoder's `create_special_dict()` stamped into the `_TYPE`
/// value — pointer identity via `Rc::ptr_eq`.
pub fn encode_check_json_key(tv: &typval_T) -> bool {
    use crate::ported::eval::decode::{eval_msgpack_type_lists, MessagePackType};
    use crate::ported::eval::typval::tv_dict_find;
    use std::rc::Rc;
    // c:784  if (tv->v_type == VAR_STRING) { return true; }
    if tv.v_type == VAR_STRING {
        return true;
    }
    // c:787  if (tv->v_type != VAR_DICT) { return false; }
    if tv.v_type != VAR_DICT {
        return false;
    }
    // c:790  const dict_T *const spdict = tv->vval.v_dict;
    let spdict = match &tv.vval {
        // c: a NULL dict has ht_used 0 != 2, so it falls through to false below.
        v_dict(Some(d)) => d,
        _ => return false,
    };
    let spdict = spdict.borrow();
    // c:791  if (spdict->dv_hashtab.ht_used != 2) { return false; }
    if spdict.dv_hashtab.len() != 2 {
        return false;
    }
    // c:794-798  type_di = tv_dict_find(spdict, S_LEN("_TYPE")) ...
    let type_tv = match tv_dict_find(&spdict, "_TYPE") {
        // c:796  || type_di == NULL
        None => return false,
        Some(t) => t,
    };
    // c:797  || type_di->di_tv.v_type != VAR_LIST
    if type_tv.v_type != VAR_LIST {
        return false;
    }
    // c:798  || type_di->di_tv.vval.v_list != eval_msgpack_type_lists[kMPString]
    let type_list = match &type_tv.vval {
        // A NULL list can never equal the (non-NULL) string type list.
        v_list(Some(l)) => l,
        _ => return false,
    };
    if !eval_msgpack_type_lists
        .with(|arr| Rc::ptr_eq(type_list, &arr[MessagePackType::kMPString as usize]))
    {
        return false;
    }
    // c:799  || (val_di = tv_dict_find(spdict, S_LEN("_VAL"))) == NULL
    let val_tv = match tv_dict_find(&spdict, "_VAL") {
        None => return false,
        Some(v) => v,
    };
    // c:800  || val_di->di_tv.v_type != VAR_LIST
    if val_tv.v_type != VAR_LIST {
        return false;
    }
    // c:803  if (val_di->di_tv.vval.v_list == NULL) { return true; }
    let val_list = match &val_tv.vval {
        v_list(Some(l)) => l.clone(),
        _ => return true,
    };
    // c:806-810  TV_LIST_ITER_CONST(...): every item must be a String.
    for li in &val_list.borrow().lv_items {
        if li.li_tv.v_type != VAR_STRING {
            return false;
        }
    }
    // c:811  return true;
    true
}

#[cfg(test)]
mod encode_check_json_key_tests {
    use super::encode_check_json_key;
    use crate::ported::eval::decode::{eval_msgpack_type_lists, MessagePackType};
    use crate::ported::eval::typval::{
        tv_dict_add, tv_dict_alloc, tv_list_alloc, tv_list_append_string,
    };
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::*, VarLockStatus::*, VarType::*,
    };

    fn list_tv(
        rc: std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::list_T>>,
    ) -> typval_T {
        typval_T {
            v_type: VAR_LIST,
            v_lock: VAR_UNLOCKED,
            vval: v_list(Some(rc)),
        }
    }

    #[test]
    fn plain_string_key_is_valid() {
        // c:784 — a plain String is always a valid json key.
        assert!(encode_check_json_key(&typval_T::from("k".to_string())));
    }

    #[test]
    fn number_key_is_invalid() {
        // c:787 — a non-String, non-Dict is rejected.
        assert!(!encode_check_json_key(&typval_T::from(
            7 as crate::ported::eval::typval_defs_h::varnumber_T
        )));
    }

    #[test]
    fn plain_dict_is_invalid() {
        // c:791 — a normal dict (wrong ht_used / not a special dict) is rejected.
        let d = tv_dict_alloc();
        tv_dict_add(&mut d.borrow_mut(), "a", typval_T::from("x".to_string()));
        let tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VAR_UNLOCKED,
            vval: v_dict(Some(d)),
        };
        assert!(!encode_check_json_key(&tv));
    }

    #[test]
    fn string_special_dict_of_strings_is_valid() {
        // c:796-811 — {_TYPE: msgpack string list, _VAL: [strings]} is valid.
        let type_list =
            eval_msgpack_type_lists.with(|a| a[MessagePackType::kMPString as usize].clone());
        let val = tv_list_alloc(0);
        tv_list_append_string(&mut val.borrow_mut(), "abc");
        let d = tv_dict_alloc();
        tv_dict_add(&mut d.borrow_mut(), "_TYPE", list_tv(type_list));
        tv_dict_add(&mut d.borrow_mut(), "_VAL", list_tv(val));
        let tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VAR_UNLOCKED,
            vval: v_dict(Some(d)),
        };
        assert!(encode_check_json_key(&tv));
    }

    #[test]
    fn special_dict_with_nonstring_val_item_is_invalid() {
        // c:807 — a _VAL item that is not a String rejects the key.
        let type_list =
            eval_msgpack_type_lists.with(|a| a[MessagePackType::kMPString as usize].clone());
        let val = tv_list_alloc(0);
        crate::ported::eval::typval::tv_list_append_number(&mut val.borrow_mut(), 3);
        let d = tv_dict_alloc();
        tv_dict_add(&mut d.borrow_mut(), "_TYPE", list_tv(type_list));
        tv_dict_add(&mut d.borrow_mut(), "_VAL", list_tv(val));
        let tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VAR_UNLOCKED,
            vval: v_dict(Some(d)),
        };
        assert!(!encode_check_json_key(&tv));
    }

    #[test]
    fn special_dict_wrong_type_list_is_invalid() {
        // c:798 — a fresh (non-shared) _TYPE list fails pointer identity.
        let type_list = tv_list_alloc(0);
        let val = tv_list_alloc(0);
        tv_list_append_string(&mut val.borrow_mut(), "abc");
        let d = tv_dict_alloc();
        tv_dict_add(&mut d.borrow_mut(), "_TYPE", list_tv(type_list));
        tv_dict_add(&mut d.borrow_mut(), "_VAL", list_tv(val));
        let tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VAR_UNLOCKED,
            vval: v_dict(Some(d)),
        };
        assert!(!encode_check_json_key(&tv));
    }
}

#[cfg(test)]
mod conv_error_tests {
    use super::conv_error;
    use crate::ported::eval::typval::{
        tv_dict_add, tv_dict_alloc, tv_list_alloc, tv_list_append_number, tv_list_append_string,
    };
    use crate::ported::eval::typval_defs_h::{
        list_T, typval_T, typval_vval_union::*, VarLockStatus::*, VarType::*,
    };
    use crate::ported::eval::typval_encode_h::{
        kvi_push, MPConvPartialStage::*, MPConvStack, MPConvStackVal, MPConvStackValData,
        MPConvStackValType::*,
    };
    use crate::ported::message::{capture_errors_begin, capture_errors_take};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn list_tv(l: Rc<RefCell<list_T>>) -> typval_T {
        typval_T {
            v_type: VAR_LIST,
            v_lock: VAR_UNLOCKED,
            vval: v_list(Some(l)),
        }
    }

    #[test]
    fn empty_stack_uses_itself() {
        // c:198-200 kv_size == 0 → the path is _("itself").
        let stack = MPConvStack::default();
        capture_errors_begin();
        assert_eq!(conv_error("E: %s, %s", &stack, "F"), 0);
        let errs = capture_errors_take();
        assert_eq!(errs, vec!["E: F, itself".to_string()]);
    }

    #[test]
    fn walks_every_stack_variant() {
        let mut stack = MPConvStack::default();

        // kMPConvDict, hi == NULL → first key "foo" → "key 'foo'".
        let d = tv_dict_alloc();
        tv_dict_add(
            &mut d.borrow_mut(),
            "foo",
            typval_T::from(1 as crate::ported::eval::typval_defs_h::varnumber_T),
        );
        kvi_push(
            &mut stack,
            MPConvStackVal {
                r#type: kMPConvDict,
                tv: None,
                saved_copyID: 0,
                data: MPConvStackValData::d {
                    dict: d.clone(),
                    dictp: d.clone(),
                    hi: None,
                    todo: 0,
                },
            },
        );

        // kMPConvList, li == Some(2) → idx = 1 → "index 1".
        let l = tv_list_alloc(0);
        tv_list_append_string(&mut l.borrow_mut(), "a");
        tv_list_append_string(&mut l.borrow_mut(), "b");
        tv_list_append_string(&mut l.borrow_mut(), "c");
        kvi_push(
            &mut stack,
            MPConvStackVal {
                r#type: kMPConvList,
                tv: None,
                saved_copyID: 0,
                data: MPConvStackValData::l {
                    list: l.clone(),
                    li: Some(2),
                },
            },
        );

        // kMPConvPairs, li == NULL → idx = len-1 = 0, current = last pair whose
        // first item is the key "k" → "key k at index 0 from special map".
        let pair = tv_list_alloc(0);
        tv_list_append_string(&mut pair.borrow_mut(), "k");
        tv_list_append_number(&mut pair.borrow_mut(), 9);
        let pairs = tv_list_alloc(0);
        pairs
            .borrow_mut()
            .lv_items
            .push(crate::ported::eval::typval_defs_h::listitem_T {
                li_tv: list_tv(pair),
            });
        kvi_push(
            &mut stack,
            MPConvStackVal {
                r#type: kMPConvPairs,
                tv: None,
                saved_copyID: 0,
                data: MPConvStackValData::l {
                    list: pairs,
                    li: None,
                },
            },
        );

        // kMPConvPartial, stage kMPConvPartialSelf → "partial".
        kvi_push(
            &mut stack,
            MPConvStackVal {
                r#type: kMPConvPartial,
                tv: None,
                saved_copyID: 0,
                data: MPConvStackValData::p {
                    stage: kMPConvPartialSelf,
                    pt: None,
                },
            },
        );

        // kMPConvPartialList, arg == 2 → idx = 1 → "argument 1".
        kvi_push(
            &mut stack,
            MPConvStackVal {
                r#type: kMPConvPartialList,
                tv: None,
                saved_copyID: 0,
                data: MPConvStackValData::a {
                    arg: 2,
                    argv: Vec::new(),
                    todo: 0,
                },
            },
        );

        capture_errors_begin();
        assert_eq!(conv_error("dump %s at %s", &stack, "object"), 0);
        let errs = capture_errors_take();
        assert_eq!(
            errs,
            vec!["dump object at key 'foo', index 1, key k at index 0 from special map, partial, argument 1".to_string()]
        );
    }
}

#[cfg(test)]
mod encode_io_tests {
    use super::{encode_blob_write, encode_vim_list_to_buf};
    use crate::ported::eval::typval::{tv_list_append_number, tv_list_append_string};
    use crate::ported::eval::typval_defs_h::{blob_T, list_T};

    #[test]
    fn blob_write_appends_bytes() {
        let mut b = blob_T::default();
        assert_eq!(encode_blob_write(&mut b, &[1, 2, 3]), 3);
        assert_eq!(encode_blob_write(&mut b, &[4]), 1);
        assert_eq!(b.bv_ga, vec![1, 2, 3, 4]);
    }

    #[test]
    fn read_from_list_matches_to_buf() {
        use super::{encode_init_lrstate, encode_read_from_list, encode_vim_list_to_buf};
        use crate::ported::eval_h::OK;
        let mut l = list_T::default();
        tv_list_append_string(&mut l, "a");
        tv_list_append_string(&mut l, "x\ny"); // embedded NL
        tv_list_append_string(&mut l, "b");
        let expected = encode_vim_list_to_buf(&l).unwrap();
        // full read into a big buffer reproduces encode_vim_list_to_buf exactly
        let mut st = encode_init_lrstate(&l);
        let mut buf = vec![0u8; 64];
        let (status, n) = encode_read_from_list(&mut st, &l, &mut buf);
        assert_eq!(status, OK);
        assert_eq!(&buf[..n], expected.as_bytes());
        // a too-small buffer reports NOTDONE (2)
        let mut st2 = encode_init_lrstate(&l);
        let mut small = vec![0u8; 2];
        let (status2, n2) = encode_read_from_list(&mut st2, &l, &mut small);
        assert_eq!(status2, 2);
        assert_eq!(n2, 2);
    }

    #[test]
    fn vim_list_to_buf_joins() {
        let mut l = list_T::default();
        tv_list_append_string(&mut l, "a");
        tv_list_append_string(&mut l, "b");
        assert_eq!(encode_vim_list_to_buf(&l).as_deref(), Some("a\nb"));
        // embedded NL within an item → NUL
        let mut l2 = list_T::default();
        tv_list_append_string(&mut l2, "x\ny");
        assert_eq!(encode_vim_list_to_buf(&l2), Some("x\0y".to_string()));
        // a non-string item → None
        let mut l3 = list_T::default();
        tv_list_append_string(&mut l3, "ok");
        tv_list_append_number(&mut l3, 7);
        assert_eq!(encode_vim_list_to_buf(&l3), None);
    }
}
