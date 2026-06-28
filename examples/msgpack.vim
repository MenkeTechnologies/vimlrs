" msgpack.vim — MessagePack codec (msgpackdump/msgpackparse, encode.c/decode.c).
" These are pure data transforms with no editor dependency: a List of Vimscript
" objects <-> MessagePack bytes. The byte output is identical to Neovim's
" (msgpack-c minimal-width packing). The Blob form (msgpackdump(l, 'B') and
" msgpackparse(0z..)) is byte-exact; the readfile()-style List form uses the
" same text convention as readfile() here. Self-test: asserts into v:errors.

" byte-exact encoding (Blob form) against known MessagePack encodings:
" empty map (0x80), 3-element array, {'a': 1}, and the nil/true/false bytes.
call assert_equal(0z80, msgpackdump([{}], 'B'))
call assert_equal(0z93010203, msgpackdump([[1, 2, 3]], 'B'))
call assert_equal(0z81A16101, msgpackdump([{'a': 1}], 'B'))
call assert_equal(0zC0C3C2, msgpackdump([v:null, v:true, v:false], 'B'))

" minimal-width integer packing: positive fixint, uint16, neg fixint, int8.
call assert_equal(0z7F, msgpackdump([127], 'B'))
call assert_equal(0zCD0100, msgpackdump([256], 'B'))
call assert_equal(0zFF, msgpackdump([-1], 'B'))
call assert_equal(0zD080, msgpackdump([-128], 'B'))

" a Float is always packed as float64 (0xcb): 1.0 -> 0x3FF0000000000000.
call assert_equal(0zCB3FF0000000000000, msgpackdump([1.0], 'B'))

" parse is the inverse for the self-describing subset.
call assert_equal([1, 2, 3], msgpackparse(0z93010203)[0])
call assert_equal({'a': 1}, msgpackparse(0z81A16101)[0])
call assert_equal([v:null, v:true, v:false], msgpackparse(0zC0C3C2))

" round-trip through the Blob form preserves numbers/containers exactly.
let objs = [42, -7, 3.5, [1, [2, 3]], {'k': 'v', 'n': 10}, v:null, v:true]
call assert_equal(objs, msgpackparse(msgpackdump(objs, 'B')))

" documented lossy edge: a String dumps as BIN, so it parses back as a Blob.
call assert_equal([0z6869], msgpackparse(msgpackdump(['hi'], 'B')))

" the default (no type) return is a readfile()-style List of byte chunks;
" 42 encodes to the single byte 0x2A, i.e. the text '*'.
call assert_equal(0z2A, msgpackdump([42], 'B'))
call assert_equal(['*'], msgpackdump([42]))

" error path: Funcrefs cannot be dumped (E5004).
call assert_fails("call msgpackdump([function('tr')])", 'E5004')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'msgpack.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'msgpack.vim: all assertions passed'
