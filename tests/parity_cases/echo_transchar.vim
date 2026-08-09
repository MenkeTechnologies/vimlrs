" `:echo` renders through vim's message layer, which does NOT write the bytes it
" was handed: `ex_echo` calls `msg_multiline` (vendor/eval.c:6181), which chunks
" on the delimiters and hands each run to `msg_outtrans_len`
" (vendor/message.c:1866), which replaces every character the terminal cannot
" show with its `transchar` text (vendor/charset.c:541).
"
" Three different answers come out of that, and the difference between them is
" the whole point of the transform:
"   - a control BYTE       -> `^X`      (transchar_nonprint, c ^ 0x40)
"   - an illegal UTF-8 BYTE -> `<ff>`   (transchar_byte_buf short-circuits >= 0x80)
"   - an unprintable CHARACTER -> `<200b>` (utf_printable says no)
" while a printable character — including one above U+10FFFF — is left alone.
"
" NOT covered here, deliberately: a BELL (0x07). vim writes `^G`; nvim consumes
" it and beeps instead (`vim_beep`, vendor/message.c:296). The vendored C is
" nvim's, so this port answers `AB` for `echo 'A' . nr2char(7) . 'B'` and cannot
" also answer vim's `A^GB`. Recorded in BUGS.md as R24-O1 rather than guessed at.

" A control byte becomes ^X, in isolation and mid-string.
echo nr2char(1)
echo "a\x01b"
echo nr2char(0x1b)
echo nr2char(0x1f)
" DEL is ^?, not <7f>.
echo nr2char(0x7f)
" The printable ASCII either side of the control range is untouched.
echo nr2char(0x20) . '|' . nr2char(0x7e)

" A byte that starts no valid UTF-8 sequence is hex, not a character.
" (list2str([-1,0,1]) is the two bytes ff 01 — see list2str_bytes.vim.)
echo list2str([-1,0,1])
" 0x80 as a CHARACTER (list2str encodes it as c2 80) is unprintable -> <80>.
echo list2str([0x80])
" ... and mixed with a printable one: c3 82 is U+00C2, which stays.
echo list2str([0xc2,0x80])
" U+00A0 is printable (g_chartab marks 0xa0-0xff so), so it is NOT hexed.
echo list2str([0xc2,0xa0])

" An unprintable CHARACTER is hexed with its full code point.
echo nr2char(0x200b)
" A printable one is not, including above U+10FFFF (utf_printable's intervals
" all sit below 0x10000, so everything past them is printable).
echo "é"
echo list2str([0x110000])

" msg_multiline's delimiters stay literal: a TAB is a TAB under :echo ...
echo "a\tb"
" ... but strtrans() calls transstr(s, untab = true), where it is ^I.
echo strtrans("a\tb")

" strtrans() is the same transform reached from an expression, so it answers
" the same three ways — and must read its argument as BYTES to do it.
echo strtrans(nr2char(1))
echo strtrans(nr2char(0x7f))
echo strtrans(list2str([-1]))
echo strtrans(list2str([0x80]))
echo strtrans(nr2char(0x200b))
echo strtrans("é")
echo strtrans(nr2char(0x110000))

" The separator between two :echo arguments is written directly, not translated.
echo 1 nr2char(1) 2

" execute() captures what the message layer produced, not what it was handed.
echo string(execute('echo nr2char(1)'))

" :echomsg goes through the same msg_multiline.
echomsg nr2char(1)
