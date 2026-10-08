" A Funcref in string context is E729 (tv_get_string_buf_chk's str_errors[]);
" callers that want the function's NAME read v_string themselves.
let F = function('strlen')
function G(a, b)
  return a:a + a:b
endfunction
let P = function('G', [1])
try | echo 'x' . F | catch | echo v:exception | endtry
try | echo strlen(F) | catch | echo v:exception | endtry
try | echo setreg('b', F) | catch | echo v:exception | endtry
echo string(getreg('b'))
try | echo setreg('c', [F]) | catch | echo v:exception | endtry
echo string(getreg('c'))
echo join([F], ',')
echo string(F) string(P)
echo F('abc') P(2) call(F, ['ab']) call(P, [5])
echo map([1, 2], function('G', [10]))
echo sort([3, 1, 2], {a, b -> a - b})
echo reduce([1, 2, 3], function('G'))
echo substitute('abc', 'b', {m -> toupper(m[0])}, '')
echo function(F)('abcd') funcref(P)(3)
echo get(F, 'name') get(P, 'name')
try | echo printf('%s', F) | catch | echo v:exception | endtry
try | echo printf('%d', F) | catch | echo v:exception | endtry
try | echo tolower(F) | catch | echo v:exception | endtry
try | echo split(F) | catch | echo v:exception | endtry
try | echo exists(F) | catch | echo v:exception | endtry
echo 'end'
