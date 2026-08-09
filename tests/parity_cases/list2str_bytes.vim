" A VimL string is char_u * — bytes, with no encoding invariant — and list2str()
" is the builtin that puts arbitrary ones in. utf_char2bytes() takes its
" `c < 0x80` arm for a NEGATIVE c too (`buf[0] = (char_u)c`), and applies no
" range check above U+10FFFF:
"
"   list2str([-1])       -> the single byte ff
"   list2str([0x110000]) -> the four bytes f4 90 80 80
"
" Neither fits in a Rust String, so this case pins the byte model through the
" observables that are themselves byte-clean: str2list() (which walks the bytes
" back with utf_ptr2char), len()/strlen() (byte counts), and equality.
"
" `:echo` of the raw bytes is deliberately NOT tested here: vim's message layer
" renders an invalid byte as `<ff>` and a control character as `^A`
" (msg_outtrans/transchar), which this port does not implement — a separate gap
" that would make this case about message escaping instead of about the value.
echo str2list(list2str([-1, 0, 1]))
echo str2list(list2str([-1]))
echo str2list(list2str([0x110000]))
echo str2list(list2str([200, 300, 255]))
echo len(list2str([-1, 0, 1]))
echo strlen(list2str([-1, 0, 1]))
echo len(list2str([0x110000]))
echo str2list(list2str([-1, 0, 1]) . list2str([65]))
echo list2str([-1]) ==# list2str([-1])
echo list2str([-1]) ==# list2str([1])
echo str2list(nr2char(0x110000))
" list2str/str2list round-trip over the whole low byte range that survives it.
echo str2list(list2str(range(1, 127))) == range(1, 127)
" The 0 contributes nothing but does not end the walk (vim's ga_concat is
" STRLEN-based), which is what R22-6 fixed; it holds for byte values too.
echo str2list(list2str([255, 0, 65]))
