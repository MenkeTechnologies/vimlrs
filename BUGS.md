# vimlrs — known parity bugs vs Vim

Goal: behavioral parity with real Vim's `:echo` / expression semantics. Each entry
below is a **reproduced divergence** between `vimlrs` and **Vim 9.2**.

Repro helpers:

```sh
V=./target/debug/viml
vimref() { vim -es -u NONE -i NONE -c 'redir! > /tmp/vr.txt' \
  -c "silent! echo $1" -c 'redir END' -c 'qa!' >/dev/null 2>&1; sed '1{/^$/d;}' /tmp/vr.txt; }
# usage: vimref "'abc' ==? 'ABC'"   ;   $V -e "'abc' ==? 'ABC'"
```

(Note: `viml -e` mis-parses an expression that *starts* with `-` as a CLI flag,
e.g. `viml -e '-3/2'`; use `-c 'echo -3/2'` instead. That is a CLI-parsing quirk,
not a language bug, and is excluded below.)

---

## Core-semantics bugs (wrong results)

### 1. Case-insensitive comparison operators broken (`==?`, `=~?`, `<?`, `!=?`, …) — ✅ FIXED
The ignore-case comparison builtin ids (`base+512` = 3532+) collided with the
`getchar`/`getcmd*` function ids added at 3532+, so `==?` dispatched to a
function instead of comparing. Remapped the ic offset to the reserved gap
3030..=3039 (`VIML_CMP_IC_OFFSET`), bumped the script-cache format version.
Covered by `examples/compare.vim`.
- `'abc' ==? 'ABC'` → Vim `1`, vimlrs `0`
- `'foo' =~? 'FOO'` → Vim `1`, vimlrs `0`
- `'abc' <? 'ABD'` → Vim `1`, vimlrs *(no output at all)*
- `'x' !=? 'X'` → Vim `0`, vimlrs *(no output at all)*
- The whole `?`-suffixed (force-ignorecase) operator family returns the wrong
  boolean or silently produces nothing. Common in real scripts. (`>?` happened to match.)

### 2. `substitute()` with `\=` expression using `.` concat loses the result — ✅ FIXED
Root cause was the parser: `submatch(0).submatch(0)` parsed `.submatch` as a dict
member read instead of `.` concatenation. `at_member_dot` now treats `.name(` as
concatenation with a function call (legacy has no direct `dict.key(args)` call —
that is vim9), so all `f(x).g(y)` chaining concatenates. Covered by
`examples/concat_dot.vim`.
- `substitute('abc','.','\=submatch(0).submatch(0)','g')` → Vim `aabbcc`, vimlrs `` (empty)
- `substitute('abc','.','\=submatch(0).submatch(0)','')` → Vim `aabc`, vimlrs `bc`
- The `\=` expression evaluates to empty specifically when it uses the `.`
  concatenation operator. `\=toupper(submatch(0))` and `\=submatch(0)*2` both work.

### 3. `split()` with zero-width pattern `\zs` doesn't split — ✅ FIXED
`regex_split` rewritten to the faithful f_split `col` algorithm (any empty-capable
separator advances one char), and `\zs`/`\ze` implemented in the regex engine
(reserved match-bound slots), which also fixes `matchstr`/`substitute`. Covered by
`examples/regex_zs.vim`.
- `split('hello','\zs')` → Vim `['h', 'e', 'l', 'l', 'o']`, vimlrs `['hello']`
- Zero-width-match splitting (the standard "split into chars" idiom) isn't handled.
  `src/ported/strings.rs` split impl.

### 4. `strpart()` with negative start doesn't shorten the length — ✅ FIXED
Ported the C offset-folding (`len += nbyte; nbyte = 0`). Covered by `examples/strings.vim`.
- `strpart('hello',-2,3)` → Vim `h`, vimlrs `hel`
- Vim clamps `start` to 0 **and** folds the negative offset into len
  (`len += off; off = 0`). vimlrs only clamps start, keeping full len.
  `src/ported/strings.rs:89` clamps `start < 0` but never subtracts the offset from len.

### 5. `get()` on a String returns a value instead of erroring — ✅ FIXED
Now errors E1531 for a String/non-container, and the Blob form is ported too.
Covered by `examples/index_get.vim`.
- `get('hello',1)` → Vim errors `E1531: Argument of get() must be a List, Tuple, Dictionary or Blob`; vimlrs returns `0`
- vimlrs wrongly accepts a String first arg.

### 6. `index()` ignores the `{ic}` (ignore-case) argument — ✅ FIXED
- `index(['A','b'],'a',0,1)` → Vim `0`, vimlrs `-1`
- The 4th-arg case-insensitive flag is not honored.
- Fixed in `tv_equal` (now case-folds strings when `ic`) and `f_index` (honours
  `{start}`/`{ic}`, plus the Blob form). Covered by `examples/index_get.vim`.

---

## Float formatting — systemic `string()` divergence

### 7. `string()` of a Float diverges in exponent format, precision, and exp threshold — ⊘ WONTFIX (matches Neovim)
vimlrs targets **Neovim** (the vendored `vendor/` is the spec), and Neovim renders
a Float with plain C `printf("%g")` (`encode.c:369`, `typval.c:4591`) — 6
significant digits, C-style `e+NN` exponent, `.0` appended when integral.
vimlrs's `vim_float_g` already reproduces that exactly, so its output **matches
Neovim** (`string(1.0e10)` → `1e+10`, `string(123456789.0)` → `1.23457e+08`).
The values quoted here are Vim 9.x's distinct float printer; not a vimlrs/Neovim
bug. (Same applies to R2-5.) EXCEPTION — the negative-zero case WAS a real bug
vs Neovim: `%g` keeps the sign of IEEE -0.0, but vim_float_g's `f == 0.0`
early-return dropped it. ✅ FIXED: `string(-0.0)` → `-0.0`.
- `string(1.0e10)` → Vim `1.0e10`, vimlrs `1e+10`
- `string(123456789.0)` → Vim `1.234568e8`, vimlrs `1.23457e+08`
- `string(0.0001)` → Vim `1.0e-4`, vimlrs `0.0001`
- `string(1.23456789012345)` → Vim `1.234568`, vimlrs `1.23457`
- `string(-0.0)` → Vim `-0.0`, vimlrs `0.0`
- Four issues at once: (1) exponent rendered C-style `e+08`/`e+10` vs Vim's
  `e8`/`e10` (no `+`, no zero-pad, mantissa keeps `.0`); (2) default precision too
  low (6 sig digits vs Vim's ~7); (3) different exponential-vs-fixed switch threshold
  (Vim uses exp form for `0.0001`); (4) negative-zero sign dropped.
  `vim_float_g()` in `src/ported/eval/encode.rs:21`. Plain cases (`string(1.0)`,
  `string(0.1+0.2)`→`0.3`, `string(1000000.0)`) match.

---

## String indexing

### 8. String index/slice is char-based; Vim is byte-based — ✅ FIXED (round 17)
- `'héllo'[1]` → Vim `<c3>` (first byte of the 2-byte `é`), vimlrs `é` (whole char)
- Vim indexes strings by byte. ASCII matches (`'hello'[1]` → both `e`); only multibyte diverges.
- Fixed in round 17: `[i]` is one byte, `[a:b]` is an inclusive byte range
  (`eval_index_inner`, eval.c) in both the bridge and the ported eval; a byte
  slice that splits a character carries U+FFFD where Vim carries the raw byte
  (both render identically). `slice()` stays character-indexed with composing
  clusters, per its own C path (`string_slice`).

---

## Error-output / edge

### 9. Spurious fallback value printed after a runtime error — ✅ FIXED
A `VIML_ERR_MARK` op snapshots `did_emsg` before `:echo`/`:echon` evaluate their
args; the echo prints nothing if it rose (the command aborted on error). The
`-e` path suppresses its result the same way. So `echo [1,2,3][10]` prints only
E684 and `echo printf('%d',3.7)` only E805 — no trailing fallback. Covered by
`examples/error_output.vim`.
- `echo printf('%d',3.7)` → Vim prints only `E805: Using a Float as a Number`; vimlrs prints the error **and then** `-1`
- `echo [1,2,3][10]` → Vim prints only `E684: List index out of range: 10`; vimlrs prints the error **and then** `v:null`
- On error vimlrs still emits a fallback result value, so erroring expressions produce
  extra output Vim never produces.

### 10. Float literals without a dot are accepted (lexer too lenient) — ✅ FIXED
The lexer now only consumes an `[eE]` exponent after a `.{digits}` fraction
(Neovim's grammar `[0-9]+\.[0-9]+([eE]...)?`), so `1e100` is the Number `1`
followed by a name (a parse error), while `1.0e100` stays a Float. Covered by
`examples/float_literals.vim`.
- `string(1e100)` → Vim errors `E15: Invalid expression` (Vim requires `1.0e100`); vimlrs returns `1e+100`
- Vim's float-literal grammar requires `{digits}.{digits}[e…]`.

### 11. Dict key iteration order differs — low severity / caveat
- `keys({'zebra':1,'apple':2,'mango':3})` → Vim `['apple', 'zebra', 'mango']`, vimlrs `['zebra', 'apple', 'mango']`; same for `values()`/`string({...})`
- Vim iterates in internal hash order (documented as **arbitrary**); vimlrs uses
  insertion order. Vim's order is officially unspecified, so portable scripts must not
  rely on it — flagged for completeness only.

---

## Coverage — verified at parity (no bug)

Integer arithmetic incl. negative `/` truncation and `%` sign; integer division; list
& string slicing (negative indices, out-of-range, reversed); `sort()` default
string-sort vs `sort(l,'n')`, numeric-string sort, custom comparator; `uniq`/`reverse`/
`join`; `split()` basic + keepempty + regex pattern; `printf` specifiers
`%d %s %x %X %o %b %f %e %g %c %% %+d` with width/precision/`-`/`0` flags, and `%d` on
bad string args; `matchstr`/`matchlist`/`match`/`matchend`; `repeat` (string & list);
`len`/`strlen`/`strchars`/`strdisplaywidth`; `type()`/`empty()`/`get()` defaults;
`==`/`==#`/string-number coercion (`'3abc'+4`, `0x1f`, `0b101`, `017`/`0o17`);
`is`/`isnot`; `&&`/`||`/`!` truthiness & return values; ternary; `map`/`filter` with
`v:val`/`v:key`, lambdas, closures; `call`/`function`; `abs`/`float2nr`/`ceil`/`floor`/
`round`/`trunc`/`pow`/`sqrt`/`fmod`/`max`/`min`; `range` (all forms); `str2nr`(bases)/
`str2float`; `nr2char`/`char2nr`; `tolower`/`toupper`/`trim`; `count`/`add`/`insert`/
`remove`/`extend`/`copy`/`deepcopy`; `has_key`/`items`; `and`/`or`/`xor`/`invert`;
`=~`/`!~`/`=~#`; `stridx`/`strridx`; `string()` of nested list/dict; `"\t"`/`"\n"` vs
`'\t'` escapes; `1/0` and float `inf`/`-inf`/`nan`.

---

# Round 2 — additional confirmed divergences (vs Vim 9.2)

Found in a second, deeper pass; all reproduced against the current binary. (These
supersede the earlier "`%g` … verified at parity" note in the coverage list —
`%g`/`%G` are **not** at parity; see #R2-5.)

### R2-1. `charidx()` PANICS (crashes the interpreter) on multibyte input — ✅ FIXED
Now walks char boundaries (maps a byte to the char that contains it); never
slices mid-character. Covered by `examples/numeric_edge.vim`.
- `charidx("héllo",2)` → Vim `1`, vimlrs **panics** (`thread 'main' panicked at
  src/ported/strings.rs:255: end byte index 2 is not a char boundary; it is inside 'é'`,
  process aborts with exit 101)
- The byte-index arg slices a UTF-8 `&str` directly (`s[..idx]`) without a
  char-boundary check. Any multibyte string crashes. **Highest severity.**

### R2-2. Very-magic mode `\v` is entirely unsupported — ✅ FIXED
A `preprocess_magic` pass rewrites a `\v` segment into the equivalent default-magic
pattern (operators `( ) | + ? = { } < >` lose their backslash; a backslash makes
them literal; classes copied verbatim), so the magic parser handles it unchanged.
`\m` switches back. Exotic `\v` atoms (`@`, `&`, `%[`) are not yet modelled.
Covered by `examples/regex_verymagic.vim`.
- `matchstr("abc123","\v\d+")` → Vim `123`, vimlrs `` (empty)
- `matchstr("color","\vcolou?r")` → Vim `color`, vimlrs `` (empty)
- The magic-mode equivalents (`\d\+`, `colou\?r`) work, so the `\v` prefix itself is
  unhandled. Common in real scripts.

### R2-3. Backreferences (`\1`, `\2`…) in patterns don't match — ✅ FIXED
Added `Node::BackRef` to the regex engine: `\1`..`\9` match the text the
corresponding group captured (unset group → empty). Covered by
`examples/regex_backref.vim`.
- `matchstr("hello","\(l\)\1")` → Vim `ll`, vimlrs `` (empty)
- `substitute("hello","\(l\)\1","X","")` → Vim `heXo`, vimlrs `hello`
- Capture-group backreferences in the search pattern are not honored.

### R2-4. `\%[...]` optional-sequence atom unsupported — ✅ FIXED
Added `Node::OptSeq`: `\%[atoms]` matches a greedy in-order prefix of its atoms
(each optional), e.g. `r\%[ead]` → r/re/rea/read. Covered by
`examples/regex_optseq.vim`.
- `matchstr("function","f\%[unc]")` → Vim `func`, vimlrs `` (empty)

### R2-5. `printf("%g"/"%G", …)` formatting diverges — ⊘ WONTFIX (matches Neovim)
Like #7: vimlrs's `%g` follows C/Neovim, not Vim 9.x's float printer. `printf`
on Neovim routes floats through the platform `%g`, which is what vimlrs emits.
Not a vimlrs/Neovim bug.
- `printf("%g",1.0)` → Vim `1.0`, vimlrs `1`
- `printf("%g",1000000.0)` → Vim `1000000.0`, vimlrs `1e+06`
- `printf("%g",0.0001)` → Vim `1.0e-4`, vimlrs `0.0001`
- `printf("%G",1000000.0)` → Vim `1000000.0`, vimlrs `1E+06`
- vimlrs emits raw C `%g` (drops `.0`, C-style `e+06`, different precision/threshold);
  Vim post-processes like its float printer. (`%f`/`%e` are fine.)

### R2-6. `printf` `%S` and `*`-width-from-arg unsupported (passed through literally) — ✅ FIXED
`%S` now renders a string (like `%s`); `%*`/`%.*` take width/precision from the
next argument (negative width left-justifies). Covered by `examples/printf_exists.vim`.
- `printf("%S","abc")` → Vim `abc`, vimlrs `%S`
- `printf("%*d",5,3)` → Vim `    3`, vimlrs `%*d`

### R2-7. A funcref value can't be called directly with `(...)` — ✅ FIXED
Added an `Expr::CallExpr` AST node (an abutting `(` after a postfix value) and a
`VIML_CALL_FUNCREF` op that calls the funcref value. Works for `function('x')(a)`,
lambda literals `{x->x}(a)`, and indexed funcrefs `fns[0](a)`. Covered by
`examples/funcref_call.vim`.
- `function("toupper")("hi")` → Vim `HI`, vimlrs `E15: Invalid expression: trailing tokens`
- `call()` works; direct call syntax on a funcref expression does not.

### R2-8. `%` on Floats should error (E804) but returns a value — ✅ FIXED
`b_mod` now raises E804 for a Float operand (`%` is integer-only). Covered by
`examples/numeric_edge.vim`.
- `1.0 % 2.0` → Vim `E804: Cannot use '%' with Float`, vimlrs `1.0`

### R2-9. `execute()` puts the newline at the wrong end — ✅ FIXED
Inside execute() (tracked by an EXECUTE_DEPTH counter) `:echo` now prefixes its
output with a newline instead of appending one, so `string(execute("echo 5"))`
== `"\n5"`. Stdout / general captures keep the trailing newline. Covered by
`examples/execute_capture.vim`.
- `string(execute("echo 5"))` → Vim `'\n5'` (leading), vimlrs `'5\n'` (trailing)

### R2-10. `str2float()` doesn't parse hex — ✅ FIXED
`string2float` now parses hex floats (`0x1f`→31.0, `0x1.8p1`→3.0), matching strtod.
Covered by `examples/numeric_edge.vim`.
- `str2float("0x1f")` → Vim `31.0`, vimlrs `0.0`

### R2-11. `exists("*funcname")` returns 0 for existing builtins — ✅ FIXED
`exists('*name')` now reports builtins and user functions via a FUNC_EXISTS_HOOK
the bridge installs. Covered by `examples/printf_exists.vim`.
- `exists("*substitute")` → Vim `1`, vimlrs `0`. The `*` (callable-exists) form is
  unimplemented; reports every function as absent.

### R2-12. `string(v:none)` returns `v:null` — ✅ FIXED
Added a distinct `kSpecialVarNone` (lexer `v:none`, encode → `v:none`). It
survives the VM `Value` round-trip by being stashed in the REFPOOL (the shared
`Value::Undef` is reserved for `v:null`). Covered by `examples/special_none.vim`.
- `string(v:none)` → Vim `v:none`, vimlrs `v:null` (`v:none`/`v:null` conflated;
  `string(v:null)` alone is correct).

### R2-13. `has("vim9script")` returns 0 — minor / feature gap
- `has("vim9script")` → Vim `1`, vimlrs `0` (likely intentional if vim9script isn't
  implemented; flagged for completeness).

Areas probed in round 2 that PASSED: `reduce`/`flatten`/`flattennew`/`extendnew`/
`mapnew`/`slice`, `sort` with `"i"`/`1`/funcref, `add`/`insert`/`remove(dict)` returns,
`v:true`/`v:false`/`v:null` printing+arithmetic+compare, magic-mode quantifiers
`\+ \? \{n,m}`, `tr`/`escape`/`shellescape`/`fnameescape`/`strgetchar`/`strcharpart`/
`byteidx`/`matchstrpos`, `json_encode`/`json_decode`, substitute case escapes
(`\U \L \u \l \E`), `printf %b %c %x(neg) %05.2f %e`, `get()` dict default.

---

# Round 3 — additional confirmed divergences (vs Vim 9.2)

Third deep pass against the current binary. Reproduced by sourcing the *same* `.vim`
probe through both interpreters (regex patterns must be single-quoted Vim strings).
Regex engine is `src/viml_regex.rs` (hand-written subset; atom table ~344-354).

### R3-1. Regex lookaround unsupported (`\@=`, `\@!`, `\@<=`, `\@<!`)
- `matchstr('foobar','foo\(bar\)\@=')` → Vim `foo`, vimlrs `` (empty)
- `matchstr('foobaz','foo\(bar\)\@!')` → Vim `foo`, vimlrs ``
- `matchstr('foobarbaz','\(foo\)\@<=bar')` → Vim `bar`, vimlrs ``
- `matchstr('xxbar','\(foo\)\@<!bar')` → Vim `bar`, vimlrs ``
- None of the four lookahead/lookbehind atoms are implemented.

### R3-2. POSIX bracket classes `[[:...:]]` entirely unsupported
- `matchstr('abc123','[[:digit:]]\+')` → Vim `123`, vimlrs ``
- `matchstr('abc123','[[:alpha:]]\+')` → Vim `abc`, vimlrs ``
- Also broken: `[[:alnum:]]`/`[[:upper:]]`/`[[:lower:]]`/`[[:xdigit:]]`/`[[:punct:]]`/
  `[[:space:]]`. The `[: :]` syntax inside a bracket expression isn't parsed.

### R3-3. `substitute()` with `\zs` replaces the wrong (un-narrowed) region
- `substitute('foobar','o\zsb','X','')` → Vim `fooXar`, vimlrs `foXar`
- `matchstr` honors `\zs`, but `substitute()` still deletes from the full match start,
  eating the `o` too. Silent wrong result.

### R3-4. `printf('%s', …)` of a List/Dict/Blob errors instead of stringifying
- `printf('%s',[1,2])` → Vim `[1, 2]`, vimlrs `E730: Using a List/Dict/Funcref/Blob as a String`
- `printf('%s',{'a':1})` → Vim `{'a': 1}`, vimlrs `E730…`; `printf('%s',0z1234)` → `0z1234` vs `E730…`
- Vim's `%s` formats composites via `string()`; vimlrs rejects them.

### R3-5. `:const` declaration unsupported (parse error)
- `const C = 5` → Vim defines `C`=5; vimlrs `E15: Invalid expression: trailing tokens`
- No `"const"` handler in `src/`; any script using `:const` fails to parse (and the
  re-assignment lock, Vim `E741`, is absent). Common in modern scripts.

### R3-6. `:echoerr` command unsupported (parse error)
- `echoerr 'boom'` → Vim raises catchable `Vim(echoerr):boom`; vimlrs `E15: Invalid expression`
- Breaks the standard error-reporting idiom and `try/catch` around it.

### R3-7. Defining a function into a Dict key (`function d.key()`) unsupported
- `function d.greet() dict … endfunction` → Vim makes `d.greet` a funcref; vimlrs
  `E716: Key not present in Dictionary: greet` (member never created).

### R3-8. Calling a funcref stored in a Dict member fails to parse
- `d.greet()` / `d['greet']()` (member = funcref) → Vim `hi X`; vimlrs `E15: Invalid
  expression: unexpected RParen`. Calling the result of a dict/index expression with
  `(...)` isn't parsed. (Distinct from R2-7; the common OOP idiom.)

### R3-9. Duplicate key in a `{}` Dict literal silently accepted
- `{'a':1,'a':2}` → Vim errors `E721: Duplicate key in Dictionary: "a"`; vimlrs `{'a': 2}` (no error)

### R3-10. `\&` concat/AND branch unsupported
- `matchstr('foobar','foo\&...')` → Vim `foo`, vimlrs `` (the all-branches-must-match operator)

### R3-11. Codepoint atoms `\%d` / `\%u` / `\%x` unsupported
- `matchstr('A','\%d65')` → Vim `A`, vimlrs ``; `matchstr('AB','\%u0041')` → `A` vs ``

### R3-12. Char-class atoms `\k`, `\f`, `\p` unsupported
- `matchstr('hello_world','\k\+')` → Vim `hello_world`, vimlrs `` (keyword)
- `matchstr('foo/bar','\f\+')` → Vim `foo/bar`, vimlrs `f` (treats `\f` as literal); `\p` (printable) → ``
- Atom table lists only `\d \w \s \a \l \u \x`.

### R3-13. `printf('%c', n)` for n > 255 should truncate to a byte
- `printf('%c',321)` → Vim `A` (321 & 0xFF = 65), vimlrs `Ł` (full codepoint)
- `printf('%c',0x263A)` → Vim `:` (low byte 0x3A), vimlrs `☺`

### R3-14. `printf('%f'/'%e', NaN)` uses wrong case
- `printf('%f',0.0/0.0)` → Vim `nan`, vimlrs `NaN` (same for `%e`). `%g` and `string()` are already correct.

### R3-15. `matchfuzzypos()` returns different scores — low severity
- `matchfuzzypos(['hello','help'],'hl')` → Vim scores `[885,880]`, vimlrs `[113,112]`.
  Ordering and positions agree; only the numeric weights differ. (`matchfuzzy` ordering matches.)

Areas probed in round 3 that PASSED: `\{-}`/`\{-n,m}`/`\{n}`/`\{n,m}` quantifiers, `\zs`/
`\ze` in `matchstr`, `\c`/`\C`, `\a\l\u\s\w\d` atoms, `[a-c]`/`[^a-c]`, backref-in-
replacement `\2\1`; `trim(mask,dir)`/`strcharlen`/`strwidth`/`reverse`(string)/`slice`/
`strcharpart`/`byteidxcomp`/`list2str`/`str2list`/`strtrans`; `str2nr`(bases)/`str2float`
(`1.5e3`/inf/nan)/`printf('%d',"0x10")`; integer overflow wrap, `float2nr(inf/nan)`/
`pow(0,0)`/`fmod`/`round`(half-away, negatives); `0=='0'`/`'abc'==0`; `sort('n'/'f'/'N')`
mixed, `uniq`, `flatten(l,depth)`, `extend` keep/force/error, `count(ic,start)`,
`index(neg)`, `insert(neg)`, `#{}` literal, `matchfuzzy`; `:let +=` append, `:for [k,v] in
items()`, `:let [a,b;rest]`, `:try/:catch/:finally`+`:throw`+`v:exception`, `:unlet`,
lambda-call `{->42}()`, partial bound args + `string()` of partial, `eval()`, `type(funcref)`,
`printf('%s',funcref)`, substitute `\r`/`\n`.

---

# Round 4 — additional confirmed divergences (vs Vim 9.2)

Fourth pass against the current binary, reproduced by sourcing the same `.vim` probe
through both interpreters. No overlap with rounds 1–3.

## High severity

### R4-1. Unspaced `.` concatenation is mis-parsed as dict member access
- `let a="foo" | let b="bar" | echo a.b` → Vim `foobar`, vimlrs `f`
- `map(['a','b'],{i,v->'x'.v})` → Vim `['xa','xb']`, vimlrs `['x','x']`
- `reduce(['a','b','c'],{a,b->a.b},'')` → Vim `'abc'`, vimlrs `''`
- The parser's `at_member_dot()` (`src/viml_parser.rs:979-1010`) treats a `.` abutting an
  identifier (no surrounding space) as `dict.key`. In legacy Vim script `.` is overloaded and
  resolved by runtime type, so `a.b` on non-dicts is **concatenation**. Spaced `a . b`, `a..b`,
  `'a'.'b'` (literal RHS), and `a.func()` (call) all work. **This is the root cause behind
  round-1 #2** (substitute `\=` with `.`). Very common idiom (`s:prefix.name`). Highest impact.

### R4-2. Numbered variadic-arg access `a:1`, `a:2`, … doesn't work
- `func! F(...) | return [a:1, a:2] | endfunc` then `F(10,20)` → Vim `[10, 20]`, vimlrs
  `E121: Undefined variable: a:1`
- `a:0` (count) and `a:000` (list) are correct; only by-number positional access is broken
  (also with a named+vararg signature).

## Medium severity

### R4-3. `#{…}` literal: single-char bareword key with no space after `:` fails to parse
- `#{a:1}` → Vim `{'a': 1}`, vimlrs `E15: expected Colon, found RBrace`
- The lexer swallows `a:`/`x:`/`g:` as a scope sigil, so the dict parser then expects another
  colon. Multi-char keys (`#{one:1}`) and a space after the colon (`#{a: 1}`) work — which is
  why round 3's "`#{}` PASSED" missed it. `#{a:1}` is a common spelling.

### R4-4. `strpart()` 4-arg charwise mode counts `len` in bytes, not characters
- `strpart('héllo',1,3,1)` → Vim `éll`, vimlrs `él`
- With `{chars}`=1, `start` is a char index correctly but `len` is still applied as a byte
  count. (3-arg byte mode is fine.)

### R4-5. `lockvar` / `unlockvar` commands unsupported (parse error)
- `let x=1` then `lockvar x` → Vim locks `x` (later write → `E741`); vimlrs `E15: Invalid
  expression: trailing tokens`. No command handler; lock semantics absent.

### R4-6. `typename()` builtin missing
- `typename([1,2])` → Vim `list<number>`, vimlrs `E117: Unknown function: typename`

### R4-7. `js_encode()` / `js_decode()` builtins missing
- `js_encode(v:null)` → Vim `null`, vimlrs `E117`; `js_decode('{a:1}')` → Vim `{'a': 1}`, vimlrs
  `E117`. The whole `js_*` pair is absent (`json_encode`/`json_decode` are at parity).

## Low severity

### R4-8. `float2nr()` negative overflow clamps one short of Vim
- `float2nr(-1.0e20)` → Vim `-9223372036854775807` (−(2^63−1)), vimlrs `-9223372036854775808`
  (i64::MIN). Positive overflow matches; only the negative side is off by one.

### R4-9. `islocked()` on a nonexistent variable returns 0 instead of -1
- `islocked('nope')` → Vim `-1`, vimlrs `0`. Vim distinguishes "no such variable" (`-1`) from
  "exists, unlocked" (`0`).

### R4-10. `:for`-loop closures capture a per-iteration value; Vim shares one loop variable
- `for i in range(3) | call add(fns,{->i}) | endfor` then calling each → Vim `[-1, -1, -1]`
  (all share the one loop var, left `-1` after the loop), vimlrs `[0, 1, 2]`. vimlrs is
  arguably "more correct," but it diverges from Vim's (quirky) ground truth.

Areas probed in round 4 that PASSED: `abs`/`round`/`ceil`/`floor`/`trunc` of negatives, `fmod`
sign, `log`/`log10`/`sqrt`/`pow` domains, `and`/`or`/`xor`/`invert` with negatives & >i32,
`min([])`/`max([])`→0, `remove(l,1,2)` range, `get([],5,'d')`, `extendnew`/`deepcopy`/
`insert(neg)`/`sort`(default+`'N'`)/`uniq`/`flattennew`, `reduce` over List/Blob/String **with
spaced/`..` dot**, `nr2char(…,1)`/`char2nr(…,1)`/`strgetchar`/`strchars(skipcc)`/`strcharpart`,
`escape`/`tr`(ranges)/`split('\d')`/`join('')`/`repeat([..])`, `eval(string(…))` round-trip,
`:while`/`:break`/`:continue`, nested `:try`/`:finally` rethrow, `execute "let …"`, script-local
`s:` vars across calls.

---

# Round 5 — found by the differential fuzzer (`fuzz-parity`)

Rounds 1–4 were hand-probed. Round 5 is machine-found: `cargo run --bin fuzz-parity`
generates random VimL expressions, runs each through vimlrs **and** `nvim` **and**
`vim`, and reports a bug only when **both** engines agree and vimlrs differs (see
`docs/FUZZING.md`). A first run of 1500 expressions produced 3 crashes and 248
divergences (155 distinct); the fixes below took that to **0 crashes and 8
divergences**, all of them the two known-divergence classes below.

Every fix is pinned by an oracle-recorded case in `tests/data/fuzz_corpus.txt`,
replayed by `tests/fuzz_corpus.rs` with no editor installed.

## Crashes (vimlrs panicked; Vim does not)

### R5-1. `filter()` on a Blob that removes bytes panicked — ✅ FIXED
`filter(0z0011, {_,v -> 0})` → index-out-of-bounds panic. `filter_map_blob` hoisted
the blob's length out of the loop and indexed the *shrinking* blob with the
un-rewound index. The C (`list.c`) re-reads `b->bv_ga.ga_len` every iteration and
does `i--` on removal so the next `i++` re-examines the shifted-down byte.

### R5-2. `stridx()` with a start index inside a multibyte char panicked — ✅ FIXED
`stridx('日本語', 'x', 1)` → "byte index 1 is not a char boundary". The C advances a
byte pointer and calls `strstr`; the port sliced a Rust `str`. Now searches bytes.

### R5-3. `str2float()` on short multibyte text panicked — ✅ FIXED
`str2float('日本語')` → "byte index 4 is not a char boundary": the `inf`/`nan` prefix
test sliced `text[..4]`. Now compares bytes.

### R5-4. `strpart()` with an INT64_MIN start panicked — ✅ FIXED
`strpart('abc', -9223372036854775808)` → "attempt to subtract with overflow". The C
does this arithmetic in `varnumber_T` and *relies on the two's-complement wrap*
(the two wraps cancel), so it yields `'abc'`. Ported with explicit wrapping ops.
(Vim and Neovim disagree on this expression, so it is not in the corpus gate:
Neovim gives `'abc'`, Vim `'bc'`. vimlrs follows Neovim, its port target.)

## Wrong results

### R5-5. Indexing/slicing a Number was E909 — ✅ FIXED
`strlen('ab')[0]` → Vim `'2'`, vimlrs E909. `eval_index_inner` (c:3263) runs
VAR_NUMBER through the **same branch as VAR_STRING**: the number is rendered with
`tv_get_string` and then indexed as that text. Also: a Float subscript is E806, a
Funcref E695, a Bool/Special E909, and a Dict *slice* is E719 — the port emitted a
blanket E909 for all of them.

### R5-6. A negative string subscript wrapped from the end — ✅ FIXED
`'hello'[-1]` → Vim `''`, vimlrs `'o'`. c:3296: "If the index is too big or negative
the result is empty." Only a *slice* bound counts from the end. `examples/string_index.vim`
had asserted the wrong (vimlrs) behavior and was corrected.

### R5-7. Float → String used Rust's `Display` — ✅ FIXED
`round(0.5) .. 'x'` → Vim `'1.0x'`, vimlrs `'1x'`; `1.0e-10` came out as
`0.0000000001`. Vim's `vim_snprintf("%g")` is not C's `%g` — it keeps the `.0` and
writes `1.0e-10`. `vim_float_g` (already used by `string()`/`printf`) is that
formatter; `tv_get_string_buf_chk` now uses it.

### R5-8. Dict/Blob in string context reported E730 — ✅ FIXED
`'x' . {'a':1}` → Vim E731, `'x' . 0zFF` → Vim E976; vimlrs said E730 (the *List*
error) for all three. The C indexes a per-type `str_errors[]` table (c:4135).

### R5-9. Float operands rejected Bool/Special, and reported the wrong code — ✅ FIXED
`1.5 - v:false` → Vim `1.5`, vimlrs E808. Arithmetic coerces the non-Float operand
with `tv_get_number_chk` and *then* promotes (c:2323) — it never calls
`tv_get_float`, which is why a Bool is a Number there. Relatedly `tv_get_float`
emitted a blanket E808 where the C has a per-type table: E891 Funcref, E892 String,
E893 List, E894 Dict, E362 Bool, E907 Special, E975 Blob.

### R5-10. `!` on a Float was E805 — ✅ FIXED
`!(0.5)` → Vim `0`, vimlrs E805. `eval7_leader` (c:2818) tests the float against
`0.0` and yields a Number; it does not run the Float through `tv_get_number`.

### R5-11. `%` reported E804 before checking its operands — ✅ FIXED
`0z61 % 2.5` → Vim E974 (Blob as Number), vimlrs E804. The C coerces both operands
left-to-right *before* the float check fires (c:2464), so operand order is
observable.

### R5-12. Over-large integer literals became `0` — ✅ FIXED
`9223372036854775808` → Vim `9223372036854775807` (saturates at VARNUMBER_MAX),
vimlrs `0`. Also hex/binary. This silently turned an out-of-range index into a valid
one: `insert([1], 9, -9223372036854775808)` inserted at 0 instead of raising E684.

### R5-13. `"\<Esc>"` and every other key escape was left literal — ✅ FIXED
`char2nr("\<Esc>")` → Vim `27`, vimlrs `60` (`<`): the `\<Key>` escape was never
translated, so `"\<Esc>"` was five characters. `src/ported/keycodes.rs` now ports
`trans_special`/`find_special_key` for every key that *is* a character (`<Esc>`,
`<Tab>`, `<CR>`, `<NL>`, `<Space>`, `<lt>`, `<Bar>`, `<Bslash>`, `<C-x>`, `<S-x>`,
`<Char-N>`), and `keytrans()` (previously a pass-through stub) ports the inverse,
`get_special_key_name`.

### R5-14. Missing argument validation — ✅ FIXED
- `printf('%.2f')` → E766 (insufficient args); `printf('%s', [], 'abc')` → E767 (too many). Neither was checked.
- `range(10, 5, 1)` → E727 (start past end); `range(2, 5, 0)` → E726 (stride is zero). Both returned `[]`.
- `str2nr('a', 15)` → E474: the base check existed but its `emsg` had been dropped, so it returned 0.
- `trim('ab', 'a', 3)` → E475: the direction was never validated.
- `len(0.0)` → E701: the C lists VAR_FLOAT with the *error* cases, not with VAR_NUMBER.
- `matchbufline(99, …)` → E158: a nonexistent buffer returned `[]`.

### R5-15. Regex codepoint atoms `\%d` / `\%o` / `\%x` / `\%u` / `\%U` — ✅ FIXED
`matchstr('abc', '\%d97')` → Vim `'a'`, vimlrs `''`. (This was R3-11, still open.)

### R5-17. Dict iteration order was insertion order, not Vim's — ✅ FIXED
`string({'x':1,'b':2,'q':3,'a':4})` → Vim (and Neovim, identically)
`{'q': 3, 'b': 2, 'a': 4, 'x': 1}`; vimlrs printed insertion order. Dict order is
observable in `string()`, `keys()`, `values()`, `items()` and `:for`, and Vim's is
neither sorted nor insertion — it is the bucket layout of `hashtab.c`. `indexmap`
could never reproduce it, so `hashtab.c` is now ported (`src/ported/hashtab.rs`:
the `hash * 101 + byte` hash, the 16-slot initial array, the
`idx = 5*idx + perturb + 1` probe, tombstones, and the grow-at-2/3-full policy) and
`dict_T::dv_hashtab` is a real `hashtab_T`. Order now matches byte-for-byte,
including after removals and across a grow-and-rehash.

The Rust map API the port's ~108 call sites use (`contains_key`, `iter_mut`, …) has
no C counterpart, so it lives in the synthesis zone (`src/hashtab_map.rs`) rather
than being allowlisted as a fake ported name.

### R5-16. `printf()` float conversions reported the per-type float error — ✅ FIXED
`printf('%f', 'abc')` → Vim E807 ("Expected Float argument for printf()"), vimlrs
E892. The C's `tvs_get_float` raises one error for *any* non-numeric argument to
`%f`/`%e`/`%g`; the integer conversions do keep `tv_get_number`'s per-type errors
(`printf('%d', [1])` is E745 in both).

## Still open (found in round 5, not yet fixed)

### R5-O2. `eval()` rejects trailing text before evaluating
`eval("nl\nhere")` → Vim E121 (it evaluates `nl`, an undefined variable), vimlrs E15
(the parser rejects the trailing tokens up front). Vim's `f_eval` parses ONE
expression, evaluates it, and only then reports E488 for what is left over. Same
root cause as R5-D3 below; fixing it needs a parser entry point that returns the
leading expression plus the unconsumed rest.

## Known divergences (NOT bugs to "fix" — recorded so the fuzzer's report stays readable)

### R5-D1. Strings are indexed by character, Vim indexes by byte
`'日本語'[0]` → Vim `'<e6>'` (one raw byte), vimlrs `'日'`. Vim strings are byte
arrays; vimlrs stores them as Rust `String` (UTF-8 text), which cannot hold a lone
`0xE6`. Fixing this means changing the string representation to `Vec<u8>` — a
deliberate, separate decision, not a bug fix. Everything else about indexing (empty
on out-of-range, no negative wrap, inclusive slices) now matches exactly.

### R5-D3. Errors surface in a different order when two operands both fail
`extend([[1,2]], [1], -1) .. strspn()` → Vim E730, vimlrs E117. Vim is a
string-walking interpreter and type-checks the left operand of `.` *before* it
parses the right one (c:2414); vimlrs parses and compiles the whole program first,
so a parse error in a later subexpression wins. Same root cause makes vimlrs report
E15 for `1e0` (an invalid literal in Vim too) where Vim reports the runtime error of
an earlier subexpression it evaluated first. The *set* of errors is the same; which
one is reported first is not.

### R5-D4. `<M-a>`/`<A-a>`, `<Up>`, `<F1>`, `<BS>`, `<Del>`, `<C-@>` key escapes stay literal
These have no character form — Vim encodes them as `K_SPECIAL` (0x80) byte sequences
that are not valid UTF-8. Vim and Neovim do not even agree on the meta forms
(`"\<M-a>"` is one byte `0xE1` in Vim, a four-byte sequence in Neovim). See
`src/ported/keycodes.rs`.

Areas probed in round 5 that PASSED (a sample of the 1151/1200 agreeing cases):
`substitute` with `\=`/`\u`/`\U`/backrefs, `split`/`join`/`trim`/`escape`/`shellescape`,
`printf` width/precision/`*`/positional/`%b`/`%x`/inf/nan, `sort`/`uniq`/`map`/`filter`/
`reduce`/`indexof` with lambdas, `matchstrpos`/`matchlist`/`matchend`, blob slicing and
`blob2list`/`list2blob`, `json_encode`/`json_decode`, float math domains and inf/nan,
`and`/`or`/`xor`/`invert`, comparison operators in all three case forms (`==`, `==#`, `==?`).


---

# Round 6 — the fuzzer widened (funcrefs, `\`-escapes, 15 more builtins)

Round 5 fuzzed a fixed set of ~110 pure builtins over operator trees. Round 6 gave
the generator three surfaces it could not reach before — **funcref values**
(`function('strlen')`, partials), **double-quoted escape strings** (`"\<Esc>"`,
`"\u00e9"`, `"\x41"`), and 15 more pure builtins (`tr`, `slice`, `sha256`,
`call`, `js_encode`, `matchfuzzypos`, …) — and found 27 more distinct divergences
in the first 1500 expressions.

### R6-1. `\u` / `\U` string escapes were not implemented — ✅ FIXED
`"\U0001F600"` → Vim `😀`, vimlrs the literal text `U0001F600`; `"\u00e9"` was five
characters instead of `é`. The lexer handled `\x`/`\X` but had no `\u`/`\U` arm, so
the letter fell through to the "unknown escape" path and was emitted literally.
(c: eval.c:3590 — `\x` takes 2 hex digits, `\u` 4, `\U` 8; fewer is fine, and *no*
hex digit means it is not an escape at all, which is what Vim's `"a\uZZb"` → `auZZb`
shows. The ported `eval_string` already had this; only the compiled path was blind.)

### R6-2. `string()` of NaN was `NaN`, not `nan` — ✅ FIXED
A regression introduced in round 5: routing `tv_get_string` through `vim_float_g`
dropped the non-finite handling, and Rust's `{:.6}` renders NaN as `NaN`. The
fuzzer caught it on the very next run.

### R6-3. `fnameescape()` returned mojibake for non-ASCII — ✅ FIXED
`fnameescape('ünïcø∂é')` → `'Ã¼nÃ¯cÃ¸âÃ©'`: the loop walked *bytes* and pushed each
one as a `char`, reinterpreting UTF-8 as Latin-1. Every escapable character is
ASCII, so it now walks characters.

### R6-4. `tr()` never checked that {from} and {to} are the same length — ✅ FIXED
`tr('-7', 'hello world', 'x')` → Vim E475, vimlrs returned `'-7'`. The C checks the
set lengths the first time an input character is *not* found in {from}
(`if (first && cpstr == in_str) … if (idx != 0) goto error;`), which the port had
skipped — so a mismatched pair went unreported whenever nothing happened to be
translated.

### R6-5. `function()` / `funcref()` validated nothing — ✅ FIXED
`funcref('nosuchfn')` happily produced a reference to a function that does not
exist. The faithful port (`common_function`) was **written but never wired up**:
`f_function` was an ad-hoc duplicate that skipped every check, and `f_funcref`
delegated to it. Now both route through `common_function`, so:
- `function('nosuchfn')` / `funcref('nosuchfn')` → E700 (unknown function)
- `funcref('a,b,,c')`, `funcref('x y')` → E475 (invalid argument)
- `funcref('')`, `funcref('1234')`, `funcref('  padded  ')` → E129 (function name required)
- `funcref('strlen')` → E700: funcref() resolves through `find_func`, so it takes a
  *user* function only, and a builtin is "unknown" to it.

vim9's `null_function`/`null_partial` used to be lowered to `function('')` — which
is now (correctly) E129 — so they became a real AST constant, `Expr::NullFunc`.

### R6-6. Regex atoms split composing characters — ✅ FIXED (was R5-O1)
`matchstr("é…", '\l')` (with `é` = `e` + U+0301) → Vim `é`, vimlrs the bare `e`. A
matching atom consumes a whole character as `mb_ptr2len`/`utfc_ptr2len` measures it
— the base codepoint *plus* its combining marks — while the engine advanced a single
`char`. Fixed for `.`, literals and classes, and for `split(s, '\zs')`, whose
zero-width step advances one character too.

---

# Round 7 — the fuzzer's next pass

### R7-1. `json_encode()` of a Blob was `null` — ✅ FIXED
`json_encode(0zFF)` → Neovim `'[255]'`, vimlrs `'null'`. A Blob is a JSON *array of
byte values* (c: `TYPVAL_ENCODE_CONV_BLOB`, encode.c:751); the encoder had no Blob
arm and fell through to the catch-all. (Vim and Neovim differ on the separator —
`'[0, 17]'` vs `'[0,17]'` — so this one is pinned to Neovim, the port target, and
is not in the corpus gate.)

### R7-2. `list2str()` did not stop at a NUL — ✅ FIXED
`list2str([65, 0, 66])` → Neovim `'A'`, vimlrs `'A<NUL>B'`. The codepoints are
written into a C string, so a 0 terminates it. (Vim gives `'AB'` here; vimlrs
follows Neovim.)

### R7-3. `slice()` mishandled every non-indexable type — ✅ FIXED
`slice(v:true, 0)` → Vim `0`, vimlrs `'v:true'`; `slice({'a':1}, -255)` → Vim hands
the Dict back unchanged, vimlrs raised E731. The C is
`if (check_can_index(&argvars[0], true, false) != OK) return;` — note `verbose =
false`: a Float/Bool/Special is *silently* rejected and the result stays the default
Number 0, and a Dict copies through `eval_index_inner`, whose range branch also
fails silently, leaving the Dict in place. Both are now ported, error-free.

### R7-4. The `\_x` regex family was unimplemented — ✅ FIXED
`matchstr(' x', '\_.')` → Vim `' '`, vimlrs `''` — and the same for `\_s`, `\_a`,
`\_d`, `\_[…]` and the negated forms. `\_x` means "x, or a newline"
(`:help /\_`), which cannot be done by adding NL to the class's item list (a
*negated* class would then exclude it), so it is modelled as what it is: an
alternation of the atom and a literal newline.

### R7-5. `shellescape()` ignored its {special} argument — ✅ FIXED
`shellescape("a\nb", 1)` → Vim escapes the newline (`'a\<NL>b'`), and likewise `!`,
`%`, `#` — the items `:!` would expand, which it strips again
(`:help shellescape`). vimlrs ignored the argument entirely. (Vim also escapes the
`<cword>`-style cmdline variables; that needs the cmdline-var table and is not
ported.)

### R7-6. `strdisplaywidth()` counted a control character as one cell — ✅ FIXED
`strdisplaywidth("a\nb")` → Vim `4`, vimlrs `3`. A control character has no glyph
and *displays* as `^J` — two cells. `strdisplaywidth` measures the display, so it
counts 2 where `strwidth` (which measures the text) counts 1; both are now right.
Relatedly an unprintable C1 char (`0x80`–`0x9f`) shows as `<80>` — four cells — and
`strwidth` counts those.

### R7-7. `matchbufline()` did not validate its line numbers — ✅ FIXED
`matchbufline(1, 'a', 0, 1)` → Vim E475 ("Invalid value for argument lnum"), vimlrs
an empty list. Line numbers are 1-based; `end < lnum` is E475 on `end_lnum`.

---

# Round 9 — errors as exceptions, `:silent!`, and operand order

### R9-1. A runtime error inside `:try` was not catchable — ✅ FIXED
```vim
try | echo [1] . 'x' | catch | echo v:exception | endtry
```
Vim catches `Vim(echo):E730: Using a List as a String`. vimlrs **printed the error,
kept running the protected block, and never entered `:catch`** — so the single most
common plugin idiom (`try | call Foo() | catch /E117/ | endtry`) did not work.

The machinery was all there and simply not connected: `cause_errthrow` (ex_eval.c)
was ported but nothing called it, and `emsg` just printed. Now `emsg` converts the
message into a pending exception whenever a `:try` is active (a runtime `trylevel`,
raised by `:try` and dropped when the body's paths converge), the existing
per-statement unwind checks carry it to the `:catch`, and the exception is tagged
with the ex-command that raised it (`Vim(echo):`, `Vim(call):`) exactly as Vim tags
it. Catching also resets `did_emsg` (c: `ex_catch`, ex_eval.c:116 — "reset did_emsg,
got_int, did_throw"), so a script that *handles* an error still exits 0.

Covered by `examples/error_exceptions.vim`, whose assertions pass unmodified in
Vim 9.2 and Neovim 0.12.

### R9-2. `:silent!` did nothing — ✅ FIXED
The parser *stripped* command modifiers, so `silent! call Foo()` on a missing
function still printed E117 and marked the script as errored. `silent!` raises
`emsg_silent` for the command it wraps: the command still fails, but the error is
neither shown nor counted (which is why a sourced script with a silenced error
exits 0). Real vimrcs lean on this constantly.

### R9-3. Operand-order errors (was R5-D3) — ✅ FIXED
`eval5` type-checks the **left** operand of `+`, `-` and `.` *before it even parses
the right one* (c:2405) — "to avoid side effects after an error" — so
`0z - remove(d, k)` reports the Blob (E974) and never runs the removal, where vimlrs
reported the removal's error. The check is now emitted between the two operands.
`*`, `/` and `%` do **not** do this (the C evaluates their right operand first too),
and it is skipped for a List/Blob under `+` (list concat is legal and cannot be
judged before the right operand), for a Float, and for a statically-numeric left
operand — which can never fail the check, so `i + 1` keeps its native-arithmetic
fast path.

---

# Round 10 — command abort, and `eval()`'s evaluation order

### R10-1. A failed `:let` stored a corrupted value — ✅ FIXED
```vim
let g:v = 'orig'
silent! let g:v = [1] . 'x'   " E730
echo g:v                      " Vim: 'orig'   vimlrs (before): '0x'
```
Vim **abandons a command whose expression raised an error**, so the assignment never
happens. vimlrs stored whatever the evaluator had recovered with and the script
carried on with corrupted data — the worst kind of divergence, because nothing
reports it. `:echo` already had this guard; `:let` did not.

The guard is skipped when the right-hand side provably cannot raise (a literal, or
an expression the compiler already proved numeric — the same judgement the
native-arithmetic fast path relies on), so `let i = i + 1` keeps its
`CallBuiltin`-free loop body and stays JIT-traceable. The bytecode-shape tests
(`*_traces_on_jit`) enforce that and caught the first version of this change, which
had put two builtin calls into every `:let`.

### R10-2. An erroring command still printed its recovered value under `:silent!` — ✅ FIXED
The `:echo` abort guard keyed on `did_emsg`, which `:silent!` deliberately leaves
alone — so a silenced error slipped past it and `silent! echo [1] . 'x'` printed
`0x` where Vim prints nothing. It now keys on a counter of *every* error raised,
which is what "did this command fail?" actually means.

### R10-3. `eval()` rejected trailing text before evaluating (was R5-O2) — ✅ FIXED
`eval("nl\nhere")` → Vim E121 (undefined variable `nl`), vimlrs E15. The C's `f_eval`
runs `eval1()` on the string, **evaluates what it parsed**, and only then reports
what is left over (`E488: Trailing characters`). vimlrs compiled the whole string up
front, so text Vim would have evaluated became a parse error instead. It now parses
the leading expression (`parse_expr_prefix`), evaluates it, and reports E488
afterwards — so `eval('1 2')` is E488 and `eval('nosuchvar')` is E121, as in Vim.

---

# Round 11 — statement-level parity (found by fuzzing *statements*, not expressions)

The fuzzer only ever generated **expressions**, and the two worst bugs of the whole
effort (`:try` not catching errors, a failed `:let` corrupting a variable) were
found by hand instead. Driving statement snippets through `execute()` — which
returns a command's output as a string, so they fit the existing expression
pipeline — exposed the rest of that blind spot immediately.

### R11-1. An error did not abandon the rest of the command line — ✅ FIXED
```vim
echo 'a' | echo [1] . 'x' | echo 'never'
```
Vim prints `a`, reports E730, and **never runs the third command**, resuming at the
next line (`do_cmdline` abandons the rest of the command line). vimlrs ran it.

The parser now keeps the `|`-separated commands of one source line together
(`Stmt::LineGroup`, grouped by line number — a one-line `if …|…|endif` still
collapses into its single block statement, so blocks are unaffected), and the
compiler abandons the group when one of its commands errors. A line holding a
single command is not wrapped: there is nothing to abandon, and no cost.

### R11-2. `:silent` did not silence output — ✅ FIXED
Round 9 implemented the bang (`:silent!` → `emsg_silent`, suppressing *errors*) but
missed the plain form: `:silent` raises `msg_silent`, which suppresses the command's
**output** — `silent echo 'x'` prints nothing. The bang does both.

### R11-3. The fuzzer's own error capture disabled `:try` in everything it ran — ✅ FIXED (harness)
The harness read each expression's error with `capture_errors_begin`, which is Vim's
**`emsg_silent`** path — and a silenced error is deliberately never converted into an
exception (`cause_errthrow` declines). So `:try`/`:catch` could not work in anything
the fuzzer ran, and it duly reported "vimlrs does not catch runtime errors" for a
dozen statement cases the real binary catches perfectly well. A tool that changes
the behavior it is measuring is worse than no tool.

The harness now *observes* errors (`observe_error`, a read-only hook in the synthesis
zone that suppresses nothing) and decides the outcome from `did_emsg` — the flag that
`:catch` resets and `:silent!` never sets, i.e. the one that actually means "an error
was reported and not handled". It also takes the **first** unhandled error, since Vim
reports one and abandons the command while this VM keeps evaluating and can raise
more.

### R11-6. A bad-arity call in DEAD CODE aborted the whole script — ✅ FIXED
```vim
if 0
  echo strlen('a', 'b')   " never runs
endif
echo 'reached'
```
Vim loads and runs this: a wrong argument count is an error it raises when it *parses
that expression*, i.e. when the command actually runs. vimlrs rejected the call at
**compile** time, so the script failed to load at all (`E118`, exit 1) — and any real
vimrc that guards a call behind `if has(…)` for another Vim version would have died
the same way. The call now compiles to a runtime raise, so an unreachable bad call is
harmless and a reachable one is a normal catchable error (E118/E119, verified
catchable in both engines). Vim does not evaluate the arguments of such a call, and
neither does this.

### R11-5. Which errors a one-line `:catch` sees — ✅ FIXED (corrected R11-4)
R11-4 got the rule half right, and the statement fuzzer caught it. The line is not
"inline `:try` never catches an error" — it is **where the error came from**:

| error raised by | example | eval1() | one-line `:catch` |
|---|---|---|---|
| a called builtin | `nosuchfn()` (E117), `insert([1],{},100000)` (E684) | OK | **catches** |
| a missing Dict key | `deepcopy({})[2]` (E716) | OK | **catches** |
| the `eval5` operand pre-check | `[1] . 'x'` (E730), `0z11 - 1` (E974) | FAIL | escapes |
| an unindexable value | `log10(-3.25)[-5:0]` (E806) | FAIL | escapes |
| coercing a condition | `sort(…) ? v:true : [1]` (E745) | FAIL | escapes |

The expression evaluator's **own type checks** make `eval1()` return FAIL, and a
command whose argument failed to evaluate takes the whole command line with it — so
the `:catch` on that line never runs. An error raised *inside* a called function does
not fail the evaluator, and is caught. A multi-line `:try` catches both, since its
`:catch` is on another line. Such a "hard" failure also abandons the rest of the line
**even under `:silent!`**, while an ordinary silenced error lets the line continue.

Every row is verified against both engines and pinned in
`examples/error_exceptions.vim`.

### R11-4. An error in a one-line `:try` was caught (Vim lets it escape) — ✅ FIXED (superseded by R11-5)
```vim
try | echo [1] . 'x' | catch | echo 'caught' | endtry
```
Vim does **not** catch this: the error abandons the command line, which takes the
`:catch` with it, and the exception escapes to an enclosing handler. An explicit
`:throw` on the same line *is* caught (the block works — it is the abandoned line
that skips the `:catch`), and a multi-line `:try` catches errors normally.

vimlrs caught it, i.e. it was **more forgiving than Vim** — the dangerous direction,
since a plugin that looks protected under vimlrs would not be under Vim. The parser
now records whether a `:try` was written on one line, the runtime records whether the
pending exception came from an error or from `:throw`, and an inline `:try` skips its
`:catch` clauses for the former. An uncaught error-exception is also reported as the
error itself (`E730: …`), not wrapped in E605 — E605 is for an uncaught `:throw`.

---

# Round 12 — the regex engine (a grammar-based pattern fuzzer)

`viml_regex` is the largest hand-written carve-out in the crate: it reproduces Vim's
pattern *dialect* from the documentation rather than porting `regexp_bt.c` /
`regexp_nfa.c`. Drawing patterns from a fixed list only ever exercises the shapes
somebody already thought of, so the fuzzer now **builds patterns from a grammar**
(atoms × quantifiers × groups × alternation × magic prefixes) and runs each through
every API that reaches the engine — `match`, `matchstr`, `matchend`, `matchlist`,
`substitute` (plain, `g`, `[&]`) and `split` — against a subject pool that straddles
the boundaries (empty, ASCII, multibyte, combining, punctuation, digits).

The first 800 cases produced **326 divergences and a crash** — a 40% failure rate, by
far the worst surface in the interpreter.

### R12-1. Inverted capture span crashed the matcher — ✅ FIXED
`matchend('abc', '\(\zs\?\)\{2}')` panicked with "slice index starts at 1 but ends
at 0": a group's span came back inverted when a `\zs` inside it moved the match start
past where the group closed.

### R12-2. An invalid pattern matched nothing instead of raising — ✅ FIXED
Vim **rejects** a bad pattern, and every function that takes one raises the error:
- `\1` with no such group (and a *forward* reference) → E65
- a quantifier that repeats `\zs`/`\ze` (`\zs*`, `\ze\{2}`) → E888. Note `\zs\?` is
  legal: `\?`/`\=` only make it optional, they do not repeat it.
- a quantifier on a quantifier (`a*\+`) → E871
- an unclosed `\(` → E54

vimlrs silently treated all of them as patterns that happened to match nothing. Now
the pattern is reported at compile time and the regex matches nothing thereafter, so
every caller returns what Vim returns once it has raised the error.

### R12-3. `\M` and `\V` were not implemented at all — ✅ FIXED
`match('a.c[x', '\Vx')` → Vim `4`, vimlrs `-1`. Nomagic and very-nomagic were simply
absent from `preprocess_magic`, so the `\V` was parsed as an escaped literal `V` and
the rest of the pattern was garbage. `\V` is common in real vimrcs (it is how you
match literal text), so this was a feature gap, not an edge case.

The four dialects differ only in *which* characters are special (`:help /magic`): in
nomagic `.` `*` `~` `[` are literal and `\.` `\*` … are the special ones — the
escaping is simply swapped — and very-nomagic swaps `^` and `$` as well, so `\V^` is a
literal caret. Everything else (`\(`, `\|`, `\zs`, `\d`, …) is identical in all four,
so translating into the magic dialect the parser already reads is enough.

Regex fuzz after these: **326 → 77 gaps, 0 panics.**

### R12-4. A backreference to an *unclosed* group was accepted — ✅ FIXED
`\(a\1\)` is E65 in Vim: the group must be **complete** before it can be referred to.
Counting *opened* groups was not enough — `\(\(a\)\2\)` is legal (group 2 closed) while
`\(\(a\)\1\)` is not (group 1 still open). The parser now tracks which groups have
closed.

### R12-5. `[z-a]` and a stray `\)` were accepted — ✅ FIXED
E944 (reverse range in a character class) and E55 (unmatched `\)`).

### R12-6. An unterminated `[` was treated as a collection — ✅ FIXED
`match('a[x', '[')` → Vim `1`, vimlrs `-1`; `match('a[x', '[abc')` → Vim `-1`, vimlrs `0`.
An unterminated collection is not a collection at all: Vim treats the `[` as a
**literal character**, so the pattern `[abc` looks for the literal text `[abc`. The
first `]` may still appear right after `[` or `[^` (`[]a]` holds `]` and `a`), so the
scan starts past it.

### R12-7. A misplaced multi was accepted — ✅ FIXED
`\+` or `\{2}` at the start of a branch has nothing to repeat: Vim rejects it (E866).
The subtlety is that a **bare `*` there is not an error** — magic treats a leading star
as a literal (`match('a*b', '*')` finds it) — while the *nomagic* special star `\M\*`
IS a multi and is rejected. Both become a magic `*` once the pattern is translated, so
the parser can no longer tell them apart; `preprocess_magic` is the only place that
still can, and it reports that one.

### R12-8. A group that can match empty could not satisfy `\{n}` — ✅ FIXED
`match('aaa', '\%(\.\?\)\{2}')` → Vim `0` (an empty match at 0), vimlrs `-1`. The
matcher counted a zero-width match **once** and stopped, so a group that matches empty
could never reach a `min` above 1. Repeating an empty match is still legal — it simply
never advances — so it now satisfies `min` and then stops, which is what keeps it from
looping forever.

### R12-9. `\_x` and `\%(` broke under `\M`/`\V` — ✅ FIXED
`match('ünïcø∂é', '\M\_.\+')` → Vim `0`, vimlrs `-1`. `\_` is a two-character prefix, so
the char after it belongs to the atom — and the nomagic translation was literal-izing
it, turning `\M\_.` into `\_\.` ("any char including newline" followed by a *literal*
dot), which matches nothing. Same root cause as the `\%d97` case fixed earlier: a
multi-character escape has to be copied whole.

Relatedly `\%(` opens a group, so a multi right after it again has nothing to repeat —
`\M\%(\*\)` is E866, and the translation was not resetting the branch state for the
non-capturing form.

Regex fuzz after round 15: **326 → 12 gaps** (5 distinct), 0 panics.

### R12-10. The `\@` lookaround family was not implemented at all — ✅ FIXED
`split('3.5e2','a\@!')` → Vim `['3', '.', '5', 'e', '2']`, vimlrs `['3.5e2']`;
`substitute('*.[]^$\','a\@!','X','abc')` → Vim `X*.[]^$\`, vimlrs unchanged.
The parser had no `\@` handling, so `a\@!` fell through to literal `a`, `@`, `!` —
a pattern that matches nothing in those subjects — instead of a zero-width
negative lookahead that matches at every position where `a` does not follow.

Ported the whole family from `nfa_regpiece` `case Magic('@')` (25 probe cases
verified identical in vim 9.2 and nvim before fixing):
- `\@=` / `\@!` — (negative) lookahead, zero-width; groups captured inside a
  successful positive lookahead are kept (`matchlist('foobar','foo\(bar\)\@=')`
  includes `'bar'`).
- `\@<=` / `\@<!` — (negative) lookbehind: the atom must match **ending exactly
  at** the assertion position; it may match empty, and the *farthest* start wins
  the captures (`matchlist('aaab','\(a*\)\@<=b')[1]` is `'aaa'`). The `\@123<=`
  form bounds how far back the attempt may start (C counts bytes; this engine's
  unit is chars).
- `\@>` — match the atom like a standalone pattern and consume it, with no
  backtracking into it (`match('aaa','\(a*\)\@>a')` is -1).
- Very magic: a bare `@` is the operator (`\v(foo)@<=bar`), and its digits and
  `<`/`=`/`!`/`>` suffix chars are copied raw by the `\v` translation so they are
  not re-translated into `\<`/`\=`/`\>`.
- Errors, matching the NFA engine: `\@` with no atom before it → E866, a multi on
  either side (`a*\@=`, `a\@!*`, `\(a\)\@=\{2}`) → E871, an unknown operator
  (`a\@x`, `a\@<x`, trailing `a\@`) → E869.

## Still open

- The remaining regex gaps (5 distinct) are deeply-nested backtracking corners —
  non-greedy `\{-}` inside a repeated group, and the *order* in which two errors in one
  pattern are reported (vimlrs reports the first it parses, Vim the first its NFA
  rejects).
- `nr2char(2147483647)` now emits the encoded bytes (round 17), so the direct
  value matches after lossy decode — but a *byte slice* of it
  (`nr2char(2147483647)[2:]`) still diverges: Vim slices the raw 6-byte
  sequence, vimlrs slices the U+FFFD-substituted string. Same
  string-representation root cause as R5-D1 (Vim strings are byte arrays), which
  remains the one structural divergence.
- `execute()` of a command that errors captures the error text in Vim but not in
  Neovim. vimlrs follows Neovim, its port target — no single spec.

---

# Round 17 — multibyte string semantics (the string-builtins fuzz cluster)

Differential-fuzz cluster over `strcharpart` / `nr2char` / `escape` / `toupper` /
`strtrans` / `trim` / `strdisplaywidth` / `strcharlen` / `strpart` / `slice`
(seeds 424242, 20260716, 987654 — both oracles agreed on every case below).
The recurring theme: Vim walks strings by **byte** (subscripts) or by
**base-plus-composing cluster** (`utfc_ptr2len` — escape/slice/strpart/trim),
and its C truncates 64-bit arguments to `int` instead of erroring.

### R17-1. `strcharpart()` PANICKED on an INT64-min start — ✅ FIXED
`strcharpart("a\\b",-9223372036854775808)` → overflow panic in
`units.len() - start`. The C computes `nbyte = (int)nchar` — a *truncating*
cast (the literal saturates to INT64_MAX, negated = `0x8000…0001`, truncated
= `1`), so Vim returns `'\b'`. `f_strcharpart` is now the C's byte walk with
the same `(int)` casts, including `{len}` counting one byte per position
before the string start.

### R17-2. `nr2char(0)` returned a NUL byte; huge codepoints returned `''` — ✅ FIXED
The C writes `utf_char2bytes()` into a buffer and `xmemdupz()`s it — a C
string, so `nr2char(0)` is `''` (terminates at the NUL). Out-of-Unicode values
(surrogates, > 0x10FFFF) now emit the same 3–6 byte sequences the C does
(U+FFFD-substituted in a Rust String). Also E5070/E5071 for negative/too-big.

### R17-3. `escape()` walked by codepoint, escaping composing marks — ✅ FIXED
`escape('écombining','écombining')` (decomposed é) escaped the combining
accent. `vim_strsave_escaped_ext` walks `utfc_ptr2len` units: any multibyte
unit — including base + composing — is copied verbatim and never matched
against `{chars}`; only single-byte (ASCII) characters are escaped. Oracle:
`'é\c\o\m\b\i\n\i\n\g'`.

### R17-4. String `[i]`/`[a:b]` subscripts were char-based — ✅ FIXED (bug #8)
`toupper('écombining')[5:]` → `'BINING'` (vim `'MBINING'`),
`strtrans('écombining')[5]` → `'b'` (vim `'m'`), `trim("é",'a,b,,c')[1:-1]` →
`''` (vim: the é's lone continuation byte), `escape('日本語',"\<Esc>")[-5:3]` →
`'日本語'` (vim `''` — bytes 4..3 is empty). `eval_index_inner`'s legacy path
is byte-indexed: `[i]` is `xmemdupz(s+n1, 1)` — one byte — and `[a:b]` wraps
and clamps against `strlen`. Fixed in the bridge (`index_value`/`slice_value`)
and the ported `eval_index_inner`; `slice()`/`char_from_string`/`string_slice`
(the `exclusive` path) stay character-based but now fold composing clusters
(`utfc_ptr2len`), fixing `slice('écombining',1)` → `'combining'`.

### R17-5. `trim()` — default/empty mask, E1174, and `(int)` dir — ✅ FIXED
Three gaps in one builtin: (a) the default mask trimmed only ASCII whitespace —
the C trims any `c1 <= ' '` plus 0xa0, so `trim("\<Esc>")` is `''`; (b) an
*empty* `{mask}` string is folded to NULL → the default set (`trim("  x  ","")`
is `'x'`), and a non-String `{mask}` is E1174; (c) `{dir}` is
`(int)tv_get_number_chk(...)` — INT64-min truncates to 0 (trim both ends),
NOT E475 (E475 only for a truncated value outside 0..2). Mask matching
compares the *base codepoint* of each cluster and advances by whole clusters
(`MB_PTR_ADV`).

### R17-6. `strdisplaywidth()` with a huge column returned the width — ✅ FIXED
`strdisplaywidth('ünïcø∂é',2147483647)` → 7 (vim 0). `linetabsize_col`
accumulates an int64 `vcol` but clamps the returned int at MAXCOL
(0x7fffffff, pos_defs.h) — a start column at INT_MAX saturates immediately, so
`result - col` is 0. The column is also `(int)`-truncated, not clamped at 0.

### R17-7. `strpart({chars})` and `strcharpart({skipcc})` cluster walks — ✅ FIXED
`strpart('écombining',0,2,0)` → `'é'` (vim `'éc'`): the C's `{chars}` walk is
`utfc_ptr2len` — composing marks ride along with their base — and it applies
whenever the 4th argument is *present*, regardless of its value.

### R17-8. `strtrans("\n")` showed `^J` instead of `^@` — ✅ FIXED
`transchar_nonprint()`: `if (c == NL) c = NUL;` — "we use newline in place of
a NUL", so a newline in a String displays as `^@`.

### R17-9. `slice()` on a List clamped an out-of-range start — ✅ FIXED
`slice([1,2,3],-4,2)` → `[1,2]` (vim `[]`). `tv_list_slice_or_index` sets
`n1 = len` when the wrapped start is still out of range — an empty slice, not
a clamp to 0. `f_slice` now routes Lists/Blobs through the ported value layer
(and a NULL-blob result indexes as length 0 → E979, not v:null).

### R17-10. A float literal after `.`/`..` concat parsed as a float — ✅ FIXED
`'a' . -0.5` → `'a-0.5'` (vim `'a05'`): `eval_number(..., want_string)` never
reads a float after the concat operator, so `-0.5` is `-0 . 5` — two more
concats. Likewise the trailing-junk rules: `1.2.3` is `'123'` (a second `.`
rejects the float wholesale) and `1.5e`/`1.5ex` is the Number 1. The lexer now
applies the C's trailing-character rejections, and the parser re-splits a
Float token in concat-RHS position. Exponent junk (`'a' .. -1.0e300`) is
Vim's *deferred* E15: raised when the expression runs, AFTER operands to its
left — `([1] .. 1.0e300)` is E730, not E15 — and even when the junk sits in a
branch evaluation never reaches (`Expr::ScriptError` operand +
`Expr::ScriptErrorGuard` whole-expression wrapper; `VIML_RAISE` yields to any
earlier error in the statement).

String-cluster fuzz after round 17 (seeds 424242 / 20260716 / 987654, 2000
exprs each): **0 gaps, 0 panics** on two seeds; the single remaining distinct
gap is `nr2char(2147483647)[2:]` — byte-slicing a string of raw out-of-Unicode
bytes — the R5-D1 structural divergence (Vim strings are byte arrays).

---

# Round 18 — error-code / check-order semantics (the call/eval/matchbufline/matchfuzzy fuzz cluster)

Differential-fuzz cluster over `call` / `funcref` / `eval` / `matchbufline` /
`matchfuzzy` / `matchfuzzypos` and their binop/ternary compositions (seeds
987654, 20260716, 424242 — both oracles agreed on every case below; all three
seeds now replay with **0 gaps, 0 panics**).

### R18-1. `call()` of a lambda with too few arguments ran the body — ✅ FIXED
`call({x -> type(x)},[])` → `7` (vim E119). `call_user_func_check`
(userfunc.c) validates arity BEFORE binding: too few (below
`uf_args - uf_def_args`) is E119, too many without varargs is E118. vimlrs
bound the missing parameter to a placeholder and ran the body. The bridge's
`call_user_function_raw` now ports the check. A lambda is created with
`uf_varargs = true` (`get_lambda_tv`, userfunc.c:396), so
`call({x -> x},[1,2,3])` stays `1` — never E118.

### R18-2. `matchbufline()` buffer/lnum validation order — ✅ FIXED
`matchbufline(-9223372036854775808,'a\>',10,-10)` → E158 (vim E475
end_lnum), and `matchbufline(0,'\h',-1,…)` → E475 lnum (vim E158). Two `(int)`
casts in the C: `tv_get_buf` does `buflist_findnr((int)v_number)` — the
saturated literal -9223372036854775807 (0x8000000000000001) truncates to
buffer **1**, which exists, so validation proceeds to the lnum checks — and
buffer `0` goes through `buflist_findnr`'s `nr = curwin->w_alt_fnum` (no
alternate buffer → E158, never "current buffer"). `linenr_T` is 32-bit, so
lnum arguments truncate the same way (`end_lnum` of INT64_MAX is -1 → E475).

### R18-3. `funcref()` with a combining char in the name was E475 — ✅ FIXED
`funcref('écombining',…)` (decomposed e + U+0301) → E475 (vim E700 with the
full name). `find_name_end()` (eval.c) advances with
`MB_PTR_ADV`/`utfc_ptr2len`, so a composing char rides along with its accepted
base char even though `eval_isnamec()` is ASCII-only; the whole
"e\u{301}combining" is one (unknown) function name. The Rust port advanced
byte-wise and cut the name at the combining char, leaving trailing text →
`s = NULL` → E475. Note the split behavior is real: the *expression* name
path (`get_id_len`, byte-wise) still stops at the combining char, which is
why `eval('écombining')` is E121 "Undefined variable: e" — both verified
against vim 9.2 + nvim 0.12.

### R18-4. `eval()` lexed the whole string instead of one expression — ✅ FIXED
`eval('a''quote')` → E115, `eval('tab\there')` → E15 (vim: E121 "Undefined
variable: a"/"tab"), `keys({…}) % eval('écombining')` → E15 (fuzz-prelude
oracle: E488). The C `f_eval` runs `eval1()`, which consumes ONE leading
expression and never looks past it: evaluation errors (E121) surface first,
and only a *successfully* evaluated expression with leftover text is E488
(`e_trailing_arg`); the E15 fallback fires only when `eval1` itself fails
(and not when aborting inside `:try`). `parse_expr_prefix` now lexes with
`lex_prefix` (stops at the first untokenizable byte instead of failing), and
`b_eval` reports E488 only on success and E15 only on failure.

### R18-5. `matchfuzzy()`/`matchfuzzypos()` used the pre-fzy scoring — ✅ FIXED
`matchfuzzypos(['B','a','C'],'a')` → `[…, [115]]` (vim `[…, [2147483647]]`).
Both vim 9.2 and nvim 0.12 score with the **fzy** algorithm (nvim fuzzy.c,
adapted from jhawthorn/fzy): a whole-string case-insensitive match is
`SCORE_MAX` → the INT_MAX sentinel, and every partial score is the DP result
scaled by 1000 (`'a'`→`'ab'` = 895, `'ba'`→`'bar'` = 1895, `'a'`→`'bar'` =
-10). vimlrs had the old recursive bonus/penalty scorer (base 100 +
first-letter/sequential bonuses), wrong on every score. Ported `has_match` /
`compute_bonus_codepoint` / `match_row` / `match_positions` / `fuzzy_match` /
`fuzzy_match_in_list` / `do_fuzzymatch` from fuzzy.c, including: `limit`
capping the *scan* (first N matches, not top-N scores), `matchseq` as a
key-presence check, the exact-match tiebreak in `fuzzy_match_item_compare`,
and the E686/E475/E1206 argument validation.

### R18-6. Junk float in a skipped branch let dead code run — ✅ FIXED
`(isnan(1.0) ? (-2147483648 .. 1.0e308) : matchbufline(0x1f,…))` → E158 (vim
E15). Follow-up to R17-10: the re-split float's exponent junk is a *parse*
failure in Vim, aborting `eval1` at the junk's position — everything
textually after it is dead, even the other ternary branch (`matchbufline`
never runs), and a short-circuited `&&`/`||` right operand still reports E15
right after its node. The parser now tracks re-split junk per operand: junk
in a ternary then-branch replaces the (dead) else-branch with the raise; junk
in an else-branch / short-circuit RHS wraps the node in
`Expr::ScriptErrorGuard` (raises after the node evaluates, yielding to any
earlier error).

# Round 19 — positional printf, locale collation, and split's default pattern

### R19-1. `$`-style printf formats skipped Vim's validation pre-pass — ✅ FIXED
`printf('%1$s %1$s')[5]` → E766 (vim E1503). Vim validates positional
(`%N$`) formats in a pre-pass over the whole format string *before* anything
renders — `parse_fmt_types()`/`adjust_types()` (strings.c:1101/1013) — with
its own error family: E1500 mixed positional/non-positional (`%1$s %s`,
also raised for an unknown specifier carrying a position), E1501 a slot the
format never uses (`%2$s` with `'a'`), E1502 a `*N$` field-width slot reused
as a non-int, E1503 a slot past the supplied arguments, E1504 a slot reused
as a different type (`%1$d %1$s` → "string/int"), E1505 a malformed spec
(`%01$d`, `%5$` after width digits), and E1510 for a huge digit run — where
`%$d` is E1510 with an *empty* digit run because `get_unsigned_int()` computes
`(unsigned)('$' - '0')` and trips the overflow check. vimlrs formatted
positionals leniently and only reported E766/E767 afterwards. Ported all six
(`format_typeof`/`format_typename`/`adjust_types`/`format_overflow_error`/
`get_unsigned_int`/`parse_fmt_types`) as a pre-pass in `f_printf`, plus the
`%N$*M$d` positional field width/precision the formatter didn't consume.
All error/value probes verified against nvim 0.12 + vim 9.2.

### R19-2. `sort()`'s `'l'` flag compared by byte order, not `strcoll()` — ✅ FIXED
`sort(['b','a','C','A'],'l')[0]` → `'A'` (vim `'a'` under en_US.UTF-8). The C
`item_compare()` (eval/typval.c:1245) calls `strcoll()`, which collates by the
locale adopted at startup — `init_locale()` (os/lang.c) does
`setlocale(LC_ALL, "")` then forces `LC_NUMERIC` back to `"C"`. vimlrs
approximated `'l'` with byte comparison, so uppercase sorted before lowercase
regardless of locale. Ported `init_locale()` (`Once`-guarded, invoked lazily
before the first `strcoll`) and routed the `'l'` branch through
`libc::strcoll`. The ordering is inherently locale-dependent (`LC_ALL=C`
yields `['A','C','a','b']` in real Vim too), so the corpus records the
locale-invariant `sort(['b','a','c'],'l')`; the locale-sensitive case is
covered by the fuzzer, whose oracles run under the same environment.

### R19-3. `split()`'s default pattern was `\s\+`; Vim's is `[\x01- ]\+` — ✅ FIXED
`split("\<C-A>")` → `['']` (vim `[]`). With `{pat}` missing *or empty*,
`f_split()` (eval/funcs.c:7089) uses `"[\\x01- ]\\+"` — a run of ANY byte from
0x01 through space, not just whitespace — so a control-char-only subject
splits to nothing, exactly like `split(' ')`. vimlrs used a whitespace split
that also ignored `{keepempty}` for the default pattern
(`split('  a  b ', '', 1)` dropped the empty edges vim keeps). Now the default
routes through the same regex path as an explicit pattern, with the
collection's `\x01` written as the literal codepoint (the pattern engine
does not yet decode `\x`-escapes inside `[…]` — the C's `coll_get_char()`).

---

# Round 20 — whole-script parity (the new `scripts/parity.sh` harness)

Rounds 1–19 fuzzed *expressions* and single statements. Round 20 added
`scripts/parity.sh`, which sources a whole `.vim` file through vimlrs and through
`vim -es -u NONE -i NONE -c 'verbose source FILE' -c 'qa!'` and byte-diffs the
captured output plus the exit status. Everything below was found by that harness
on the first pass over a hand-written corpus (`tests/parity_cases/`), and none of
it is reachable one expression at a time: these are divergences *between*
statements — message state, option state, and definitions that parse but leave
nothing behind. Each case is committed with vim's recorded output and replayed in
CI by `tests/parity_cases.rs`.

### R20-1. `:echo` appended a newline; vim's newline is a LEADING separator — ✅ FIXED
The single highest-impact one. Vim's message layer breaks the line only when the
column is non-zero (`msg_start`), and `:echon` never breaks it, so:

```vim
echo 'A' | echo '' | echo 'B' | echon 'C' | echo '' | echo '' | echo 'D'
```

prints `A / BC / D` in vim and printed `A / <blank> / B / C / <blank> / D` in
vimlrs — every empty `:echo` became a blank line instead of ending the current
one, and every `:echo` after an `:echon` started on the wrong line. Modelled with
`MSG_COL` in `fusevm_bridge.rs`: `:echo` emits the break first *if* the line has
text, `:echon` never does, and the run's last line is closed once at exit by
`msg_flush_line()` (a CLI convenience — vim itself leaves it open). Errors call
the same flush before writing to stderr, matching `msg_start`. The `execute()`
and embedding capture paths keep their existing conventions.
Cases: `tests/parity_cases/echo_column.vim`, `echo_mixed.vim`.

### R20-2. `'ignorecase'` did not reach `==` / `<` / `=~`, and `=~#` ignored case — ✅ FIXED
`eval4` (c:2209) resolves an unsuffixed comparison's case rule from `p_ic` **when
the comparison runs**; `#`/`?` fix it at parse time. vimlrs baked all three into
the opcode id at compile time, mapping "no suffix" to match-case, so
`set ignorecase | echo 'abc' == 'ABC'` printed 0 (vim: 1) — and the *same* bug
inverted for regex, where `pattern_match` OR-ed the option in unconditionally, so
`set ignorecase | echo 'abc' =~# 'ABC'` printed 1 (vim: 0). Added a third
comparison-id family (`VIML_CMP_OPT_BASE`, 3080..=3089) whose handlers read
`'ignorecase'` at run time, and removed the OR in `pattern_match`. Affects `==`,
`!=`, `<`, `<=`, `>`, `>=`, `=~`, `!~` and the List/Dict element comparisons that
recurse through `tv_equal`. Script-cache format version bumped (opcode ids
changed meaning). Case: `tests/parity_cases/ignorecase.vim`.

### R20-3. `:lockvar` / `:unlockvar` did not parse — ✅ FIXED
Both commands were a parse error, which had a second-order effect far worse than
the missing feature: a file containing one fell back to `source_tolerant`, and
that path (R20-4) silently dropped earlier assignments. The full `ex_lockvar` /
`ex_unletlock` / `do_lock_var` chain was already ported in `vars.rs` and simply
unreachable — the parser now produces `Stmt::LockVar` and the bridge rebuilds the
`exarg_T` those take. `islocked()` (already ported) consequently went from always
`0` to vim's `1`/`0`/`-1`.

### R20-4. A statement-at-a-time fallback lost top-level `:let` — ✅ FIXED
When a file contains anything the parser rejects, `eval_file` falls back to
running it statement by statement so the rest of a real `.vimrc` still takes
effect. Each statement was compiled with `compile_program`, whose top-level slot
planner sees `let A = 1` alone as a write nobody reads and absorbs it into a
chunk-local slot — so `g:A` was never written and the *next* statement's
`echo A` raised E121 where vim prints 1. Split out `compile_script_stmt`, which
is `compile_program` with top-level slotting disabled.

### R20-5. `:const` assigned but never locked — ✅ FIXED
`const C = 5` behaved as `let`, so `let C = 9` afterwards silently succeeded
where vim raises `E741: Value is locked: C`. `:const` now lowers to the
assignment plus the `:lockvar!` it implies (c: `set_var_const` locks with
`DICT_MAXNEST`), and the `:let` bridge path checks the lock before overwriting.

### R20-6. `function d.key()` defined nothing, and `d.key()` could not call it — ✅ FIXED
Three separate holes in the same feature:
- `function d.get() dict … endfunction` registered a function literally named
  `d.get` and never touched the Dict, so `d.get` was E716. Vim defines an
  anonymous numbered function and stores a reference under the key; the compiler
  now does the same (`func_nr`-style counter from 1) and lowers the definition
  line to the assignment it implies.
- `d.key(args)` parsed as `d . key(args)` — string concatenation — because the
  earlier fix for `substitute(…).submatch(0)` (bug #2) made `.name(` *always*
  concat. It is actually the same runtime question `Expr::Member` already
  answers: added `Expr::MemberCall`, which tests the base's type at run time and
  either calls `base[key]` or concatenates with `key(args)`. Both forms verified
  (`substitute('abc','.','\=submatch(0).submatch(0)','g')` still gives `aabbcc`).
- `self` was entirely unimplemented, so any `dict` function raised E121. The
  `self` dict is now bound into the function-local scope for a `d.key()` call, a
  `d['key']()` call, a Partial carrying `pt_dict`, and `call(F, args, dict)`.

Its remaining divergence is closed by R21-3 below.

### R20-7. `typename()` was missing — ✅ FIXED
Ported as `f_typename` (c: `type_name(typval2type(…))`, vim9type.c). Scalars,
`list<T>`/`dict<T>` member inference and the `<any>` fallbacks all match vim
exactly. A Funcref reports `func(...): any`, which is what vim prints for every
legacy `:function`; vim's precise signatures for *builtin* funcrefs and vim9
lambdas need the vim9 argument-type table this port does not carry.
Its arity is not in `funcs_argc.rs` (generated from Neovim's `eval.lua`, and
Neovim has no `typename`), so `EXTRA_BUILTIN_ARGC` in `compile_viml.rs` supplies
it — consulted only when the generated table has no entry, so it cannot
contradict it.

### R20-8. `\f` / `\F` regex atoms were unimplemented — ✅ FIXED
`matchstr('foo/bar','\f\+')` → vim `foo/bar`, vimlrs `f` (the `\f` was taken as a
literal). Implemented against `'isfname'`'s Unix default, enumerated char by char
against vim 9.2 over `0x20..=0x7E`: `#$%+,-./0-9=A-Z_a-z~` plus multibyte
alphabetics. (`\k`, `\p`, `\i` and the POSIX classes were already present — the
remaining R3-12 item was `\f` alone.)

### R20-9. `v:exception` was not restored by `:endtry` — ✅ FIXED
`try | throw 'a' | catch | endtry` left `v:exception` set to `a` forever; vim
restores the value the enclosing level had (empty at the top). c: `ex_try` saves
it and `ex_endtry` puts it back, so a nested `:try` restores the *outer* catch
clause's value rather than clearing it — both verified.

### R20-10. The first lambda was `<lambda>0`, vim's is `<lambda>1` — ✅ FIXED
c: `get_lambda_name()` (userfunc.c:269) is `"<lambda>%d", ++lambda_no` — a
pre-increment. Observable through `string({x -> x})`.

### R21-1. `expr ?` with no `:` was accepted, silently — ✅ FIXED (was R20-O2)
`echo 1 ? 'a'` ran and printed nothing where vim raises E109 — vimlrs was *more
permissive* than the reference, so a script vim refuses to run ran here.

The parser did raise `E15: expected Colon, found Eof`, but that is a parse
failure, and `fusevm_bridge::source_tolerant` (the fallback that keeps a real
`.vimrc` sourcing past a construct this port cannot parse) drops a statement that
fails to parse without a word. The error never reached the user.

c: `eval1()` (eval.c) parses the then-branch, *evaluates* it when the condition
is true, and only then checks for the colon and
`emsg(_(e_missing_colon_after_questionmark))`. So E109 is a **run-time** error
carrying the ex-command tag, not a parse failure. Verified against vim 9.2 with a
counter-bumping function: `try | echo 1 ? Bump() | catch | endtry` catches
`Vim(echo):E109: …` with `g:n` already 1, and `echo 0 ? Bump()` raises the same
error without calling `Bump()`.

`Expr::ScriptErrorGuard` (evaluate, discard, raise) is exactly that shape, so
`viml_parser::missing_colon` builds a `Ternary` whose then-arm is the parsed
branch wrapped in a guard and whose else-arm is a bare `Expr::ScriptError` —
side effects on the truthy path only, E109 on both. Recorded as
`tests/parity_cases/ternary_e109.vim`.

### R21-2. The `Vim(cmd):` tag leaked out of a returning function — ✅ FIXED
Found while pinning R21-1: `let g:z = G() + [][0]` reported
`Vim(return):E684` where vim reports `Vim(let):E684`. `CUR_CMDNAME` is set per
statement and was never restored when a user function returned, so the *callee's
last* command tagged the *caller's* next error. c: the tag names the command
`do_cmdline` is executing at the throw site, and a function body is its own
`do_cmdline`, so the caller's command is back in force on return —
`call_user_function_raw` now saves it across the body chunk. Covered by the
`after-call` lines of `tests/parity_cases/ternary_e109.vim`.

### R21-3. A Funcref read out of a Dict was not bound to it — ✅ FIXED
`string(d.get)` was `function('1')` where vim prints `function('1', {…})`, and
the divergence was wider than `string()`: `let F = d.get` followed by `F()`
raised `E121: Undefined variable: self`, and `let e.get = d.get` then `e.get()`
ran with the *old* dict.

c: `set_selfdict` (eval.c:6014) -> `make_partial` (userfunc.c:3805) — a Dict
subscript that yields a function turns it into a partial bound to that Dict.
Two gates, both ported and both verified against vim 9.2:

- `make_partial` binds only when the function carries `FC_DICT`
  (userfunc.c:3837) — the `dict` attribute after the parameter list, or the
  `:function d.key()` form, which sets it implicitly (`function d.nodict()` with
  no attribute still yields `function('1', {…})`). A plain function stored in a
  Dict stays a plain Funcref.
- `set_selfdict` declines to *re*-bind a partial the script bound explicitly
  (`!pt_auto && pt_dict != NULL`, eval.c:6018), so `function('F', d)` keeps `d`
  after being stored in another Dict while a `d.key` reference follows whichever
  Dict it is read from.

The binding fires on the subscript, so `get(d, 'key')` and a Funcref inside a
List come back unbound — also verified. Carried: `FC_DICT` as
`UserFuncDef::dict`, `pt_auto` on `partial_T`, the `, {self}` suffix in
`encode_tv2string` (c: `TYPVAL_ENCODE_CONV_FUNC_BEFORE_SELF`, encode.c:400), and
`set_selfdict` on both `VIML_INDEX` and `VIML_CALL_MEMBER`.
`SHARD_FORMAT_VERSION` 4 -> 5 for the `UserFuncDef` layout change. Recorded as
`tests/parity_cases/dict_partial.vim`.

### R21-4. `v:throwpoint` was always empty — ✅ FIXED (was R20-O1)
vim reports the whole exception stack plus the raising line
(`…script /path/x.vim[23]..function Outer[1]..Thrower, line 1`). This port
tracked source lines only in a `--dap` build, via `SET_LINENO` marker ops.

**Line tracking costs nothing.** `fusevm::ChunkBuilder::emit` already takes a
line and `fusevm::Chunk` already keeps a `lines` vector parallel to `ops`; the
compiler was passing the constant `1` for every op. Passing the real line
instead emits no bytecode at all — no marker op, no builtin call — so a numeric
loop body stays `CallBuiltin`-free and JIT-eligible (`--tiers` and the
`tiers::tests` trace tests are unchanged), and the line is readable from any
builtin as `vm.chunk.lines[vm.ip - 1]`. The `SET_LINENO` markers remain the DAP
path and were not touched.

What that cost was an AST change: block bodies are now `viml_ast::Block`
(`Vec<(u32, Stmt)>`) rather than `Vec<Stmt>`, so every statement carries the line
the parser *already had* and was discarding in `strip_lines`. `parse_program` and
`parse_program_lines` collapse into one function as a result. Inside a function
body the line is made relative to the `:function` (`Compiler::line_base`),
because that is what vim reports: a throw on the third body line is
`…function F, line 3`, not the file line.

The exception stack is `fs::sourcing_names()` (already maintained) plus
`funccal_stack`'s `fc_name`s, with `CALL_SITE_LNUM` recording each frame's call
site — that is the `[23]` and `[1]` above. Rendered by
`fusevm_bridge::throw_point` (c: `estack_sfile` + `", line %ld"`,
ex_eval.c:482-486 / 599-607), snapshotted at the raise (`:throw` *and* an
error-turned-exception) and published by the `:catch`, with `:try`/`:endtry`
saving and restoring it exactly as they already did for `v:exception`.

`v:throwpoint` needs its own thread-local (`V_THROWPOINT`) for the same reason
`v:exception` has one: `install()` runs `evalvars_init()` on every VM, and a user
function body runs on a nested VM, so a value living only in the `vimvars` table
is wiped the first time the `:catch` calls anything.

RUST-PORT NOTE: vim's chain begins `command line..` because the harness launches
it with `-c 'source …'`. This interpreter is handed a script path, so it has no
such entry and its chain starts at the script. Everything after that is
byte-identical. `tests/parity_cases/throwpoint.vim` compares the value with the
directory stripped, which drops both that prefix and the absolute path
(un-diffable between machines anyway) and keeps the frame chain, the per-frame
entry lines and the raising line. Its `KNOWN_OPEN` entry in
`tests/parity_cases.rs` is removed — the list is now empty.

### R21-5. `Vim(cmd):` was wrong for `:if` / `:while` / `:for` — ✅ FIXED
Found while wiring R21-4, which reads the line from the same per-statement
marker. `Compiler::stmt_cmdname` named only the leaf commands, so an error in a
block opener's condition was tagged with whatever the *previous* statement had
set: `try | if [][0] | endif | catch` reported `Vim:E684` where vim reports
`Vim(if):E684`, and `:while`/`:for` reported `Vim(echo)`. `:silent` is a modifier
and now looks through to the command it modifies (`silent echo [][0]` is
`Vim(echo)`, verified), and each bar-separated command of a `LineGroup` sets its
own. All four verified against vim 9.2.

### R21-6. `throw {expr}` threw the value even when the expression errored — ✅ FIXED
`throw [][0]` caught a thrown `v:null`; vim raises `Vim(throw):E684: List index
out of range: 0`. c: `ex_throw` evaluates the argument with `eval0()` first and
throws only if that succeeded. `VIML_THROW` now yields when the error count rose
while its argument was evaluated, the same `ERR_MARK` test `VIML_RAISE` uses.

### R21-7. `reverse()` accepted any type — ✅ FIXED
`reverse(10)` silently returned 0; both oracles raise. c: the FIRST statement of
`f_reverse` is `tv_check_for_string_or_list_or_blob_arg` (vendor/eval/list.c:828),
which the port had dropped. Another *more permissive than vim* divergence.

The two oracles disagree on the message — vim 9.2 `E1253: String, List, Tuple or
Blob required for argument 1`, neovim `E1252: String, List or Blob …` — and this
follows vim, the reference `scripts/parity.sh` drives. The ported
`tv_check_for_string_or_list_or_blob_arg` in `typval.rs` keeps neovim's wording
and is left alone. Recorded as `tests/parity_cases/reverse_argcheck.vim`.

**How it was found, and what that says about the fuzzer.** It sat in
`fuzz-parity`'s `Divergent` bucket — the advisory one, for "vim and neovim
disagree, vimlrs matches one of them". vim and neovim *did* disagree (E1253 vs
E1252), which is what put it there, but vimlrs matched NEITHER. Over ~9,500
fuzzed expressions across four fresh seeds, 179 divergent cases had vimlrs
agreeing with vim, 0 with neovim, and **36 with neither** — so the bucket is not
purely advisory and has to be read case by case, never skipped because an oracle
split is expected. The triage rule as written cannot separate the two; that is a
gap in the fuzzer's reporting, not in its findings. Closed by R22-1.

## Round 22 — the `Divergent` bucket re-read, and the six gaps it named

Every fix below is byte-diffed against vim 9.2 by `scripts/parity.sh`, and each
has a case in `tests/parity_cases/` whose `.expected` is vim's own output.

### R22-1. The fuzzer could not tell "oracles disagree" from "vimlrs is wrong" — ✅ FIXED

R21-7 closed `reverse(10)` but left the reporting gap that hid it: `Divergent`
lumped "vimlrs matches vim, neovim differs" (advisory) together with "vimlrs
matches NEITHER" (a bug). `classify()` now separates them and `Class::Neither`
is reported under its own actionable heading, counted separately, and included
in the non-zero exit alongside `GAPS`/`PANICS`:

```text
  NEITHER:     2 (2 distinct)
  divergent:   78 (advisory — vimlrs matches vim)
```

Two references that disagree still *bracket* the answer; a third result outside
that bracket is vimlrs's own, whatever the oracles are doing. This is the
standing check R21-7 asked for — it costs nothing to keep and cannot be skipped.

**What the re-read found.** Over six seeds x 2,500 expressions, the `Divergent`
bucket resolved to (a) `v:none .. -2` -> R22-5, (b) `list2str([-1,0,1])` ->
R22-6, (c) `id()`, which is not a finding at all (R22-11), and (d) two classes
that are not bugs: a vim-only builtin vimlrs has not ported (R22-O3) and a
neovim builtin vim lacks (`msgpackdump`), where being a superset of both means
matching neither. After the fixes the bucket is 1-2 cases per seed, all named
below.

### R22-2. `typename()` of a builtin Funcref / lambda was `func(...): any` — ✅ FIXED (was R21-O5)

`scripts/gen_builtin_signatures.sh` drives vim once over the builtin names vim
itself reports (`getcompletion('', 'function')`) and records
`typename(function(name))` verbatim into the generated
`src/ported/eval/builtin_signatures.rs` — 588 signatures. No argument-type table
exists or is needed: vim prints every builtin argument as `[unknown]`, so the
only per-builtin facts are the argument count and the return type, and a
recorded string cannot be wrong about what vim prints.

`type_name_of()` then reproduces vim's whole rule set, each row measured:

| value | vim, and now vimlrs |
|---|---|
| `function('strlen')` | `func([unknown]): number` |
| `function('add')` | `func([unknown], [unknown])` |
| `function('argv')` | `func(?[unknown], ?[unknown]): list<string>` |
| `function('function')` | `func([unknown], ?[unknown], ?[unknown]): func(...): unknown` |
| `function('strlen', {})` | `func` |
| `function('UF')`, `function('UF', [1])` | `func(...): any` |
| `{-> 1}` | `func(...): [unknown]` |
| `{x -> x}` / `{x, y -> x}` | `func(any): [unknown]` / `func(any, any): [unknown]` |
| `function({x -> x}, [1])` | `func(): [unknown]` |
| `function({x, y -> x}, [1])` | `func(any): [unknown]` |
| `function({x, y -> x}, [1,2,3])` | `func(...): [unknown]` |

A partial over a builtin is the bare `func` because vim's `partial_T.pt_func` is
NULL for one, so there is no `ufunc_T` to read a type from. Lambdas store
nothing per-function: the shape is the declared parameter count `d` minus what a
partial bound `k`, with `d == 0` or `k > d` rendering `...`. One shape was left
open at the time — R22-O1, a zero-parameter lambda that captures — and is now
closed too.

`typename` is Vim-only, so it is absent from the neovim-derived `funcs_argc.rs`
and `function('typename')` raised `E700: Unknown function: typename`. The
existing `EXTRA_BUILTIN_ARGC` supplement already carried it;
`translated_function_exists()` was reading the GENERATED table directly instead
of `builtin_argc_range()`, the one place that merges the two. Generated tables
stay untouched; the supplement is the only place a Vim-only name is written.

`tests/parity_cases/typename_funcref.vim`.

### R22-3. `:unlet` on a missing variable was silently accepted — ✅ FIXED (was R21-O1)

`vim(unlet):E108: No such variable: "g:nope"`. The parser stripped the `!` and
threw it away, so `Stmt::Unlet` could not tell the two forms apart and `b_unlet`
passed `forceit: true` unconditionally. The bang is now carried on the statement
and reaches `do_unlet`, whose tail is the C's verbatim (`vendor/eval/vars.c`):

```c
  if (forceit) { return OK; }
  semsg(_("E108: No such variable: \"%s\""), name);   // vars.c:1772
  return FAIL;
```

`:unlet!` stays silent; the message carries the name as written; a failing name
mid-list aborts the rest exactly as vim does.
`tests/parity_cases/unlet_e108.vim`.

### R22-4. `function('F')` before `F` was defined was accepted — ✅ FIXED (was R21-O2)

vim reads the whole script but EXECUTES it line by line, and `:function` is an
ordinary command: `F` does not exist until its `:function` line has run, so
`let F = function('Later')` above `:function Later()` is
`Vim(let):E700: Unknown function: Later`.

`compile_program` hoisted every script-level definition into
`CompiledProgram::funcs`, which registers at load. It now leaves them in the
statement stream, where the register-on-reach path built for block-level
`:function` already existed (`deferred_funcs` + `VIML_DEFINE_FUNC`). Nothing new
was written — the correct machinery was there and script level was the one case
routed around it. `deferred_funcs` now means "every named definition" and
`funcs` means "bodies with no `:function` line to reach" (lambdas, `d.key()`);
`CompiledProgram::all_funcs()` is for callers that just want to inspect.
`tests/parity_cases/function_forward.vim`.

### R22-5. `v:none` stringified as `v:null`, and compared equal to it — ✅ FIXED (was R21-O3)

One table, as the C has it. `tv_get_string_buf_chk` had `"v:null"` written into
its `VAR_SPECIAL` arm and `encode_vim_to_string` open-coded the two names, so
the two could disagree — and did. Both now index the ported
`encode_special_var_names[]` (`vendor/eval/encode.c:41`), a table and not a
function, because both readers INDEX it.

That one change also settles the COMPARISON, which is why this was left whole
rather than half-done: neither operand of `v:null == v:none` is a
Blob/List/Dict/Funcref/Float/Number, so `typval_compare` falls through to its
string branch and compares exactly these two names. `v:null != v:none` is 1 and
`repeat(v:none, 3)` is `'v:nonev:nonev:none'` because the names now differ.
`tests/parity_cases/none_special.vim`.

### R22-6. `list2str()` stopped at a 0 instead of skipping it — ✅ FIXED (was R21-O4)

c: `buf[utf_char2bytes((int)n, buf)] = NUL; ga_concat(&ga, buf);` — a 0 encodes
to a single NUL byte, which the STRLEN-based `ga_concat` measures as length 0.
The item contributes nothing and **the walk continues**. The port `break`ed,
dropping every element after a 0: `list2str([65, 0, 66])` was `'A'` where vim
gives `'AB'` (verified byte-for-byte via `writefile(..., 'b')`).

**The out-of-range half is now closed too — see R23-1**, which replaced the
string model this entry was blocked on. `list2str([-1,0,1])` is the two bytes
`ff 01` in vim and now here.
`tests/parity_cases/list2str_nul.vim`, `tests/parity_cases/list2str_bytes.vim`.

### R22-7. `eval()` of a lambda literal raised E700 — ✅ FIXED (was R21-O6)

`compile_program` returns a lambda's body as a SEPARATE `<lambda>N` chunk
alongside `main`. `b_eval` and `compile_expr_chunk` both took `.main` and dropped
the rest, so the Funcref `main` produced named a body nobody had registered.
Both now register them the way the script loader and `run_source_nested` already
did. This is not only `eval()`: `map(l, '{a, b -> b * 10}(0, v:val)')` went
through the same drop (proved by reverting the one line — `E700: Unknown
function: <lambda>1`). `tests/parity_cases/eval_lambda.vim`.

### R22-8. Nested compiles re-used `<lambda>1` and clobbered the outer one — ✅ FIXED

Found by R22-7, which exposed it: registering the nested lambda is what made the
collision reachable.

```vim
let A = {x -> x * 2}
let B = eval('{x -> x + 100}')
echo A(5)      " 105 here, 10 in vim
```

`compile_program_inner` reset `LAMBDA_COUNTER` to 0 on EVERY compile, so every
nested compile (`eval()`, an expression string handed to `map()`, `:execute`)
restarted at `<lambda>1` and registered over the outer script's. c: `lambda_no`
is a `static int` inside `get_lambda_name()` (userfunc.c:271) that is only ever
incremented — one counter for the life of the process. The reset is gone. A
side effect is that the generated names now line up with vim's for the cases in
`eval_lambda.vim`.

### R22-9. `scripts/parity.sh` truncated vim's output at the first non-UTF-8 byte — ✅ FIXED

`norm()` piped vim through `tr -d '\r'`. In a UTF-8 locale macOS `tr` aborts
with "Illegal byte sequence" on a byte that is not valid UTF-8 and TRUNCATES the
rest of the stream — and vim writes such bytes for real (`list2str([-1])` is the
single byte `0xff`). Any case whose output contained one would have had its
`.expected` silently recorded SHORT, and the harness would then have "passed" it
against a truncated record. Found while pinning R22-6, whose expected output is
exactly such a byte. The CR strip is now folded into the existing perl pass,
which is byte-transparent without `use utf8`.

### R22-10. The fuzzer allow-listed `id()`, which can never match — ✅ FIXED

`id()` returns the address of the value's heap object: `'0x9ab06df80'` in nvim,
`'000c4ce17'` in vim, a third pointer in vimlrs, different on every run. It was
in `FUNCS`, whose own rule is that every entry is "pure, deterministic … an
impure builtin would report a false gap on every run" — which is precisely what
it did, 4-5 permanent false findings per seed, all landing in the bucket R22-1
had just made actionable. Removed. No outcome it can produce carries signal.

## Left open by round 22

Both were closed in round 23; R22-O3 is still open, with a different reason than
the one recorded here.

### R22-O1. `typename()` of a zero-parameter lambda that captures — ✅ FIXED

`{-> a}` printed `func(): [unknown]` where vim prints `func(...): [unknown]`.

vim renders a lambda from its DECLARED parameter count `d` and the count `k` a
Partial bound: `d == 0` or `k > d` prints `...`, else `d - k` `any`s. vim keeps a
closure's captures in the funccal chain and out of `uf_args`, so `{-> a}` is
(d 0, k 0). This port desugars each capture into a leading parameter that the
lambda's own Partial pre-binds, making it (1, 1) — numerically identical to
`function({x -> x}, [1])`, which vim answers differently. No rule over the two
numbers can separate them; it needed the capture count, which is now recorded.

`UserFuncDef.captures` counts the leading parameters the `Expr::Lambda`
desugaring synthesized, and it reaches the ported reader as
`ufunc_T.uf_captures` (marked NO C COUNTERPART, because in C the situation cannot
arise). `type_name_of` subtracts it from BOTH `d` and `k` — the desugaring
inflates both, which is exactly why the four capturing shapes that bind an
argument already matched by accident and the two zero-parameter ones did not.

All twelve shapes, read out of vim 9.2 and now matched:

| lambda | vim | before |
|---|---|---|
| `{-> a}` | `func(...): [unknown]` | `func(): [unknown]` |
| `{-> a + b}` | `func(...): [unknown]` | `func(): [unknown]` |
| `{x -> x + a}` | `func(any): [unknown]` | matched |
| `{x, y -> x + y + a}` | `func(any, any): [unknown]` | matched |
| `{-> 1}` | `func(...): [unknown]` | matched |
| `{x -> x}` | `func(any): [unknown]` | matched |
| `{x, y -> x}` | `func(any, any): [unknown]` | matched |
| `function({x -> x+a}, [1])` | `func(): [unknown]` | matched |
| `function({x -> x}, [1])` | `func(): [unknown]` | matched |
| `function({x,y -> x+y+a}, [1])` | `func(any): [unknown]` | matched |
| `function({x,y -> x}, [1])` | `func(any): [unknown]` | matched |
| `function({-> a}, [1])` | `func(...): [unknown]` | matched |

`tests/parity_cases/typename_lambda_capture.vim` now carries the whole matrix
instead of the single open shape, and its `KNOWN_OPEN` entry is gone —
`parity_cases.rs` reported the entry as stale on the first run after the fix,
which is what that check is for. `KNOWN_OPEN` is now empty and the corpus is
26/26 byte-identical to vim.

### R22-O2. `E684` omits the index, and a negative index should not raise at all — ✅ FIXED

Both named halves, in one place. `b_setindex` (`:let l[i] = v`) and
`b_unlet_index` (`:unlet l[i]`) each open-coded the index resolution as
`if i < 0 { i += len }` + a range test + a bare `emsg`. The C does neither: both
reach the same `get_lval` list arm (`vendor/eval.c:978-984`), which resolves the
index with the already-ported `tv_list_check_range_index_one` ->
`tv_list_find_index` (`vendor/eval/typval.c:1716`):

```c
  listitem_T *li = tv_list_find(l, *idx);
  if (li != NULL) { return li; }
  if (*idx < 0) { *idx = 0; li = tv_list_find(l, *idx); }
  return li;
```

That helper was correct and correctly reported the index the whole time — the
two call sites simply routed around it. They now call it, which fixes the
message and the negative index together, and deletes the duplicate arithmetic
rather than adding anything.

Reading an index (`echo l[9]`) goes through `eval_index`, which has NO such
clamp, so a negative out-of-range index IS an error there. vimlrs already
matched that and still does; the asymmetry is real and is what the case pins.

| expression | vim & nvim | vimlrs before | after |
|---|---|---|---|
| `echo l[9]` | `E684: List index out of range: 9` | same | same |
| `echo l[-9]` | `E684: List index out of range: -9` | same | same |
| `let l[9] = 1` | `E684: List index out of range: 9` | `E684: List index out of range` | matches |
| `let l[-9] = 99` on `[1,2,3]` | no error, `[99, 2, 3]` | `E684` | matches |
| `unlet l[9]` | `E684: List index out of range: 9` | `E684: List index out of range` | matches |
| `unlet l[-9]` on `[1,2,3]` | no error, `[2, 3]` | `E684` | matches |

`tests/parity_cases/list_index_e684.vim`.

Two things found while pinning it are NOT fixed and are recorded below as
R23-O1 (the oracles disagree about the empty-list clamp) and R23-O2 (a `|`-form
`:unlet` inside `:try`).

### R22-O3. Vim-only builtins vimlrs has not ported

`str2blob()` and `js_encode()` reach the fuzzer's `NEITHER` bucket as
`E117: Unknown function` against vim's `E119: Not enough arguments`. Not a
semantics bug — the functions are simply absent. Recorded so the bucket's
remaining contents are accounted for rather than re-triaged every wave.

**`str2blob` is no longer blocked on the string model** (R23-1 replaced it —
`str2blob([list2str([-1,0,1])])` is `0zFF01` in vim, which this port can now
represent). It is blocked on something else, and so are `blob2str`/`js_encode`:
`grep -rn 'str2blob\|blob2str\|js_encode\|js_decode' vendor/` returns nothing.
These are Vim-only, absent from the vendored Neovim source, so there is no C to
port and the porting rule ("the C source is the spec") has nothing to read.
Writing them from observed behaviour would be an ad-hoc reimplementation, which
is exactly what `src/ported/` exists to prevent — and it would also add
ported-name-gate violations, since the gate resolves names against `vendor/`.
(The same is true of `f_typename`/`type_name_of`/`member_of`, which is why those
are the gate's 3 standing violations.)

What it needs, in order: Vim's own source vendored alongside Neovim's, and a
decision about where a Vim-only builtin lives given that `src/ported/` is
defined as "strict 1:1 ports of the Neovim eval C source". Neither is a guess
that can be bolted on here.

Measured contract, for whoever does vendor it:

| expression | vim |
|---|---|
| `str2blob(["ab"])` | `0z6162` |
| `str2blob(["ab","cd"])` | `0z61620A6364` (items joined with NL) |
| `str2blob([])` / `str2blob([""])` | `0z` |
| `str2blob([list2str([-1,0,1])])` | `0zFF01` |
| `blob2str(0z6162)` | `['ab']` |
| `blob2str(0z61620a6364)` | `['ab', 'cd']` |
| `blob2str(0zff)` | `E1515: Unable to convert from 'utf-8' encoding`, then `[]` |
| `js_encode(v:none)` | `''` (empty) |
| `js_encode(v:null)` | `null` |
| `js_encode([1,v:none,3])` | `[1,,3]` |
| `js_encode({'a':1,'b':'s'})` | `{a:1,b:"s"}` (identifier keys unquoted) |
| `js_encode({'a b':1})` | `{"a b":1}` (non-identifier key quoted) |
| `js_encode(function('strlen'))` | `E1161: Cannot json encode a func` |


---

# Round 23 — the string model

Round 23 replaced the string model, which unblocked R21-O4/R22-6, and closed the
two items round 22 left open that were about semantics rather than about a
missing source (R22-O1, R22-O2). `tests/parity_cases` is 26/26 byte-identical to
vim with an EMPTY `KNOWN_OPEN`.

## R23-1. A VimL string is bytes, not UTF-8 — ✅ FIXED (unblocks R21-O4 / R22-6)

`typval_vval_union::v_string` held a Rust `String`. A Vim string is `char_u *` —
a byte array with no encoding invariant — and Vim writes bytes into one that are
not valid UTF-8 as a matter of routine:

```c
utf_char2bytes(c, buf)                          // Src/mbyte.c:1076
    if (c < 0x80) { buf[0] = (char_u)c; return 1; }
```

`c` is a signed `int`, so `list2str([-1])` takes that arm and stores
`(char_u)-1` == `0xff`. There is no range check above `U+10FFFF` either, so
`list2str([0x110000])` is `f4 90 80 80`. A Rust `String` cannot hold either, so
those items were dropped on the floor — the concrete symptom R22-6 recorded as
blocked.

`v_string` now holds `vimstr::VimStr`, a `Vec<u8>`. The type lives in the
synthesis zone, not under `src/ported/`, because it has no C counterpart (the C
just dereferences a pointer) — so the ported-name gate still reports only its 3
pre-existing violations, with no allowlist entry added.

Measured with `writefile(..., 'b')` + `xxd -p`:

| expression | vim | nvim | vimlrs before | after |
|---|---|---|---|---|
| `list2str([-1,0,1])` | `ff01` | `ff` | `01` | `ff01` |
| `list2str([0x110000])` | `f4908080` | `f4908080` | *(empty)* | `f4908080` |
| `list2str([-1])` | `ff` | `ff` | *(empty)* | `ff` |
| `list2str([200,300,255])` | `c388c4acc3bf` | same | `c388c4acc3bf` | `c388c4acc3bf` |

(vim and nvim disagree on the first: Vim's `ga_concat` is STRLEN-based so the NUL
contributes nothing and the walk continues, while nvim's `ga_concat_len` appends
it and the C string ends there. This port follows vim, as R22-6 decided.)

Carrying the bytes to an observer took five further places, each measured:

| path | before | after |
|---|---|---|
| `fusevm::Value::Str` (an `Arc<String>`) | could not carry them | a non-UTF-8 string rides the REFPOOL handle Lists/Dicts/Blobs already use |
| `writefile()` | wrote `ef bf bd` per byte | writes the bytes |
| `strlen(list2str([-1,0,1]))` | `4` | `2`, as vim |
| `str2list(list2str([-1,0,1]))` | `[65533, 1]` | `[255, 1]`, as vim and nvim |
| `.` concat, the `:echo` sink | lost the byte to `U+FFFD` | byte paths |

`tv_get_string_buf_chk` is the byte-exact accessor (the C returns `char *`). The
three `String`-returning wrappers over it stay as the *text* read for the several
hundred text-shaped call sites; their doc says so, and says which sites must not
use them. That split is the deliberate boundary of this change, not an oversight:
converting all ~400 to bytes would force a `to_string_lossy()` at nearly every
one for no behaviour change, while the sites where bytes are observable are the
named few above.

`tests/parity_cases/list2str_bytes.vim`.

## Still open

### R23-O1. vim and nvim disagree on the index reported for an empty-list clamp

Found while pinning R22-O2.

| expression | vim | nvim | vimlrs |
|---|---|---|---|
| `let e[-9] = 1` on `[]` | `E684: List index out of range: -9` | `E684: … : 0` | `… : 0` |

`tv_list_find_index` (`vendor/eval/typval.c:1716`) writes `0` into `*idx` before
its second lookup, and `tv_list_check_range_index_one` then reports whatever is
left in `*idx` — so `0` is what the vendored C produces, and nvim agrees with it.
vim reports the original `-9`, which means Vim's own source (not vendored here)
does something the Neovim source does not.

Every non-empty case matches both oracles; this is only the list with no item at
index 0 for the clamp to land on. Following vim would mean writing a rule the
vendored C does not contain, so it is recorded rather than guessed at. It is
excluded from `tests/parity_cases/list_index_e684.vim` with this reason in the
case file.

### R23-O2. A `|`-separated `:unlet` inside `:try` should not be caught

Found while pinning R22-O2. Both oracles agree, so this is a real gap.

```vim
let b = [1,2,3]
try | unlet b[9] | catch | echo 'UNLET-BAR:' v:exception | endtry
echo 'still here'
```

vim and nvim: the `E684` escapes the bar-form `:try` and ABORTS the script —
neither `UNLET-BAR:` nor `still here` is printed. vimlrs catches it and carries
on. The same line with `:let` instead of `:unlet` IS caught by all three, so this
is specific to `:unlet` in the `|` form, not to `:try` or to E684.

The multi-line `try` / `catch` / `endtry` form matches all three exactly,
including which command tags the exception (`Vim(unlet):`), so the divergence is
in how a `|`-separated `:unlet` joins the enclosing `:try`, not in `:unlet`
itself. Not investigated further; recorded with the repro.

### R23-O3. `:echo` does not escape unprintable bytes the way vim's message layer does

Pre-existing (measured before the R23-1 work: `echo nr2char(1)` wrote a raw
`0x01` then and now), and now the only thing between vimlrs and vim on a byte
string.

| expression | vim | vimlrs |
|---|---|---|
| `echo nr2char(1)` | `^A` | raw `0x01` |
| `echo "a\x01b"` | `a^Ab` | `a` `0x01` `b` |
| `echo list2str([-1,0,1])` | `<ff>^A` | raw `ff 01` |

Vim renders a message through `msg_outtrans`/`transchar`, which shows a control
character as `^X` and a byte that is not part of a valid character as `<ff>`.
This port writes the bytes. Every *value*-level answer about such a string now
matches vim (`len`, `strlen`, `str2list`, `==#`, `.`, `writefile`) — see R23-1 —
so what is left is exactly the display transform, which is its own port
(`transchar` is not ported) and its own pass.

It is why `tests/parity_cases/list2str_bytes.vim` pins the byte model through
`str2list()`/`len()` rather than through `:echo` of the raw bytes: that would
make the case about message escaping instead of about the value.
