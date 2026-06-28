" numeric_edge.vim — numeric/string edge cases that diverged from Vim.
" charidx() maps a byte index to its character index and must not crash on a
" byte inside a multibyte character; '%' is integer-only (a Float operand is
" E804, unlike '*'/'/'); str2float() parses hex like strtod. Self-test.

" --- charidx(): byte -> character index, multibyte-safe (no panic)
call assert_equal(0, charidx('héllo', 0))
call assert_equal(1, charidx('héllo', 1))
call assert_equal(1, charidx('héllo', 2))
call assert_equal(2, charidx('héllo', 3))
call assert_equal(-1, charidx('héllo', 99))

" --- '%' with a Float operand is an error (E804); integer '%' is fine
call assert_equal(1, 7 % 3)
call assert_equal(-1, -7 % 3)
call assert_fails('echo 1.0 % 2.0', 'E804')

" --- str2float() parses hex (strtod semantics), and still decimal/exponent
call assert_equal(31.0, str2float('0x1f'))
call assert_equal(-16.0, str2float('-0x10'))
call assert_equal(3.14, str2float('3.14'))
call assert_equal(1500.0, str2float('1.5e3'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'numeric_edge.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'numeric_edge.vim: all assertions passed'
