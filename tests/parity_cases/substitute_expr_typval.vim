" `substitute()` with a `\=` replacement that yields a List or a Dict.
"
" c: `vim_regsub_both()` renders the expression's result with
" `eval_to_string(source + 2, true, false)` (`Src/nvim/regexp.c:2205`) — note
" `join_list = TRUE`. That reaches `typval2string()` (`vendor/eval.c:486`):
"
"   * a List is `tv_list_join(…, "\n")` plus a trailing NL, so `\=[1,2]` is
"     "1\n2\n" — and an EMPTY List is the empty string, with no trailing NL;
"   * any other List/Dict is `encode_tv2string()`, i.e. the `string()` form —
"     which is what a Dict takes, since `join_list` only rewrites Lists;
"   * a scalar is `tv_get_string()`, unchanged.
"
" Reading the result with `tv_get_string()` instead raised E730/E731 on both.
" `str2list()` pins the separator as NL (0x0a), not CR: `do_string_sub()` runs
" with `rsm.sm_line_lbr` set, so the c:2216 NL→CAR rewrite does not apply.

echo str2list(substitute('x', 'x', '\=[1,2]', ''))
echo string(substitute('x', 'x', '\=[1,2]', ''))
echo string(substitute('x', 'x', '\=[]', ''))
echo str2list(substitute('x', 'x', '\=[]', ''))
echo string(substitute('x', 'x', '\=[[1,2],[3]]', ''))
echo string(substitute('x', 'x', '\=[1,[2,3]]', ''))
echo string(substitute('x', 'x', '\=["a\nb","c"]', ''))
echo str2list(substitute('x', 'x', '\=["a\nb","c"]', ''))
echo string(substitute('x', 'x', '\=[1.5,v:true]', ''))
echo string(substitute('axa', 'a', '\=[9]', 'g'))
echo str2list(substitute('axa', 'a', '\=[9]', 'g'))

" A Dict takes the `encode_tv2string` branch, so it renders as `string()` does.
echo string(substitute('x', 'x', '\={"a":1}', ''))
echo string(substitute('x', 'x', '\={}', ''))
echo string(substitute('x', 'x', '\={"a":[1,2]}', ''))

" Scalars are unchanged by the join_list branch.
echo string(substitute('x', 'x', '\=1.5', ''))
echo string(substitute('x', 'x', '\=v:true', ''))
echo string(substitute('x', 'x', '\=v:null', ''))

" A Blob is neither List nor Dict, so it falls through to `tv_get_string()` and
" is still E976 — the error, then the empty replacement.
echo string(substitute('x', 'x', '\=0z0102', ''))
echo string(substitute('x', 'x', '\=[0z01]', ''))
