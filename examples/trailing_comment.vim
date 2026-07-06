" trailing_comment.vim — legacy trailing `"` comments after a single-expression
" command (:if / :elseif / :while / :for / :return / :let), plus b:changedtick.
"
" Crash-hunt regression. Several Vim 9.2 runtime scripts (runtime/indent/cdl.vim,
" php.vim, r.vim, …) define a function whose :while / :elseif lines carry a
" trailing `" …` comment. vimlrs mis-lexed that `"` as the start of an
" unterminated double-quoted string (E114), so the strict parse failed and the
" file dropped to the tolerant fallback, which ran the function body at script
" scope — where the JIT-compiled loop dereferenced a null pointer (exit 139,
" SIGSEGV). With the trailing comment recognised, the strict parse succeeds and
" the function is DEFINED, exactly as real Vim sources these files.

" --- trailing comment after a :let RHS (value must not absorb the comment)
let s:a = 1 " a note
call assert_equal(1, s:a)
let s:b = 'x' " a note with a quote's in it
call assert_equal('x', s:b)

" --- trailing comment after :if / :elseif conditions, in the exact cdl.vim
"     shape: single-quote char literals + `||` + a comment that itself contains
"     single quotes. Closed with `end` (a legal :endif abbreviation), as cdl does.
func! s:Classify(c)
  let r = 'other'
  if a:c == '(' " open paren
    let r = 'open'
  elseif a:c == ')' || a:c ==? 'f' " '(' or 'if'
    let r = 'close_or_f'
  else " everything else
    let r = 'other'
  end
  return r
endf
" The function must actually be defined (strict parse), not skipped by fallback.
call assert_equal(1, exists('*s:Classify'))
call assert_equal('open', s:Classify('('))
call assert_equal('close_or_f', s:Classify(')'))
call assert_equal('close_or_f', s:Classify('F'))
call assert_equal('other', s:Classify('x'))

" --- trailing comment after :while, closed with `endw` (as cdl.vim does)
func! s:CountDown(n)
  let i = a:n
  let acc = 0
  while i > 0 " keep going
    let acc += i
    let i -= 1
  endw
  return acc " the running total
endf
call assert_equal(1, exists('*s:CountDown'))
call assert_equal(6, s:CountDown(3))

" --- trailing comment after :for
func! s:SumList(xs)
  let t = 0
  for x in a:xs " each element
    let t += x
  endfor
  return t
endf
call assert_equal(10, s:SumList([1, 2, 3, 4]))

" --- a genuine double-quoted string operand is a value, NOT a comment: both
"     `"a"` strings close, so only the trailing unterminated `"` is the comment.
let s:matched = 0
if "a" == "a" " they are equal
  let s:matched = 1
end
call assert_equal(1, s:matched)

" --- b:changedtick is Vim's always-present buffer change counter: a Number,
"     never an undefined-variable (E121) error.
call assert_equal(type(0), type(b:changedtick))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'trailing_comment.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'trailing_comment.vim: all assertions passed'
