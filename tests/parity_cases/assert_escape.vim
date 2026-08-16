" ga_concat_shorten_esc() / ga_concat_esc() — how assert_*() renders the values
" it reports.
"
" The values already went through string(); this is a SECOND pass over that
" rendering which escapes the C0 controls and the backslash, and collapses a run
" of more than 20 identical characters. So `'a\b'` is reported as `'a\\b'` — a
" doubled backslash in a v:errors entry is correct, not a bug.
"
" Only the location stamp is stripped (it carries this file's absolute path);
" everything the escapers produced is compared verbatim.
function! Msg()
  let e = v:errors[0]
  let v:errors = []
  return substitute(e, '^.\{-}line \d\+: ', '', '')
endfunction

" '\\' — the case that made assert_match() reports look wrong.
call assert_equal('a\b', 'c')
echo Msg()
call assert_match('a\+', 'bbb')
echo Msg()
call assert_notmatch('b\+', 'bbb')
echo Msg()

" The named control escapes, in the order the C switch lists them. BS is written
" as the raw byte, not "\<BS>": that notation is the K_SPECIAL <80>kb sequence,
" which is a separate open gap (see BUGS.md) and would test the lexer, not this.
call assert_equal("a\x08b", 'c')
echo Msg()
call assert_equal("a\<Esc>b", 'c')
echo Msg()
call assert_equal("a\<C-L>b", 'c')
echo Msg()
call assert_equal("a\nb", 'c')
echo Msg()
call assert_equal("a\tb", 'c')
echo Msg()
call assert_equal("a\rb", 'c')
echo Msg()

" Any other control byte, and DEL, take the \xNN form.
call assert_equal("a\x01b", 'c')
echo Msg()
call assert_equal("a\x7fb", 'c')
echo Msg()

" A multibyte character is copied through untouched — the escaper only looks at
" single-byte characters.
call assert_equal('é', 'c')
echo Msg()

" More than 20 of the same character collapses; exactly 20 does not.
call assert_equal(repeat('x', 21), 'c')
echo Msg()
call assert_equal(repeat('x', 20), 'c')
echo Msg()

" The run is counted in characters, not bytes.
call assert_equal(repeat('é', 25), 'c')
echo Msg()

" The user {msg} is NOT escaped — only the values are.
call assert_equal(1, 2, 'a\b')
echo Msg()
