" msgpack.vim — MessagePack codec (msgpackdump/msgpackparse, encode.c/decode.c).
" These are pure data transforms with no editor dependency: a List of Vimscript
" objects <-> MessagePack bytes. The byte output is identical to Neovim's
" (msgpack-c minimal-width packing). BOTH return forms are byte-exact: the Blob
" form (msgpackdump(l, 'B') / msgpackparse(0z..)) and the readfile()-style List
" form, which is a byte stream split on NL with each line's NUL bytes stored as
" NL (encode_list_write's `memchrsub`). Self-test: asserts into v:errors.

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

" a String dumps as BIN and parses back as a String — measured from nvim, which
" decodes a BIN whose bytes are valid text to a String, not a Blob. (This line
" asserted `[0z6869]`, which nvim rejects too; it was never enforced because
" `assert_fails()` below used to empty `v:errors`.)
call assert_equal(['hi'], msgpackparse(msgpackdump(['hi'], 'B')))

" the default (no type) return is a readfile()-style List of byte chunks;
" 42 encodes to the single byte 0x2A, i.e. the text '*'.
call assert_equal(0z2A, msgpackdump([42], 'B'))
call assert_equal(['*'], msgpackdump([42]))

" ── the readfile()-style List form (BUGS.md R24-O3) ──
" Every value below was measured from nvim 0.12.4; vim has no msgpackdump at
" all (E117), so nvim is the only oracle and these cannot live in
" tests/parity_cases/, which records real vim.
"
" The List form is a BYTE stream split on NL, and it round-trips. It used to be
" read back through a Rust `String`, so every byte of a MessagePack payload that
" is not valid UTF-8 became U+FFFD and the parse answered E5766 for all but the
" accidentally-ASCII payloads.
call assert_equal([v:true, v:false, v:null], msgpackparse(msgpackdump([v:true, v:false, v:null])))
call assert_equal([[1, 2], {'a': 1}], msgpackparse(msgpackdump([[1, 2], {'a': 1}])))
call assert_equal([3.5, -7, 'hi'], msgpackparse(msgpackdump([3.5, -7, 'hi'])))

" a 3-element array is 0x93 0x01 0x02 0x03 — 0x93 is not valid UTF-8.
call assert_equal([[147, 1, 2, 3]], map(copy(msgpackdump([[1, 2, 3]])), 'str2list(v:val)'))

" encode_list_write() maps a NUL byte in the stream to NL inside a line
" (`memchrsub`, encode.c:78/90), and msgpackparse() inverts it. 0 encodes to the
" single byte 0x00, which is therefore stored as 0x0A.
call assert_equal([[10]], map(copy(msgpackdump([0])), 'str2list(v:val)'))
call assert_equal([[10, 10, 10]], map(copy(msgpackdump([0, 0, 0])), 'str2list(v:val)'))
call assert_equal([0], msgpackparse(msgpackdump([0])))

" empty in, empty out.
call assert_equal([], msgpackdump([]))
call assert_equal([], msgpackparse([]))
call assert_equal([], msgpackparse(['']))

" error paths, all measured from nvim.
call assert_fails('call msgpackparse([1])', 'E475: Invalid argument: List item is not a string')
call assert_fails("call msgpackparse('x')", 'E899: Argument of msgpackparse() must be a List or Blob')
call assert_fails('call msgpackparse(0zC1)', 'E475: Invalid argument: Failed to parse msgpack string')
call assert_fails('call msgpackparse(0z93)', 'E475: Invalid argument: Incomplete msgpack string')

" error path: Funcrefs cannot be dumped (E5004).
call assert_fails("call msgpackdump([function('tr')])", 'E5004')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'msgpack.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'msgpack.vim: all assertions passed'
