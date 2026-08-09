//! EXTENSION — NO `vendor/` COUNTERPART. The byte-string backing a VimL
//! `VAR_STRING`/`VAR_FUNC` value.
//!
//! ## Why this type exists
//!
//! A Vim string is `char_u *` — a NUL-terminated array of **bytes**, with no
//! encoding invariant. Vim writes bytes into one that are not valid UTF-8 as a
//! matter of routine, and the language exposes them:
//!
//! ```text
//! utf_char2bytes(c, buf)         // Src/mbyte.c:1076
//!     if (c < 0x80) { buf[0] = (char_u)c; return 1; }
//! ```
//!
//! `c` is a signed `int`, so `list2str([-1])` takes the `c < 0x80` arm and
//! stores `(char_u)-1` == `0xff` — a single byte that is not a UTF-8 sequence.
//! There is no range check above `U+10FFFF` either, so `list2str([0x110000])`
//! is the four bytes `f4 90 80 80`, which decode to a code point Rust's `char`
//! cannot hold. Both were measured against the reference editor:
//!
//! ```text
//! $ vim -es -u NONE -c "call writefile([list2str([-1,0,1])],'/tmp/a','b')" -c 'qa!'
//! $ xxd -p /tmp/a
//! ff01
//! ```
//!
//! Backing such a value with a Rust `String` is not possible: `String` carries a
//! hard UTF-8 invariant, so a lone `0xff` cannot be stored in one at all. That
//! is why this port previously dropped those items on the floor
//! (`list2str([-1,0,1])` was the single byte `01`) and why `str2blob()` could not
//! be ported. `VimStr` is `Vec<u8>`, which is what the C has.
//!
//! ## Why it is a newtype rather than a bare `Vec<u8>`
//!
//! The overwhelming majority of VimL strings *are* valid UTF-8 — they come from
//! source text, buffers, and files — and the ported code reads them as text
//! (`starts_with`, `split`, `chars`). A newtype lets the text-shaped accessors
//! ([`VimStr::as_str`], [`VimStr::to_string_lossy`]) sit next to the byte-exact
//! ones ([`VimStr::as_bytes`]) so a call site states which one it means, instead
//! of every site open-coding a `from_utf8` decision.
//!
//! ## The UTF-8 boundary that remains
//!
//! `fusevm::Value::Str` is an `Arc<String>`, so a byte string that is not valid
//! UTF-8 cannot ride the VM stack as a `Value::Str`. `fusevm_bridge` routes
//! those through the same `REFPOOL` handle that Lists, Dicts and Blobs already
//! use, so the VM carries an opaque reference and every VimL string operation
//! (which is a host builtin here — `VIML_CONCAT`, `VIML_COMPARE`, the `f_*`
//! table — never a native fusevm op) sees the real bytes.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

/// A VimL string: a byte array, matching the C's `char_u *`.
///
/// `Deref`s to `[u8]`, so `len()`, `is_empty()`, `iter()`, `contains()` and
/// slicing are the byte-exact ones. Text-shaped reads go through [`Self::as_str`]
/// (`None` when the bytes are not UTF-8) or [`Self::to_string_lossy`].
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VimStr(Vec<u8>);

impl VimStr {
    /// An empty string (the C's `NULL` / `""`).
    pub fn new() -> Self {
        VimStr(Vec::new())
    }

    /// The raw bytes — what the C's `char_u *` points at.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The raw bytes, consuming the string.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Mutable byte access, for the ports that build a string in place.
    pub fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }

    /// The bytes as `&str`, or `None` when they are not valid UTF-8.
    ///
    /// Use this where a wrong answer is better caught than papered over; use
    /// [`Self::to_string_lossy`] where the C would have walked the bytes as
    /// text regardless.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// The bytes as text, replacing anything that is not valid UTF-8 with
    /// `U+FFFD`. Borrows when the bytes are already UTF-8, which is the
    /// overwhelmingly common case.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// True when the bytes are valid UTF-8 — i.e. when this string can cross
    /// the `fusevm::Value::Str` boundary without going through the `REFPOOL`.
    pub fn is_utf8(&self) -> bool {
        std::str::from_utf8(&self.0).is_ok()
    }

    /// Append raw bytes (the C's `ga_concat` over a byte buffer).
    pub fn push_bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }

    /// Append a `char`, UTF-8 encoded (the common text-building path).
    pub fn push_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.0.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }

    /// Append text.
    pub fn push_str(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }
}

// ── conversions in ──

impl From<String> for VimStr {
    fn from(s: String) -> Self {
        VimStr(s.into_bytes())
    }
}

impl From<&str> for VimStr {
    fn from(s: &str) -> Self {
        VimStr(s.as_bytes().to_vec())
    }
}

impl From<&String> for VimStr {
    fn from(s: &String) -> Self {
        VimStr(s.as_bytes().to_vec())
    }
}

impl From<Vec<u8>> for VimStr {
    fn from(b: Vec<u8>) -> Self {
        VimStr(b)
    }
}

impl From<&[u8]> for VimStr {
    fn from(b: &[u8]) -> Self {
        VimStr(b.to_vec())
    }
}

impl From<Cow<'_, str>> for VimStr {
    fn from(s: Cow<'_, str>) -> Self {
        VimStr(s.into_owned().into_bytes())
    }
}

impl From<char> for VimStr {
    fn from(c: char) -> Self {
        let mut v = VimStr::new();
        v.push_char(c);
        v
    }
}

// ── conversions out ──

impl From<VimStr> for String {
    /// Lossy — see [`VimStr::to_string_lossy`].
    fn from(s: VimStr) -> String {
        match String::from_utf8(s.0) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }
}

impl From<VimStr> for Vec<u8> {
    fn from(s: VimStr) -> Vec<u8> {
        s.0
    }
}

impl Deref for VimStr {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for VimStr {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// ── comparison against text, so `s == "foo"` keeps working ──

impl PartialEq<str> for VimStr {
    fn eq(&self, other: &str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for VimStr {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<String> for VimStr {
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<VimStr> for str {
    fn eq(&self, other: &VimStr) -> bool {
        self.as_bytes() == other.0
    }
}

impl PartialEq<VimStr> for &str {
    fn eq(&self, other: &VimStr) -> bool {
        self.as_bytes() == other.0
    }
}

impl PartialEq<VimStr> for String {
    fn eq(&self, other: &VimStr) -> bool {
        self.as_bytes() == other.0
    }
}

// ── rendering ──

impl fmt::Display for VimStr {
    /// Lossy: `Display` produces text, and text is UTF-8 in Rust. Byte-exact
    /// output goes through [`VimStr::as_bytes`] and a `Write`, not through here.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_string_lossy(), f)
    }
}

impl fmt::Debug for VimStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string_lossy(), f)
    }
}

impl std::iter::FromIterator<u8> for VimStr {
    fn from_iter<I: IntoIterator<Item = u8>>(iter: I) -> Self {
        VimStr(iter.into_iter().collect())
    }
}

impl std::iter::FromIterator<char> for VimStr {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
        let mut v = VimStr::new();
        for c in iter {
            v.push_char(c);
        }
        v
    }
}
