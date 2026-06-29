" env_vars.vim — environment variables: $VAR, getenv()/setenv(), expand().
"
" $NAME reads an environment variable as a string; :let $NAME = val sets it for
" this process; getenv() returns v:null for an unset variable (distinct from an
" empty string), and expand('$NAME') performs the same lookup. Self-tests with
" assert_*; exits non-zero on any failure.
"
"   vimlrs examples/env_vars.vim

" ── set and read back through every surface ──
let $VIMLRS_DEMO = 'hello'
call assert_equal('hello', $VIMLRS_DEMO)
call assert_equal('hello', getenv('VIMLRS_DEMO'))
call assert_equal('hello', expand('$VIMLRS_DEMO'))

" ── setenv() is the functional form of :let $X = ... ──
call setenv('VIMLRS_DEMO', 'world')
call assert_equal('world', $VIMLRS_DEMO)

" ── an unset variable: getenv() is v:null, but $X reads as "" ──
call assert_equal(v:null, getenv('VIMLRS_NOT_SET_XYZ'))
call assert_equal('', $VIMLRS_NOT_SET_XYZ)

" ── setenv(name, v:null) removes the variable ──
call setenv('VIMLRS_DEMO', v:null)
call assert_equal(v:null, getenv('VIMLRS_DEMO'))

" ── $VAR participates in string concatenation and interpolation ──
let $GREET = 'hi'
let $WHO = 'there'
call assert_equal('hi there', $GREET . ' ' . $WHO)
call assert_equal('hi-there', printf('%s-%s', $GREET, $WHO))

" ── expand() also resolves the ${NAME} brace form ──
let $PLACE = 'home'
call assert_equal('home', expand('${PLACE}'))

" ── demo ──
let $STAGE = 'demo'
echo '$STAGE        ->' $STAGE
echo "getenv unset  ->" getenv('NOPE_NOPE')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'env_vars.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
