//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — a JSON parser producing `typval_T` values, the engine behind the
//! ported `json_decode_string` (`src/ported/eval/decode.rs`). Neovim's
//! `decode.c` parses JSON with an explicit value/container stack machine; this
//! is a recursive-descent parser that yields the identical `typval_T` tree (the
//! same synthesis stance as `viml_regex` vs the C regex VM). The encode side is
//! a faithful port and lives in `encode.rs` (`encode_tv2json`).
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_dict_add_tv, tv_dict_alloc, tv_list_alloc, tv_list_append_tv,
};
use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, BoolVarValue::*, SpecialVarValue::*,
    VarLockStatus::VAR_UNLOCKED, VarType::*,
};

/// Decode a JSON document into a `typval_T`, or `None` on malformed input.
pub fn decode(s: &str) -> Option<typval_T> {
    let mut p = Parser {
        chars: s.chars().collect(),
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    // Trailing non-whitespace = malformed (Vim's json_decode is strict).
    if p.i != p.chars.len() {
        return None;
    }
    Some(v)
}

struct Parser {
    chars: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Option<typval_T> {
        match self.peek()? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => self.string().map(typval_T::from),
            't' | 'f' => self.boolean(),
            'n' => self.null(),
            '-' | '0'..='9' => self.number(),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<typval_T> {
        self.i += 1; // '{'
        let d = tv_dict_alloc();
        self.ws();
        if self.peek() == Some('}') {
            self.i += 1;
        } else {
            loop {
                self.ws();
                let key = self.string()?;
                self.ws();
                if self.peek()? != ':' {
                    return None;
                }
                self.i += 1;
                self.ws();
                let val = self.value()?;
                tv_dict_add_tv(&mut d.borrow_mut(), &key, val);
                self.ws();
                match self.peek()? {
                    ',' => self.i += 1,
                    '}' => {
                        self.i += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        Some(dict_tv(d))
    }

    fn array(&mut self) -> Option<typval_T> {
        self.i += 1; // '['
        let l = tv_list_alloc(0);
        self.ws();
        if self.peek() == Some(']') {
            self.i += 1;
        } else {
            loop {
                self.ws();
                let v = self.value()?;
                tv_list_append_tv(&mut l.borrow_mut(), v);
                self.ws();
                match self.peek()? {
                    ',' => self.i += 1,
                    ']' => {
                        self.i += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        Some(list_tv(l))
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != '"' {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            match self.peek()? {
                '"' => {
                    self.i += 1;
                    return Some(out);
                }
                '\\' => {
                    self.i += 1;
                    match self.peek()? {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\x08'),
                        'f' => out.push('\x0c'),
                        'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                self.i += 1;
                                let h = self.peek()?.to_digit(16)?;
                                code = code * 16 + h;
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                    self.i += 1;
                }
                c => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn number(&mut self) -> Option<typval_T> {
        let start = self.i;
        let mut is_float = false;
        if self.peek() == Some('-') {
            self.i += 1;
        }
        while matches!(self.peek(), Some('0'..='9')) {
            self.i += 1;
        }
        if self.peek() == Some('.') {
            is_float = true;
            self.i += 1;
            while matches!(self.peek(), Some('0'..='9')) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.i += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.i += 1;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.i += 1;
            }
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if is_float {
            Some(float_tv(text.parse().ok()?))
        } else {
            Some(typval_T::from(text.parse::<i64>().ok()?))
        }
    }

    fn boolean(&mut self) -> Option<typval_T> {
        if self.take("true") {
            Some(bool_tv(true))
        } else if self.take("false") {
            Some(bool_tv(false))
        } else {
            None
        }
    }

    fn null(&mut self) -> Option<typval_T> {
        self.take("null").then(special_null)
    }

    fn take(&mut self, word: &str) -> bool {
        let end = self.i + word.len();
        if end <= self.chars.len() && self.chars[self.i..end].iter().collect::<String>() == word {
            self.i = end;
            true
        } else {
            false
        }
    }
}

fn float_tv(f: f64) -> typval_T {
    typval_T {
        v_type: VAR_FLOAT,
        v_lock: VAR_UNLOCKED,
        vval: v_float(f),
    }
}
fn bool_tv(b: bool) -> typval_T {
    typval_T {
        v_type: VAR_BOOL,
        v_lock: VAR_UNLOCKED,
        vval: v_bool(if b { kBoolVarTrue } else { kBoolVarFalse }),
    }
}
fn special_null() -> typval_T {
    typval_T {
        v_type: VAR_SPECIAL,
        v_lock: VAR_UNLOCKED,
        vval: v_special(kSpecialVarNull),
    }
}
fn list_tv(l: Rc<RefCell<crate::ported::eval::typval_defs_h::list_T>>) -> typval_T {
    typval_T {
        v_type: VAR_LIST,
        v_lock: VAR_UNLOCKED,
        vval: v_list(Some(l)),
    }
}
fn dict_tv(d: Rc<RefCell<crate::ported::eval::typval_defs_h::dict_T>>) -> typval_T {
    typval_T {
        v_type: VAR_DICT,
        v_lock: VAR_UNLOCKED,
        vval: v_dict(Some(d)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::eval::encode::encode_tv2json;

    fn roundtrip(s: &str) -> String {
        encode_tv2json(&decode(s).expect("decode"))
    }

    #[test]
    fn decode_encode_roundtrip() {
        assert_eq!(roundtrip("42"), "42");
        assert_eq!(roundtrip("[1,2,3]"), "[1,2,3]");
        assert_eq!(
            roundtrip(r#"{"a":1,"b":[true,null]}"#),
            r#"{"a":1,"b":[true,null]}"#
        );
        assert_eq!(roundtrip(r#""he\"llo""#), r#""he\"llo""#);
        assert_eq!(roundtrip("3.5"), "3.5");
    }

    #[test]
    fn malformed_is_none() {
        assert!(decode("{bad}").is_none());
        assert!(decode("[1,2").is_none());
        assert!(decode("42 garbage").is_none());
    }
}
