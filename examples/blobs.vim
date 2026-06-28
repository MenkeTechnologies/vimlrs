" blobs.vim — Blob literals and byte operations, with embedded unit tests.
"
" Demonstrates the 0z… Blob literal (now lexed natively), indexing, slicing,
" len(), and the blob/list conversions. Self-checks and exits non-zero on
" failure.
"
"   vimlrs examples/blobs.vim

" ── Blob literals ──
let b = 0z00112233
call assert_equal(4, len(b))
call assert_equal(0, b[0])
call assert_equal(17, b[1])
call assert_equal(51, b[3])

" '.' may group bytes for readability.
call assert_equal(0z001122, 0z00.11.22)

" ── slice() uses byte indices on a Blob (exclusive end) ──
call assert_equal(0z1122, slice(b, 1, 3))
call assert_equal(0z2233, slice(b, 2))

" ── blob <-> list round-trip ──
call assert_equal([0, 17, 34, 51], blob2list(b))
call assert_equal(b, list2blob([0, 17, 34, 51]))

" ── an empty blob ──
call assert_equal(0, len(0z))

" ── hex case-insensitive ──
call assert_equal(0zdeadbeef, 0zDEADBEEF)

" ── demo ──
echo 'blob    :' b
echo 'as list :' blob2list(b)
echo 'sliced  :' slice(b, 1, 3)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: blobs assertions passed'
