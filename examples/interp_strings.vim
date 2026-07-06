" interp_strings.vim — vim9 interpolated strings: $'…{expr}…' and $"…{expr}…".
"
" An interpolated string embeds Vim expressions in curly braces: each {expr} is
" evaluated and its value converted to text (echo-style: strings verbatim, other
" types as :echo would render them), then concatenated with the literal parts.
" $'…' uses literal-string body rules ('' → '), $"…" uses double-quote escapes.
" To get a literal brace, double it ({{ / }}); in $"…" a backslash also works
" (\{ / \}). Works in both vim9 and legacy scripts. Self-tests with assert_*;
" exits non-zero on any failure.
"
"   vimlrs examples/interp_strings.vim

" ── the basic {var} substitution, single- and double-quote body ──
let label = 'x'
call assert_equal('[x]', $'[{label}]')
call assert_equal('[x]', $"[{label}]")

" ── an arbitrary expression, converted to a string ──
call assert_equal('sum=3', $"sum={1 + 2}")
call assert_equal('sum=3', $'sum={1 + 2}')

" ── {{ and }} are literal braces in both forms ──
call assert_equal('a{b}c', $'a{{b}}c')
call assert_equal('a{b}c', $"a{{b}}c")

" ── the eval.txt example: doubled braces plus a real expression ──
call assert_equal('The square root of {9} is 3.0', $"The square root of {{9}} is {sqrt(9)}")

" ── several expressions and interleaved literals ──
let a = 'A'
let b = 'B'
call assert_equal('A-B', $'{a}-{b}')
call assert_equal('n is 3!', $'n is {1 + 2}!')

" ── a function call inside the braces ──
call assert_equal('2', $'{len("hi")}')

" ── a double-quoted string literal may appear inside the braces, even in a
"    single-quoted interpolation; its own } does not close the interpolation ──
call assert_equal('ab}cd', $'a{"b}c"}d')

" ── a nested dict literal (its inner braces are balanced, not terminators) ──
call assert_equal('5', $'{ {"a": 5}["a"] }')

" ── the body's own escape rules: $"…" honours \t, $'…' keeps it literal ──
call assert_equal("tab\tend", $"tab\tend")
call assert_equal('tab\tend', $'tab\tend')

" ── in $"…", a backslash before a brace yields a literal brace ──
call assert_equal('a{b}c', $"a\{b\}c")

" ── '' inside a $'…' body is one literal quote ──
call assert_equal("it's", $'it''s')

" ── the result is always a String, whatever the expression's type ──
call assert_equal(1, type($'{1 + 2}'))
call assert_equal('3', $'{1 + 2}')

" ── value conversion matches :echo: Float and List render structurally ──
call assert_equal('v=1.5', $'v={1.5}')
call assert_equal('v=[1, 2]', $'v={[1, 2]}')

" ── a nested interpolated string inside the braces ──
let x = 'Q'
call assert_equal('out <Q> end', $'out {$'<{x}>'} end')

" ── an empty interpolation is the empty string ──
call assert_equal('', $'')
call assert_equal('', $"")

" ── interpolation does NOT disturb plain strings or $ENV access ──
call assert_equal('no interp {here}', 'no interp {here}')
let $INTERP_DEMO = 'env'
call assert_equal('env', $INTERP_DEMO)
call assert_equal('has env', $'has {$INTERP_DEMO}')

" ── demo ──
let who = 'world'
echo $"Hello, {who}!"

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'interp_strings.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
