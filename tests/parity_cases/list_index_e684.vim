" E684 from an assignment/removal index, both halves of BUGS.md R22-O2.
"
" `:let l[i] = v` and `:unlet l[i]` reach the same `get_lval` list arm in the C,
" which resolves the index with `tv_list_check_range_index_one` ->
" `tv_list_find_index` (vendor/eval/typval.c:1716):
"
"   listitem_T *li = tv_list_find(l, *idx);
"   if (li != NULL) { return li; }
"   if (*idx < 0) { *idx = 0; li = tv_list_find(l, *idx); }
"   return li;
"
" so a NEGATIVE index that is out of range is not an error at all — it clamps to
" 0 — and the message for the positive case carries the index
" (`semsg(_(e_list_index_out_of_range_nr), (int64_t)(*n1))`, c:641).
"
" Reading an index (`echo l[9]`) goes through `eval_index` instead, which has no
" such clamp: there a negative out-of-range index IS an error. That asymmetry is
" the point of the case, so both are exercised.
let l = [1, 2, 3]

" Read: out of range either way, and the index is in the message.
try
  echo l[9]
catch
  echo 'read+:' v:exception
endtry
try
  echo l[-9]
catch
  echo 'read-:' v:exception
endtry
echo l[-3]
echo l[-1]

" Assign: positive out of range errors with the index, negative clamps to 0.
let m = [1, 2, 3]
try
  let m[9] = 99
catch
  echo 'let+:' v:exception
endtry
echo 'm=' string(m)
let m2 = [1, 2, 3]
try
  let m2[-9] = 99
catch
  echo 'let-:' v:exception
endtry
echo 'm2=' string(m2)
let m3 = [1, 2, 3]
let m3[-2] = 77
echo 'm3=' string(m3)

" Remove: same resolution, so a negative out-of-range index removes item 0.
let n = [1, 2, 3]
try
  unlet n[9]
catch
  echo 'unlet+:' v:exception
endtry
echo 'n=' string(n)
let n2 = [1, 2, 3]
try
  unlet n2[-9]
catch
  echo 'unlet-:' v:exception
endtry
echo 'n2=' string(n2)
let n3 = [1, 2, 3]
unlet n3[-1]
echo 'n3=' string(n3)

" NOT tested here: `let e[-9] = 1` on an EMPTY list, where the clamp finds no
" item at 0 either and the two oracles report different indices —
" vim `E684: List index out of range: -9`, nvim `... : 0`. The vendored C
" (`tv_list_find_index`) writes 0 into `*idx` before its second lookup, which is
" nvim's answer; vim's own source is not vendored, so which of the two this port
" should follow is an open question rather than a guess. Recorded in BUGS.md.
