" setreg()'s Dict form, and its FAIL-default return.
"
" f_setreg opens with `rettv->vval.v_number = 1;  // FAIL is default`
" (vendor/eval/funcs.c:6617) and only clears it to 0 at c:6742, after the write.
" Every early return therefore answers 1, which is the opposite of the usual
" "0 means success" reading and is easy to get backwards.
"
" Two early returns were missing here:
"
"   c:6633  an EMPTY dict clears the register "like setreg(0, [])" and returns
"           at c:6637 — so `setreg('a', {})` is 1, and the register ends empty.
"           This port fell through to the string path and raised
"           E908: Using an invalid value as a String.
"
"   c:6645  a `regtype` that is PRESENT must parse completely — c:6649 fails on
"           `ret == FAIL || *(++stropt) != NUL`, so a bad type char, a trailing
"           character, and the empty string are all
"           E475: Invalid value for argument value. This port accepted them and
"           silently wrote with the default type.
"
" EXCLUDED: `setreg(r, function('strlen'))`. c:6732 runs the value through
" tv_get_string_chk, which errors for VAR_FUNC (c:4604-4611) — vim answers
" E729 and 1. This port's tv_get_string_buf_chk returns the function NAME for
" VAR_FUNC instead, which callback resolution currently depends on; see BUGS.md
" R26-O7.
let s = 'seed'

" An empty Dict: clears the register, and returns the FAIL default.
call setreg('a', s)
let r = setreg('a', {})
echo 'A' r string(getreg('a'))

" A well-formed regtype still works, blockwise width included.
call setreg('b', s)
let r = setreg('b', {'regcontents': 'x'})
echo 'B' r string(getreg('b'))
let r = setreg('c', {'regcontents': ['ab', 'cd'], 'regtype': 'b1'})
echo 'C' r string(getreg('c')) string(getregtype('c'))
let r = setreg('d', {'regcontents': 'x', 'regtype': 'V'})
echo 'D' r string(getreg('d')) string(getregtype('d'))

" A regtype that does not parse: the register is left exactly as it was.
call setreg('e', s)
try
  let r = setreg('e', {'regcontents': 'x', 'regtype': 'zz'})
  echo 'E' r string(getreg('e'))
catch
  echo 'E threw' v:exception string(getreg('e'))
endtry

" A regtype that parses but has trailing junk fails the same way.
call setreg('f', s)
try
  let r = setreg('f', {'regcontents': 'x', 'regtype': 'vv'})
  echo 'F' r string(getreg('f'))
catch
  echo 'F threw' v:exception string(getreg('f'))
endtry

" Present-but-empty is present, so it is an error too, not "no type given".
call setreg('g', s)
try
  let r = setreg('g', {'regcontents': 'x', 'regtype': ''})
  echo 'G' r string(getreg('g'))
catch
  echo 'G threw' v:exception string(getreg('g'))
endtry

" A Dict with no regcontents at all is not empty, so it does NOT take the
" clear path; it writes the empty contents with the given type.
let r = setreg('h', {'regtype': 'v'})
echo 'H' r string(getreg('h'))

" Non-dict forms are unchanged: a List joins as lines, a Number stringifies,
" and an empty List clears.
let r = setreg('i', ['ab', 'cd'])
echo 'I' r string(getreg('i'))
let r = setreg('j', 42)
echo 'J' r string(getreg('j'))
call setreg('k', s)
let r = setreg('k', [])
echo 'K' r string(getreg('k'))
