" A lambda body is an EXPRESSION with its own text, not a do_cmdline — and both
" halves of that are observable.
"
" c: get_lambda_tv (Src/nvim/eval/userfunc.c:356-405) stores the BODY TEXT for
" the function it generates, so a diagnostic raised inside a lambda quotes only
" up to the body's end. E116's text is `%s` over a pointer INTO the source
" (emsg_funcname, vendor/eval/userfunc.c:492-500), which is what makes the
" difference visible: outside a lambda the quote runs to the end of the eval
" buffer, inside one it stops at the `}`.
"
" c:1276-1282 is the other half: "when the function was aborted because of an
" error, return -1" — `if ((did_emsg && (fp->uf_flags & FC_ABORT)) ||
" rettv->v_type == VAR_UNKNOWN)`. Only a lambda reaches the second disjunct,
" because get_lambda_tv sets neither FC_ABORT (c:392-397) nor a return value
" when its single expression fails.

echo map([1], {i,v -> strlen([1] . '')})
echo '--1'
echo sort([2,1], {a,b -> type([1] . '')})
echo '--2'
let g:seen = []
call foreach([1,2], {i,v -> add(g:seen, v == 1 ? [] . '' : v)})
echo 'seen=' string(g:seen)
echo '--3'
let F = {x -> len([1] . '')}
echo F(1)
echo '--4'
echo call({x -> abs([1] . '')}, [1])
echo '--5'

" Nested lambdas clip at their OWN body end.
echo map([1], {i,v -> map([1], {j,w -> strlen([1] . '')})})
echo '--6'

" The same call written OUTSIDE a lambda still quotes to the end of the buffer.
echo strlen([1] . '') 'TAIL'
echo '--7'

" The -1 default, next to the shapes that do NOT get it.
function! A()
endfunction
function! B()
  return
endfunction
function! C()
  return len([1] . '')
endfunction
function! D() abort
  return len([1] . '')
endfunction
function! Show(n, v)
  echo a:n . ' = ' . string(a:v)
endfunction
let g:r = 'U'
silent! let g:r = A()
call Show('fn no-return  ', g:r)
let g:r = 'U'
silent! let g:r = B()
call Show('fn bare return', g:r)
let g:r = 'U'
silent! let g:r = C()
call Show('fn failing ret', g:r)
let g:r = 'U'
silent! let g:r = D()
call Show('abort failing ', g:r)
let g:r = 'U'
silent! let g:r = {x -> len([1] . '')}(1)
call Show('lambda failing', g:r)
let g:r = 'U'
silent! let g:r = {x -> 7}(1)
call Show('lambda ok     ', g:r)
echo '--8'
