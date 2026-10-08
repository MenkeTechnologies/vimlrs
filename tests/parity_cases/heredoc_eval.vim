" `:let =<< eval` runs eval_all_expr_in_str on every line: {expr} is replaced
" by its value, {{ and }} are literal braces, and quotes are ordinary text.
let n = 5
let s = 'str'
let a =<< eval END
n={n} twice={n * 2}
it's {s}'s {{braces}} and {'quoted'}
{[1, 'x']} {{'a': 1}} {{n}}
END
echo a
let b =<< trim eval END
    {n}
      {n + 1}
END
echo b
let c =<< END
{n} stays
END
echo c
function! H(x)
  let r =<< eval EOT
arg {a:x} local {toupper(a:x)}
EOT
  return r
endfunction
echo H('q')
