# vimlrs examples

Standalone `.vim` scripts that run outside any editor:

```sh
cargo build
./target/debug/vimlrs examples/fizzbuzz.vim
```

| Script | Shows |
|---|---|
| [`fizzbuzz.vim`](fizzbuzz.vim) | `:for`/`range()`, `:if`/`:elseif`, native integer `%` |
| [`fib.vim`](fib.vim) | `:function`/recursion + a numeric `while` loop (trace-JIT target) |
| [`lists_dicts.vim`](lists_dicts.vim) | list/dict literals, slicing, `map`/`filter`/`reduce`/`sort` |
| [`strings.vim`](strings.vim) | `split`/`join`/`printf`, the Vim-magic regex engine (`=~`, `matchstr`, `substitute`) |
| [`json.vim`](json.vim) | `json_encode()` / `json_decode()` round-trip |
| [`editor_compat.vim`](editor_compat.vim) | editor-position builtins (`getpos`, `search`, `wordcount`, …) returning faithful "no editor" values so editor-oriented scripts still load |
| [`interactive.vim`](interactive.vim) | `input()` / `inputlist()` / `confirm()` reading from the terminal (stdin) |
| [`wordfreq.vim`](wordfreq.vim) | text pipeline: `writefile`/`readfile`, `split`, frequency dict, `sort` with a Funcref |
| [`testing.vim`](testing.vim) | the test framework itself: `assert_fails` (a command must error) and `assert_exception` (inside `:catch`), plus `:try`/`:throw` |

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

CI runs them all via `cargo test` — `tests/examples.rs` executes each script
through the built binary and fails if any exits non-zero. So a behaviour
regression in a ported builtin turns a green assert red. The interactive example
is fed canned answers from `tests/fixtures/interactive.in`.

### Notes on the current language surface

These scripts stick to what the parser supports today. Two idioms common in
modern Vimscript are not yet wired and are avoided here:

- **`{ x -> ... }` lambdas** — use a string-expression body (`'v:val * v:val'`)
  for `map()`/`filter()`, and a named function via `function('Name')` for
  `reduce()`/`sort()` comparators.
- **`d.key` member *read*** — use bracket access `d['key']` (dot-form is parsed
  as string concatenation in expression position for now).
- **`\` line continuation** — keep each statement on one line.
