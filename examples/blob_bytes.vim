" blob_bytes.vim — the Blob index/slice OPERATORS and Blob arithmetic (blob.c/
" eval.c), distinct from blobs.vim's slice()-builtin + positive-index coverage.
" A Blob indexes to an unsigned byte value (0..255); the [a:b] index form is
" INCLUSIVE on both ends (unlike slice()'s exclusive end) and yields a sub-Blob;
" negative indices count from the end; '+' concatenates two Blobs; '=='/'!='
" compare byte content; string() renders the 0z… literal form; type() is 10
" (v:t_blob); get() returns a byte or a supplied default for an out-of-range i.
" Self-test: asserts into v:errors, throws if any failed.

" --- element index yields the unsigned byte value, not a 1-byte Blob
call assert_equal(0, 0z00112233[0])
call assert_equal(17, 0z00112233[1])
call assert_equal(51, 0z00112233[3])
call assert_equal(239, 0zDEADBEEF[-1])
call assert_equal(190, 0zDEADBEEF[-2])

" --- [a:b] slice is INCLUSIVE of both ends (contrast slice()'s exclusive end)
call assert_equal(0z1122, 0z00112233[1:2])
call assert_equal(0z0011, 0z00112233[0:1])
call assert_equal(0z00112233, 0z00112233[0:3])
call assert_equal(0zEEDD, 0zFFEEDD[1:])
call assert_equal(0z00112233, 0z00112233[:])
call assert_equal(0z2233, 0z00112233[-2:])

" --- '+' concatenates Blobs (associative, order-preserving)
call assert_equal(0z00112233, 0z0011 + 0z2233)
call assert_equal(0z001122, 0z00 + 0z11 + 0z22)
call assert_equal(0z0011, 0z0011 + 0z)
call assert_equal(0z0011, 0z + 0z0011)

" --- '=='/'!=' compare byte content (length + every byte)
call assert_equal(1, 0z0011 == 0z0011)
call assert_equal(0, 0z0011 == 0z0012)
call assert_equal(1, 0z0011 != 0z001122)
call assert_equal(0, 0z != 0z)

" --- string() renders the canonical uppercase-nibble 0z literal
call assert_equal('0z00112233', string(0z00112233))
call assert_equal('0z', string(0z))
call assert_equal('0zDEADBEEF', string(0zdeadbeef))

" --- type() is 10, matching v:t_blob
call assert_equal(10, type(0z00))
call assert_equal(v:t_blob, type(0z00112233))

" --- get() returns the byte, or the default when the index is out of range
call assert_equal(17, get(0z001122, 1))
call assert_equal(-1, get(0z001122, 9, -1))
call assert_equal(34, get(0z001122, -1))

" --- empty()/len() on Blobs
call assert_equal(1, empty(0z))
call assert_equal(0, empty(0z00))
call assert_equal(3, len(0z001122))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'blob_bytes.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'blob_bytes.vim: all assertions passed'
