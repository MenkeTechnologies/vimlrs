# vimlrs examples

Standalone `.vim` scripts that run outside any editor:

```sh
cargo build
./target/debug/viml examples/fizzbuzz.vim
```

| Script | Shows |
|---|---|
| [`fizzbuzz.vim`](fizzbuzz.vim) | `:for`/`range()`, `:if`/`:elseif`, native integer `%` |
| [`fib.vim`](fib.vim) | `:function`/recursion + a numeric `while` loop (trace-JIT target) |
| [`lists_dicts.vim`](lists_dicts.vim) | list/dict literals, slicing, `map`/`filter`/`reduce`/`sort` |
| [`strings.vim`](strings.vim) | `split`/`join`/`printf`, the Vim-magic regex engine (`=~`, `matchstr`, `substitute`) |
| [`floatmath.vim`](floatmath.vim) | the floating-point math library: `floor`/`ceil`/`round`/`trunc`, the trig/hyperbolic/`exp`/`log` family, `isnan`/`isinf` |
| [`strindex.vim`](strindex.vim) | byte/char indexing on multibyte strings: `byteidx`/`byteidxcomp`/`charidx`, `strgetchar`/`strcharpart`, `values`/`shellescape`/`matchstrpos` |
| [`json.vim`](json.vim) | `json_encode()` / `json_decode()` round-trip |
| [`editor_compat.vim`](editor_compat.vim) | editor-position builtins (`getpos`, `search`, `wordcount`, …) returning faithful "no editor" values so editor-oriented scripts still load |
| [`interactive.vim`](interactive.vim) | `input()` / `inputlist()` / `confirm()` reading from the terminal (stdin) |
| [`wordfreq.vim`](wordfreq.vim) | text pipeline: `writefile`/`readfile`, `split`, frequency dict, `sort` with a Funcref |
| [`testing.vim`](testing.vim) | the test framework itself: `assert_fails` (a command must error) and `assert_exception` (inside `:catch`), plus `:try`/`:throw` |
| [`system.vim`](system.vim) | OS interaction: `system()`/`systemlist()` (shell out, with stdin), `v:shell_error`, `environ()` |
| [`slicing.vim`](slicing.vim) | `slice()` (exclusive-end List/String/Blob slice), `strcharlen()` (folds composing marks), `strtrans()` |
| [`width.vim`](width.vim) | display width: `strwidth()` (wide CJK/emoji = 2 cells), `strdisplaywidth()` (Tab expansion), `charclass()` |
| [`glob.vim`](glob.vim) | `glob()` — list files by wildcard (`*`/`?`), `$VAR`/`~` expansion, String vs List form |
| [`lambdas.vim`](lambdas.vim) | `{args -> body}` lambdas in `map`/`filter`/`sort`/`reduce` |
| [`buffers.vim`](buffers.vim) | editor-absent buffer/window/tab builtins (`bufnr`/`winnr`/`tabpagenr`…), `strutf16len`/`utf16idx`, `globpath` |
| [`sourcing.vim`](sourcing.vim) | `:source` another `.vim` file — its functions / globals persist (`lib/mathlib.vim`) |
| [`autoload.vim`](autoload.vim) | autoload: an undefined `pkg#func()` sources `autoload/pkg.vim` on demand |
| [`oneline.vim`](oneline.vim) | one-line block bars: `if c \| … \| endif`, `for`/`while` on one line |
| [`varargs.vim`](varargs.vim) | variadic functions (`...`/`a:000`/`a:0`), `:unlet` |
| [`compare.vim`](compare.vim) | comparison operators incl. case suffixes (`==?`/`==#`/`=~?`), `is`/`isnot` |
| [`bitops.vim`](bitops.vim) | bitwise builtins `and`/`or`/`xor`/`invert` (two's-complement `invert(x) == -x-1`) |
| [`math_extra.vim`](math_extra.vim) | `pow`/`fmod` — powers, roots, and sign-preserving IEEE remainder |
| [`path_funcs.vim`](path_funcs.vim) | path strings: `simplify`/`resolve`/`isabsolutepath`, `escape`/`fnameescape` |
| [`list_transform.vim`](list_transform.vim) | non-mutating structural ops `flatten`/`flattennew`/`extendnew`/`mapnew`/`deepcopy`/`foreach` (copy-independence asserted) |
| [`strconvert.vim`](strconvert.vim) | codepoint conversion `str2list`/`list2str` (round-trip) and byte-offset search `stridx`/`strridx` |
| [`dict_ops.vim`](dict_ops.vim) | Dictionary builtins `keys`/`values`/`items`/`has_key`/`get`/`extend`/`filter`/`map`/`remove`/`copy` (hash order normalised via `sort`) |
| [`list_ops.vim`](list_ops.vim) | mutating/searching list builtins `insert`/`add`/`remove` (index+range+negative), `count`/`index`/`extend`/`uniq` |
| [`sort_variants.vim`](sort_variants.vim) | every `sort()` mode (default/`n`/`N`/`i`/`f`/custom comparator/legacy `1`), `reverse`, and the `sort`+`uniq` dedupe idiom |
| [`printf_fmt.vim`](printf_fmt.vim) | `printf()` specifiers: `%d`/`%x`/`%o`/`%b` radices, `0`/`-`/`+`/`#` flags, width/precision, `%e`/`%f`/`%g`, `%c`, `*` width, `N$` positional |
| [`str_case.vim`](str_case.vim) | case folding (`toupper`/`tolower`), `trim` (mask+direction), and match position `match`/`matchend`/`matchstr`/`matchlist` |
| [`dot_operator.vim`](dot_operator.vim) | the `.` operator's runtime dispatch: no-space `base.name` is a Dict subscript when `base` is a Dict at runtime, else string concat (`a.b` over string vars → `pq`), chains, lambda-body concat, base-evaluated-once |

Run any script with `VIMLRS_JIT_STATS=1` to see JIT activity, or `VIMLRS_NO_JIT=1`
to force the interpreter baseline.

### Self-testing — these examples are the regression suite

Every script is also a **unit test**: it asserts its expected results with Vim's
built-in test framework (`assert_equal`, `assert_true`, `assert_match`,
`assert_inrange`, …), which records failures in `v:errors`. Each ends with an
epilogue that `throw`s — making the process exit non-zero — when `v:errors` is
non-empty:

```vim
call assert_equal('Fizz', FizzBuzz(3))
" ...
if !empty(v:errors)
  for e in v:errors | echo 'FAIL:' e | endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
```

CI runs them all two ways, so a behaviour regression in a ported builtin turns a
green assert red:

- a dedicated **`examples` CI job** runs `sh scripts/run_examples.sh`, which
  executes every script through the release binary and fails if any exits
  non-zero (run it locally the same way);
- `tests/examples.rs` does the same under `cargo test` (the `test` job).

The interactive example is fed canned answers from `tests/fixtures/interactive.in`.

### Notes on the current language surface

These idioms — common in modern Vimscript — are now all supported:

- **`{ x -> … }` lambdas** — supported (desugar to anonymous functions); use
  them directly in `map()`/`filter()`/`sort()`/`reduce()`. They **capture
  enclosing-scope variables** (by value at creation, including nested closures),
  and a lambda/Funcref stored in a variable is callable both directly —
  `F(args)` — and via `call(F, [args])`.
- **`d.key` member *read*** — now supported, and disambiguated at **runtime** by
  the type of the base: a no-space `base.name` is a Dict subscript when `base` is
  a Dict at runtime, and string concatenation otherwise (so `a.b` over two string
  vars yields `'pq'`, matching Vim). A spaced `a . b` is always concatenation.
  Bracket access `d['key']` also works and is needed for non-literal keys.
- **`\` line continuation** — now supported: a line whose first non-blank char
  is `\` joins onto the previous one, so multi-line list/dict literals work (see
  `json.vim`).
