" regex_atoms.vim — the option-derived Vim regex char-class atoms.
" \h \H (head-of-word), \o \O (octal), and the four "excluding digits" atoms
" \p \P / \i \I / \k \K (default 'isprint'/'isident'/'iskeyword'). Before this
" work these fell through to literal matching. Every expected value below was
" verified byte-identical against nvim 0.12.3 and vim 9.2 with default options.
"
" Key non-obvious facts pinned here (see src/viml_regex.rs):
"   * \P \I \K are NOT set-complements of \p \i \k — they mean "same class but
"     excluding digits" (:help /\P). \H \O ARE true negations.
"   * \h = [A-Za-z_] (ASCII, no digits, unlike \w).
"   * \i (isident) is single-byte only: é (U+00E9) matches, Ω (U+03A9) does not.
"   * \k (iskeyword) is multibyte-aware: é, Ω, 中 all match.
"   * \p is printable incl. multibyte (é Ω 中); tab is not printable.
" Self-test: asserts into v:errors, throws if any failed.

" --- \h [A-Za-z_]: head-of-word, stops at first non-letter/underscore
call assert_equal('f', matchstr('f0o_9 ', '\h\+'))
call assert_equal('_id', matchstr('_id0 x', '\h\+'))
" --- \H (non-\h, true negation): digit/space/multibyte are all non-\h
call assert_equal('0', matchstr('0abÿc', '\H\+'))
call assert_equal('9', matchstr('9_id0', '\H\+'))
call assert_equal('!Ω9', matchstr('!Ω9', '\H\+'))

" --- \o [0-7]: octal digits, stops at 8/9
call assert_equal('01234567', matchstr('01234567 8', '\o\+'))
call assert_equal('70', matchstr('7089', '\o\+'))
call assert_equal('', matchstr('89', '\o\+'))
" --- \O (non-\o, true negation)
call assert_equal('89ab', matchstr('89ab', '\O\+'))
" --- substitute over \o: only 0-7 replaced
call assert_equal('aXb8', substitute('a7b8', '\o', 'X', 'g'))

" --- \p printable, includes multibyte (é Ω 中) but NOT tab
call assert_equal('abc éΩ中', matchstr('abc éΩ中', '\p\+'))
call assert_equal('a', matchstr("a\tb printable", '\p\+'))
" --- \P = \p excluding digits (NOT a negation): 'a' matches, digit stops it
call assert_equal('a', matchstr('a1b2', '\P\+'))
" --- \P index: first printable non-digit is 'a' at 0 (tab later is non-\p)
call assert_equal(0, match("ab\tc", '\P'))

" --- \i identifier: single-byte only, é (192-255) matches, Ω (>255) does not
call assert_equal('1fooé', matchstr('1fooé Ω', '\i\+'))
call assert_equal('foo', matchstr('foo.bar', '\i\+'))
" --- \I = \i excluding digits: leading digit dropped, then stops at '1'
call assert_equal('fooé', matchstr('1fooé Ω', '\I\+'))
call assert_equal('x', matchstr('x123y', '\I\+'))

" --- \k keyword: multibyte-aware, é Ω 中 all keyword chars; '-'/space are not
call assert_equal('abc123', matchstr('abc123-Ω中', '\k\+'))
call assert_equal('café_1', matchstr('café_1 z', '\k\+'))
" --- \K = \k excluding digits: leading digit dropped, multibyte kept
call assert_equal('abc', matchstr('abc123-Ω中', '\K\+'))
call assert_equal('café中', matchstr('1café中 z', '\K\+'))
" --- substitute over \k: ASCII word, digit AND é all replaced (é is a keyword)
call assert_equal('___', substitute('a1é', '\k', '_', 'g'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_atoms.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_atoms.vim: all assertions passed'
