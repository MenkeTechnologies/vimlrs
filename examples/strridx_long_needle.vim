" strridx_long_needle.vim — strridx() byte index of the LAST occurrence of a
" needle (strings.c f_strridx). C strstr() never matches a needle longer than
" the haystack, so the result is -1. The Rust port floored its search range at 0
" with saturating_sub(), which then indexed hb[0..needle_len] out of bounds and
" panicked whenever needle_len > haystack_len (Vim corpus: syntax/tera.vim runs
" strridx(s:filename, '.tera') on short filenames). Verify the boundary plus the
" ordinary cases, matching real Vim.

" --- needle longer than haystack never matches (the panic case)
call assert_equal(-1, strridx('ab', 'abcd'))
call assert_equal(-1, strridx('', 'a'))
call assert_equal(-1, strridx('x', 'xyz'))

" --- LAST occurrence, not the first
call assert_equal(12, strridx('hello world hello', 'hello'))
call assert_equal(0, strridx('hello world hello', 'hello world'))

" --- no match among fitting-length needles is -1
call assert_equal(-1, strridx('hello', 'xyz'))

" --- empty needle matches past the end (index == haystack length)
call assert_equal(3, strridx('abc', ''))
call assert_equal(0, strridx('', ''))

" --- optional 3rd arg caps the match index; a needle after it is not found
call assert_equal(0, strridx('hello world hello', 'hello', 5))
call assert_equal(-1, strridx('hello world hello', 'hello', -1))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'strridx_long_needle.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'strridx_long_needle.vim: all assertions passed'
