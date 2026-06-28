" sourcing.vim — :source another .vim file; its functions and globals persist.
"
" Demonstrates modular scripts: a library file is sourced (read + run in the
" current scope), after which its functions are callable and its globals are
" set. Self-checks and exits non-zero on failure.
"
"   vimlrs examples/sourcing.vim   (run from the project root)

source examples/lib/mathlib.vim

" ── unit tests: the sourced functions and globals are now available ──
call assert_equal(1, g:mathlib_loaded)
call assert_equal(16, Square(4))
call assert_equal(125, Cube(5))

" ── the sourced functions compose with lambdas ──
call assert_equal([1, 4, 9], map([1, 2, 3], {i, v -> Square(v)}))

" ── demo ──
echo 'squares:' map(range(1, 5), {i, v -> Square(v)})
echo 'cubes  :' map(range(1, 5), {i, v -> Cube(v)})

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: sourcing assertions passed'
