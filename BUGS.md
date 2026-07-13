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

### 8. String index/slice is char-based; Vim is byte-based
- `'héllo'[1]` → Vim `<c3>` (first byte of the 2-byte `é`), vimlrs `é` (whole char)
- Vim indexes strings by byte. ASCII matches (`'hello'[1]` → both `e`); only multibyte diverges.

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

## Still open

- The remaining regex gaps (26 distinct) are mostly deeply-nested quantifier and
  backtracking corners, plus a few where Vim rejects a pattern vimlrs still accepts.
- `nr2char(2147483647)` → Vim emits the raw replacement bytes, vimlrs `''`. Same
  string-representation root cause as R5-D1 (Vim strings are byte arrays), which
  remains the one structural divergence.
- `matchbufline(-9223372036854775808, …)` → Vim E475, vimlrs E158. The C truncates
  the buffer number to `int`, and what that yields for INT64_MIN is undefined
  behavior; not worth reproducing.
- `execute()` of a command that errors captures the error text in Vim but not in
  Neovim. vimlrs follows Neovim, its port target — no single spec.
