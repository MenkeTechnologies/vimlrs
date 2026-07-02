//! Port of `src/nvim/eval/decode.c` (vendored at `csrc/eval/decode.c`).
//!
//! The JSON decoder: an explicit value/container stack machine
//! ([`json_decode_string`], [`json_decoder_pop`], [`parse_json_string`],
//! [`parse_json_number`]) that turns a UTF-8 JSON document into a [`typval_T`]
//! tree, plus the special-dictionary helpers ([`create_special_dict`],
//! [`decode_create_map_special_dict`], [`decode_string`],
//! [`positive_integer_to_special_typval`]) shared with the msgpack path.
//!
//! RUST-PORT NOTE (control flow): C walks a `const char *p` over a
//! NUL-terminated buffer and uses `goto` for the parse loop's restart / trailing
//! / fail / ret exits. This port walks byte indices into a `&[u8]` and replaces
//! the `goto`s with labeled-block breaks and explicit `do_restart` /
//! `goto_fail` / `goto_after_cycle` flags; the byte offsets, the two stack
//! vectors, and every decision mirror the C line-for-line.
//!
//! RUST-PORT NOTE (containers): C reaches the shared list/dict through the
//! `list_T *`/`dict_T *` stored on both the value stack and the container stack;
//! here that is one `Rc<RefCell<…>>` cloned onto both stacks, and the C
//! pointer-identity test (`(void *)obj.vval.v_list == (void *)container.vval
//! .v_list`) becomes [`Rc::ptr_eq`].
//!
//! RUST-PORT NOTE (msgpack): the `typval_parse_enter`/`typval_parse_exit`/
//! `mpack_parse_typval`/`unpack_typval` half of `decode.c` drives the libmpack
//! streaming parser (`csrc/mpack/{mpack_core,object,conv}.c`: `mpack_parser_t`,
//! `mpack_node_t`, `mpack_parse`, `mpack_unpack_*`, the `MPACK_TOKEN_*` enum),
//! now ported at [`crate::ported::mpack`]. Those four functions are ported here;
//! the libmpack node `data[0]`/`data[1]` `void *p` union members are represented
//! by [`TypvalNodeData`] (see its note) since Rust cannot store raw pointers into
//! the shared `Rc<RefCell<…>>` containers.
#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    clippy::comparison_chain,
    clippy::needless_range_loop
)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::encode::encode_list_write;
use crate::ported::eval::string2float; // c: string2float() (eval.c)
use crate::ported::eval::typval::{
    tv_blob_alloc_ret, tv_clear, tv_dict_add, tv_dict_alloc, tv_dict_find, tv_list_alloc,
    tv_list_append_list, tv_list_append_number, tv_list_append_owned_tv, tv_list_len, tv_list_ref,
};
use crate::ported::eval::typval_defs_h::{
    list_T, typval_T, typval_vval_union::*, varnumber_T, BoolVarValue::*, SpecialVarValue::*,
    VarLockStatus::VAR_UNLOCKED, VarType::*, VARNUMBER_MAX,
};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::mbyte::{utf_char2bytes, utf_char2len, utf_ptr2char, utf_ptr2len};
use crate::ported::message::{emsg, semsg};
use crate::ported::mpack::{
    mpack_data_t, mpack_parse, mpack_parser_init, mpack_parser_t, mpack_token_data,
    mpack_token_type_t::*, mpack_unpack_boolean, mpack_unpack_float, mpack_unpack_sint,
    mpack_unpack_uint, MPACK_OK,
};

// ── ascii_defs.h control-character constants used by the JSON grammar. ──
const NUL: u8 = 0x00;
const BS: u8 = 0x08;
const TAB: u8 = 0x09;
const NL: u8 = 0x0a;
const FF: u8 = 0x0c;
const CAR: u8 = 0x0d;

/// `enum ListLenSpecials { kListLenMayKnow = -3 }` (typval_defs.h:38) — passed to
/// [`tv_list_alloc`], which ignores the hint in this value model.
const kListLenMayKnow: isize = -3;

// ── encode.h surrogate-pair constants (`\uXXXX` decoding). ──
/// `#define SURROGATE_HI_START 0xD800` (encode.h:40).
const SURROGATE_HI_START: u64 = 0xD800;
/// `#define SURROGATE_HI_END 0xDBFF` (encode.h:43).
const SURROGATE_HI_END: u64 = 0xDBFF;
/// `#define SURROGATE_LO_START 0xDC00` (encode.h:46).
const SURROGATE_LO_START: u64 = 0xDC00;
/// `#define SURROGATE_LO_END 0xDFFF` (encode.h:49).
const SURROGATE_LO_END: u64 = 0xDFFF;
/// `#define SURROGATE_FIRST_CHAR 0x10000` (encode.h:52).
const SURROGATE_FIRST_CHAR: i32 = 0x10000;

/// `typedef enum { kMPNil, … } MessagePackType;` (eval_defs.h:6) — the type of a
/// MessagePack special dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePackType {
    kMPNil,
    kMPBoolean,
    kMPInteger,
    kMPFloat,
    kMPString,
    kMPArray,
    kMPMap,
    kMPExt,
}
/// `#define NUM_MSGPACK_TYPES (kMPExt + 1)` (eval_defs.h:16).
pub const NUM_MSGPACK_TYPES: usize = 8;

thread_local! {
    /// Port of `const list_T *eval_msgpack_type_lists[]` (eval_defs.h /
    /// `vars.c:247`) — one empty, fixed list per MessagePack type, used as the
    /// `_TYPE` value of a special dictionary.
    ///
    /// RUST-PORT NOTE: C fills this global in `evalvars_init()` (vars.c) and
    /// shares the same lists with `v:msgpack_types`. Here the decode path keeps
    /// its own lazily-created empty lists so the module is self-contained; the
    /// observable JSON/`string()` result is identical (an empty `_TYPE` list).
    pub static eval_msgpack_type_lists: [Rc<RefCell<list_T>>; NUM_MSGPACK_TYPES] =
        std::array::from_fn(|_| tv_list_alloc(0));
}

/// Helper structure for container_struct. (`ContainerStackItem`, decode.c:30)
#[derive(Clone)]
struct ContainerStackItem {
    /// Index of current container in stack.
    stack_index: usize,
    /// `_VAL` key contents for special maps. `None` when the container is not a
    /// special dictionary.
    special_val: Option<Rc<RefCell<list_T>>>,
    /// Location (byte offset) where container starts.
    s: usize,
    /// Container. Either VAR_LIST, VAR_DICT or the VAR_LIST which is `_VAL` from
    /// a special dictionary.
    container: typval_T,
}

/// Helper structure for values struct. (`ValuesStackItem`, decode.c:41)
struct ValuesStackItem {
    /// Indicates that current value is a special dictionary with string.
    is_special_string: bool,
    /// True if previous token was comma.
    didcomma: bool,
    /// True if previous token was colon.
    didcolon: bool,
    /// Actual value.
    val: typval_T,
}

/// Create special dictionary
///
/// Port of `create_special_dict()` from `csrc/eval/decode.c:62`.
///
/// @param[out]  rettv  Location where created dictionary will be saved.
/// @param[in]  type  Type of the dictionary.
/// @param[in]  val  Value associated with the _VAL key.
fn create_special_dict(rettv: &mut typval_T, r#type: MessagePackType, val: typval_T) {
    // c: dict_T *const dict = tv_dict_alloc();
    let dict = tv_dict_alloc();
    // c: dictitem_T *type_di = tv_dict_item_alloc_len(S_LEN("_TYPE"));
    //    type_di->di_tv = { VAR_LIST, VAR_UNLOCKED, eval_msgpack_type_lists[type] };
    //    tv_list_ref(...); tv_dict_add(dict, type_di);
    // c: eval_msgpack_type_lists[type]
    let type_list = eval_msgpack_type_lists.with(|arr| arr[r#type as usize].clone());
    tv_list_ref(&mut type_list.borrow_mut());
    tv_dict_add(
        &mut dict.borrow_mut(),
        "_TYPE",
        typval_T {
            v_type: VAR_LIST,
            v_lock: VAR_UNLOCKED,
            vval: v_list(Some(type_list)),
        },
    );
    // c: dictitem_T *val_di = tv_dict_item_alloc_len(S_LEN("_VAL"));
    //    val_di->di_tv = val; tv_dict_add(dict, val_di);
    tv_dict_add(&mut dict.borrow_mut(), "_VAL", val);
    // c: dict->dv_refcount++;
    dict.borrow_mut().dv_refcount += 1;
    // c: *rettv = { VAR_DICT, VAR_UNLOCKED, .v_dict = dict };
    *rettv = typval_T {
        v_type: VAR_DICT,
        v_lock: VAR_UNLOCKED,
        vval: v_dict(Some(dict)),
    };
}

/// Helper function used for working with stack vectors used by JSON decoder
///
/// Port of `json_decoder_pop()` from `csrc/eval/decode.c:106`.
///
/// RUST-PORT NOTE: `buf` is passed explicitly so error messages can reproduce
/// the C `%s` suffix at a byte offset (C reaches it via the `char *` position
/// pointer directly).
///
/// @return OK in case of success, FAIL in case of error.
fn json_decoder_pop(
    buf: &[u8],
    obj: ValuesStackItem,
    stack: &mut Vec<ValuesStackItem>,
    container_stack: &mut Vec<ContainerStackItem>,
    pp: &mut usize,
    next_map_special: &mut bool,
    didcomma: &mut bool,
    didcolon: &mut bool,
) -> i32 {
    let suffix =
        |loc: usize| -> String { String::from_utf8_lossy(&buf[loc.min(buf.len())..]).into_owned() };
    // c: if (kv_size(*container_stack) == 0) { kv_push(*stack, obj); return OK; }
    if container_stack.is_empty() {
        stack.push(obj);
        return OK;
    }
    let mut last_container = container_stack.last().unwrap().clone(); // c: kv_last(*container_stack)
    let mut val_location = *pp;
    // c: obj.v_type == container.v_type && (void*)obj.vval.v_list == (void*)container.vval.v_list
    let same_ptr = obj.val.v_type == last_container.container.v_type
        && match (&obj.val.vval, &last_container.container.vval) {
            (v_list(Some(a)), v_list(Some(b))) => Rc::ptr_eq(a, b),
            (v_dict(Some(a)), v_dict(Some(b))) => Rc::ptr_eq(a, b),
            _ => false,
        };
    if same_ptr {
        container_stack.pop(); // c: (void)kv_pop(*container_stack);
        val_location = last_container.s;
        last_container = container_stack.last().unwrap().clone();
    }
    if last_container.container.v_type == VAR_LIST {
        // c: list container
        let list_rc = match &last_container.container.vval {
            v_list(Some(l)) => l.clone(),
            _ => unreachable!(),
        };
        if tv_list_len(&list_rc.borrow()) != 0 && !obj.didcomma {
            semsg(&format!(
                "E474: Expected comma before list item: {}",
                suffix(val_location)
            ));
            let mut ov = obj.val;
            tv_clear(&mut ov);
            return FAIL;
        }
        debug_assert!(last_container.special_val.is_none());
        tv_list_append_owned_tv(&mut list_rc.borrow_mut(), obj.val);
    } else if last_container.stack_index + 2 == stack.len() {
        // c: last_container.stack_index == kv_size(*stack) - 2  (dict value)
        if !obj.didcolon {
            semsg(&format!(
                "E474: Expected colon before dictionary value: {}",
                suffix(val_location)
            ));
            let mut ov = obj.val;
            tv_clear(&mut ov);
            return FAIL;
        }
        let mut key = stack.pop().unwrap(); // c: ValuesStackItem key = kv_pop(*stack);
        match &last_container.special_val {
            None => {
                // c: assert(!(key.is_special_string || key.val.vval.v_string == NULL));
                debug_assert!(!(key.is_special_string || !matches!(key.val.vval, v_string(_))));
                // c: dictitem_T *obj_di = tv_dict_item_alloc(key.val.vval.v_string);
                let keystr = match &key.val.vval {
                    v_string(s) => s.clone(),
                    _ => String::new(),
                };
                tv_clear(&mut key.val); // c: tv_clear(&key.val);
                let dict_rc = match &last_container.container.vval {
                    v_dict(Some(d)) => d.clone(),
                    _ => unreachable!(),
                };
                // c: if (tv_dict_add(dict, obj_di) == FAIL) abort(); obj_di->di_tv = obj.val;
                if tv_dict_add(&mut dict_rc.borrow_mut(), &keystr, obj.val) == FAIL {
                    std::process::abort();
                }
            }
            Some(special_val) => {
                // c: list_T *kv_pair = tv_list_alloc(2);
                let kv_pair = tv_list_alloc(2);
                tv_list_append_list(&mut special_val.borrow_mut(), kv_pair.clone());
                tv_list_append_owned_tv(&mut kv_pair.borrow_mut(), key.val);
                tv_list_append_owned_tv(&mut kv_pair.borrow_mut(), obj.val);
            }
        }
    } else {
        // Object with key only
        if !obj.is_special_string && obj.val.v_type != VAR_STRING {
            semsg(&format!("E474: Expected string key: {}", suffix(*pp)));
            let mut ov = obj.val;
            tv_clear(&mut ov);
            return FAIL;
        } else if !obj.didcomma
            && last_container.special_val.is_none()
            && match &last_container.container.vval {
                // c: DICT_LEN(dict) != 0
                v_dict(Some(d)) => d.borrow().dv_hashtab.len() != 0,
                _ => false,
            }
        {
            semsg(&format!(
                "E474: Expected comma before dictionary key: {}",
                suffix(val_location)
            ));
            let mut ov = obj.val;
            tv_clear(&mut ov);
            return FAIL;
        }
        // Handle special dictionaries
        // c: special_val == NULL && (obj.is_special_string || obj.vval.v_string == NULL
        //    || tv_dict_find(dict, obj.vval.v_string, -1))
        // RUST-PORT NOTE: v_string is never NULL here (decode_string always yields
        // a real String), so that middle sub-condition drops out.
        let restart = last_container.special_val.is_none() && {
            obj.is_special_string
                || match &last_container.container.vval {
                    v_dict(Some(d)) => match &obj.val.vval {
                        v_string(s) => tv_dict_find(&d.borrow(), s).is_some(),
                        _ => false,
                    },
                    _ => false,
                }
        };
        if restart {
            let mut ov = obj.val;
            tv_clear(&mut ov); // c: tv_clear(&obj.val);
            container_stack.pop(); // c: (void)kv_pop(*container_stack);
                                   // c: ValuesStackItem last_container_val = kv_A(*stack, stack_index);
            let dc = stack[last_container.stack_index].didcomma;
            let dcol = stack[last_container.stack_index].didcolon;
            // c: while (kv_size(*stack) > stack_index) tv_clear(&(kv_pop(*stack).val));
            while stack.len() > last_container.stack_index {
                let mut it = stack.pop().unwrap();
                tv_clear(&mut it.val);
            }
            *pp = last_container.s;
            *didcomma = dc;
            *didcolon = dcol;
            *next_map_special = true;
            return OK;
        }
        stack.push(obj); // c: kv_push(*stack, obj);
    }
    OK
}

/// Create a new special dictionary that ought to represent a MAP
///
/// Port of `decode_create_map_special_dict()` from `csrc/eval/decode.c:230`.
///
/// @return [allocated] list which should contain key-value pairs.
pub fn decode_create_map_special_dict(ret_tv: &mut typval_T, len: isize) -> Rc<RefCell<list_T>> {
    // c: list_T *const list = tv_list_alloc(len); tv_list_ref(list);
    let list = tv_list_alloc(len);
    tv_list_ref(&mut list.borrow_mut());
    // c: create_special_dict(ret_tv, kMPMap, { VAR_LIST, VAR_UNLOCKED, .v_list = list });
    create_special_dict(
        ret_tv,
        MessagePackType::kMPMap,
        typval_T {
            v_type: VAR_LIST,
            v_lock: VAR_UNLOCKED,
            vval: v_list(Some(list.clone())),
        },
    );
    list
}

/// Convert char* string to typval_T
///
/// Port of `decode_string()` from `csrc/eval/decode.c:257`.
///
/// Depending on whether string has (no) NUL bytes, it may use a special
/// dictionary, VAR_BLOB, or decode string to VAR_STRING.
///
/// RUST-PORT NOTE: the C `s_allocated` flag only chooses between adopting the
/// caller's buffer and copying it; both yield the same owned bytes here, so the
/// two branches collapse.
pub fn decode_string(s: &[u8], len: usize, force_blob: bool, _s_allocated: bool) -> typval_T {
    // c: assert(s != NULL || len == 0);
    // c: use_blob = force_blob || (s != NULL && memchr(s, NUL, len) != NULL);
    let use_blob = force_blob || s[..len].contains(&NUL);
    if use_blob {
        let mut tv = typval_T::default();
        tv.v_lock = VAR_UNLOCKED;
        let b = tv_blob_alloc_ret(&mut tv);
        // c: ga_data adopt / ga_concat_len — both land the len bytes in the blob.
        b.borrow_mut().bv_ga.extend_from_slice(&s[..len]);
        return tv;
    }
    // c: { VAR_STRING, VAR_UNLOCKED, .v_string = xmemdupz(s, len) };
    typval_T {
        v_type: VAR_STRING,
        v_lock: VAR_UNLOCKED,
        vval: v_string(String::from_utf8_lossy(&s[..len]).into_owned()),
    }
}

/// Parse JSON double-quoted string
///
/// Port of `parse_json_string()` from `csrc/eval/decode.c:301`.
///
/// @return OK in case of success, FAIL in case of error.
fn parse_json_string(
    buf: &[u8],
    pp: &mut usize,
    stack: &mut Vec<ValuesStackItem>,
    container_stack: &mut Vec<ContainerStackItem>,
    next_map_special: &mut bool,
    didcomma: &mut bool,
    didcolon: &mut bool,
) -> i32 {
    let e = buf.len(); // c: const char *const e = buf + buf_len;
    let mut p = *pp; // c: const char *p = *pp;
    let mut len: usize = 0;
    p += 1; // c: const char *const s = ++p;
    let s = p;
    let mut ret = OK;
    'string: {
        // ── validation pass (counts decoded byte length) ──
        while p < e && buf[p] != b'"' {
            if buf[p] == b'\\' {
                p += 1;
                if p == e {
                    semsg(&format!(
                        "E474: Unfinished escape sequence: {}",
                        String::from_utf8_lossy(buf)
                    ));
                    ret = FAIL;
                    break 'string;
                }
                match buf[p] {
                    b'u' => {
                        if p + 4 >= e {
                            semsg(&format!(
                                "E474: Unfinished unicode escape sequence: {}",
                                String::from_utf8_lossy(buf)
                            ));
                            ret = FAIL;
                            break 'string;
                        } else if !buf[p + 1].is_ascii_hexdigit()
                            || !buf[p + 2].is_ascii_hexdigit()
                            || !buf[p + 3].is_ascii_hexdigit()
                            || !buf[p + 4].is_ascii_hexdigit()
                        {
                            semsg(&format!(
                                "E474: Expected four hex digits after \\u: {}",
                                String::from_utf8_lossy(&buf[p - 1..])
                            ));
                            ret = FAIL;
                            break 'string;
                        }
                        // One UTF-8 character below U+10000 can take up to 3
                        // bytes, above up to 6, but they are encoded using two
                        // \u escapes.
                        len += 3;
                        p += 5;
                    }
                    b'\\' | b'/' | b'"' | b't' | b'b' | b'n' | b'r' | b'f' => {
                        len += 1;
                        p += 1;
                    }
                    _ => {
                        semsg(&format!(
                            "E474: Unknown escape sequence: {}",
                            String::from_utf8_lossy(&buf[p - 1..])
                        ));
                        ret = FAIL;
                        break 'string;
                    }
                }
            } else {
                let p_byte = buf[p];
                // unescaped = %x20-21 / %x23-5B / %x5D-10FFFF
                if p_byte < 0x20 {
                    semsg(&format!(
                        "E474: ASCII control characters cannot be present inside string: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    ret = FAIL;
                    break 'string;
                }
                let ch = utf_ptr2char(&buf[p..]);
                // All characters above U+007F are encoded using two or more
                // bytes and thus cannot possibly be equal to *p ... the only
                // exception is U+00C3 which is represented as 0xC3 0x83.
                if ch >= 0x80
                    && p_byte as i32 == ch
                    && !(ch == 0xC3 && p + 1 < e && buf[p + 1] == 0x83)
                {
                    semsg(&format!(
                        "E474: Only UTF-8 strings allowed: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    ret = FAIL;
                    break 'string;
                } else if ch > 0x10FFFF {
                    semsg(&format!(
                        "E474: Only UTF-8 code points up to U+10FFFF are allowed to appear unescaped: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    ret = FAIL;
                    break 'string;
                }
                let ch_len = utf_char2len(ch) as usize;
                debug_assert!(
                    ch_len
                        == if ch != 0 {
                            utf_ptr2len(&buf[p..]) as usize
                        } else {
                            1
                        }
                );
                len += ch_len;
                p += ch_len;
            }
        }
        if p == e || buf[p] != b'"' {
            semsg(&format!(
                "E474: Expected string end: {}",
                String::from_utf8_lossy(buf)
            ));
            ret = FAIL;
            break 'string;
        }
        // ── build pass ──
        // c: char *str = xmalloc(len + 1);
        let mut str_buf: Vec<u8> = Vec::with_capacity(len + 1);
        let mut fst_in_pair: i32 = 0;
        // c: #define PUT_FST_IN_PAIR(fst_in_pair, str_end)
        macro_rules! put_fst_in_pair {
            () => {{
                if fst_in_pair != 0 {
                    let mut b6 = [0u8; 6];
                    let n = utf_char2bytes(fst_in_pair, &mut b6);
                    str_buf.extend_from_slice(&b6[..n as usize]);
                    fst_in_pair = 0;
                }
            }};
        }
        let mut t = s;
        while t < p {
            // c: if (t[0] != '\\' || t[1] != 'u') PUT_FST_IN_PAIR(...);
            if buf[t] != b'\\' || buf.get(t + 1).copied() != Some(b'u') {
                put_fst_in_pair!();
            }
            if buf[t] == b'\\' {
                t += 1;
                match buf[t] {
                    b'u' => {
                        // c: const char ubuf[] = { t[1], t[2], t[3], t[4] };
                        let ubuf = [buf[t + 1], buf[t + 2], buf[t + 3], buf[t + 4]];
                        t += 4;
                        let ubuf_s = String::from_utf8_lossy(&ubuf).into_owned();
                        let mut ch: u64 = 0;
                        // c: vim_str2nr(ubuf, NULL, NULL, STR2NR_HEX|STR2NR_FORCE, NULL, &ch, 4, true, NULL);
                        crate::ported::charset::vim_str2nr(
                            &ubuf_s,
                            None,
                            None,
                            crate::ported::charset::STR2NR_HEX
                                | crate::ported::charset::STR2NR_FORCE,
                            None,
                            Some(&mut ch),
                            4,
                            true,
                            None,
                        );
                        if SURROGATE_HI_START <= ch && ch <= SURROGATE_HI_END {
                            put_fst_in_pair!();
                            fst_in_pair = ch as i32;
                        } else if SURROGATE_LO_START <= ch
                            && ch <= SURROGATE_LO_END
                            && fst_in_pair != 0
                        {
                            let full_char = (ch - SURROGATE_LO_START) as i32
                                + ((fst_in_pair - SURROGATE_HI_START as i32) << 10)
                                + SURROGATE_FIRST_CHAR;
                            let mut b6 = [0u8; 6];
                            let n = utf_char2bytes(full_char, &mut b6);
                            str_buf.extend_from_slice(&b6[..n as usize]);
                            fst_in_pair = 0;
                        } else {
                            put_fst_in_pair!();
                            let mut b6 = [0u8; 6];
                            let n = utf_char2bytes(ch as i32, &mut b6);
                            str_buf.extend_from_slice(&b6[..n as usize]);
                        }
                    }
                    // c: static const char escapes[] = { … };  *str_end++ = escapes[*t];
                    b'\\' => str_buf.push(b'\\'),
                    b'/' => str_buf.push(b'/'),
                    b'"' => str_buf.push(b'"'),
                    b't' => str_buf.push(TAB),
                    b'b' => str_buf.push(BS),
                    b'n' => str_buf.push(NL),
                    b'r' => str_buf.push(CAR),
                    b'f' => str_buf.push(FF),
                    _ => std::process::abort(), // c: default: abort();
                }
            } else {
                str_buf.push(buf[t]); // c: *str_end++ = *t;
            }
            t += 1; // c: for-loop t++
        }
        put_fst_in_pair!();
        // c: *str_end = NUL; (the trailing NUL is implicit in Rust's owned bytes)
        // c: typval_T obj = decode_string(str, str_end - str, false, true);
        let obj = decode_string(&str_buf, str_buf.len(), false, true);
        // c: POP(obj, obj.v_type != VAR_STRING);
        let is_sp = obj.v_type != VAR_STRING;
        let item = ValuesStackItem {
            is_special_string: is_sp,
            val: obj,
            didcomma: *didcomma,
            didcolon: *didcolon,
        };
        if json_decoder_pop(
            buf,
            item,
            stack,
            container_stack,
            &mut p,
            next_map_special,
            didcomma,
            didcolon,
        ) == FAIL
        {
            ret = FAIL;
            break 'string;
        }
        // (if *next_map_special: fall through to ret with ret == OK)
    }
    // parse_json_string_ret:
    *pp = p;
    ret
}

/// Parse JSON number: both floating-point and integer
///
/// Port of `parse_json_number()` from `csrc/eval/decode.c:492`.
/// Number format: `-?\d+(?:.\d+)?(?:[eE][+-]?\d+)?`.
///
/// @return OK in case of success, FAIL in case of error.
fn parse_json_number(
    buf: &[u8],
    pp: &mut usize,
    stack: &mut Vec<ValuesStackItem>,
    container_stack: &mut Vec<ContainerStackItem>,
    next_map_special: &mut bool,
    didcomma: &mut bool,
    didcolon: &mut bool,
) -> i32 {
    let e = buf.len();
    let mut p = *pp;
    let mut ret = OK;
    let s = p;
    let mut ints: Option<usize> = None;
    let mut fracs: Option<usize> = None;
    let mut exps: Option<usize> = None;
    let mut exps_s: Option<usize> = None;
    let mut fail = false;
    'check: {
        'scan: {
            if buf[p] == b'-' {
                p += 1;
            }
            ints = Some(p);
            if p >= e {
                break 'scan; // c: goto parse_json_number_check;
            }
            while p < e && buf[p].is_ascii_digit() {
                p += 1;
            }
            // c: if (p != ints + 1 && *ints == '0') { leading zeroes; goto fail; }
            if p != ints.unwrap() + 1 && buf[ints.unwrap()] == b'0' {
                semsg(&format!(
                    "E474: Leading zeroes are not allowed: {}",
                    String::from_utf8_lossy(&buf[s..])
                ));
                fail = true;
                break 'check;
            }
            if p >= e || p == ints.unwrap() {
                break 'scan;
            }
            if buf[p] == b'.' {
                p += 1;
                fracs = Some(p);
                while p < e && buf[p].is_ascii_digit() {
                    p += 1;
                }
                if p >= e || p == fracs.unwrap() {
                    break 'scan;
                }
            }
            if buf[p] == b'e' || buf[p] == b'E' {
                p += 1;
                exps_s = Some(p);
                if p < e && (buf[p] == b'-' || buf[p] == b'+') {
                    p += 1;
                }
                exps = Some(p);
                while p < e && buf[p].is_ascii_digit() {
                    p += 1;
                }
            }
        }
        // parse_json_number_check:
        if Some(p) == ints {
            semsg(&format!(
                "E474: Missing number after minus sign: {}",
                String::from_utf8_lossy(&buf[s..])
            ));
            fail = true;
            break 'check;
        } else if Some(p) == fracs || (fracs.is_some() && exps_s == fracs.map(|f| f + 1)) {
            semsg(&format!(
                "E474: Missing number after decimal dot: {}",
                String::from_utf8_lossy(&buf[s..])
            ));
            fail = true;
            break 'check;
        } else if Some(p) == exps {
            semsg(&format!(
                "E474: Missing exponent: {}",
                String::from_utf8_lossy(&buf[s..])
            ));
            fail = true;
            break 'check;
        }
        // c: typval_T tv = { VAR_NUMBER, VAR_UNLOCKED };
        let mut tv = typval_T {
            v_type: VAR_NUMBER,
            v_lock: VAR_UNLOCKED,
            vval: v_number(0),
        };
        let exp_num_len = p - s; // c: const size_t exp_num_len = (size_t)(p - s);
                                 // RUST-PORT NOTE: C passes the NUL-terminated `s` and lets string2float /
                                 // vim_str2nr find the token; here the scanned slice buf[s..p] (all ASCII)
                                 // is passed, which is exactly that token.
        let numstr = String::from_utf8_lossy(&buf[s..p]).into_owned();
        if fracs.is_some() || exps.is_some() {
            // Convert floating-point number
            let (fv, num_len) = string2float(&numstr);
            if exp_num_len != num_len {
                semsg(&format!(
                    "E685: internal error: while converting number \"{}\" to float string2float consumed {} bytes in place of {}",
                    numstr, num_len, exp_num_len
                ));
            }
            tv.v_type = VAR_FLOAT;
            tv.vval = v_float(fv);
        } else {
            // Convert integer
            let mut nr: varnumber_T = 0;
            let mut num_len: i32 = 0;
            crate::ported::charset::vim_str2nr(
                &numstr,
                None,
                Some(&mut num_len),
                0,
                Some(&mut nr),
                None,
                (p - s) as i32,
                true,
                None,
            );
            if exp_num_len as i32 != num_len {
                semsg(&format!(
                    "E685: internal error: while converting number \"{}\" to integer vim_str2nr consumed {} bytes in place of {}",
                    numstr, num_len, exp_num_len
                ));
            }
            tv.vval = v_number(nr);
        }
        // c: if (json_decoder_pop(OBJ(tv, false, *didcomma, *didcolon), …) == FAIL) goto fail;
        let item = ValuesStackItem {
            is_special_string: false,
            val: tv,
            didcomma: *didcomma,
            didcolon: *didcolon,
        };
        if json_decoder_pop(
            buf,
            item,
            stack,
            container_stack,
            &mut p,
            next_map_special,
            didcomma,
            didcolon,
        ) == FAIL
        {
            fail = true;
            break 'check;
        }
        if *next_map_special {
            break 'check; // c: goto parse_json_number_ret;
        }
        p -= 1; // c: p--;
    }
    if fail {
        ret = FAIL; // parse_json_number_fail:
    }
    // parse_json_number_ret:
    *pp = p;
    ret
}

/// Convert JSON string into Vimscript object
///
/// Port of `json_decode_string()` from `csrc/eval/decode.c:619`.
///
/// RUST-PORT NOTE (signature): C is `int json_decode_string(const char *buf,
/// size_t buf_len, typval_T *rettv)` returning OK/FAIL and writing `*rettv`.
/// Here the entry takes the document as `&str` and returns the value as
/// `Some(typval_T)` on OK / `None` on FAIL, matching how `f_json_decode` uses
/// it.
pub fn json_decode_string(input: &str) -> Option<typval_T> {
    let buf = input.as_bytes();
    let e = buf.len(); // c: const char *const e = buf + buf_len;
    let mut p = 0usize; // c: const char *p = buf;
    while p < e && (buf[p] == b' ' || buf[p] == TAB || buf[p] == NL || buf[p] == CAR) {
        p += 1;
    }
    if p == e {
        emsg("E474: Attempt to decode a blank string");
        return None; // c: return FAIL;
    }
    let mut stack: Vec<ValuesStackItem> = Vec::new(); // c: ValuesStack stack = KV_INITIAL_VALUE;
    let mut container_stack: Vec<ContainerStackItem> = Vec::new();
    let mut didcomma = false;
    let mut didcolon = false;
    let mut next_map_special = false;

    let mut goto_fail = false;
    let mut goto_after_cycle = false;

    // c: for (; p < e; p++) { json_decode_string_cycle_start: switch (*p) { … } … }
    while p < e {
        // json_decode_string_cycle_start:
        debug_assert!(buf[p] == b'{' || !next_map_special);
        let mut do_continue = false; // c: continue;
        let mut do_restart = false; // c: goto json_decode_string_cycle_start;

        // c: #define POP(obj_tv, is_sp_string) — sets goto_fail / do_restart.
        macro_rules! pop {
            ($obj:expr, $is_sp:expr) => {{
                let __item = ValuesStackItem {
                    is_special_string: $is_sp,
                    val: $obj,
                    didcomma,
                    didcolon,
                };
                if json_decoder_pop(
                    buf,
                    __item,
                    &mut stack,
                    &mut container_stack,
                    &mut p,
                    &mut next_map_special,
                    &mut didcomma,
                    &mut didcolon,
                ) == FAIL
                {
                    goto_fail = true;
                } else if next_map_special {
                    do_restart = true;
                }
            }};
        }

        match buf[p] {
            b'}' | b']' => {
                if container_stack.is_empty() {
                    semsg(&format!(
                        "E474: No container to close: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    let last_container = container_stack.last().unwrap().clone();
                    if buf[p] == b'}' && last_container.container.v_type != VAR_DICT {
                        semsg(&format!(
                            "E474: Closing list with curly bracket: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if buf[p] == b']' && last_container.container.v_type != VAR_LIST {
                        semsg(&format!(
                            "E474: Closing dictionary with square bracket: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if didcomma {
                        semsg(&format!(
                            "E474: Trailing comma: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if didcolon {
                        semsg(&format!(
                            "E474: Expected value after colon: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if last_container.stack_index + 1 != stack.len() {
                        debug_assert!(last_container.stack_index < stack.len() - 1);
                        semsg(&format!(
                            "E474: Expected value: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if stack.len() == 1 {
                        p += 1;
                        container_stack.pop();
                        goto_after_cycle = true;
                    } else {
                        let top = stack.pop().unwrap();
                        if json_decoder_pop(
                            buf,
                            top,
                            &mut stack,
                            &mut container_stack,
                            &mut p,
                            &mut next_map_special,
                            &mut didcomma,
                            &mut didcolon,
                        ) == FAIL
                        {
                            goto_fail = true;
                        } else {
                            debug_assert!(!next_map_special);
                            // c: break; -> falls to bottom of loop
                        }
                    }
                }
            }
            b',' => {
                if container_stack.is_empty() {
                    semsg(&format!(
                        "E474: Comma not inside container: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    let last_container = container_stack.last().unwrap().clone();
                    if didcomma {
                        semsg(&format!(
                            "E474: Duplicate comma: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if didcolon {
                        semsg(&format!(
                            "E474: Comma after colon: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if last_container.container.v_type == VAR_DICT
                        && last_container.stack_index + 1 != stack.len()
                    {
                        semsg(&format!(
                            "E474: Using comma in place of colon: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if match &last_container.special_val {
                        None => match &last_container.container.vval {
                            v_dict(Some(d)) => d.borrow().dv_hashtab.is_empty(),
                            v_list(Some(l)) => tv_list_len(&l.borrow()) == 0,
                            _ => true,
                        },
                        Some(sv) => tv_list_len(&sv.borrow()) == 0,
                    } {
                        semsg(&format!(
                            "E474: Leading comma: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else {
                        didcomma = true;
                        do_continue = true; // c: continue;
                    }
                }
            }
            b':' => {
                if container_stack.is_empty() {
                    semsg(&format!(
                        "E474: Colon not inside container: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    let last_container = container_stack.last().unwrap().clone();
                    if last_container.container.v_type != VAR_DICT {
                        semsg(&format!(
                            "E474: Using colon not in dictionary: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if last_container.stack_index + 2 != stack.len() {
                        semsg(&format!(
                            "E474: Unexpected colon: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if didcomma {
                        semsg(&format!(
                            "E474: Colon after comma: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else if didcolon {
                        semsg(&format!(
                            "E474: Duplicate colon: {}",
                            String::from_utf8_lossy(&buf[p..])
                        ));
                        goto_fail = true;
                    } else {
                        didcolon = true;
                        do_continue = true; // c: continue;
                    }
                }
            }
            b' ' | TAB | NL | CAR => {
                do_continue = true; // c: continue;
            }
            b'n' => {
                if p + 3 >= e || &buf[p + 1..p + 4] != b"ull" {
                    semsg(&format!(
                        "E474: Expected null: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    p += 3;
                    pop!(
                        typval_T {
                            v_type: VAR_SPECIAL,
                            v_lock: VAR_UNLOCKED,
                            vval: v_special(kSpecialVarNull),
                        },
                        false
                    );
                }
            }
            b't' => {
                if p + 3 >= e || &buf[p + 1..p + 4] != b"rue" {
                    semsg(&format!(
                        "E474: Expected true: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    p += 3;
                    pop!(
                        typval_T {
                            v_type: VAR_BOOL,
                            v_lock: VAR_UNLOCKED,
                            vval: v_bool(kBoolVarTrue),
                        },
                        false
                    );
                }
            }
            b'f' => {
                if p + 4 >= e || &buf[p + 1..p + 5] != b"alse" {
                    semsg(&format!(
                        "E474: Expected false: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                } else {
                    p += 4;
                    pop!(
                        typval_T {
                            v_type: VAR_BOOL,
                            v_lock: VAR_UNLOCKED,
                            vval: v_bool(kBoolVarFalse),
                        },
                        false
                    );
                }
            }
            b'"' => {
                if parse_json_string(
                    buf,
                    &mut p,
                    &mut stack,
                    &mut container_stack,
                    &mut next_map_special,
                    &mut didcomma,
                    &mut didcolon,
                ) == FAIL
                {
                    // Error message was already given
                    goto_fail = true;
                } else if next_map_special {
                    do_restart = true; // c: goto json_decode_string_cycle_start;
                }
            }
            b'-' | b'0'..=b'9' => {
                if parse_json_number(
                    buf,
                    &mut p,
                    &mut stack,
                    &mut container_stack,
                    &mut next_map_special,
                    &mut didcomma,
                    &mut didcolon,
                ) == FAIL
                {
                    goto_fail = true;
                } else if next_map_special {
                    do_restart = true;
                }
            }
            b'[' => {
                // c: list_T *list = tv_list_alloc(kListLenMayKnow); tv_list_ref(list);
                let list = tv_list_alloc(kListLenMayKnow);
                tv_list_ref(&mut list.borrow_mut());
                let tv = typval_T {
                    v_type: VAR_LIST,
                    v_lock: VAR_UNLOCKED,
                    vval: v_list(Some(list)),
                };
                container_stack.push(ContainerStackItem {
                    stack_index: stack.len(),
                    s: p,
                    container: tv.clone(),
                    special_val: None,
                });
                stack.push(ValuesStackItem {
                    is_special_string: false,
                    val: tv,
                    didcomma,
                    didcolon,
                });
            }
            b'{' => {
                let tv;
                let val_list;
                if next_map_special {
                    next_map_special = false;
                    let mut ret_tv = typval_T::default();
                    val_list = Some(decode_create_map_special_dict(&mut ret_tv, kListLenMayKnow));
                    tv = ret_tv;
                } else {
                    let dict = tv_dict_alloc();
                    dict.borrow_mut().dv_refcount += 1;
                    tv = typval_T {
                        v_type: VAR_DICT,
                        v_lock: VAR_UNLOCKED,
                        vval: v_dict(Some(dict)),
                    };
                    val_list = None;
                }
                container_stack.push(ContainerStackItem {
                    stack_index: stack.len(),
                    s: p,
                    container: tv.clone(),
                    special_val: val_list,
                });
                stack.push(ValuesStackItem {
                    is_special_string: false,
                    val: tv,
                    didcomma,
                    didcolon,
                });
            }
            _ => {
                semsg(&format!(
                    "E474: Unidentified byte: {}",
                    String::from_utf8_lossy(&buf[p..])
                ));
                goto_fail = true;
            }
        }

        if goto_fail || goto_after_cycle {
            break;
        }
        if do_restart {
            continue; // goto json_decode_string_cycle_start (no p++)
        }
        if do_continue {
            p += 1; // c: continue; -> loop increment
            continue;
        }
        // ── bottom of the loop body ──
        didcomma = false;
        didcolon = false;
        if container_stack.is_empty() {
            p += 1;
            break; // c: break; -> json_decode_string_after_cycle
        }
        p += 1; // c: for-loop increment
    }

    if !goto_fail {
        // json_decode_string_after_cycle:
        while p < e {
            match buf[p] {
                NL | b' ' | TAB | CAR => {}
                _ => {
                    semsg(&format!(
                        "E474: Trailing characters: {}",
                        String::from_utf8_lossy(&buf[p..])
                    ));
                    goto_fail = true;
                    break;
                }
            }
            p += 1;
        }
        if !goto_fail {
            if stack.len() == 1 && container_stack.is_empty() {
                // c: *rettv = kv_pop(stack).val; goto ret;
                return Some(stack.pop().unwrap().val);
            }
            semsg(&format!(
                "E474: Unexpected end of input: {}",
                String::from_utf8_lossy(buf)
            ));
            goto_fail = true;
        }
    }

    // json_decode_string_fail:  ret = FAIL; while (kv_size(stack)) tv_clear(pop.val);
    while let Some(mut it) = stack.pop() {
        tv_clear(&mut it.val);
    }
    None // ret = FAIL
}

/// Port of `positive_integer_to_special_typval()` from `csrc/eval/decode.c:888`.
///
/// Store a `uint64_t` in `rettv`, spilling to a `kMPInteger` special dictionary
/// when the value does not fit in a signed `varnumber_T`.
fn positive_integer_to_special_typval(rettv: &mut typval_T, val: u64) {
    if val <= VARNUMBER_MAX as u64 {
        *rettv = typval_T {
            v_type: VAR_NUMBER,
            v_lock: VAR_UNLOCKED,
            vval: v_number(val as varnumber_T),
        };
    } else {
        // c: list_T *const list = tv_list_alloc(4); tv_list_ref(list);
        let list = tv_list_alloc(4);
        tv_list_ref(&mut list.borrow_mut());
        create_special_dict(
            rettv,
            MessagePackType::kMPInteger,
            typval_T {
                v_type: VAR_LIST,
                v_lock: VAR_UNLOCKED,
                vval: v_list(Some(list.clone())),
            },
        );
        tv_list_append_number(&mut list.borrow_mut(), 1);
        tv_list_append_number(&mut list.borrow_mut(), ((val >> 62) & 0x3) as varnumber_T);
        tv_list_append_number(
            &mut list.borrow_mut(),
            ((val >> 31) & 0x7FFFFFFF) as varnumber_T,
        );
        tv_list_append_number(&mut list.borrow_mut(), (val & 0x7FFFFFFF) as varnumber_T);
    }
}

/// The libmpack node `data[0]`/`data[1]` `void *p` payload for the typval parse.
///
/// RUST-PORT NOTE: `csrc/eval/decode.c` casts the untyped `mpack_data_t.p`
/// (object.h:27) to four different pointee types across the walk —
/// `typval_T *result` (a write location), `list_T *` (an array's list),
/// `char *` (a str/bin/ext byte buffer) and `typval_T (*)[2]` (a map's pending
/// key/value pairs). Rust cannot store a raw pointer into the shared
/// `Rc<RefCell<…>>` containers, so this enum carries the corresponding owned
/// handle instead. `data[0]` always holds a `root`/`list_elem`/`map_elem`
/// write-location; `data[1]` holds a `bytes`/`list`/`map_items` payload.
#[derive(Debug, Clone)]
enum TypvalNodeData {
    /// `data[0]`: the parser's root result (`parser.data.p`).
    root(Rc<RefCell<typval_T>>),
    /// `data[0]`: `tv_list_append_owned_tv(list, …)` — the appended list slot.
    list_elem(Rc<RefCell<list_T>>, usize),
    /// `data[0]`: `&items[pos][key_visited]` of a map's pending pair array.
    map_elem(Rc<RefCell<Vec<[typval_T; 2]>>>, usize, usize),
    /// `data[1]`: `xmallocz(tok.length)` — the str/bin/ext byte buffer.
    bytes(Rc<RefCell<Vec<u8>>>),
    /// `data[1]`: the array's `list_T`.
    list(Rc<RefCell<list_T>>),
    /// `data[1]`: `xmallocz(tok.length * 2 * sizeof(typval_T))` — the map's
    /// pending key/value pairs.
    map_items(Rc<RefCell<Vec<[typval_T; 2]>>>),
}

/// Port of `typval_parse_enter()` from `csrc/eval/decode.c:911`.
fn typval_parse_enter(parser: &mut mpack_parser_t<TypvalNodeData>, node: usize) {
    // RUST-PORT NOTE: the C `*result = …` store writes through a raw
    // `typval_T *`; here `result` is a [`TypvalNodeData`] write-location handle,
    // so the store goes through this local closure (glue for the pointer-to-enum
    // deviation only — not a C function).
    let set_result = |loc: &Option<TypvalNodeData>, tv: typval_T| match loc {
        Some(TypvalNodeData::root(rc)) => *rc.borrow_mut() = tv,
        Some(TypvalNodeData::list_elem(l, i)) => l.borrow_mut().lv_items[*i].li_tv = tv,
        Some(TypvalNodeData::map_elem(m, pos, key)) => m.borrow_mut()[*pos][*key] = tv,
        _ => {}
    };

    // c: typval_T *result = NULL;
    let mut result: Option<TypvalNodeData> = None;

    // c: mpack_node_t *parent = MPACK_PARENT_NODE(node);  (object.h:11)
    let parent = node - 1;
    if parser.items[parent].pos != usize::MAX {
        match parser.items[parent].tok.r#type {
            MPACK_TOKEN_ARRAY => {
                // c: list_T *list = parent->data[1].p;
                let list = match &parser.items[parent].data[1] {
                    mpack_data_t::p(TypvalNodeData::list(l)) => l.clone(),
                    _ => std::process::abort(),
                };
                // c: result = tv_list_append_owned_tv(list, { .v_type = VAR_UNKNOWN });
                let idx = tv_list_append_owned_tv(&mut list.borrow_mut(), typval_T::default());
                result = Some(TypvalNodeData::list_elem(list, idx));
            }
            MPACK_TOKEN_MAP => {
                // c: typval_T(*items)[2] = parent->data[1].p;
                //    result = &items[parent->pos][parent->key_visited];
                let items = match &parser.items[parent].data[1] {
                    mpack_data_t::p(TypvalNodeData::map_items(m)) => m.clone(),
                    _ => std::process::abort(),
                };
                let pos = parser.items[parent].pos;
                let key = parser.items[parent].key_visited as usize;
                result = Some(TypvalNodeData::map_elem(items, pos, key));
            }
            MPACK_TOKEN_STR | MPACK_TOKEN_BIN | MPACK_TOKEN_EXT => {
                debug_assert!(parser.items[node].tok.r#type == MPACK_TOKEN_CHUNK);
                // result stays None.
            }
            _ => std::process::abort(), // c: default: abort();
        }
    } else {
        // c: result = parser->data.p;
        result = match &parser.data {
            mpack_data_t::p(p) => Some(p.clone()),
            _ => None,
        };
    }

    // c: node->data[0].p = result;  node->data[1].p = NULL;  // free on error if non-NULL
    parser.items[node].data[0] = match &result {
        Some(r) => mpack_data_t::p(r.clone()),
        None => mpack_data_t::Null,
    };
    parser.items[node].data[1] = mpack_data_t::Null;

    match parser.items[node].tok.r#type {
        MPACK_TOKEN_NIL => {
            // c: *result = { VAR_SPECIAL, VAR_UNLOCKED, .v_special = kSpecialVarNull };
            set_result(
                &result,
                typval_T {
                    v_type: VAR_SPECIAL,
                    v_lock: VAR_UNLOCKED,
                    vval: v_special(kSpecialVarNull),
                },
            );
        }
        MPACK_TOKEN_BOOLEAN => {
            // c: .v_bool = mpack_unpack_boolean(node->tok) ? kBoolVarTrue : kBoolVarFalse
            let b = if mpack_unpack_boolean(&parser.items[node].tok) {
                kBoolVarTrue
            } else {
                kBoolVarFalse
            };
            set_result(
                &result,
                typval_T {
                    v_type: VAR_BOOL,
                    v_lock: VAR_UNLOCKED,
                    vval: v_bool(b),
                },
            );
        }
        MPACK_TOKEN_SINT => {
            // c: .v_number = mpack_unpack_sint(node->tok)
            set_result(
                &result,
                typval_T {
                    v_type: VAR_NUMBER,
                    v_lock: VAR_UNLOCKED,
                    vval: v_number(mpack_unpack_sint(&parser.items[node].tok) as varnumber_T),
                },
            );
        }
        MPACK_TOKEN_UINT => {
            // c: positive_integer_to_special_typval(result, mpack_unpack_uint(node->tok));
            let mut tmp = typval_T::default();
            positive_integer_to_special_typval(
                &mut tmp,
                mpack_unpack_uint(&parser.items[node].tok),
            );
            set_result(&result, tmp);
        }
        MPACK_TOKEN_FLOAT => {
            // c: .v_float = mpack_unpack_float(node->tok)
            set_result(
                &result,
                typval_T {
                    v_type: VAR_FLOAT,
                    v_lock: VAR_UNLOCKED,
                    vval: v_float(mpack_unpack_float(&parser.items[node].tok)),
                },
            );
        }
        MPACK_TOKEN_BIN | MPACK_TOKEN_STR | MPACK_TOKEN_EXT => {
            // actually converted in typval_parse_exit after the data chunks
            // c: node->data[1].p = xmallocz(node->tok.length);
            let len = parser.items[node].tok.length as usize;
            parser.items[node].data[1] =
                mpack_data_t::p(TypvalNodeData::bytes(Rc::new(RefCell::new(vec![0u8; len]))));
        }
        MPACK_TOKEN_CHUNK => {
            // c: char *data = parent->data[1].p;
            //    memcpy(data + parent->pos, node->tok.data.chunk_ptr, node->tok.length);
            let buf = match &parser.items[parent].data[1] {
                mpack_data_t::p(TypvalNodeData::bytes(b)) => b.clone(),
                _ => std::process::abort(),
            };
            let ppos = parser.items[parent].pos;
            let len = parser.items[node].tok.length as usize;
            let chunk = match &parser.items[node].tok.data {
                mpack_token_data::chunk_ptr(c) => c.clone(),
                _ => std::process::abort(),
            };
            buf.borrow_mut()[ppos..ppos + len].copy_from_slice(&chunk[..len]);
        }
        MPACK_TOKEN_ARRAY => {
            // c: list_T *const list = tv_list_alloc((ptrdiff_t)node->tok.length);
            //    tv_list_ref(list); *result = { VAR_LIST, list }; node->data[1].p = list;
            let list = tv_list_alloc(parser.items[node].tok.length as isize);
            tv_list_ref(&mut list.borrow_mut());
            set_result(
                &result,
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VAR_UNLOCKED,
                    vval: v_list(Some(list.clone())),
                },
            );
            parser.items[node].data[1] = mpack_data_t::p(TypvalNodeData::list(list));
        }
        MPACK_TOKEN_MAP => {
            // we don't know if this will be safe to convert to a typval dict yet
            // c: node->data[1].p = xmallocz(node->tok.length * 2 * sizeof(typval_T));
            let len = parser.items[node].tok.length as usize;
            let items: Vec<[typval_T; 2]> = (0..len)
                .map(|_| [typval_T::default(), typval_T::default()])
                .collect();
            parser.items[node].data[1] =
                mpack_data_t::p(TypvalNodeData::map_items(Rc::new(RefCell::new(items))));
        }
    }
}

/// Free node which was entered but never exited, due to a nested error
///
/// Port of `typval_parser_error_free()` from `csrc/eval/decode.c:1016`.
///
/// Don't bother with typvals as these will be GC:d eventually.
///
/// RUST-PORT NOTE: the C `XFREE_CLEAR(node->data[1].p)` frees the manually
/// `xmallocz`'d byte / map-pair buffers of BIN/STR/EXT/MAP nodes. Here those are
/// `Rc<RefCell<…>>` handles in [`TypvalNodeData`] that drop automatically when
/// the parser is dropped, so there is nothing to free → no-op.
pub fn typval_parser_error_free(_parser: &mpack_parser_t<TypvalNodeData>) {}

/// Port of `typval_parse_exit()` from `csrc/eval/decode.c:1033`.
fn typval_parse_exit(parser: &mut mpack_parser_t<TypvalNodeData>, node: usize) {
    // RUST-PORT NOTE: see `typval_parse_enter` — the C `*result = …` store
    // becomes this write-location closure.
    let set_result = |loc: &Option<TypvalNodeData>, tv: typval_T| match loc {
        Some(TypvalNodeData::root(rc)) => *rc.borrow_mut() = tv,
        Some(TypvalNodeData::list_elem(l, i)) => l.borrow_mut().lv_items[*i].li_tv = tv,
        Some(TypvalNodeData::map_elem(m, pos, key)) => m.borrow_mut()[*pos][*key] = tv,
        _ => {}
    };

    // c: typval_T *result = node->data[0].p;
    let result = match &parser.items[node].data[0] {
        mpack_data_t::p(p) => Some(p.clone()),
        _ => None,
    };
    match parser.items[node].tok.r#type {
        MPACK_TOKEN_BIN | MPACK_TOKEN_STR => {
            // c: *result = decode_string(node->data[1].p, node->tok.length, false, true);
            //    node->data[1].p = NULL;
            let buf = match &parser.items[node].data[1] {
                mpack_data_t::p(TypvalNodeData::bytes(b)) => b.clone(),
                _ => std::process::abort(),
            };
            let len = parser.items[node].tok.length as usize;
            let tv = decode_string(&buf.borrow(), len, false, true);
            set_result(&result, tv);
            parser.items[node].data[1] = mpack_data_t::Null;
        }
        MPACK_TOKEN_EXT => {
            // c: list_T *const list = tv_list_alloc(2); tv_list_ref(list);
            let list = tv_list_alloc(2);
            tv_list_ref(&mut list.borrow_mut());
            // c: tv_list_append_number(list, node->tok.data.ext_type);
            let ext_type = match &parser.items[node].tok.data {
                mpack_token_data::ext_type(e) => *e,
                _ => std::process::abort(),
            };
            tv_list_append_number(&mut list.borrow_mut(), ext_type as varnumber_T);
            // c: list_T *const ext_val_list = tv_list_alloc(kListLenMayKnow);
            //    tv_list_append_list(list, ext_val_list);
            let ext_val_list = tv_list_alloc(kListLenMayKnow);
            tv_list_append_list(&mut list.borrow_mut(), ext_val_list.clone());
            // c: create_special_dict(result, kMPExt, { VAR_LIST, list });
            let mut tmp = typval_T::default();
            create_special_dict(
                &mut tmp,
                MessagePackType::kMPExt,
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VAR_UNLOCKED,
                    vval: v_list(Some(list)),
                },
            );
            set_result(&result, tmp);
            // c: encode_list_write(ext_val_list, node->data[1].p, node->tok.length);
            // RUST-PORT NOTE: the ported encode_list_write takes a `&str` (it
            // splits on '\n' into list items); the raw ext bytes are passed
            // lossily, matching the readfile()-style text convention used
            // elsewhere in this crate.
            let buf = match &parser.items[node].data[1] {
                mpack_data_t::p(TypvalNodeData::bytes(b)) => b.clone(),
                _ => std::process::abort(),
            };
            let len = parser.items[node].tok.length as usize;
            let ext_bytes = String::from_utf8_lossy(&buf.borrow()[..len]).into_owned();
            encode_list_write(&mut ext_val_list.borrow_mut(), &ext_bytes);
            parser.items[node].data[1] = mpack_data_t::Null; // c: XFREE_CLEAR
        }
        MPACK_TOKEN_MAP => {
            // c: typval_T(*items)[2] = node->data[1].p;
            let items_rc = match &parser.items[node].data[1] {
                mpack_data_t::p(TypvalNodeData::map_items(m)) => m.clone(),
                _ => std::process::abort(),
            };
            let length = parser.items[node].tok.length as usize;
            // c: for each key: if not a non-empty STRING → goto generic map.
            // RUST-PORT NOTE: C detects a duplicate key lazily (tv_dict_add
            // returns FAIL) and then rolls the half-built dict back to the
            // generic map; since Rust moves the values into the dict, the port
            // instead pre-scans for both invalid and duplicate keys so it never
            // has to un-move a value. Behaviour is identical (tv_dict_add fails
            // iff the key repeats).
            let mut generic = false;
            let mut keys: Vec<String> = Vec::new();
            {
                let items = items_rc.borrow();
                for i in 0..length {
                    let key = &items[i][0];
                    match (&key.v_type, &key.vval) {
                        (VAR_STRING, v_string(s)) if !s.is_empty() => keys.push(s.clone()),
                        _ => {
                            generic = true;
                            break;
                        }
                    }
                }
            }
            if !generic {
                let mut seen = std::collections::HashSet::new();
                for k in &keys {
                    if !seen.insert(k.clone()) {
                        generic = true;
                        break;
                    }
                }
            }
            if !generic {
                // c: dict_T *const dict = tv_dict_alloc(); dict->dv_refcount++;
                //    *result = { VAR_DICT, dict };
                let dict = tv_dict_alloc();
                dict.borrow_mut().dv_refcount += 1;
                let mut items = items_rc.borrow_mut();
                for i in 0..length {
                    // c: char *key = items[i][0].vval.v_string; di->di_tv = items[i][1];
                    let key = match &items[i][0].vval {
                        v_string(s) => s.clone(),
                        _ => String::new(),
                    };
                    let val = std::mem::take(&mut items[i][1]);
                    tv_dict_add(&mut dict.borrow_mut(), &key, val);
                }
                drop(items);
                set_result(
                    &result,
                    typval_T {
                        v_type: VAR_DICT,
                        v_lock: VAR_UNLOCKED,
                        vval: v_dict(Some(dict)),
                    },
                );
            } else {
                // msgpack_to_vim_generic_map:
                // c: list_T *const list = decode_create_map_special_dict(result, length);
                let mut tmp = typval_T::default();
                let list = decode_create_map_special_dict(&mut tmp, length as isize);
                let mut items = items_rc.borrow_mut();
                for i in 0..length {
                    // c: list_T *const kv_pair = tv_list_alloc(2);
                    //    tv_list_append_list(list, kv_pair);
                    let kv_pair = tv_list_alloc(2);
                    tv_list_append_list(&mut list.borrow_mut(), kv_pair.clone());
                    // c: tv_list_append_owned_tv(kv_pair, items[i][0]);
                    //    tv_list_append_owned_tv(kv_pair, items[i][1]);
                    let k = std::mem::take(&mut items[i][0]);
                    let v = std::mem::take(&mut items[i][1]);
                    tv_list_append_owned_tv(&mut kv_pair.borrow_mut(), k);
                    tv_list_append_owned_tv(&mut kv_pair.borrow_mut(), v);
                }
                drop(items);
                set_result(&result, tmp);
            }
            parser.items[node].data[1] = mpack_data_t::Null; // c: XFREE_CLEAR
        }
        _ => {
            // other kinds are handled completely in typval_parse_enter
        }
    }
}

/// Port of `mpack_parse_typval()` from `csrc/eval/decode.c:1117`.
pub fn mpack_parse_typval(
    parser: &mut mpack_parser_t<TypvalNodeData>,
    data: &mut &[u8],
    size: &mut usize,
) -> i32 {
    // c: return mpack_parse(parser, data, size, typval_parse_enter, typval_parse_exit);
    mpack_parse(parser, data, size, typval_parse_enter, typval_parse_exit)
}

/// Port of `unpack_typval()` from `csrc/eval/decode.c:1122`.
///
/// RUST-PORT NOTE (signature): C is `int unpack_typval(const char **data,
/// size_t *size, typval_T *ret)`; `data` is a moving `const char *` cursor with a
/// separate `size` remaining count — here `data: &mut &[u8]` (the cursor slice)
/// and `size: &mut usize` mirror that pair. C aliases `parser.data.p = ret` so
/// the parser writes straight into `*ret`; Rust cannot alias `ret` into the
/// parser, so the root result is built in an `Rc<RefCell<typval_T>>` and moved
/// into `*ret` on success.
pub fn unpack_typval(data: &mut &[u8], size: &mut usize, ret: &mut typval_T) -> i32 {
    // c: ret->v_type = VAR_UNKNOWN;
    *ret = typval_T::default();
    // c: mpack_parser_t parser; mpack_parser_init(&parser, 0);
    let mut parser: mpack_parser_t<TypvalNodeData> = mpack_parser_t {
        data: mpack_data_t::Null,
        size: 0,
        capacity: 0,
        status: 0,
        exiting: 0,
        tokbuf: crate::ported::mpack::mpack_tokbuf_t::default(),
        items: Vec::new(),
    };
    mpack_parser_init(&mut parser, 0);
    // c: parser.data.p = ret;
    let root = Rc::new(RefCell::new(typval_T::default()));
    parser.data = mpack_data_t::p(TypvalNodeData::root(root.clone()));
    // c: int status = mpack_parse_typval(&parser, data, size);
    let status = mpack_parse_typval(&mut parser, data, size);
    if status != MPACK_OK {
        // c: typval_parser_error_free(&parser); tv_clear(ret);
        typval_parser_error_free(&parser);
        tv_clear(ret);
    } else {
        *ret = std::mem::take(&mut *root.borrow_mut());
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::eval::encode::encode_tv2json;

    fn roundtrip(s: &str) -> String {
        encode_tv2json(&json_decode_string(s).expect("decode"))
    }

    #[test]
    fn scalars_and_containers() {
        assert_eq!(roundtrip("42"), "42");
        assert_eq!(roundtrip("  -7 "), "-7");
        assert_eq!(roundtrip("3.5"), "3.5");
        assert_eq!(roundtrip("[1,2,3]"), "[1,2,3]");
        assert_eq!(roundtrip("[]"), "[]");
        assert_eq!(roundtrip("{}"), "{}");
        assert_eq!(
            roundtrip(r#"{"a":1,"b":[true,null]}"#),
            r#"{"a":1,"b":[true,null]}"#
        );
    }

    #[test]
    fn string_escapes_and_unicode() {
        assert_eq!(roundtrip(r#""he\"llo""#), r#""he\"llo""#);
        // A == 'A'
        assert_eq!(roundtrip(r#""A""#), r#""A""#);
        // Directly-encoded multibyte U+1F600 (😀).
        let v = json_decode_string(r#""😀""#).expect("decode");
        match v.vval {
            v_string(s) => assert_eq!(s, "\u{1F600}"),
            _ => panic!("expected string"),
        }
        // Surrogate-pair \u escapes decode to the same U+1F600.
        let v = json_decode_string(r#""\uD83D\uDE00""#).expect("decode");
        match v.vval {
            v_string(s) => assert_eq!(s, "\u{1F600}"),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn nested_and_whitespace() {
        assert_eq!(roundtrip(" { \"k\" : [ 1 , 2 ] } "), r#"{"k":[1,2]}"#);
    }

    #[test]
    fn malformed_returns_none() {
        assert!(json_decode_string("").is_none());
        assert!(json_decode_string("   ").is_none());
        assert!(json_decode_string("{bad}").is_none());
        assert!(json_decode_string("[1,2").is_none());
        assert!(json_decode_string("42 garbage").is_none());
        assert!(json_decode_string("[1,]").is_none()); // trailing comma
        assert!(json_decode_string("01").is_none()); // leading zero
        assert!(json_decode_string("{\"a\" 1}").is_none()); // missing colon
    }

    #[test]
    fn duplicate_key_becomes_special_map() {
        // A duplicate key forces the special-map (_TYPE/_VAL) representation.
        let v = json_decode_string(r#"{"a":1,"a":2}"#).expect("decode");
        assert_eq!(v.v_type, VAR_DICT);
        match &v.vval {
            v_dict(Some(d)) => {
                assert!(d.borrow().dv_hashtab.contains_key("_TYPE"));
                assert!(d.borrow().dv_hashtab.contains_key("_VAL"));
            }
            _ => panic!("expected special dict"),
        }
    }

    #[test]
    fn blob_from_embedded_nul() {
        // A \\u0000 escape decodes to bytes containing a NUL, so
        // decode_string yields a Blob rather than a String.
        let v = json_decode_string("\"\\u0000\"").expect("decode");
        assert_eq!(v.v_type, VAR_BLOB);
    }

    // ── msgpack path (unpack_typval / typval_parse_enter / typval_parse_exit) ──

    /// Decode exactly one MessagePack object from `bytes`.
    fn mp_decode(bytes: &[u8]) -> typval_T {
        let mut data: &[u8] = bytes;
        let mut size = bytes.len();
        let mut ret = typval_T::default();
        let st = unpack_typval(&mut data, &mut size, &mut ret);
        assert_eq!(st, MPACK_OK, "unpack_typval status");
        ret
    }

    #[test]
    fn msgpack_scalars() {
        // positive fixint 42
        let v = mp_decode(&[0x2a]);
        assert_eq!(v.v_type, VAR_NUMBER);
        assert!(matches!(v.vval, v_number(42)));
        // negative fixint -1
        assert!(matches!(mp_decode(&[0xff]).vval, v_number(-1)));
        // int 64 = -1
        assert!(matches!(
            mp_decode(&[0xd3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).vval,
            v_number(-1)
        ));
        // nil → v:null
        assert_eq!(mp_decode(&[0xc0]).v_type, VAR_SPECIAL);
        // true / false
        assert!(matches!(mp_decode(&[0xc3]).vval, v_bool(kBoolVarTrue)));
        assert!(matches!(mp_decode(&[0xc2]).vval, v_bool(kBoolVarFalse)));
        // float64 1.5
        let mut fbytes = vec![0xcb];
        fbytes.extend_from_slice(&1.5f64.to_bits().to_be_bytes());
        match mp_decode(&fbytes).vval {
            v_float(f) => assert_eq!(f, 1.5),
            _ => panic!("expected float"),
        }
    }

    #[test]
    fn msgpack_uint64_overflow_special_dict() {
        // uint 64 = 2^63 (> VARNUMBER_MAX) → kMPInteger special dictionary.
        let mut b = vec![0xcf];
        b.extend_from_slice(&0x8000_0000_0000_0000u64.to_be_bytes());
        let v = mp_decode(&b);
        assert_eq!(v.v_type, VAR_DICT);
        match &v.vval {
            v_dict(Some(d)) => {
                assert!(d.borrow().dv_hashtab.contains_key("_TYPE"));
                assert!(d.borrow().dv_hashtab.contains_key("_VAL"));
            }
            _ => panic!("expected special dict"),
        }
    }

    #[test]
    fn msgpack_str_and_bin() {
        // fixstr "abc" → String
        let v = mp_decode(&[0xa3, b'a', b'b', b'c']);
        match v.vval {
            v_string(s) => assert_eq!(s, "abc"),
            _ => panic!("expected string"),
        }
        // bin without NUL → String (decode_string, force_blob=false)
        assert_eq!(mp_decode(&[0xc4, 0x02, 0x01, 0x02]).v_type, VAR_STRING);
        // bin WITH a NUL byte → Blob
        assert_eq!(mp_decode(&[0xc4, 0x02, 0x00, 0x01]).v_type, VAR_BLOB);
    }

    #[test]
    fn msgpack_array_and_map() {
        // fixarray [1,2,3]
        let v = mp_decode(&[0x93, 0x01, 0x02, 0x03]);
        assert_eq!(encode_tv2json(&v), "[1,2,3]");
        // nested array [[1],2]
        let v = mp_decode(&[0x92, 0x91, 0x01, 0x02]);
        assert_eq!(encode_tv2json(&v), "[[1],2]");
        // fixmap {"a":1,"b":2} (insertion-ordered dict)
        let v = mp_decode(&[0x82, 0xa1, b'a', 0x01, 0xa1, b'b', 0x02]);
        assert_eq!(v.v_type, VAR_DICT);
        assert_eq!(encode_tv2json(&v), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn msgpack_nonstring_key_becomes_special_map() {
        // {1:2} — integer key is not a valid dict key → generic special map.
        let v = mp_decode(&[0x81, 0x01, 0x02]);
        assert_eq!(v.v_type, VAR_DICT);
        match &v.vval {
            v_dict(Some(d)) => {
                assert!(d.borrow().dv_hashtab.contains_key("_TYPE"));
                assert!(d.borrow().dv_hashtab.contains_key("_VAL"));
            }
            _ => panic!("expected special dict"),
        }
    }

    #[test]
    fn msgpack_duplicate_key_becomes_special_map() {
        // {"a":1,"a":2} — duplicate key → generic special map (tv_dict_add FAIL).
        let v = mp_decode(&[0x82, 0xa1, b'a', 0x01, 0xa1, b'a', 0x02]);
        assert_eq!(v.v_type, VAR_DICT);
        match &v.vval {
            v_dict(Some(d)) => assert!(d.borrow().dv_hashtab.contains_key("_VAL")),
            _ => panic!("expected special dict"),
        }
    }
}
