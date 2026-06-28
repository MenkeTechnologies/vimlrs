" equalfile.vim — assert_equalfile(), the file-comparison assertion ported from
" Neovim's testing.c. Writes scratch files with writefile() and compares them.
" Self-test: asserts into v:errors, throws at the end if anything failed.

let s:dir = '/tmp'
let s:a = s:dir . '/vimlrs_equalfile_a.txt'
let s:b = s:dir . '/vimlrs_equalfile_b.txt'
let s:c = s:dir . '/vimlrs_equalfile_c.txt'

call writefile(['line one', 'line two', 'line three'], s:a)
call writefile(['line one', 'line two', 'line three'], s:b)
call writefile(['line one', 'DIFFERENT', 'line three'], s:c)

" --- identical files: assert_equalfile() passes (returns 0, no v:errors)
call assert_equal(0, assert_equalfile(s:a, s:b))
call assert_true(empty(v:errors))

" --- differing files: it returns 1 and records one error. Capture and clear
"     v:errors so this expected failure does not fail the script.
call assert_equal(1, assert_equalfile(s:a, s:c))
call assert_equal(1, len(v:errors))
let v:errors = []

" --- a missing file is also a (recorded) failure
call assert_equal(1, assert_equalfile(s:a, s:dir . '/vimlrs_equalfile_missing.txt'))
call assert_equal(1, len(v:errors))
let v:errors = []

" clean up scratch files
call delete(s:a)
call delete(s:b)
call delete(s:c)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'equalfile.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'equalfile.vim: all assertions passed'
