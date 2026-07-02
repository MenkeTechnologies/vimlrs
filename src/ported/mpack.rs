//! Port of neovim's vendored libmpack streaming MessagePack parser
//! (`vendor/mpack/{mpack_core,object,conv}.{c,h}`).
//!
//! Only the *decode* half is ported — the token reader
//! ([`mpack_read`]/[`mpack_rtoken`]/[`mpack_rvalue`]/[`mpack_rblob`]), the
//! object walker ([`mpack_parse`]/[`mpack_parse_tok`]/[`mpack_parser_init`] and
//! the [`mpack_parser_push`]/[`mpack_parser_pop`] node stack), and the
//! number-unpacking conversions ([`mpack_unpack_boolean`]/[`mpack_unpack_uint`]/
//! [`mpack_unpack_sint`]/[`mpack_unpack_float_fast`]). These are exactly what
//! `vendor/eval/decode.c`'s `unpack_typval()` drives. The packing/writing half
//! (`mpack_write`, `mpack_wtoken`, `mpack_pack_*`) is left unported: it is not
//! referenced by the four decode stubs and `msgpackdump()` has its own encoder.
//!
//! RUST-PORT NOTES (unavoidable deviations from the C, each faithful otherwise):
//!
//! * **The `void *p` user-data union is a type parameter.** libmpack's
//!   `mpack_data_t` union carries arbitrary user state on nodes via `void *p`
//!   (the C caller casts it to their own type). Here [`mpack_data_t`] is generic
//!   over that payload `P`, so `vendor/eval/decode.c` instantiates the parser with
//!   its own [`crate::ported::eval::decode`] payload enum instead of casting raw
//!   pointers. The `u`/`i`/`d` union members are kept for fidelity though the
//!   typval path only uses `p`.
//! * **Callbacks take a node index, not a node pointer.** The C walk callbacks
//!   are `void cb(mpack_parser_t *w, mpack_node_t *n)` where `n` points into
//!   `w->items`; a Rust `&mut parser` plus `&mut node` into the same array would
//!   alias. The port passes the node's index into `parser.items` instead, and
//!   `MPACK_PARENT_NODE(n)` (object.h:11) becomes the `node_idx - 1` /
//!   sentinel-`pos` check inlined at each call site.
//! * **The moving `const char **buf` / `size_t *buflen` pair** becomes a
//!   `buf: &mut &[u8]` cursor plus a `buflen: &mut usize` remaining count, kept
//!   in sync exactly as the C advances them together; `chunk_ptr` then borrows
//!   the current input slice rather than a bare pointer.
#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    clippy::comparison_chain,
    clippy::needless_range_loop
)]

// ── mpack_core.h ──────────────────────────────────────────────────────────

/// `typedef unsigned int mpack_uint32_t;` (mpack_core.h:27).
pub type mpack_uint32_t = u32;
/// `typedef int mpack_sint32_t;` (mpack_core.h:26).
pub type mpack_sint32_t = i32;
/// `typedef unsigned long long mpack_uintmax_t;` (conv.h:8).
pub type mpack_uintmax_t = u64;
/// `typedef long long mpack_sintmax_t;` (conv.h:7).
pub type mpack_sintmax_t = i64;

/// `enum { MPACK_OK = 0, MPACK_EOF = 1, MPACK_ERROR = 2 };` (mpack_core.h:40).
pub const MPACK_OK: i32 = 0;
pub const MPACK_EOF: i32 = 1;
pub const MPACK_ERROR: i32 = 2;

/// `enum { MPACK_EXCEPTION = -1, MPACK_NOMEM = MPACK_ERROR + 1 };` (object.h:19).
pub const MPACK_EXCEPTION: i32 = -1;
pub const MPACK_NOMEM: i32 = MPACK_ERROR + 1;

/// `#define MPACK_MAX_TOKEN_LEN 9` (mpack_core.h:46).
pub const MPACK_MAX_TOKEN_LEN: usize = 9;
/// `#define MPACK_MAX_OBJECT_DEPTH 32` (object.h:8).
pub const MPACK_MAX_OBJECT_DEPTH: u32 = 32;

/// `typedef enum { MPACK_TOKEN_NIL = 1, … } mpack_token_type_t;` (mpack_core.h:48).
///
/// Explicit discriminants + declaration order make the derived `PartialOrd`
/// match the C integer comparisons `tok.type > MPACK_TOKEN_MAP` (mpack_core.c)
/// and `top->tok.type > MPACK_TOKEN_CHUNK` (object.c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum mpack_token_type_t {
    MPACK_TOKEN_NIL = 1,
    MPACK_TOKEN_BOOLEAN = 2,
    MPACK_TOKEN_UINT = 3,
    MPACK_TOKEN_SINT = 4,
    MPACK_TOKEN_FLOAT = 5,
    MPACK_TOKEN_CHUNK = 6,
    MPACK_TOKEN_ARRAY = 7,
    MPACK_TOKEN_MAP = 8,
    MPACK_TOKEN_BIN = 9,
    MPACK_TOKEN_STR = 10,
    MPACK_TOKEN_EXT = 11,
}
pub use mpack_token_type_t::*;

/// `typedef struct mpack_value_s { mpack_uint32_t lo, hi; } mpack_value_t;`
/// (mpack_core.h:35).
#[derive(Debug, Clone, Copy, Default)]
pub struct mpack_value_t {
    pub lo: mpack_uint32_t,
    pub hi: mpack_uint32_t,
}

/// The `data` union of [`mpack_token_t`] (mpack_core.h:66) — the active member is
/// selected by the token type.
#[derive(Debug, Clone)]
pub enum mpack_token_data {
    /// `mpack_value_t value;` — 32-bit parts of primitives (bool/int/float).
    value(mpack_value_t),
    /// `const char *chunk_ptr;` — chunk of data from str/bin/ext.
    ///
    /// RUST-PORT NOTE: owns the copied chunk bytes rather than borrowing a
    /// `const char *` into the input buffer, so the token outlives the read.
    chunk_ptr(Vec<u8>),
    /// `int ext_type;` — type field for ext tokens.
    ext_type(i32),
}

impl Default for mpack_token_data {
    fn default() -> Self {
        mpack_token_data::value(mpack_value_t::default())
    }
}

/// `typedef struct mpack_token_s { … } mpack_token_t;` (mpack_core.h:62).
#[derive(Debug, Clone)]
pub struct mpack_token_t {
    /// Type of token.
    pub r#type: mpack_token_type_t,
    /// Byte length for str/bin/ext/chunk/float/int/uint; item count for
    /// array/map.
    pub length: mpack_uint32_t,
    /// 32-bit parts of primitives / chunk bytes / ext type.
    pub data: mpack_token_data,
}

impl Default for mpack_token_t {
    /// `memset(..., 0, ...)` leaves an all-zero token; the type is never read
    /// before it is assigned, so `MPACK_TOKEN_NIL` is a harmless placeholder.
    fn default() -> Self {
        mpack_token_t {
            r#type: MPACK_TOKEN_NIL,
            length: 0,
            data: mpack_token_data::default(),
        }
    }
}

/// `typedef struct mpack_tokbuf_s { … } mpack_tokbuf_t;` (mpack_core.h:73).
#[derive(Debug, Clone)]
pub struct mpack_tokbuf_t {
    pub pending: [u8; MPACK_MAX_TOKEN_LEN],
    pub pending_tok: mpack_token_t,
    pub ppos: usize,
    pub plen: usize,
    pub passthrough: mpack_uint32_t,
}

impl Default for mpack_tokbuf_t {
    /// `#define MPACK_TOKBUF_INITIAL_VALUE { {0}, {0,0,{{0,0}}}, 0, 0, 0 }`
    /// (mpack_core.h:80).
    fn default() -> Self {
        mpack_tokbuf_t {
            pending: [0; MPACK_MAX_TOKEN_LEN],
            pending_tok: mpack_token_t::default(),
            ppos: 0,
            plen: 0,
            passthrough: 0,
        }
    }
}

// ── object.h ──────────────────────────────────────────────────────────────

/// `typedef union { void *p; mpack_uintmax_t u; mpack_sintmax_t i; double d; }
/// mpack_data_t;` (object.h:27).
///
/// RUST-PORT NOTE: the `void *p` member is generic over the caller's payload
/// type `P` (see the module note). `u`/`i`/`d` are kept for fidelity.
#[derive(Debug, Clone)]
pub enum mpack_data_t<P> {
    /// `void *p` — NULL.
    Null,
    /// `void *p` — caller payload.
    p(P),
    /// `mpack_uintmax_t u`.
    u(mpack_uintmax_t),
    /// `mpack_sintmax_t i`.
    i(mpack_sintmax_t),
    /// `double d`.
    d(f64),
}

impl<P> Default for mpack_data_t<P> {
    fn default() -> Self {
        mpack_data_t::Null
    }
}

/// `typedef struct mpack_node_s { … } mpack_node_t;` (object.h:34).
#[derive(Debug, Clone)]
pub struct mpack_node_t<P> {
    /// `mpack_token_t tok;`.
    pub tok: mpack_token_t,
    /// `size_t pos;`.
    pub pos: usize,
    /// `int key_visited;` — set while traversing a map's key.
    pub key_visited: i32,
    /// `mpack_data_t data[2];`.
    pub data: [mpack_data_t<P>; 2],
}

impl<P> Default for mpack_node_t<P> {
    fn default() -> Self {
        mpack_node_t {
            tok: mpack_token_t::default(),
            pos: 0,
            key_visited: 0,
            data: [mpack_data_t::Null, mpack_data_t::Null],
        }
    }
}

/// `typedef MPACK_PARSER_STRUCT(MPACK_MAX_OBJECT_DEPTH) mpack_parser_t;`
/// (object.h:63 via the `MPACK_PARSER_STRUCT` macro, object.h:45).
#[derive(Debug, Clone)]
pub struct mpack_parser_t<P> {
    /// `mpack_data_t data;`.
    pub data: mpack_data_t<P>,
    /// `mpack_uint32_t size, capacity;`.
    pub size: mpack_uint32_t,
    pub capacity: mpack_uint32_t,
    /// `int status;`.
    pub status: i32,
    /// `int exiting;`.
    pub exiting: i32,
    /// `mpack_tokbuf_t tokbuf;`.
    pub tokbuf: mpack_tokbuf_t,
    /// `mpack_node_t items[c + 1];` — index 0 is the parent sentinel.
    pub items: Vec<mpack_node_t<P>>,
}

/// `typedef void(*mpack_walk_cb)(mpack_parser_t *w, mpack_node_t *n);`
/// (object.h:64).
///
/// RUST-PORT NOTE: the node pointer is replaced by the node's index into
/// `parser.items` (see the module note on aliasing).
pub type mpack_walk_cb<P> = fn(&mut mpack_parser_t<P>, usize);

// ── mpack_core.c ────────────────────────────────────────────────────────────

/// `#define ADVANCE(buf, buflen) ((*buflen)--, (unsigned char)*((*buf)++))`
/// (mpack_core.c:6).
macro_rules! ADVANCE {
    ($buf:expr, $buflen:expr) => {{
        let __s: &[u8] = *$buf;
        let __b = __s[0];
        *$buf = &__s[1..];
        *$buflen -= 1;
        __b
    }};
}

/// `#define TLEN(val, range_start) ((mpack_uint32_t)(1 << (val - range_start)))`
/// (mpack_core.c:7).
macro_rules! TLEN {
    ($val:expr, $range_start:expr) => {
        (1u32 << ($val - $range_start)) as mpack_uint32_t
    };
}

/// `#define MIN(X, Y) ((X) < (Y) ? (X) : (Y))` (mpack_core.c:9).
macro_rules! MIN {
    ($x:expr, $y:expr) => {
        if $x < $y {
            $x
        } else {
            $y
        }
    };
}

/// Port of `mpack_tokbuf_init()` from `vendor/mpack/mpack_core.c:37`.
pub fn mpack_tokbuf_init(tokbuf: &mut mpack_tokbuf_t) {
    tokbuf.ppos = 0;
    tokbuf.plen = 0;
    tokbuf.passthrough = 0;
}

/// Port of `mpack_read()` from `vendor/mpack/mpack_core.c:44`.
pub fn mpack_read(
    tokbuf: &mut mpack_tokbuf_t,
    buf: &mut &[u8],
    buflen: &mut usize,
    tok: &mut mpack_token_t,
) -> i32 {
    let status;
    let initial_ppos;
    let mut ptrlen: usize;
    // c: if (*buflen == 0) return MPACK_EOF;
    if *buflen == 0 {
        return MPACK_EOF;
    }

    if tokbuf.passthrough != 0 {
        // pass data from str/bin/ext directly as a MPACK_TOKEN_CHUNK, adjusting
        // *buf and *buflen
        tok.r#type = MPACK_TOKEN_CHUNK;
        // c: tok->data.chunk_ptr = *buf;  tok->length = MIN(*buflen, passthrough);
        let length = MIN!(*buflen as mpack_uint32_t, tokbuf.passthrough);
        tok.length = length;
        tok.data = mpack_token_data::chunk_ptr(buf[..length as usize].to_vec());
        tokbuf.passthrough -= length;
        let s: &[u8] = *buf;
        *buf = &s[length as usize..];
        *buflen -= length as usize;
        // c: goto done;
        return MPACK_OK;
    }

    initial_ppos = tokbuf.ppos;

    // c: `ptr`/`ptrlen` read either from tokbuf.pending or from the *buf cursor.
    // RUST-PORT NOTE: `cur` is the byte slice standing in for the C `const char
    // *ptr`; `ptr_save_len` stands in for `ptr_save` so `ptr - ptr_save` becomes
    // the consumed-byte count `ptr_save_len - ptrlen`.
    let mut cur: &[u8];
    if tokbuf.plen != 0 {
        if mpack_rpending(buf, buflen, tokbuf) == 0 {
            return MPACK_EOF;
        }
        ptrlen = tokbuf.ppos;
        cur = &tokbuf.pending[..ptrlen];
    } else {
        cur = &buf[..*buflen];
        ptrlen = *buflen;
    }

    let ptr_save_len = ptrlen;

    status = mpack_rtoken(&mut cur, &mut ptrlen, tok);
    if status != 0 {
        if status != MPACK_EOF {
            return MPACK_ERROR;
        }
        // need more data
        debug_assert!(tokbuf.plen == 0);
        // read the remainder of *buf to tokbuf->pending so it can be parsed
        // later with more data.
        tokbuf.plen = tok.length as usize + 1;
        debug_assert!(tokbuf.plen <= tokbuf.pending.len());
        tokbuf.ppos = 0;
        let status2 = mpack_rpending(buf, buflen, tokbuf);
        debug_assert!(status2 == 0);
        let _ = status2;
        return MPACK_EOF;
    }

    // c: advanced = (size_t)(ptr - ptr_save) - initial_ppos;
    let advanced = (ptr_save_len - ptrlen) - initial_ppos;
    tokbuf.plen = 0;
    tokbuf.ppos = 0;
    *buflen -= advanced;
    let s: &[u8] = *buf;
    *buf = &s[advanced..];

    if tok.r#type > MPACK_TOKEN_MAP {
        tokbuf.passthrough = tok.length;
    }

    // done:
    MPACK_OK
}

/// Port of `mpack_rtoken()` from `vendor/mpack/mpack_core.c:171`.
pub fn mpack_rtoken(buf: &mut &[u8], buflen: &mut usize, tok: &mut mpack_token_t) -> i32 {
    if *buflen == 0 {
        return MPACK_EOF;
    }
    let t: u8 = ADVANCE!(buf, buflen);
    if t < 0x80 {
        // positive fixint
        mpack_value(MPACK_TOKEN_UINT, 1, mpack_byte(t), tok)
    } else if t < 0x90 {
        // fixmap
        mpack_blob(MPACK_TOKEN_MAP, (t & 0xf) as mpack_uint32_t, 0, tok)
    } else if t < 0xa0 {
        // fixarray
        mpack_blob(MPACK_TOKEN_ARRAY, (t & 0xf) as mpack_uint32_t, 0, tok)
    } else if t < 0xc0 {
        // fixstr
        mpack_blob(MPACK_TOKEN_STR, (t & 0x1f) as mpack_uint32_t, 0, tok)
    } else if t < 0xe0 {
        match t {
            0xc0 => mpack_value(MPACK_TOKEN_NIL, 0, mpack_byte(0), tok), // nil
            0xc2 => mpack_value(MPACK_TOKEN_BOOLEAN, 1, mpack_byte(0), tok), // false
            0xc3 => mpack_value(MPACK_TOKEN_BOOLEAN, 1, mpack_byte(1), tok), // true
            // bin 8/16/32
            0xc4 | 0xc5 | 0xc6 => mpack_rblob(MPACK_TOKEN_BIN, TLEN!(t, 0xc4), buf, buflen, tok),
            // ext 8/16/32
            0xc7 | 0xc8 | 0xc9 => mpack_rblob(MPACK_TOKEN_EXT, TLEN!(t, 0xc7), buf, buflen, tok),
            // float 32/64
            0xca | 0xcb => mpack_rvalue(MPACK_TOKEN_FLOAT, TLEN!(t, 0xc8), buf, buflen, tok),
            // uint 8/16/32/64
            0xcc | 0xcd | 0xce | 0xcf => {
                mpack_rvalue(MPACK_TOKEN_UINT, TLEN!(t, 0xcc), buf, buflen, tok)
            }
            // int 8/16/32/64
            0xd0 | 0xd1 | 0xd2 | 0xd3 => {
                mpack_rvalue(MPACK_TOKEN_SINT, TLEN!(t, 0xd0), buf, buflen, tok)
            }
            // fixext 1/2/4/8/16
            0xd4 | 0xd5 | 0xd6 | 0xd7 | 0xd8 => {
                if *buflen == 0 {
                    // require only one extra byte for the type code
                    tok.length = 1;
                    return MPACK_EOF;
                }
                tok.length = TLEN!(t, 0xd4);
                tok.r#type = MPACK_TOKEN_EXT;
                tok.data = mpack_token_data::ext_type(ADVANCE!(buf, buflen) as i32);
                MPACK_OK
            }
            // str 8/16/32
            0xd9 | 0xda | 0xdb => mpack_rblob(MPACK_TOKEN_STR, TLEN!(t, 0xd9), buf, buflen, tok),
            // array 16/32
            0xdc | 0xdd => mpack_rblob(MPACK_TOKEN_ARRAY, TLEN!(t, 0xdb), buf, buflen, tok),
            // map 16/32
            0xde | 0xdf => mpack_rblob(MPACK_TOKEN_MAP, TLEN!(t, 0xdd), buf, buflen, tok),
            _ => MPACK_ERROR,
        }
    } else {
        // negative fixint
        mpack_value(MPACK_TOKEN_SINT, 1, mpack_byte(t), tok)
    }
}

/// Port of `mpack_rpending()` from `vendor/mpack/mpack_core.c:251`.
fn mpack_rpending(buf: &mut &[u8], buflen: &mut usize, state: &mut mpack_tokbuf_t) -> i32 {
    let count;
    debug_assert!(state.ppos < state.plen);
    count = MIN!(state.plen - state.ppos, *buflen);
    // c: memcpy(state->pending + state->ppos, *buf, count);
    state.pending[state.ppos..state.ppos + count].copy_from_slice(&buf[..count]);
    state.ppos += count;
    if state.ppos < state.plen {
        // consume buffer since no token will be parsed yet.
        let s: &[u8] = *buf;
        *buf = &s[*buflen..];
        *buflen = 0;
        return 0;
    }
    1
}

/// Port of `mpack_rvalue()` from `vendor/mpack/mpack_core.c:268`.
fn mpack_rvalue(
    r#type: mpack_token_type_t,
    remaining: mpack_uint32_t,
    buf: &mut &[u8],
    buflen: &mut usize,
    tok: &mut mpack_token_t,
) -> i32 {
    let mut remaining = remaining;
    if (*buflen as mpack_uint32_t) < remaining {
        tok.length = remaining;
        return MPACK_EOF;
    }

    mpack_value(r#type, remaining, mpack_byte(0), tok);

    while remaining != 0 {
        let byte = ADVANCE!(buf, buflen) as mpack_uint32_t;
        remaining -= 1;
        let byte_idx = remaining;
        let byte_shift = (byte_idx % 4) * 8;
        // c: tok->data.value.lo |= byte << byte_shift;
        if let mpack_token_data::value(v) = &mut tok.data {
            v.lo |= byte << byte_shift;
            if remaining == 4 {
                // unpacked the first half of an 8-byte value: shift what was
                // parsed to the "hi" field and reset "lo" for the trailing bytes.
                v.hi = v.lo;
                v.lo = 0;
            }
        }
    }

    if r#type == MPACK_TOKEN_SINT {
        if let mpack_token_data::value(v) = &tok.data {
            let hi = v.hi;
            let lo = v.lo;
            let msb = (tok.length == 8 && (hi >> 31) != 0)
                || (tok.length == 4 && (lo >> 31) != 0)
                || (tok.length == 2 && (lo >> 15) != 0)
                || (tok.length == 1 && (lo >> 7) != 0);
            if !msb {
                tok.r#type = MPACK_TOKEN_UINT;
            }
        }
    }

    MPACK_OK
}

/// Port of `mpack_rblob()` from `vendor/mpack/mpack_core.c:306`.
fn mpack_rblob(
    r#type: mpack_token_type_t,
    tlen: mpack_uint32_t,
    buf: &mut &[u8],
    buflen: &mut usize,
    tok: &mut mpack_token_t,
) -> i32 {
    let mut l = mpack_token_t::default();
    let required = tlen + if r#type == MPACK_TOKEN_EXT { 1 } else { 0 };

    if (*buflen as mpack_uint32_t) < required {
        tok.length = required;
        return MPACK_EOF;
    }

    // c: l.data.value.lo = 0;
    l.data = mpack_token_data::value(mpack_value_t { lo: 0, hi: 0 });
    mpack_rvalue(MPACK_TOKEN_UINT, tlen, buf, buflen, &mut l);
    tok.r#type = r#type;
    tok.length = match &l.data {
        mpack_token_data::value(v) => v.lo,
        _ => 0,
    };

    if r#type == MPACK_TOKEN_EXT {
        tok.data = mpack_token_data::ext_type(ADVANCE!(buf, buflen) as i32);
    }

    MPACK_OK
}

/// Port of `mpack_value()` from `vendor/mpack/mpack_core.c:554`.
fn mpack_value(
    r#type: mpack_token_type_t,
    length: mpack_uint32_t,
    value: mpack_value_t,
    tok: &mut mpack_token_t,
) -> i32 {
    tok.r#type = r#type;
    tok.length = length;
    tok.data = mpack_token_data::value(value);
    MPACK_OK
}

/// Port of `mpack_blob()` from `vendor/mpack/mpack_core.c:563`.
fn mpack_blob(
    r#type: mpack_token_type_t,
    length: mpack_uint32_t,
    ext_type: i32,
    tok: &mut mpack_token_t,
) -> i32 {
    tok.r#type = r#type;
    tok.length = length;
    tok.data = mpack_token_data::ext_type(ext_type);
    MPACK_OK
}

/// Port of `mpack_byte()` from `vendor/mpack/mpack_core.c:572`.
fn mpack_byte(byte: u8) -> mpack_value_t {
    mpack_value_t {
        lo: byte as mpack_uint32_t,
        hi: 0,
    }
}

// ── object.c ────────────────────────────────────────────────────────────────

/// Port of `mpack_parser_init()` from `vendor/mpack/object.c:9`.
pub fn mpack_parser_init<P>(parser: &mut mpack_parser_t<P>, capacity: mpack_uint32_t) {
    mpack_tokbuf_init(&mut parser.tokbuf);
    parser.data = mpack_data_t::Null;
    parser.capacity = if capacity != 0 {
        capacity
    } else {
        MPACK_MAX_OBJECT_DEPTH
    };
    parser.size = 0;
    parser.exiting = 0;
    // c: memset(parser->items, 0, sizeof(mpack_node_t) * (capacity + 1));
    parser.items = (0..=parser.capacity)
        .map(|_| mpack_node_t::default())
        .collect();
    parser.items[0].pos = usize::MAX; // c: parser->items[0].pos = (size_t)-1;
    parser.status = 0;
}

/// Port of `mpack_parse_tok()` from `vendor/mpack/object.c:52` (the `MPACK_WALK`
/// macro from object.c:29 is inlined here — this is the parse, not unparse,
/// direction, so `action` is `{n->tok = tok; enter_cb(parser, n);}`).
pub fn mpack_parse_tok<P>(
    parser: &mut mpack_parser_t<P>,
    tok: mpack_token_t,
    enter_cb: mpack_walk_cb<P>,
    exit_cb: mpack_walk_cb<P>,
) -> i32 {
    // c: MPACK_EXCEPTION_CHECK(parser);
    if parser.status == MPACK_EXCEPTION {
        return MPACK_EXCEPTION;
    }

    // MPACK_WALK:
    if parser.exiting == 0 {
        if mpack_parser_full(parser) != 0 {
            return MPACK_NOMEM;
        }
        let n = mpack_parser_push(parser);
        // action: {n->tok = tok; enter_cb(parser, n);}
        parser.items[n].tok = tok;
        enter_cb(parser, n);
        if parser.status == MPACK_EXCEPTION {
            return MPACK_EXCEPTION;
        }
        parser.exiting = 1;
        return MPACK_EOF;
    }

    // exit:
    parser.exiting = 0;
    while let Some(n) = mpack_parser_pop(parser) {
        exit_cb(parser, n);
        if parser.status == MPACK_EXCEPTION {
            return MPACK_EXCEPTION;
        }
        if parser.size == 0 {
            return MPACK_OK;
        }
    }

    MPACK_EOF
}

/// Port of `mpack_parse()` from `vendor/mpack/object.c:66`.
pub fn mpack_parse<P>(
    parser: &mut mpack_parser_t<P>,
    buf: &mut &[u8],
    buflen: &mut usize,
    enter_cb: mpack_walk_cb<P>,
    exit_cb: mpack_walk_cb<P>,
) -> i32 {
    let mut status = MPACK_EOF;
    if parser.status == MPACK_EXCEPTION {
        return MPACK_EXCEPTION;
    }

    while *buflen != 0 && status != 0 {
        let mut tok = mpack_token_t::default();
        // c: const char *buf_save = *buf; size_t buflen_save = *buflen;
        let buf_save = *buf;
        let buflen_save = *buflen;

        status = mpack_read(&mut parser.tokbuf, buf, buflen, &mut tok);
        if status == MPACK_EOF {
            continue;
        } else if status == MPACK_ERROR {
            // c: goto rollback;
            *buf = buf_save;
            *buflen = buflen_save;
            break;
        }

        // c: do { status = mpack_parse_tok(...); } while (parser->exiting);
        loop {
            status = mpack_parse_tok(parser, tok.clone(), enter_cb, exit_cb);
            if parser.status == MPACK_EXCEPTION {
                return MPACK_EXCEPTION;
            }
            if parser.exiting == 0 {
                break;
            }
        }

        if status != MPACK_NOMEM {
            continue;
        }

        // rollback: restore buf/buflen so the next call re-reads the same token.
        *buf = buf_save;
        *buflen = buflen_save;
        break;
    }

    status
}

/// Port of `mpack_parser_full()` from `vendor/mpack/object.c:146`.
fn mpack_parser_full<P>(parser: &mpack_parser_t<P>) -> i32 {
    (parser.size == parser.capacity) as i32
}

/// Port of `mpack_parser_push()` from `vendor/mpack/object.c:151`.
///
/// RUST-PORT NOTE: returns the new top node's index into `parser.items` rather
/// than a `mpack_node_t *`.
fn mpack_parser_push<P>(parser: &mut mpack_parser_t<P>) -> usize {
    debug_assert!(parser.size < parser.capacity);
    let top = (parser.size + 1) as usize; // c: top = parser->items + parser->size + 1;
    parser.items[top].data[0] = mpack_data_t::Null;
    parser.items[top].data[1] = mpack_data_t::Null;
    parser.items[top].pos = 0;
    parser.items[top].key_visited = 0;
    // increase size and invoke callback, passing parent node if any
    parser.size += 1;
    top
}

/// Port of `mpack_parser_pop()` from `vendor/mpack/object.c:166`.
///
/// RUST-PORT NOTE: returns `Some(index)` / `None` rather than a
/// `mpack_node_t *` / `NULL`.
fn mpack_parser_pop<P>(parser: &mut mpack_parser_t<P>) -> Option<usize> {
    debug_assert!(parser.size != 0);
    let top = parser.size as usize; // c: top = parser->items + parser->size;

    if parser.items[top].tok.r#type > MPACK_TOKEN_CHUNK
        && parser.items[top].pos < parser.items[top].tok.length as usize
    {
        // continue processing children
        return None;
    }

    // c: parent = MPACK_PARENT_NODE(top);
    let parent = top - 1;
    if parser.items[parent].pos != usize::MAX {
        // we use parent->tok.length to keep track of how many children remain.
        if parser.items[top].tok.r#type == MPACK_TOKEN_CHUNK {
            parser.items[parent].pos += parser.items[top].tok.length as usize;
        } else if parser.items[parent].tok.r#type == MPACK_TOKEN_MAP {
            // maps use an extra flag to know if the key at a position was visited
            if parser.items[parent].key_visited != 0 {
                parser.items[parent].pos += 1;
            }
            parser.items[parent].key_visited = (parser.items[parent].key_visited == 0) as i32;
        } else {
            parser.items[parent].pos += 1;
        }
    }

    parser.size -= 1;
    Some(top)
}

// ── conv.c (unpack half) ────────────────────────────────────────────────────

/// `#define POW2(n) …` (conv.c:9).
fn POW2(n: u32) -> f64 {
    (1u64 << (n / 2)) as f64 * (1u64 << (n / 2)) as f64 * (1u64 << (n % 2)) as f64
}

/// Port of `mpack_unpack_boolean()` from `vendor/mpack/conv.c:193`.
pub fn mpack_unpack_boolean(t: &mpack_token_t) -> bool {
    match &t.data {
        mpack_token_data::value(v) => v.lo != 0 || v.hi != 0,
        _ => false,
    }
}

/// Port of `mpack_unpack_uint()` from `vendor/mpack/conv.c:198`.
pub fn mpack_unpack_uint(t: &mpack_token_t) -> mpack_uintmax_t {
    match &t.data {
        mpack_token_data::value(v) => {
            (((v.hi as mpack_uintmax_t) << 31) << 1) | v.lo as mpack_uintmax_t
        }
        _ => 0,
    }
}

/// Port of `mpack_unpack_sint()` from `vendor/mpack/conv.c:205`.
///
/// Unpack a signed integer without relying on two's complement as the internal
/// representation.
pub fn mpack_unpack_sint(t: &mpack_token_t) -> mpack_sintmax_t {
    let (hi, lo) = match &t.data {
        mpack_token_data::value(v) => (v.hi, v.lo),
        _ => (0, 0),
    };
    let mut rv: mpack_uintmax_t = lo as mpack_uintmax_t;
    debug_assert!(t.length as usize <= std::mem::size_of::<mpack_sintmax_t>());

    if t.length == 8 {
        rv |= ((hi as mpack_uintmax_t) << 31) << 1;
    }
    // reverse the two's complement so lo/hi hold the absolute value; mask ~rv so
    // it reflects the two's complement of the appropriate byte length.
    rv = (!rv & (((1 as mpack_uintmax_t) << ((t.length * 8) - 1)) - 1)) + 1;
    // negate and return the absolute value.
    -((rv - 1) as mpack_sintmax_t) - 1
}

/// Port of `mpack_unpack_float_compat()` from `vendor/mpack/conv.c:224`.
pub fn mpack_unpack_float_compat(t: &mpack_token_t) -> f64 {
    let (vlo, vhi) = match &t.data {
        mpack_token_data::value(v) => (v.lo, v.hi),
        _ => (0, 0),
    };
    let sign: mpack_uint32_t;
    let mut exponent: mpack_sint32_t;
    let mantbits: u32;
    let expbits: u32;
    let bias: mpack_sint32_t;
    let mut mant: f64;

    if vlo == 0 && vhi == 0 {
        // nothing to do
        return 0.0;
    }

    if t.length == 4 {
        mantbits = 23;
        expbits = 8;
    } else {
        mantbits = 52;
        expbits = 11;
    }
    bias = (1i32 << (expbits - 1)) - 1;

    // restore sign/exponent/mantissa
    if mantbits == 52 {
        sign = vhi >> 31;
        exponent = ((vhi >> 20) & ((1 << 11) - 1)) as mpack_sint32_t;
        mant = (vhi & ((1 << 20) - 1)) as f64 * POW2(32);
        mant += vlo as f64;
    } else {
        sign = vlo >> 31;
        exponent = ((vlo >> 23) & ((1 << 8) - 1)) as mpack_sint32_t;
        mant = (vlo & ((1 << 23) - 1)) as f64;
    }

    mant /= POW2(mantbits);
    if exponent != 0 {
        mant += 1.0; // restore leading 1
    } else {
        exponent = 1; // subnormal
    }
    exponent -= bias;

    // restore original value
    while exponent > 0 {
        mant *= 2.0;
        exponent -= 1;
    }
    while exponent < 0 {
        mant /= 2.0;
        exponent += 1;
    }
    mant * if sign != 0 { -1.0 } else { 1.0 }
}

/// Port of `mpack_unpack_float_fast()` from `vendor/mpack/conv.c:263`.
pub fn mpack_unpack_float_fast(t: &mpack_token_t) -> f64 {
    let (lo, hi) = match &t.data {
        mpack_token_data::value(v) => (v.lo, v.hi),
        _ => (0, 0),
    };
    if t.length == 4 {
        // c: union { float f; mpack_uint32_t m; } conv; conv.m = lo; return conv.f;
        f32::from_bits(lo) as f64
    } else {
        // c: union { double d; mpack_value_t m; } conv; conv.m = value; …
        let mut m = mpack_value_t { lo, hi };
        if mpack_is_be() != 0 {
            // MPACK_SWAP_VALUE(conv.m)
            std::mem::swap(&mut m.lo, &mut m.hi);
        }
        // On little-endian the double's memory is [lo (4 bytes), hi (4 bytes)].
        f64::from_bits(((m.hi as u64) << 32) | m.lo as u64)
    }
}

/// `#ifndef mpack_unpack_float # define mpack_unpack_float
/// mpack_unpack_float_fast #endif` (conv.h:47).
pub fn mpack_unpack_float(t: &mpack_token_t) -> f64 {
    mpack_unpack_float_fast(t)
}

/// Port of `mpack_unpack_number()` from `vendor/mpack/conv.c:287`.
pub fn mpack_unpack_number(t: &mpack_token_t) -> f64 {
    let rv;
    let (mut hi, mut lo) = match &t.data {
        mpack_token_data::value(v) => (v.hi, v.lo),
        _ => (0, 0),
    };
    if t.r#type == MPACK_TOKEN_FLOAT {
        return mpack_unpack_float(t);
    }
    debug_assert!(t.r#type == MPACK_TOKEN_UINT || t.r#type == MPACK_TOKEN_SINT);
    if t.r#type == MPACK_TOKEN_SINT {
        // same idea as mpack_unpack_sint, operating on the 32-bit words.
        if hi == 0 {
            debug_assert!(t.length <= 4);
            lo = !lo & (((1 as mpack_uint32_t) << ((t.length * 8) - 1)) - 1);
        } else {
            hi = !hi;
            lo = !lo;
        }
        lo = lo.wrapping_add(1);
        if lo == 0 {
            hi = hi.wrapping_add(1);
        }
    }
    rv = lo as f64 + POW2(32) * hi as f64;
    if t.r#type == MPACK_TOKEN_SINT {
        -rv
    } else {
        rv
    }
}

/// Port of `mpack_is_be()` from `vendor/mpack/conv.c:358`.
fn mpack_is_be() -> i32 {
    (cfg!(target_endian = "big")) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_value(
        r#type: mpack_token_type_t,
        length: mpack_uint32_t,
        lo: u32,
        hi: u32,
    ) -> mpack_token_t {
        mpack_token_t {
            r#type,
            length,
            data: mpack_token_data::value(mpack_value_t { lo, hi }),
        }
    }

    /// Read a single scalar token from a byte buffer.
    fn read_one(bytes: &[u8]) -> mpack_token_t {
        let mut tb = mpack_tokbuf_t::default();
        let mut buf: &[u8] = bytes;
        let mut buflen = bytes.len();
        let mut tok = mpack_token_t::default();
        let st = mpack_read(&mut tb, &mut buf, &mut buflen, &mut tok);
        assert_eq!(st, MPACK_OK);
        tok
    }

    #[test]
    fn rtoken_positive_and_negative_fixint() {
        // positive fixint 0x7f = 127
        let t = read_one(&[0x7f]);
        assert_eq!(t.r#type, MPACK_TOKEN_UINT);
        assert_eq!(mpack_unpack_uint(&t), 127);
        // negative fixint 0xff = -1
        let t = read_one(&[0xff]);
        assert_eq!(t.r#type, MPACK_TOKEN_SINT);
        assert_eq!(mpack_unpack_sint(&t), -1);
    }

    #[test]
    fn rtoken_uint_widths() {
        // uint 8: 0xcc 0xff = 255
        let t = read_one(&[0xcc, 0xff]);
        assert_eq!(t.r#type, MPACK_TOKEN_UINT);
        assert_eq!(mpack_unpack_uint(&t), 255);
        // uint 16: 0xcd 0x01 0x00 = 256
        let t = read_one(&[0xcd, 0x01, 0x00]);
        assert_eq!(mpack_unpack_uint(&t), 256);
        // uint 32: 0xce 0x00 0x01 0x00 0x00 = 65536
        let t = read_one(&[0xce, 0x00, 0x01, 0x00, 0x00]);
        assert_eq!(mpack_unpack_uint(&t), 65536);
        // uint 64: 0xcf ... = 0x1_0000_0000
        let t = read_one(&[0xcf, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(mpack_unpack_uint(&t), 0x1_0000_0000);
    }

    #[test]
    fn rtoken_sint_widths() {
        // int 8: 0xd0 0x80 = -128
        let t = read_one(&[0xd0, 0x80]);
        assert_eq!(t.r#type, MPACK_TOKEN_SINT);
        assert_eq!(mpack_unpack_sint(&t), -128);
        // int 16: 0xd1 0x80 0x00 = -32768
        let t = read_one(&[0xd1, 0x80, 0x00]);
        assert_eq!(mpack_unpack_sint(&t), -32768);
        // int 32: 0xd2 0x80 0x00 0x00 0x00
        let t = read_one(&[0xd2, 0x80, 0x00, 0x00, 0x00]);
        assert_eq!(mpack_unpack_sint(&t), -(1i64 << 31));
        // int 64: 0xd3 0xff.. = -1
        let t = read_one(&[0xd3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(mpack_unpack_sint(&t), -1);
    }

    #[test]
    fn rtoken_nil_bool() {
        assert_eq!(read_one(&[0xc0]).r#type, MPACK_TOKEN_NIL);
        assert!(!mpack_unpack_boolean(&read_one(&[0xc2])));
        assert!(mpack_unpack_boolean(&read_one(&[0xc3])));
    }

    #[test]
    fn float_unpack_roundtrip() {
        // 1.5 as float64: 0xcb followed by big-endian IEEE-754 bits.
        let bits = 1.5f64.to_bits().to_be_bytes();
        let mut input = vec![0xcb];
        input.extend_from_slice(&bits);
        let t = read_one(&input);
        assert_eq!(t.r#type, MPACK_TOKEN_FLOAT);
        assert_eq!(mpack_unpack_float(&t), 1.5);

        // -0.25 as float32: 0xca + big-endian f32 bits.
        let bits = (-0.25f32).to_bits().to_be_bytes();
        let mut input = vec![0xca];
        input.extend_from_slice(&bits);
        let t = read_one(&input);
        assert_eq!(mpack_unpack_float(&t), -0.25);
    }

    #[test]
    fn unpack_number_matches_int_and_float() {
        // A big uint token unpacks to the same double via mpack_unpack_number.
        let t = tok_value(MPACK_TOKEN_UINT, 8, 0x0000_0002, 0x0000_0000);
        assert_eq!(mpack_unpack_number(&t), 2.0);
        // sint -3 (fixint) → number -3.0
        let t = read_one(&[0xfd]); // -3
        assert_eq!(mpack_unpack_number(&t), -3.0);
    }

    #[test]
    fn rblob_str_length() {
        // fixstr length 3 ("abc") — mpack_read then passthrough chunk.
        let mut tb = mpack_tokbuf_t::default();
        let bytes = [0xa3u8, b'a', b'b', b'c'];
        let mut buf: &[u8] = &bytes;
        let mut buflen = bytes.len();
        let mut tok = mpack_token_t::default();
        assert_eq!(
            mpack_read(&mut tb, &mut buf, &mut buflen, &mut tok),
            MPACK_OK
        );
        assert_eq!(tok.r#type, MPACK_TOKEN_STR);
        assert_eq!(tok.length, 3);
        // Next read yields the CHUNK with the 3 payload bytes.
        let mut chunk = mpack_token_t::default();
        assert_eq!(
            mpack_read(&mut tb, &mut buf, &mut buflen, &mut chunk),
            MPACK_OK
        );
        assert_eq!(chunk.r#type, MPACK_TOKEN_CHUNK);
        match &chunk.data {
            mpack_token_data::chunk_ptr(b) => assert_eq!(b, b"abc"),
            _ => panic!("expected chunk_ptr"),
        }
    }
}
