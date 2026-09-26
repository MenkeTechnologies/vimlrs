//! `:for` over an object that is not a String, List or Blob, and `:for [a, b]`
//! over an item that is not a List. Both are errors in vim and Neovim alike, but
//! the TEXT differs (vim 9.2 has Tuples: `E1523: String, List, Tuple or Blob
//! required` / `E1535: List or Tuple required`), so these are pinned here against
//! the vendored C's wording instead of in the vim-recorded parity corpus.

use vimlrs::fusevm_bridge::{capture_begin, capture_take};

/// Run `src`, returning its captured `:echo` output.
fn echo(src: &str) -> String {
    capture_begin();
    let _ = vimlrs::eval_source(src);
    capture_take().trim_end_matches('\n').to_string()
}

/// c: `eval_for_line` (`vendor/eval.c:1493-1495`) — anything but a String, List
/// or Blob is E1098 and the body never runs. It used to run once with the
/// Number itself as the item, and a Dict or Float was `E701: Invalid type for
/// len()`.
#[test]
fn a_non_iterable_is_e1098_and_the_body_never_runs() {
    for obj in ["5", "{}", "1.5", "v:null", "function('tr')"] {
        let out = echo(&format!(
            "let v:errmsg = ''\nfor x in {obj}\n  echo 'ran' x\nendfor\necho v:errmsg"
        ));
        assert_eq!(
            out, "E1098: String, List or Blob required",
            "for x in {obj}"
        );
    }
}

/// c: `next_for_item` is `ex_let_vars(...) == OK` — an item that is not a List
/// reports `ex_let_vars`' E714 and ends the loop, so the items after it are
/// never reached. It used to index the String "xy" as if it were a List.
#[test]
fn a_non_list_item_under_an_unpack_target_is_e714_and_ends_the_loop() {
    let out = echo(
        "let v:errmsg = ''\nfor [a, b] in [[1, 2], 'xy', [3, 4]]\n  echo a b\nendfor\necho v:errmsg",
    );
    assert_eq!(out, "1 2\nE714: List required");
}
