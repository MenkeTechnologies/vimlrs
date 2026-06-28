# vimlrs — known parity bugs vs Vim

Goal: behavioral parity with real Vim's `:echo` / expression semantics. Each entry
below is a **reproduced divergence** between `vimlrs` and **Vim 9.2**.

Repro helpers:

```sh
V=./target/debug/vimlrs
vimref() { vim -es -u NONE -i NONE -c 'redir! > /tmp/vr.txt' \
  -c "silent! echo $1" -c 'redir END' -c 'qa!' >/dev/null 2>&1; sed '1{/^$/d;}' /tmp/vr.txt; }
# usage: vimref "'abc' ==? 'ABC'"   ;   $V -e "'abc' ==? 'ABC'"
```

(Note: `vimlrs -e` mis-parses an expression that *starts* with `-` as a CLI flag,
e.g. `vimlrs -e '-3/2'`; use `-c 'echo -3/2'` instead. That is a CLI-parsing quirk,
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

### 7. `string()` of a Float diverges in exponent format, precision, and exp threshold
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

### 9. Spurious fallback value printed after a runtime error
- `echo printf('%d',3.7)` → Vim prints only `E805: Using a Float as a Number`; vimlrs prints the error **and then** `-1`
- `echo [1,2,3][10]` → Vim prints only `E684: List index out of range: 10`; vimlrs prints the error **and then** `v:null`
- On error vimlrs still emits a fallback result value, so erroring expressions produce
  extra output Vim never produces.

### 10. Float literals without a dot are accepted (lexer too lenient) — minor
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

### R2-2. Very-magic mode `\v` is entirely unsupported
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

### R2-4. `\%[...]` optional-sequence atom unsupported
- `matchstr("function","f\%[unc]")` → Vim `func`, vimlrs `` (empty)

### R2-5. `printf("%g"/"%G", …)` formatting diverges
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

### R2-9. `execute()` puts the newline at the wrong end
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

### R2-12. `string(v:none)` returns `v:null`
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
