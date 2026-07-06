" colornames.vim — the v:colornames predefined variable (evalvars.c vimvars[]).
" v:colornames (vim 8.1.0729+) is a Dict mapping a color name to its "#rrggbb"
" hex, empty at startup and populated at runtime by the colors/lists/*.vim files
" (via extend()/indexed assignment). The binding is read-only (VV_RO — you cannot
" reassign v:colornames) but its contents are writable. This script pins only the
" top-level behavior: type, empty-at-startup, and mutability via indexed
" assignment / extend(). (v: persistence across nested user-function calls is a
" separate pre-existing limitation and is deliberately NOT asserted here.)
" Self-test: asserts into v:errors, throws if any failed.

" --- startup: a Dict (type 4), empty
call assert_equal(4, type(v:colornames))
call assert_equal(0, len(v:colornames))

" --- indexed assignment mutates the shared Dict
let v:colornames['red'] = '#ff0000'
call assert_equal(1, len(v:colornames))
call assert_equal('#ff0000', v:colornames['red'])

" --- extend() with the 3-arg 'keep' form (as colors/lists/default.vim uses):
" existing keys are kept, new keys added.
call extend(v:colornames, {'red': '#000000', 'blue': '#0000ff'}, 'keep')
call assert_equal(2, len(v:colornames))
call assert_equal('#ff0000', v:colornames['red'])
call assert_equal('#0000ff', v:colornames['blue'])

" --- get() with a default for a missing name
call assert_equal('MISSING', get(v:colornames, 'chartreuse', 'MISSING'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'colornames.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'colornames.vim: all assertions passed'
