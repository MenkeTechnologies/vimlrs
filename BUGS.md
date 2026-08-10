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
  (`eval_index_inner`, eval.c) in both the bridge and the ported eval.
  `slice()` stays character-indexed with composing clusters, per its own C path
  (`string_slice`).
- CORRECTION (round 30): this entry used to claim a byte slice that splits a
  character "carries U+FFFD where Vim carries the raw byte (**both render
  identically**)". They do not render identically, and the claim was never
  checked with `xxd`:
  ```
  $ printf "echo 'héllo'[1]\n" > b8.vim
  $ viml b8.vim | xxd                          → 00000000: efbf bd0a   ....
  $ vim -es -u NONE -i NONE -N -c 'verbose source b8.vim' -c 'qa!' | xxd
                                               → 00000000: 3c63 333e   <c3>
  ```
  vim writes the raw byte through `transchar_byte_buf`, which renders it as the
  four ASCII characters `<c3>`; this port substitutes U+FFFD. Still open —
  R30-O2. `tests/parity_cases.rs:20-25` warns about exactly this shape (two
  different byte strings comparing EQUAL once both collapse to U+FFFD), which is
  why that harness compares bytes and not `String`s.

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


---

# Round 24 — the display transform

Round 23 closed every *value*-level question about a byte string and left one
thing between vimlrs and vim on one: `:echo` wrote the bytes instead of rendering
them. Round 24 ports that transform from the C, which also closed R23-O2 and
turned up three defects the transform had been masking. `tests/parity_cases` is
**29/29** byte-identical to vim with an EMPTY `KNOWN_OPEN`.

The C for it was not vendored. `charset.c` and `message.c` are now, from Neovim
master at `982d2f253168be9dbfecd0c1a41ec9cff8017a4e` — the commit whose
`src/nvim/eval.c` is byte-identical to the `vendor/eval.c` already here, so the
caller (`ex_echo`) and the callee (`msg_multiline`) come from one revision. That
widens the name gate's legal set by 228 names (3756 → 3984); none of them is one
of the gate's 3 standing violations, and the gate still reports exactly those
three with no allowlist entry added.

## R24-1. `:echo` did not escape unprintable bytes — ✅ FIXED (was R23-O3)

`ex_echo` does not print the string it built. It calls
`msg_multiline(cstr_as_string(tofree), …)` (`vendor/eval.c:6181`), which chunks
on `\n`/TAB/`\r`/BELL and hands each run to `msg_outtrans_len`
(`vendor/message.c:1866`), which replaces every character the terminal cannot
show with its `transchar` text (`vendor/charset.c:541`).

Three different answers come out of that, and telling them apart is the whole
job — a single "escape the unprintable bytes" rule gets all three wrong:

| input | what it is | vim + nvim | vimlrs before |
|---|---|---|---|
| `echo nr2char(1)` | a control BYTE | `^A` | raw `01` |
| `echo list2str([-1,0,1])` | an ILLEGAL UTF-8 byte | `<ff>^A` | raw `ff 01` |
| `echo list2str([0x80])` | an unprintable CHARACTER (`c2 80`) | `<80>` | raw `c2 80` |
| `echo nr2char(0x200b)` | ditto, above 0xFF | `<200b>` | raw `e2 80 8b` |
| `echo list2str([0x110000])` | a PRINTABLE character | raw `f4 90 80 80` | raw `f4 90 80 80` |
| `echo "é"` | a printable character | `c3 a9` | `c3 a9` |
| `echo "a\tb"` | a msg_multiline delimiter | literal TAB | literal TAB |

The split that produces them: a byte that starts no valid UTF-8 sequence goes
through `transchar_byte_buf`, whose `c >= 0x80` arm goes straight to
`transchar_hex` — so `<ff>`, without ever asking whether the *character* 0xff is
printable. A decoded character goes through `transchar_buf`, which asks
`vim_isprintc` — `g_chartab[]` below 0x100, `utf_printable()` above it. All nine
of `utf_printable`'s unprintable intervals sit below 0x10000, which is why
U+110000 is printable and echoes as its four raw bytes.

Ported: `vim_isprintc`, `byte2cells`, `transchar_hex`, `transchar_nonprint`,
`transchar_buf`, `transchar_byte_buf`, `transstr_buf`, `transstr` and the
`g_chartab[]` build (`src/ported/charset.rs`); `utf_printable`
(`src/ported/mbyte.rs`); `msg_outtrans_len`, `msg_outtrans`, `msg_multiline`
(`src/ported/message.rs`); the `:echo`/`:echon`/`:echomsg` sink calls
`msg_multiline` per argument (`src/fusevm_bridge.rs`).

Two deliberate omissions, both documented at the port:

- `msg_outtrans_len` does not return the C's screen-CELL count. There is no
  consumer — this port's sink is a byte stream, and `MSG_COL` is a boolean
  "line is dirty" flag, not a column — and producing it needs `utf_char2cells`,
  whose width data is utf8proc property tables that are not vendored.
- `transchar_buf`'s opening `IS_SPECIAL(c)` arm is not ported. No caller can
  reach it (`c` comes from `utf_ptr2char` or from a byte, never negative) and
  its `K_SECOND` resolves against `K_SPECIAL`/`KS_ZERO`, which this port has no
  producer for. A guessed stand-in would be a wrong answer behind an unreachable
  branch — and `k_second` was in fact caught by the name gate as a fn with no C
  origin, which is what the gate is for.

`tests/parity_cases/echo_transchar.vim`.

### R24-1a. `strtrans()` was an approximation, and read its argument as text

`f_strtrans` is two lines of C — `transstr(tv_get_string(&argvars[0]), true)`
(`Src/nvim/strings.c:2960`) — but was a hand-written `.chars()` loop over
`^X` and `^?`, on a `String`. Both halves were wrong, and the second is the
`tv_get_string` hazard R23-1 recorded: `strtrans()` exists to name bytes a
terminal cannot show, so reading it as text first destroys the input.

| expression | vim + nvim | vimlrs before |
|---|---|---|
| `strtrans(list2str([-1]))` | `<ff>` | `ef bf bd` (`U+FFFD`) |
| `strtrans(nr2char(0x110000))` | `f4 90 80 80` | `U+FFFD` ×4 |
| `strtrans(nr2char(0x200b))` | `<200b>` | raw `e2 80 8b` |
| `strtrans(list2str([0x80]))` | `<80>` | raw `c2 80` |

It now reads `tv_get_string_buf_chk` and calls the ported `transstr`.

### R24-1b. `substitute()`'s `\n` inserted a NUL — ✅ FIXED

Unmasked by R24-1a: the old `strtrans` mapped 0x00 and 0x0a to the same `^@`, so
`examples/substitute_edge.vim` passed over a wrong value. `vim_regsub_both`
(`Src/nvim/regexp.c:2300`) is explicit — `case 'n': c = NL;` — and both engines
agree:

| expression | vim + nvim | vimlrs before |
|---|---|---|
| `str2list(substitute('x','x','a\nb',''))` | `[97, 10, 98]` | `[97, 0, 98]` |
| `str2list(substitute('x','x','a\bb',''))` | `[97, 8, 98]` | `[97, 98, 98]` (`\b` unhandled) |

The "`\n` in a `:s` replacement inserts a NUL" line that the old comment cited is
about `:s` in a BUFFER, where a NL byte in stored line text stands for a NUL. It
is the storage convention, not this expansion. What made it look like a NUL in a
string is that `strtrans()` *displays* 0x0a as `^@` (`transchar_nonprint`: "we
use newline in place of a NUL").

### R24-1c. `msgpackdump()` returned `U+FFFD` where the bytes were — ✅ FIXED

Also the `tv_get_string` hazard, one layer along: the byte stream was split into
lines with `String::from_utf8_lossy`. MessagePack is binary —
`msgpackdump([v:true,v:false,v:null])` is `c3 c2 c0`, none of it valid UTF-8 —
so every byte became `U+FFFD`.

| expression | nvim (vim has no `msgpackdump`) | vimlrs before |
|---|---|---|
| `let d.a = msgpackdump([v:true,v:false,v:null]) \| echo d` | `{'a': ['<c3><c2><c0>']}` | `{'a': ['<fffd>×3']}` |

The split is now on the byte `\n`, which cannot occur inside a UTF-8 sequence,
so it is the same split. This moved a 43-instance entry out of the fuzzer's
`NEITHER` bucket.

## R24-2. A `|`-separated `:unlet` inside `:try` was caught — ✅ FIXED (was R23-O2)

The mechanism is in the vendored C, and it is not about `:unlet` being special —
it is about where the command left the parse cursor. `ex_unletlock`
(`vendor/eval/vars.c`) resolves an ELEMENT lval with `get_lval()`; when that
fails the loop `break`s, and the tail

```c
  eap->nextcmd = check_nextcmd(arg);
```

runs with `arg` still on the UNCONSUMED name — so `check_nextcmd` finds no `|`
and answers NULL. With no next command, the `| catch | … | endtry` that follows
on the line is never executed and the exception escapes the one-line `:try`.

The plain-NAME form is the opposite, and the contrast is the proof: its E108
comes from the `callback` (`do_unlet`), which only sets `error = true`; the loop
runs on, `arg` reaches the `|`, and the same-line `:catch` DOES run.

| one-line form | vim + nvim | vimlrs before |
|---|---|---|
| `try \| unlet b[9] \| catch \| … \| endtry` | aborts, `E684` uncaught | caught |
| `try \| unlet nosuchvar \| catch \| … \| endtry` | caught | caught |
| `try \| let b[9] = 1 \| catch \| … \| endtry` | caught | caught |
| multi-line `try` / `unlet b[9]` / `catch` | caught | caught |

vimlrs already had the machinery: `VIML_EXC_IS_HARD` is exactly "this error
abandoned the command line, so an inline `:catch` must not see it". The fix is
that `b_unlet_index` now runs under `eval_op`, the existing helper that marks any
error raised inside it as hard. `b_unlet` (the name form) deliberately does not.

`tests/parity_cases/unlet_bar_try.vim`, which pins all four rows above.

## R24-3. The parity gate compared expectations as UTF-8 — ✅ FIXED

Not a language bug; a hole in the gate that this round walked straight into.
`tests/parity_cases.rs` read `<name>.expected` with `read_to_string` and compared
with `from_utf8_lossy`. vim writes non-UTF-8 output as a matter of routine, so:

- a case whose recorded answer contains such a byte could not be read at all —
  it failed with "has no recorded vim output", naming a file that was right
  there. `echo_transchar.vim` is such a case (`f4 90 80 80`).
- worse, the comparison could pass on a difference. Verified:
  `String::from_utf8_lossy(b"\xf4\x90\x80\x80") == String::from_utf8_lossy(b"\xff\xff\xff\xff")`
  is `true` — both collapse to four `U+FFFD`.

Both sides are now handled as bytes end to end; only the failure *report* is
lossy. `scripts/parity.sh` was never affected — it has always compared with
`cmp`. This makes the gate strictly stricter; it reports more, never less.

## R24-4. A subscript attached across whitespace — ✅ FIXED

`handle_subscript()` (`vendor/eval.c:5961`) loops while

```c
  ((**arg == '[' || (**arg == '.' && rettv->v_type == VAR_DICT)
    || (**arg == '(' && (!evaluate || tv_is_func(*rettv))))
   && !ascii_iswhite(*(*arg - 1)))
  || (**arg == '-' && (*arg)[1] == '>')
```

so a `[`, a `.name` or a `(` only subscripts the expression it **abuts**; with a
space in front it starts a new expression instead. `->` is the one form the C
exempts. vimlrs had the guard for `(` (`lparen_abuts_prev`) and for `.name`
(`at_member_dot`) but not for `[`, so every spaced `[` was read as an index.

| script | vim 9.2 | vimlrs (before) |
|---|---|---|
| `echo 12345 [1,2]` | `12345 [1, 2]` | *(nothing — the whole `:echo` aborted)* |
| `let l=[1,2] \| echo l [0]` | `[1, 2] [0]` | `1` |
| `let d={'a':1}` + `echo d ['a']` | `{'a': 1} ['a']` | `1` |
| `echo 'ab' [0]` | `ab [0]` | `a` |
| `echo 1.5 [1,2]` | `1.5 [1, 2]` | *(nothing)* |

The Number/Float rows printed *nothing at all*: `12345[1,2]` is not a valid
index, the error aborted the command, and `:echo` prints nothing after an error
in its own arguments — so the divergence was a silently missing line, not a
wrong value.

Fixed with `lbracket_abuts_prev()` (`src/viml_parser.rs`), the exact counterpart
of the existing `lparen_abuts_prev()`. Covered by
`tests/parity_cases/subscript_whitespace.vim`, which pins the abutting forms,
the spaced forms and the `->` exemption.

## Still open

### R24-O1. vim shows a BELL, nvim consumes it

Found while porting `msg_multiline`. The oracles disagree, so this is the
`NEITHER` class, and the vendored C decides which side this port lands on.

| expression | vim | nvim | vimlrs before | after |
|---|---|---|---|---|
| `echo 'A' . nr2char(7) . 'B'` | `A^GB` | `AB` | `A` `0x07` `B` | `AB` |

`msg_multiline` treats BELL as a delimiter and calls `vim_beep(kOptBoFlagShell)`
in place of writing it (`vendor/message.c:296`), so the byte never reaches the
output. Vim's message layer shows `^G` instead. Porting the vendored C gives
nvim's answer; matching vim as well would mean writing a rule the vendored C does
not contain — the same call R23-O1 records for the empty-list clamp. Before this
round vimlrs matched NEITHER (it wrote the raw `0x07`), so the bucket improved
either way.

Excluded from `tests/parity_cases/echo_transchar.vim`, with this reason in the
case file.

### R24-O2. `:unlet` on a bad element reports a shorter name than vim

The abort behaviour matches after R24-2; the message text does not, for the two
errors vim raises from inside `get_lval` with the rest of the line still
unconsumed. vim embeds that remainder in the message because it is genuinely part
of the name it failed on; vimlrs splits the line on `|` at parse time, so it
never had it.

| one-line form | vim | vimlrs |
|---|---|---|
| `try \| unlet d.nokey \| catch \| … \| endtry` | `E716: Key not present in Dictionary: "nokey \| catch \| … "` | `E716: … "nokey"` |
| `try \| unlet n[0] \| catch \| … \| endtry` | `E689: Index not allowed after a number: n[0] \| catch \| … ` | `E689: Can only index a List, Dictionary or Blob` |

Both now abort the script in all three, which is the part R23-O2 was about. The
E689 row is a second, independent difference: vim and nvim use a different
message for indexing a Number than the one this port emits. Closing either needs
the un-split command line to reach the lval resolver, which is a parser change.

### R24-O3. `msgpackparse(msgpackdump(…))` does not round-trip

Found while fixing R24-1c; PRE-EXISTING and unchanged by it (the pre-change
binary fails identically).

```vim
echo msgpackparse(msgpackdump([v:true,v:false,v:null]))
```

nvim: `[v:true, v:false, v:null]`. vimlrs: `E5766: failed to parse msgpack
string`. The Blob form (`msgpackdump([…], 'B')`) is `0zC3C2C0` in both, so the
DUMP side is right and the failure is on the parse side, in how a
readfile()-style List of byte strings is turned back into a byte stream. vim has
no `msgpackdump`, so nvim is the only oracle.

### R24-O4. A leading composing char: vim draws a space first, nvim does not

The other half of the `NEITHER` class R24-O1 opened, found the same way. vim's
`msg_outtrans_len_attr()` opens with

```c
    if (enc_utf8 && utf_iscomposing(utf_ptr2char(msgstr)))
	msg_puts_attr(" ", attr);
```

(vim `message.c:1763`) — a space for the mark to sit on. The vendored
`msg_outtrans_len` (`vendor/message.c`) has no such line, so this port does not
draw it either.

```sh
$ printf 'echo nr2char(0x180b)\n' > /tmp/c.vim
$ vim  -es -u NONE -i NONE -c 'verbose source /tmp/c.vim' -c 'qa!' 2>&1 | xxd
00000000: 203c 3138 3062 3e                         <180b>
$ nvim --clean -es -c 'verbose source /tmp/c.vim' -c 'qa!' 2>&1 | xxd
00000000: 3c31 3830 623e                           <180b>
$ ./target/debug/viml /tmp/c.vim 2>&1 | xxd
00000000: 3c31 3830 623e 0a                        <180b>.
```

Same call as R24-O1: matching vim as well would mean writing a rule the vendored
C does not contain. Excluded from `tests/parity_cases/echo_transchar.vim`.

### R24-O5. `scripts/parity.sh` cannot compare a message CR

The harness strips CR from vim's stream because silent Ex mode writes CRLF (see
its header). A `:echo` of a string *containing* a CR therefore can never compare
equal even when the two engines agree, which they do:

```sh
$ printf 'echo "a" . nr2char(13) . "b"\n' > /tmp/r.vim
$ vim  -es -u NONE -i NONE -c 'verbose source /tmp/r.vim' -c 'qa!' 2>&1 | xxd
00000000: 610d 62                                  a.b
$ nvim --clean -es -c 'verbose source /tmp/r.vim' -c 'qa!' 2>&1 | xxd
00000000: 610d 62                                  a.b
$ ./target/debug/viml /tmp/r.vim 2>&1 | xxd
00000000: 610d 620a                                a.b.
```

This is a harness limit, not a language divergence: `msg_multiline` chunks on
`\r` and writes it through, and vimlrs does the same. Recorded so a future case
author does not read the normalisation as a bug. Telling a message CR from the
line-terminator CR needs a different normalisation than the one-pass byte filter
the harness uses, which is a harness change and belongs in its own review.

### R22-O3. Vim-only builtins — the C IS available now

The blocker recorded in round 22 was "there is no C to read":
`str2blob`/`blob2str`/`js_encode` are Vim-only and absent from the vendored
Neovim source. Half of that is now resolved — the C exists and was located, at
the exact patch level of the vim this repo tests against (`vim --version`:
`9.2`, "Included patches: 1-900"):

| function | vim/vim@v9.2.0900 |
|---|---|
| `f_blob2str` | `src/strings.c:1422` |
| `f_str2blob` | `src/strings.c:1567` |
| `f_js_encode` | `src/json.c:1544` |

What is still open is not availability but placement, and it is an architecture
decision rather than a porting one: `vendor/` is currently defined as Neovim's
sources and `src/ported/` as strict 1:1 ports of them, the name gate resolves
against `vendor/`, and vendoring a second upstream into the same tree changes
what a `Port of` citation means. The 13-row measured contract recorded in round
22 still stands and is unchanged.

### R23-O1. vim and nvim disagree on the empty-list clamp index — still open, re-verified

Unchanged this round, and re-measured against both engines: `let e[-9] = 1` on
`[]` is `E684: … : -9` in vim, `… : 0` in nvim, `… : 0` here. The exclusion note
in `tests/parity_cases/list_index_e684.vim` still describes it correctly.

# Round 25 — the gate that could not fail

This round set out to close R24-O3 and assess R24-O2. It closed R24-O3, and on
the way it found the reason the example corpus had never reported a failure: a
`v:errors` reset that discarded assertion results mid-script. Eight scripts were
failing in silence. That find outranks the one that surfaced it.

Versions used as oracles throughout, verbatim:

```
$ vim --version | head -1
VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 02 2026 19:00:41)
$ vim --version | grep 'Included patches'
Included patches: 1-900
$ nvim --version | head -1
NVIM v0.12.4
```

## R25-1. Every nested VM emptied `v:errors` — ✅ FIXED (the masking bug)

`install()` ran `crate::ported::eval::vars::evalvars_init()` unconditionally, and
`install()` runs for EVERY VM — including the nested ones built for `execute()`,
`assert_fails()`, `assert_beeps()` and user-function bodies. `evalvars_init()`
rebuilds `vimvars[]` from its type-zero defaults, so each of those emptied
`v:errors` mid-script.

The C calls it exactly once, from startup:

```c
void eval_init(void)      // vendor/eval.c:204
{
  evalvars_init();
  func_init();
}
```

`eval_init()` was an empty stub in this port (`src/ported/eval.rs`) while the
thing it exists to sequence was being called per-VM. Measured:

| step | nvim 0.12.4 | vimlrs before |
|---|---|---|
| `call assert_equal(1, 2)` | `len(v:errors)` = 1 | 1 |
| `call execute('echo 1')` | 1 | **0** |
| `call assert_fails(…)` | 1 | **0** |
| `call assert_beeps(…)` | grows | **0** |

Why it mattered more than any single wrong value: `examples/*.vim` are
self-testing scripts whose epilogue throws when `v:errors` is non-empty. An
example whose last assertion was `assert_fails()` printed "all assertions
passed" **no matter what had failed above it**. `tests/examples.rs` reported a
clean corpus for the same reason.

Fixed by porting `eval_init()` faithfully and seeding once per thread
(`EVAL_INITED`), since `install()` is per-VM by design here.

The blast radius, measured by running every `examples/*.vim` under the
pre-change binary and the fixed one:

```
BASE failures: 0
NEW  failures: 8
newly failing: builtin_arity json map_commands msgpack strings testing
               varargs vim9_script_scope
newly passing: (none)
```

Three of the eight were **wrong expectations in the example**, not port defects —
verified by running the same script files through vim and nvim, which fail the
identical assertions:

| example | assertion | vim 9.2 | nvim 0.12.4 | vimlrs |
|---|---|---|---|---|
| `varargs.vim:20` | `Sum(10,20,30)/10*1` | 6 | 6 | 6 (script said 3) |
| `varargs.vim:56` | `map([1,2,3], {i,v -> Sum(v, i*v)})` | `[1,4,9]` | `[1,4,9]` | `[1,4,9]` (script said `[1,3,6]`) |
| `strings.vim:109` | `printf('%g %g %g', 0.1, 1000000.0, 0.0001)` | `0.1 1000000.0 1.0e-4` | same | same (script said `0.1 1e+06 0.0001`) |
| `strings.vim:110` | `printf('%.3g', 3.14159)` | `3.142` | `3.142` | `3.142` (script said `3.14`) |
| `msgpack.vim:34` | `msgpackparse(msgpackdump(['hi'],'B'))` | *(no `msgpackdump`)* | `['hi']` | `['hi']` (script said `[0z6869]`) |

Those five expectations were corrected to the measured values. The remaining
five scripts are genuine open gaps and are now named in a `KNOWN_OPEN` table in
`tests/examples.rs`, the same contract `tests/parity_cases.rs` uses: an entry
that starts passing FAILS the test, every entry must be an open item here, and a
script not listed must still pass outright. That is strictly stricter than the
previous state, which reported nothing at all.

## R25-2. `msgpackparse(msgpackdump(…))` did not round-trip — ✅ FIXED (was R24-O3)

The recorded diagnosis ("the failure is on the PARSE side") was right, and the
cause was one layer further back than a bug in the decoder: **the faithful ports
were already present and unreachable.** `msgpackparse_unpack_list()` and
`msgpackparse_unpack_blob()` (`src/ported/eval/funcs.rs`) — including
`encode_read_from_list()`, the `List item is not a string` check and the
NL→NUL mapping — were dead code, referenced only by one unit test.
`f_msgpackparse()` ignored them and decoded an inline stream instead, built by a
helper that read each List item with `tv_get_string`, i.e. as a Rust `String`.

MessagePack is binary, so that read destroyed it. Only accidentally-ASCII
payloads survived:

| expression | nvim (vim has no `msgpackdump`) | vimlrs before |
|---|---|---|
| `msgpackparse(msgpackdump([1,2,3]))` | `[1, 2, 3]` | `[1, 2, 3]` (bytes `01 02 03` are valid UTF-8) |
| `msgpackparse(msgpackdump([v:true,v:false,v:null]))` | `[v:true, v:false, v:null]` | `E5766` |
| `msgpackparse(msgpackdump(['abc']))` | `['abc']` | `E5766` |
| `msgpackparse(msgpackdump([[1,2]]))` | `[[1, 2]]` | `E5766` |

Fixing only that would still have left the DUMP side wrong, which the round-24
record did not have. The C's readfile() convention is a byte stream split on NL
in which each line's NUL bytes are stored as NL — `memchrsub(str, NUL, NL, …)`
in `encode_list_write()` (`vendor/eval/encode.c:78`, `:90`), inverted by
`ch == NL ? NUL` in `encode_read_from_list()` (`:269`). This port did neither, so
the round trip was self-consistent but the intermediate List diverged:

| expression | nvim | vimlrs before |
|---|---|---|
| `str2list(msgpackdump([0])[0])` | `[10]` | `[0]` |
| `str2list(msgpackdump([0,0])[0])` | `[10, 10]` | `[0, 0]` |

`encode_list_write()` took a `&str` and so could not carry a msgpack payload at
all; its C signature is `(void *, const char *buf, size_t len)` and it is a byte
buffer here now. That also removed a `String::from_utf8_lossy` on the msgpack
**ext** payload in `decode.rs`, which had a note admitting it was lossy.

Three error messages were wrong as well, all verified against nvim:

| call | nvim | vimlrs before |
|---|---|---|
| `msgpackparse('x')` | `E899: Argument of msgpackparse() must be a List or Blob` | `E5070: msgpackparse() argument must be a List or Blob` |
| `msgpackparse(0zC1)` | `E475: Invalid argument: Failed to parse msgpack string` | `E5766: failed to parse msgpack string` |
| `msgpackparse(0z93)` | `E475: Invalid argument: Incomplete msgpack string` | `E5766: failed to parse msgpack string` |
| `msgpackparse([1])` | `E475: Invalid argument: List item is not a string` | *(no error — `[49]`)* |

`emsg_mpack_error()` was a stub that collapsed the C's three-way switch
(`vendor/eval/funcs.c:4666`) into one wrong message; it is a faithful port now.

The stand-in helper was **removed, not allowlisted** — `mpack_input_bytes` is
gone from `tests/data/fake_fn_allowlist.txt` with a note saying why.

Covered by `examples/msgpack.vim`, extended with the List form. It cannot be a
`tests/parity_cases/` case: `scripts/parity.sh` records **real vim** as ground
truth and vim answers `E117: Unknown function: msgpackdump`, so nvim is the only
oracle and every added value is cited to it.

## R24-O1 — DECISION: this port follows Neovim, and that is now a standing rule

`echo 'A' . nr2char(7) . 'B'` is `A^GB` in vim and `AB` in nvim, because
`msg_multiline` treats BELL as a delimiter and calls `vim_beep()` in place of
writing it (`vendor/message.c:296`).

**vimlrs follows nvim.** Not as a preference for that output, but because
`vendor/` **is** Neovim: `src/ported/` is defined as 1:1 ports of it, every
`Port of` citation resolves against it, and `tests/ported_fn_names_match_c.rs`
computes its legal name set by scanning it. Matching vim here would mean writing
a body that no vendored C contains and citing it as a port — the exact thing the
name gate exists to catch.

The value of stating it as a rule rather than settling this one case: R24-O4 (a
leading composing char) and R23-O1 (the empty-list clamp index) are the same
call, and deciding them case-by-case would make the port's oracle
non-deterministic — no citation would tell you which engine a given function
follows. One invariant is worth more than three individually-defensible choices.

**Consequence that must be recorded with it:** `scripts/parity.sh` measures
against **real vim**. So the port follows nvim while its main gate follows vim,
and every vim/nvim divergence is therefore permanently ineligible as a parity
case and must be excluded with a reason in the case file (as R24-O1 and R24-O4
already are). That tension is structural, not a bug, and it is the price of the
rule above.

## R22-O3 — CALL: do not vendor vim's C into `vendor/`

The blocker is availability no longer; the C is real and the round-22 citations
are verified byte-for-byte at the installed vim's exact patch level
(`vim/vim@v9.2.0900`):

| function | file:line | line content |
|---|---|---|
| `f_blob2str` | `src/strings.c:1422` | `f_blob2str(typval_T *argvars, typval_T *rettv)` |
| `f_str2blob` | `src/strings.c:1567` | `f_str2blob(typval_T *argvars, typval_T *rettv)` |
| `f_js_encode` | `src/json.c:1544` | `f_js_encode(typval_T *argvars, typval_T *rettv)` |

The placement question is now measurable rather than a matter of taste. Running
the name gate's own extraction (an identifier immediately followed by `(`) over
the vendored tree and over vim's two files:

| quantity | count |
|---|---|
| callable names in vendored Neovim `vendor/` | 3984 |
| callable names in vim `strings.c` + `json.c` | 312 |
| names vendoring would ADD | 175 |
| names that COLLIDE with the Neovim set | **137** |

The 137 is the decision. Those names exist in both upstreams with independent
semantics, and they are not obscure — `emsg`, `dict_add`, `concat_str`,
`convert_setup`, `eval_expr_typval`, and `byteidx`/`byteidxcomp`/`charidx`, which
are themselves a known vim/nvim behavioural divergence point. Dropping vim's C
into `vendor/` would mean the gate could no longer say which upstream a name
traces to; it would only say "some upstream". That trades the one mechanism
keeping this port honest for three builtins.

**Call: `vendor/` stays Neovim-only.** The constructive path, when these three
are actually wanted, is a separate tree the name gate does NOT scan (e.g.
`vendor_vim/`), ports carrying a distinct citation form
(`/// Port of vim's f_blob2str() — vim/vim@v9.2.0900 src/strings.c:1422`) and
their own allowlist section. The Neovim name set then keeps its exact present
meaning and a vim-sourced port is syntactically obvious.

Deliberately NOT implemented in this round: it changes the audit gate, and a
gate change has to be reviewed in isolation from the work it measures. It is a
proposal here, not a fait accompli.

## R24-O2 — ASSESSED, not attempted; and the round-24 record was wrong

Cost, measured: the AST carries **no source spans**. `Block` is
`Vec<(u32, Stmt)>` — a line number and nothing else — and by the time E716 is
raised, `b_unlet_index` holds two evaluated `typval_T`s popped off the VM stack.
The `|` split happens in `viml_parser.rs` pass 2, which converts `&str` slices
into owned `String`s keyed only by line number, destroying the offset. Reaching
the un-split line means threading a per-argument source tail through `Lines`,
`parse_one`, `parse_stmt`, `split_unlet_args`, `parse_unlet_arg`, a new
`UnletArg::Item` field, the lowering, and the runtime builtin — about 5 files and
10 functions. **Not attempted this round.**

Two corrections to what round 24 recorded, both measured with a MULTI-line
`:try`, which separates the message from the `|`-remainder effect:

| case | vim 9.2 | nvim 0.12.4 | vimlrs |
|---|---|---|---|
| `unlet d.nokey` | `E716: … "nokey"` | `E716: … "nokey"` | `E716: … "nokey"` — **all three agree** |
| `unlet n[0]` | `E689: Index not allowed after a number: n[0]` | `E689: Can only index a List, Dictionary or Blob` | **identical to nvim** |
| `unlet s.key` | `E1203: Dot not allowed after a string: s.key` | `E1203: Dot can only be used on a dictionary: s.key` | `E689: Can only index a List, Dictionary or Blob` |

1. **The E689 row is not a vimlrs defect.** Round 24 recorded that "vim and nvim
   use a different message for indexing a Number than the one this port emits".
   That is wrong: nvim emits *exactly* this port's message. Only vim differs, and
   vim is not the vendored spec (see the R24-O1 decision above). `grep -rn E689
   vendor/` is one nameless `emsg` at `vendor/eval.c:1039`; "Index not allowed
   after a number" does not exist in the vendored source. Closing that row would
   mean deliberately deviating from `vendor/`. **This row is closed as
   not-a-bug.**
2. **The E716 remainder only appears in the one-line `|` form**, where vim and
   nvim agree with each other and differ from this port:
   `E716: Key not present in Dictionary: "nokey | catch | echo 'A-caught' | endtry"`.
   That row remains open and is what the parser change above would buy.

## Still open

### R25-O1. `assert_fails()` misses a builtin/user-function arity error in context

`examples/builtin_arity.vim` reports `command did not fail` for
`call assert_fails('call abs()', 'E119')` and three siblings; `examples/testing.vim`
does the same for a user function. In ISOLATION the identical call works on both
the pre-change and fixed binaries, and inserting any statement between the
assertions makes the whole file pass — so this is context-dependent, not a plain
detection bug, and it is not the JIT (`VIMLRS_NO_JIT=1` reproduces it). Both
oracles pass both scripts. `call abs()` does raise `E119: Not enough arguments
for function: abs` in vim, nvim and vimlrs alike, so the error exists and
`assert_fails` is not seeing it. Newly visible, not newly introduced — the
pre-change binary fails identically once `v:errors` survives.

### R25-O2. `json_encode()` Dict key order

`examples/json.vim:20` hardcodes vim 9.2's key order, which vim reproduces and
this port does not. nvim emits this port's order but with a space after `:` and
`,`. All three disagree, so the assertion is oracle-dependent and needs
rewriting to be order-independent rather than "fixed" toward any one engine.

### R25-O3. `len(maplist())` is an absolute count in a relative world

`examples/map_commands.vim` expects 5 and 4. vimlrs gives 6 and 5, nvim gives
103 and 102, vim gives 12 and 11 — the editors count their own default mappings.
The expectation has to be made relative (a delta around the mappings the script
itself defines).

### R25-O4. A vim9 script-scope counter reads 0

`examples/vim9_script_scope.vim:44` expects 3 and gets 0. Both oracles pass the
script, so this is a real vim9 scoping gap.

### R25-O5. A one-line `try | call … | catch` does not abort

Found while measuring R25-2's error paths, and **general, not msgpack-specific**.
Both oracles agree with each other and differ from this port:

| one-line form | vim 9.2 | nvim 0.12.4 | vimlrs |
|---|---|---|---|
| `try \| echo 'ok' \| catch \| … \| endtry` | runs on | runs on | same |
| `try \| throw 'boom' \| catch \| … \| endtry` | caught | caught | same |
| `try \| call add(1, 2) \| catch \| … \| endtry` | **not caught, script aborts** | same | **caught, script survives** |
| `try \| echo add(1, 2) \| catch \| … \| endtry` | caught | caught | same |
| `try \| let x = add(1, 2) \| catch \| … \| endtry` | caught | caught | same |
| `try \| unlet nosuchvar \| catch \| … \| endtry` | caught | caught | same |

It is specific to `:call` whose function raised an ERROR — the `:echo` and `:let`
rows evaluate the same failing expression and ARE caught. The mechanism is the
same family as R24-2 and the C says so at the site:

```c
  // When inside :try we need to check for following "| catch" or "| endtry".
  // Not when there was an error, but do check if an exception was thrown.
  if ((!aborting() || did_throw) && (!failed || eap->cstack->cs_trylevel > 0)) {
```

(`vendor/eval/userfunc.c:3614`, in `ex_call`.) vimlrs already has the machinery
this needs — `VIML_EXC_IS_HARD` / `eval_op`, which R24-2 used for
`b_unlet_index`. Not attempted here: it changes `:call`'s error path and deserves
its own round with its own parity case.

### R25-O6. `unlet <String>.key` reports E689 instead of E1203

The third row of the R24-O2 table above. Both oracles raise `E1203` (differing
only in wording); this port raises `E689`, because `parse_unlet_arg`
(`src/viml_parser.rs`) collapses `s.key` and `s['key']` into the same node while
both engines distinguish them (`s['key']` really is E689 — verified). The ported
`get_lval` already gets this right; the synthesized `:unlet` path does not reach
it.

### R25-O7. `assert_beeps()` does not record a failure where nvim does

`call assert_beeps("call nosuchfunc()")` adds an entry to `v:errors` in nvim and
none here. Visible only now that `v:errors` survives. Not investigated.

### `tv_get_string` and its two siblings are still lossy — narrowed, not closed

`tv_get_string`, `tv_get_string_chk` and `tv_get_string_buf` still return a Rust
`String` and replace a non-UTF-8 byte with `U+FFFD`; all four are `char *` in the
C and only `tv_get_string_buf_chk` is byte-exact. Two call sites moved off the
lossy read this round (`encode_list_write`'s input, via
`tv_list_append_allocated_string` now taking a `VimStr`; and the msgpack ext
payload in `decode.rs`). The remaining several hundred are text-shaped and
mostly safe, but the two defects this round fixed were both this hazard, one
layer along from where R23-1 first recorded it. Each fix so far has been found by
a symptom rather than by auditing the call sites, which is not a strategy.

---

# Round 26 — the id nobody was checking, and a byte that was read as a character

Two independent things this round. First, a class of defect that had already hit
two sibling fusevm frontends and had no guard here at all: hand-assigned builtin
ids, where a duplicate number is a silent handler replacement rather than a
build failure. Second, the parity work — an argument documented in bytes that
this port read in characters, a shared error format string that four sites
wrote by hand and got wrong, and a regex scan that stepped a codepoint where the
C steps a character.

Versions used as oracles throughout, verbatim:

```
$ vim --version | head -1
VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 02 2026 19:00:41)
$ vim --version | grep 'Included patches'
Included patches: 1-900
$ nvim --version | head -1
NVIM v0.12.4
```

Every table in this section was re-derived from a fresh oracle run before the
round landed, rather than carried over from the draft that produced it. That
review changed three things, all recorded in place below:

| | outcome |
|---|---|
| the `E28` / `setmatches()` claim in R26-5 | **wrong, corrected.** vim *is* a usable oracle; the setmatches rows moved out of prose and into the parity case, and the real E28 divergence became R26-O5 |
| `match_add` / `w_next_match_id` / `w_match_head` C citations | **unverifiable, removed.** `vendor/window.c` is a 70-line subset that does not contain `match_add`; the rules are measured off vim instead |
| a `message.c` line cited for vim's leading space (R24-O4) | **unverifiable, removed.** vim's C is not in this repo; the conclusion never depended on it |

Everything else survived re-derivation unchanged, including all four `.expected`
files, which were re-run against this vim and matched byte for byte.

## R26-1. Builtin ids had no collision guard — ✅ ADDED (`tests/opcodes.rs`)

`src/fusevm_bridge.rs` hands out 537 `pub const VIML_*: u16` numbers by hand, and
fusevm's registration is a plain overwrite:

```rust
pub fn register_builtin(&mut self, id: u16, handler: BuiltinHandler) {
    let idx = id as usize;
    if idx >= self.builtin_table.len() {
        self.builtin_table.resize(idx + 1, None);
    }
    self.builtin_table[idx] = Some(handler);
}
```

(fusevm 0.17.0, `src/vm.rs:912`.) Two constants on one number does not fail to
build, does not warn and does not panic — the later registration silently
replaces the earlier handler. Two edits adding one builtin each can pick the same
free number and merge without a conflict marker. scalars shipped `MAKE_ORDERING`
and `MAKE_QUEUE` both at 754; phplang shipped `INDEX_ISSET` at 105 where
`LIST_ELEM_GET` already sat.

**Audit result: no collision exists today.** 537 constants over 3000..=3602, all
distinct, 536 registered exactly once, and the one that is not
(`VIML_CMP_BASE`) is a family base that `cmp_id` adds an offset to.

The shape has bitten here before, in its derived form. The comment above
`VIML_CMP_IC_OFFSET` records two ignore-case offsets that were shipped and
withdrawn because the ids `cmp_id` derived from them landed on
`VIML_INDEX`/`VIML_SLICE`/`VIML_ECHO` and on the `VIML_FN_GETCHAR` cluster, so
`==?` dispatched to those instead of comparing. `tests/opcodes.rs` pins five
invariants, all read out of the source rather than from a hand-kept list:

| test | what a violation means |
|---|---|
| `every_builtin_id_is_used_by_exactly_one_constant` | two constants, one number |
| `no_derived_comparison_id_collides_with_a_registered_constant` | a `cmp_id` result shadows another builtin |
| `no_builtin_id_reaches_below_the_viml_block` | an id under 3000 shadows fusevm's own builtins or awk's block |
| `no_constant_is_registered_twice` | two `register_builtin` calls, one id |
| `the_only_unregistered_constant_is_the_comparison_family_base` | a declared builtin the compiler can emit with no handler |

The comparison ids are obtained by calling the real `cmp_id`, not by re-deriving
its arithmetic in the test, and the `ALL_CMP_OPS`/`ALL_CASE_FLAGS` arrays are
length-checked against the enums as written in `src/viml_lexer.rs`.

Each of the five was confirmed to fail by injecting the defect it describes and
reverting. Every row below was re-run from scratch at pre-landing review; the two
injections that change dispatch were also *executed*, to show that the thing the
guard prevents is a wrong answer and not merely an untidy table:

| injection | `cargo build` | runtime | guard |
|---|---|---|---|
| `VIML_FN_TYPE` 3101 → 3100 (onto `VIML_FN_LEN`) | clean, no warning | `echo type("abcde") len("abcde")` → `1 1`, vim says `1 5` | `3100 => VIML_FN_LEN, VIML_FN_TYPE` |
| `VIML_CMP_IC_OFFSET` 10 → `0x20` (the historical value) | clean, no warning | `echo ("ABC" ==? "abc")` → `A`, vim says `1` | `3052 => VIML_INDEX, cmp_id(Equal, IgnoreCase); 3053 => VIML_SLICE, cmp_id(NotEqual, IgnoreCase); 3054 => VIML_SETINDEX, …; 3055 => VIML_SETRANGE, …; 3056 => VIML_IS_DICT, …; 3060 => VIML_ECHO, cmp_id(Is, IgnoreCase); 3061 => VIML_ECHON, cmp_id(IsNot, IgnoreCase)` |
| a second `register_builtin(VIML_FN_EMPTY, …)` | clean | — | `registered more than once — only the last handler survives: VIML_FN_EMPTY` |
| a new `VIML_FN_ORPHAN = 3990`, unregistered | clean | — | ``left: ["VIML_CMP_BASE", "VIML_FN_ORPHAN"]`` |
| `VIML_FN_TYPE` 3101 → 101 | clean | — | `builtin ids below the VimL block start (3000) …: VIML_FN_TYPE = 101` |

`"ABC" ==? "abc"` answering `A` is the whole argument for this file: the compiler
emitted a comparison, the VM ran a string index, nothing warned, and the only
symptom was a wrong value.

### The other registry — keyed by NAME — is already gated, by rustc

The id space is not the only place a duplicate could hide. `builtin_fn_id`
(`src/compile_viml.rs:2776`) is the name→id registry the compiler resolves
through, and it is large: **469 arms** over `"len" => h::VIML_FN_LEN` and the
like. A sibling frontend found 340 name-keyed registrations where a duplicate
name silently wins with no build signal at all, so this one was audited the same
way.

- **No duplicate names.** All 469 are distinct.
- **7 groups of names share an id**, and all 7 are vim's own documented aliases,
  not typos: `bufexists`/`buffer_exists`, `bufname`/`buffer_name`,
  `bufnr`/`buffer_number`, `chanclose`/`jobclose`, `chansend`/`jobsend`,
  `filereadable`/`file_readable`, `hlID`/`highlightID`.
- **A duplicate would not be silent.** It is a `match` on `&str`, so rustc
  reports `unreachable pattern`, and CI escalates that: `.github/workflows/ci.yml`
  runs `cargo clippy --all-targets --locked -- -D warnings`. Injecting a second
  `"len" =>` arm was verified to produce a plain warning under `cargo build` and
  a hard `error: unreachable pattern --> src/compile_viml.rs:2780:9` under the
  CI command. Reverted.

So no test was added for the name registry: the compiler already is the test.
The id space needed `tests/opcodes.rs` precisely because `u16` constants carry no
such structure — two names for one number is legal Rust.

## R26-2. `match()`'s `{start}` is a byte offset, not a character index — ✅ FIXED

`find_some_match` (`src/ported/eval/funcs.rs:910`) measured `{start}` in
characters. The C measures it in bytes: `len = (int64_t)strlen(str)`
(`vendor/eval/funcs.c:4111`), `if (start > len) goto theend` (4137), `str +=
start` (4146), and with `{count}` `startcol = (colnr_T)start` (4144) — a byte
column. On a multi-byte subject every `{start}` past the first such character
searched from the wrong place, and any `{start}` at or past the character count
returned "no match" while the subject still had bytes left.

Subject `"ünïcø∂é"` (7 characters, 13 bytes at 0 2 3 5 6 8 11), columns
`match(s,'.',i)  match(s,'\p',i)  matchend(s,'\p',i)  matchstr(s,'\p',i)`:

| `i` | vim | nvim | vimlrs before | vimlrs after |
|---|---|---|---|---|
| 0 | `0 0 2 'ü'` | same | `0 0 2 'ü'` | `0 0 2 'ü'` |
| 2 | `2 2 3 'n'` | same | `3 3 5 'ï'` | `2 2 3 'n'` |
| 3 | `3 3 5 'ï'` | same | `5 5 6 'c'` | `3 3 5 'ï'` |
| 5 | `5 5 6 'c'` | same | `8 8 11 '∂'` | `5 5 6 'c'` |
| 6 | `6 6 8 'ø'` | same | `11 11 13 'é'` | `6 6 8 'ø'` |
| 8 | `8 8 11 '∂'` | same | `-1 -1 -1 ''` | `8 8 11 '∂'` |
| 11 | `11 11 13 'é'` | same | `-1 -1 -1 ''` | `11 11 13 'é'` |
| 13 | `-1 -1 -1 ''` | same | `-1 -1 -1 ''` | `-1 -1 -1 ''` |

Pinned in `tests/parity_cases/match_start_bytes.vim`, together with the
`{count}` startcol form, the negative clamp, the ASCII rows and the List-subject
form (where `{start}` really is an item index).

This also closed one of the two gaps the differential fuzzer was reporting:
`matchlist('ünïcø∂é','\p',10)[-5]` was `E684` (empty list) against `''` in both
engines.

## R26-3. E716 printed the key unquoted at four sites — ✅ FIXED

Every E716 in the C goes through one format string, `semsg(_(e_dictkey), key)` —
`vendor/eval.c:901`, `:3346`, `vendor/eval/funcs.c:3250`,
`vendor/eval/userfunc.c:2694`, `:3568`, `vendor/eval/typval.c:3355` — and it
quotes the key. Six sites in this port wrote the message out by hand and four of
them dropped the quotes: `index_value` (`src/fusevm_bridge.rs:4534`),
`f_islocked` (`src/ported/eval/funcs.rs`), and both `fd_newkey` reports in
`src/ported/eval/userfunc.rs`.

| expression | vim / nvim | vimlrs before |
|---|---|---|
| `{'a':1}['b']` | `E716: Key not present in Dictionary: "b"` | `… : b` |
| `d.b` | `… : "b"` | `… : b` |
| `call d.nokey()` | `… : "nokey"` | `… : nokey` |
| `islocked("d.nokey")` | `… : "nokey"` | `… : nokey` |

`unlet d.nokey` and `let d.a.b` were already quoted. Pinned in
`tests/parity_cases/dict_key_e716.vim`, which also covers a key containing a
space and one containing a `"` (the quoting is literal, not an escape pass).

## R26-4. A regex scan stepped a codepoint where the C steps a character — ✅ FIXED

R6-6 made a matching *atom* consume a base codepoint plus its composing marks.
The other half was never done: the scan that chooses where to try the next match
advanced one `char`, so a match could BEGIN on a mark belonging to the character
before it. All three scan loops in `src/viml_regex.rs` had it — `Regex::find_from`,
`regex_substitute`, and the local `find_from` inside the split helper — as did
the `{count}` step in `regex_search_nth`, which advanced `s + 1` where the C
advances `startp[0] + utfc_ptr2len(startp[0])`.

Subject `nr2char(0x65) . nr2char(0x301) . "x"` — a decomposed `é` followed by `x`:

| expression | vim | nvim | vimlrs before | vimlrs after |
|---|---|---|---|---|
| `match(a, '\W')` | `-1` | `-1` | `1` | `-1` |
| `matchlist(a, '\W')` | `[]` | `[]` | `['́', '', …]` | `[]` |
| `matchstr(a, '\W')` | `''` | `''` | `'́'` | `''` |
| `substitute(a, '\W', '!', 'g')` | `'éx'` | `'éx'` | `'e!x'` | `'éx'` |

A subject that *opens* with a composing mark still matches it at 0 in all three
— the scan always tries its starting position before advancing — so
`match(nr2char(0x301) . "z", '\W')` is 0 everywhere. Pinned in
`tests/parity_cases/regex_composing_start.vim`.

This closed the fuzzer's second gap, `matchlist('écombining','\W')`. Seed 260001
over 4000 expressions went from `GAPS: 2` to `GAPS: 0` with `PANICS: 0`,
`NEITHER: 3` and `divergent: 167` unchanged.

## R26-5. `matchadd()` ids started at 1001 and ignored priority order — ✅ FIXED

Two defects in the same function. The auto-id counter is READ and then
incremented, so the first auto-assigned id is the seed itself; this port
pre-incremented, so every id was one too high. And the match list is kept in
ascending priority order, so an equal priority appends after its peers and a
higher one sinks to the end regardless of insertion order; this port appended
everything.

**The evidence here is measured, not cited.** `match_add` is NOT in the vendored
tree: `vendor/window.c` is a 70-line subset carrying only `find_tabpage` and
`win_get_tabwin` (its own header says so), and `grep -rn 'match_add' vendor/`
returns nothing. `:help matchadd()` promises only "a free ID, which is at least
1000" (`/opt/homebrew/share/vim/vim92/doc/builtin.txt:7376`) and documents no
ordering at all. So the rule this port now implements is read off vim 9.2's
output and pinned in `tests/parity_cases/matchadd_priority.vim`, whose
`.expected` is vim's own bytes. An earlier draft of this section wrote it as a
`w_next_match_id` / `w_match_head` / `prio >= cur->priority` C citation; that
attribution was removed because it cannot be checked from this repo.

```
matchadd('Search','a') / ('Search','b') / ('Search','c',20,42) / ('Search','d')
```

| | vim | nvim | vimlrs before | vimlrs after |
|---|---|---|---|---|
| ids returned | `1000 1001 42 1002` | same | `1001 1002 42 1003` | `1000 1001 42 1002` |
| `getmatches()` ids | `[1000, 1001, 1002, 42]` | same | `[1001, 1002, 42, 1003]` | `[1000, 1001, 1002, 42]` |

`setmatches()` re-adds every entry, so it re-sorts too — a *stable* sort by
`priority` is the same permutation as adding them in turn.

An earlier draft recorded that "vim cannot be the oracle for that row (`-u NONE` has no
highlight groups, so it stops at `E28`)" and fell back to nvim. **That is wrong
and has been corrected.** `Search` is a default highlight group, so `-u NONE`
resolves it and vim answers normally; only an *unknown* group raises E28 (see
R26-O5). Re-measured this round, all three engines on the same script:

```vim
call setmatches([{'group':'Search','pattern':'p','priority':50,'id':7},
              \  {'group':'Search','pattern':'q','priority':1, 'id':8}])
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
echo matchadd('Search','r')
echo string(map(getmatches(), {_, v -> v.id}))
```

| | vim | nvim | vimlrs |
|---|---|---|---|
| restored list | `[[8, 1], [7, 50]]` | same | same |
| next auto id | `1000` | same | same |
| after `matchadd` | `[8, 1000, 7]` | same | same |

Because vim *is* a usable oracle, the setmatches rows were moved INTO
`tests/parity_cases/matchadd_priority.vim` rather than left as prose, including
an equal-priority row that pins the sort's stability — input ids `3`(prio 10),
`4`(prio 2), `5`(prio 10) come back as `[[4, 2], [3, 10], [5, 10]]`, from vim.

## Still open

### R26-O1. A `{start}` inside a multi-byte sequence

The residue of R26-2. The C chops the subject with `str += start` even when that
lands mid-sequence, and the regex then matches the orphan continuation bytes one
byte at a time — `matchstr("ünïcø∂é", '\p', 1)` is `'<bc>'` in both engines. The
matcher here indexes `Vec<char>` and has no way to represent a lone continuation
byte, so it begins at the next whole character instead. Measured, subject as in
R26-2:

| `i` | vim / nvim `match(s,'.',i)` | vimlrs | vim / nvim `matchstr(s,'\p',i)` | vimlrs |
|---|---|---|---|---|
| 1 | `1` | `2` | `'<bc>'` | `'n'` |
| 4 | `4` | `5` | `'<af>'` | `'c'` |
| 7 | `7` | `8` | `'<b8>'` | `'∂'` |
| 9 | `9` | `11` | `'é'` | `'é'` |
| 10 | `10` | `11` | `'é'` | `'é'` |
| 12 | `12` | `-1` | `'<a9>'` | `''` |

Closing it means a byte-indexed matcher, which is a `src/viml_regex.rs` rewrite
and its own round. Rounding the other way (down to the character containing the
offset) would be worse: it reports matches starting *before* the requested
`{start}`, which `{start}` exists to forbid.

### R26-O2. A builtin that reports an error but returns still aborts the command

`ex_echo` prints an argument whenever `eval1()` returned OK and consults
`did_emsg` only to decide whether to add E15 (`vendor/eval.c:6146`, `:6150`). A
`f_*` that calls `semsg` and then returns normally therefore still contributes
its value, and the arguments to its left have already been printed, because
`ex_echo` evaluates and prints one argument at a time. This port evaluates every
argument, then checks whether the error count rose since the statement started
and prints nothing if it did (`echo_impl`, `src/fusevm_bridge.rs:2557`).

```vim
let d = {}
echo "A" islocked("d.nokey")
echo "B" str2nr("10", 99)
echo "C" range(1,5,0)
echo "D" sort([3,1,2], "NoSuchCmpFn")
```

| | vim | nvim | vimlrs |
|---|---|---|---|
| A | `A` / `E716: … "nokey"` / ` -1` | same | `E716: … "nokey"` |
| B | `B` / `E474: Invalid argument 0` | same | `E474: Invalid argument` |
| C | `C` / `E726: Stride is zero` / ` []` | `… / ` 0`` | `E726: Stride is zero` |
| D | `D` / `E117: …NoSuchCmpFn` / `E702: …` / ` [3, 1, 2]` | same | `E702: …` |

Row D shows a second defect on the same line: the E117 from inside sort's
compare callback is never reported at all.

Fixing this is two changes, both to the statement model rather than to a
builtin: `:echo` has to lower as a per-argument evaluate-then-print loop, and the
abort has to key on "the evaluator returned FAIL" (the existing `HARD_ERR`
signal) rather than on "an error was reported". The 32 parity cases and the
`:silent!` rule in `echo_impl`'s comment both depend on the current behaviour, so
this is its own round.

### R26-O3. `setreg()` with a Dict value

`setreg("a", {})` is `1` in both engines; here it is
`E908: Using an invalid value as a String`. Found while measuring R26-O2, not
investigated.

### R26-O4. `tr()` checks its two strings for equal length eagerly, and counts codepoints

Found by the fuzzer on seed 260002, pre-existing (nothing this round touches
`f_tr`). Two things at once, with `f` = `nr2char(0x65) . nr2char(0x301) . "combining"`
(11 codepoints, 10 characters):

| expression | vim | nvim | vimlrs |
|---|---|---|---|
| `tr("0x1f", f, "  padded  ")` | `'0x1f'` | `'0x1f'` | `E475: Invalid argument: écombining` |
| `tr("éx", nr2char(0x65) . nr2char(0x301), "Z")` | `'éx'` | `'éx'` | `E475: Invalid argument: é` |
| `tr("abc", "ab", "xy")` | `'xyc'` | `'xyc'` | `'xyc'` |

Neither engine compares the two strings up front: `f_tr` walks the input, looks
each character up in `{fromstr}`, and only raises `E475` when a character that IS
present has no counterpart left in `{tostr}` — `"0x1f"` shares no character with
`f`, so nothing is looked up and nothing errors. This port compares the lengths
first, and counts them in codepoints rather than in the `mb_ptr2len` units the C
walks. `f_tr`'s home file is `strings.c`, which is not vendored (it is one of the
allowlisted names), so closing this needs the same placement decision R22-O3 is
waiting on.

### R26-O5. vim rejects an unknown highlight group; nvim and this port accept it

Found while disproving an earlier draft's `E28` claim in R26-5 (which had it backwards —
it read the error as "`-u NONE` has no highlight groups", when in fact `Search`
resolves fine and only an *unknown* name errors). Measured this round:

| | vim | nvim | vimlrs |
|---|---|---|---|
| `matchadd('NoSuchGrp','a')` | `E28: No such highlight group name: NoSuchGrp`, returns `-1` | `1000` | `1000` |
| `len(getmatches())` after it | `0` | `1` | `1` |
| `setmatches([{'group':'NoSuchGrp',…}])` then `getmatches()` | `E28: …`, `[]` | the entry, restored | the entry, restored |

This port follows nvim, which is the standing rule (R24-O1), so it is **not a
defect** — but it is a real engine divergence on a *whole builtin's* success or
failure rather than on formatting, and the parity harness's oracle is **vim**.
Any future case that names a highlight group vim does not know will diverge for
this reason and not because of a bug. Recorded so the next case author sees it
before re-deriving it. Nothing in `tests/parity_cases/` names an unknown group
today.

### R26-O6. `fusevm_bridge::tests::vim_vars` has been failing since round 25 — the TEST is stale, not the code

Found by running the lib suite before landing this round. Not caused by anything
here; bisected to the previous commit by checking out each and running the one
test:

| commit | `cargo test --lib fusevm_bridge::tests::vim_vars` |
|---|---|
| `a7228bd0e3` | PASS |
| `203fcee768` | PASS |
| `1a79e9c235` (round 25, HEAD) | **FAIL** |

```
assertion `left == right` failed
  left: "boom\n"
 right: "\n"
```

The two lines at issue (`src/fusevm_bridge.rs`):

```rust
assert_eq!(run("let v:errmsg = 'boom'\necho v:errmsg"), "boom\n");
assert_eq!(run("echo v:errmsg"), "\n");
```

`run()` builds a fresh VM per call but stays on one thread, so the second line
asserts that a new VM resets `v:`. Round 25 deliberately removed exactly that:
`install()` used to call `evalvars_init()` on every VM — including the nested VMs
built for `execute()`, `assert_fails()` and every user-function body — which
emptied `v:errors` mid-script; it now seeds once per thread, matching the C,
which calls it once from `eval_init()` (`vendor/eval.c:206`).

**The new behaviour is the correct one**, measured across a `:source` boundary in
one session:

```sh
$ vim  -es -u NONE -i NONE -c "source $S/a1.vim" -c "verbose source $S/a2.vim" -c 'qa!'
errmsg=[boom]
$ nvim --clean -es -c "source $S/a1.vim" -c "verbose source $S/a2.vim" -c 'qa!'
errmsg=[boom]
```

(`a1.vim` is `let v:errmsg = 'boom'`, `a2.vim` echoes it.) So `v:` state
surviving between two `run()` calls is right, and the second assertion encodes
the pre-round-25 model.

**Deliberately not touched.** Editing an assertion so the suite goes green is the
shape of change that has to be proposed on its own and reviewed apart from the
work it grades — and this one is not this round's work at all. The fix is to make
the test state the semantics it actually wants (one script, or an explicit reset
between the two reads) rather than to relax the expectation, and it belongs to
whoever owns round 25's change. Reported, left failing.

### R24-O4 — re-verified, still correctly excluded

`nr2char(0x180b)` re-measured again at pre-landing review, all three from commands run
that session (`$S` is the session scratch dir):

```sh
$ printf 'echo nr2char(0x180b)\n' > $S/o4.vim
$ vim  -es -u NONE -i NONE -c "verbose source $S/o4.vim" -c 'qa!' 2>&1 | xxd
00000000: 203c 3138 3062 3e                         <180b>
$ nvim --clean -es -c "verbose source $S/o4.vim" -c 'qa!' 2>&1 | xxd
00000000: 3c31 3830 623e                           <180b>
$ ./target/debug/viml $S/o4.vim 2>&1 | xxd
00000000: 3c31 3830 623e 0a                        <180b>.
```

vim opens with byte `0x20`; nvim and this port do not.

**What is verified, and what is not.** The vendored `msg_outtrans_len`
(`vendor/message.c:1866`) has no leading-space branch: the whole body,
1866..1935, was read again at pre-landing review, and `grep -n utf_iscomposing
vendor/message.c` returns **nothing at all**. That is the part that decides the
question, because `vendor/` is Neovim and this port follows Neovim (R24-O1).
This round also originally attributed vim's space to a specific line of *vim's* `message.c`;
vim's C is not in this repo and that citation could not be re-checked, so it has
been dropped rather than repeated. The conclusion does not rest on it.
Unchanged.

### R24-O5 — still open, and still not bundled

Re-confirmed as a harness limit rather than a language divergence. The fix is a
`scripts/parity.sh` change — telling a message CR from the line-terminator CR
needs a different normalisation than the single byte-filter pass the harness runs
— and `scripts/parity.sh` is measurement infrastructure, so it gets reviewed on
its own and in isolation from the behaviour it measures. **Nothing about the
harness was touched this round or last.** The proposal, unchanged: normalise only
a CR that is immediately followed by LF (the tty artifact), leaving a lone CR
inside a message intact.

### R22-O3 — the three name-gate violations are still open, and still will not be allowlisted

`cargo test --test ported_fn_names_match_c`, run unmodified at pre-landing review, reports
exactly three and no others:

```
fn names under src/ported/ with no Neovim C origin and not allowlisted:
  src/ported/eval/funcs.rs: fn f_typename
  src/ported/eval/funcs.rs: fn type_name_of
  src/ported/eval/funcs.rs: fn member_of
```

All three implement vim's `typename()` / `type_name()` (`vim9type.c`), which
Neovim does not have, so there is no Neovim name for them to take. Three ways to
make the gate green were considered and all three rejected:

1. **Add them to `tests/data/fake_fn_allowlist.txt`.** Forbidden. The allowlist
   is the audit tool; editing it to accept your own code is exactly the failure
   the file exists to catch. `git diff --exit-code -- tests/data/fake_fn_allowlist.txt`
   is clean.
2. **Rename them to Neovim names.** There are none. Any name that passed would be
   a false citation, which is worse than the failure.
3. **Move them out of `src/ported/` so the scan misses them.** This is the same
   bypass wearing a different hat — the code would not change, only the
   detector's view of it.

The real fix is the one R22-O3 already adjudicated above: a separate
`vendor_vim/` tree the name gate does not scan, a distinct citation form, and its
own allowlist section. That restructures the gate, so it is its own reviewed
change. **Left failing and reported**, which is the correct end state for a gap
that cannot be closed honestly inside this round.

Note that this round's own additions were written to avoid *adding* to the count:
the priority-insert in `f_matchadd`/`f_matchaddpos` is spelled out at both call
sites rather than factored into a helper, because a helper would need a name the
gate has no C origin for.

### R23-O1, R25-O1..O7 — unchanged

---

# Round 27 — setreg()'s FAIL default, and a Funcref that had a string value

One item closed (R26-O3) and two new ones opened out of it. Oracles, verbatim
from commands run this round:

```
$ vim --version | head -1
VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 02 2026 19:00:41)
$ vim --version | grep 'Included patches'
Included patches: 1-900
$ nvim --version | head -1
NVIM v0.12.4
```

vim and nvim agreed on every row measured this round, so no engine had to be
preferred anywhere below.

## R27-1. `setreg()` ignored three of the C's early returns — ✅ FIXED (closes R26-O3)

`f_setreg` opens with `rettv->vval.v_number = 1;  // FAIL is default`
(`vendor/eval/funcs.c:6617`) and clears it to 0 only at `c:6742`, after the
write. Every early return therefore answers **1**, which reads backwards next to
the usual "0 is success" and is what made these easy to miss. Three returns were
absent here, and a fourth NULL check was being stringified instead:

| # | C | before | after |
|---|---|---|---|
| 1 | `c:6633` empty Dict clears the register and `c:6637` returns | `E908: Using an invalid value as a String` | `1`, register emptied |
| 2 | `c:6645`–`c:6652` a present `regtype` must parse completely, else `semsg(_(e_invargval), "value")` and return | accepted silently, wrote with the default type | `E475: Invalid value for argument value`, register untouched, `1` |
| 3 | `c:6628`/`c:6695`/`c:6731` absent `regcontents` leaves the pointer NULL, so NEITHER write branch runs | stringified a default typval → `E908` | writes nothing, `0` |
| 4 | `c:6709`–`c:6711` a bad list item `goto free_lstval`s past the write but still falls into `c:6742` | wrote the item's coerced text | register untouched, still `0` |

Row 2 needs the whole of `get_yank_type` (`c:6580`), not just its first
character: a blockwise type may carry a width (`b10` → `block_len =
getdigits(...) - 1`), and the cursor is left on the last digit so `c:6649`'s
`*(++stropt)` lands exactly one past it. So `b1` parses, while `zz` (bad char),
`vv` (trailing junk) and `''` (present but unparseable) all fail. Measured, all
three engines identical:

| expression | vim | nvim | vimlrs before | vimlrs after |
|---|---|---|---|---|
| `setreg('a', {})` | `1`, `getreg` `''` | same | `E908` | `1`, `''` |
| `setreg('c', {'regcontents':['ab','cd'],'regtype':'b1'})` | `0`, `'^V1'` | same | `0`, `'^V1'` | unchanged |
| `setreg('e', {'regcontents':'x','regtype':'zz'})` | `E475`, `1`, `'seed'` | same | `0`, `'x'` | `E475`, `1`, `'seed'` |
| `setreg('f', {'regcontents':'x','regtype':'vv'})` | `E475`, `1`, `'seed'` | same | `0`, `'x'` | `E475`, `1`, `'seed'` |
| `setreg('g', {'regcontents':'x','regtype':''})` | `E475`, `1`, `'seed'` | same | `0`, `'x'` | `E475`, `1`, `'seed'` |
| `setreg('h', {'regtype':'v'})` | `0`, `''` | same | `E908`, `0` | `0`, `''` |

Pinned in `tests/parity_cases/setreg_dict.vim`; the corpus is 34/34
byte-identical. The regtype parse is written out at its call site rather than
factored into a helper — `src/ported/` may only define names the Neovim C has,
and the C's own `get_yank_type` name is already taken by the reduced form in
`src/ported/ops.rs` that the option-string loop uses.

## Still open

### R27-O1. A Funcref has a string value here, and callback resolution depends on it

The remaining half of R26-O3, carved out because the fix is not local. `c:4604`
groups `VAR_FUNC` with `VAR_PARTIAL`/`VAR_LIST`/`VAR_DICT`/`VAR_BLOB`/
`VAR_UNKNOWN` in `tv_get_string_buf_chk` and `c:4610` errors:

```c
case VAR_PARTIAL:
case VAR_FUNC:
...
  emsg(_(str_errors[tv->v_type]));
  return NULL;
```

`src/ported/eval/typval.rs:163` instead had `(VAR_FUNC, v_string(s)) =>
Some(s.clone())`, returning the function NAME. Measured with
`let F = function('strlen')`:

| expression | vim | nvim | vimlrs |
|---|---|---|---|
| `'x' . F` | `E729: Using a Funcref as a String` | same | `'xstrlen'` |
| `strlen(F)` | `E729` | same | `6` |
| `setreg('b', F)` | `E729`, returns `1`, register unchanged | same | `0`, register set to `'strlen'` |
| `setreg('c', [F])` | `E729`, returns `0`, register unchanged | same | `0`, register set to `"strlen\n"` |
| `join([F], ',')` | `'strlen'` | same | `'strlen'` ✓ |

The last row is the one that is already right, and it shows the fix is safe in
principle: `join` does not use this function at all, it uses `encode_tv2echo`
(`vendor/eval/typval.c:1005`), which is why vim renders the name there without
an error.

**Deleting the `VAR_FUNC` arm was tried this round and reverted.** It builds
clean and then fails nine lib tests:

```
fusevm_bridge::tests::call_resolves_builtins, callback_builtins, batch4_builtins,
dictwatcher, index_assignment, map_filter_foreach_mapnew, reduce_builtin
ported::eval::typval::tests::callback_family
```

(plus `vim_vars`, which was already red — see R26-O6). So the arm is not a
stringification convenience; callback resolution reads a Funcref's name through
`tv_get_string`. Closing this means routing those paths at their own call sites
— the C reads `v_string` directly for `VAR_FUNC` when it wants a callback name
— and only then restoring the error. That is a cross-cutting change to the
callback layer and belongs in its own round; a local Funcref check inside
`f_setreg` was deliberately NOT added, because it would make the one measured
symptom disappear while leaving `'x' . F` and `strlen(F)` wrong and the shared
primitive still lying.

### R27-O2. `getregtype()` on an unset or cleared register is `'v'`, not `''`

Found while fixing R27-1, pre-existing and untouched by it. The C's `kMTUnknown`
has no representation in this port's register model, so a register that has
never held anything reports charwise:

```vim
echo 'fresh' string(getreg('z')) string(getregtype('z'))
call setreg('y', 'seed') | call setreg('y', [])
echo 'emptylist' string(getreg('y')) string(getregtype('y'))
```

| | vim | vimlrs |
|---|---|---|
| `fresh` | `'' ''` | `'' 'v'` |
| `emptylist` | `'' ''` | `'' 'v'` |

`getreg()` and the `setreg()` return value agree in both rows, which is why
`tests/parity_cases/setreg_dict.vim` can pin the clear path without tripping on
this: the case reads `getregtype` only for registers it has actually written.
Closing it means carrying `kMTUnknown` as a real state through `yankreg_T`
rather than defaulting to charwise, which touches `src/ported/ops.rs` broadly.

### R26-O1, R26-O2, R26-O4, R26-O5, R26-O6, R24-O5, R22-O3, R23-O1, R25-O1..O7 — unchanged

R26-O2 is visible in this round's own measurements: every erroring row of the
`setreg` table had to be written as `try`/`catch` around a `:let` rather than as
a bare `:echo`, because `:echo` here still discards the whole line once an error
is reported. That is a statement-model change, not a builtin one, and stays its
own round. `tests/data/fake_fn_allowlist.txt` was not touched; the detector
still reports exactly the three standing R22-O3 names.

## R28-1. The debugger reported one stack frame, and its three step verbs were one verb — ✅ FIXED

`--dap` answered every `stackTrace` with a single synthetic frame, and
`next` / `stepIn` / `stepOut` all resumed to "the next statement". Both are now
driven by call depth, read off `funccal_stack` (which is also what
`call_user_function_raw` measures against `'maxfuncdepth'`) rather than a
counter the debugger keeps in parallel — awkrs balances such a counter by hand
across every exit arm, and strykelang leaks one on the `?` at its `vm.rs:3259`.

Stepping ports awkrs's predicates (`awkrs/src/debugger.rs:141-186`), the one
frontend where they are wired to the live line hook:

| verb | armed state | stops when |
|---|---|---|
| `stepIn` | `step_mode` | the next statement, any depth |
| `next` | `step_over_depth` | `depth <= armed` |
| `stepOut` | `step_out_depth` | `depth < armed` |

Measured against `VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 02 2026
19:00:41)` driven through a pty in `:debug` mode. Stopped two calls deep, vim's
`backtrace` is one frame per active call, each with its own call-site line:

```text
>backtrace
  3 command line
  2 script …/bt.vim[11]
  1 function Foo[2]
->0 Bar
```

Frames now match that shape (innermost first, absolute file lines rather than
vim's body-relative display numbering, and `totalFrames` reporting the whole
stack through a `startFrame`/`levels` window).

## R28-2. vim's debugger stops per COMMAND; `viml --dap` stopped per line — ✅ FIXED

Measured on the same vim, stepping `let a = 1 | let b = 2`:

```text
>step
line 1: let a = 1 | let b = 2
>step
line 1: let b = 2
```

Two stops, one line. A `|` group is a single `Stmt::LineGroup` to
`compile_stmts`, so only its first command carried a `SET_LINENO` marker and
the debugger silently skipped every later command on the line. The group now
marks each of its commands (debug builds only). This is also why awkrs's
same-line guard (`awkrs/src/debugger.rs:159`), which suppresses a second stop on
an already-stopped line, is deliberately NOT ported: awk's debugger is
line-oriented, vim's is not.

## R28-3. The fuzz generator could not produce a debug session — ✅ FIXED

`fuzz-parity --dap` generates whole programs (nested user functions, branches,
loops, `|` groups) and runs each through a live `viml --dap` session, stepping
with a seed-driven mix of verbs. Three findings come out, the first two needing
no editor at all because the program is its own oracle:

| class | condition |
|---|---|
| `DEBUG DRIFT` | `--dap` output != the plain run's output |
| `INVARIANTS` | a backtrace or step-depth contract broke |
| `gaps vs vim` | the plain run != vim's output |

It is verified to detect, on a deliberately broken binary, all three of the
defects this round was about: re-introducing the discarded-`funcs` debug compile
flags 5 of 6 programs as `DEBUG DRIFT`; re-aliasing the verbs reports
``next` from depth 0 stopped at depth 1 — it stepped INTO the call`; truncating
the backtrace reports `outermost frame is `F0`, not `script``. A session that
never stops is counted separately and never as a pass, so a generator that
reaches nothing cannot read as a clean run.

## Still open

### R28-O1. An error inside a called function abandons the caller's `|` line

Found by `fuzz-parity --dap` on its first 60-program run — a new manifestation
of R26-O2's root cause (the abort keys on "an error was reported" rather than on
"the evaluator returned FAIL"), on the `|` line-group abort rather than on
`:echo`.

```vim
function! F0(x)
  let d = [3, 3]
  echo 'f0' . d
  return a:x - 2
endfunction
echo 'start'
let r = F0(3) | echo r
echo 'end'
```

| | vim | vimlrs |
|---|---|---|
| output | `start` / *E730* / `1` / `end` | `start` / *E730* / `end` |

vim reports E730, carries on to the function's next line, returns `1`, and still
runs the `| echo r`. Here the reported error trips `VIML_LINE_ABORT` and the
rest of the line is abandoned, so `1` never prints. Without the `|` the two
agree exactly — `echo r` on its own line prints `1` in both — which places the
divergence in the line-group abort and not in the function's own error handling.
Closing it is the R26-O2 statement-model change, not a separate fix.

### R27-O1, R27-O2, R26-O1, R26-O2, R26-O4, R26-O5, R26-O6, R24-O5, R22-O3, R23-O1, R25-O1..O7 — unchanged

`tests/data/fake_fn_allowlist.txt` was not touched this round: still 241 entries.

---

## R29-1. `exists()` answered 0 for every option and every environment variable — ✅ FIXED

`f_exists` handled `#autocmd`, `*callable` and plain variables. The `$env` branch
(`vendor/eval/funcs.c:1368`) and the `&opt` / `+opt` branch (c:1380) were both
absent, so every spelling below answered 0 against vim's 1.

```
$ vim -es -u NONE -i NONE -c 'verbose source ex.vim' -c 'qa!'
$ ./target/debug/viml ex.vim
```

| expression | vim 9.2.0900 | vimlrs (before) | vimlrs (after) |
|---|---|---|---|
| `exists('&ignorecase')` | 1 | 0 | 1 |
| `exists('&ic')` | 1 | 0 | 1 |
| `exists('+ts')` | 1 | 0 | 1 |
| `exists('&t_Co')` | 1 | 0 | 1 |
| `exists('&term')` | 1 | 0 | 1 |
| `exists('&ic ')` | 1 | 0 | 1 |
| `exists('$HOME')` | 1 | 0 | 1 |
| `exists('&nosuchoptionxyz')` | 0 | 0 | 0 |
| `exists('&ic x')` | 0 | 0 | 0 |

The C calls `eval_option(&p, NULL, true)` with a NULL `rettv`, which is what makes
it a *query*: `eval_option` still takes its `kOptInvalid` branch but emits no
E113, so the result reduces to "the name resolves, or it is a TTY option", then
`*skipwhite(p) != NUL` rejects trailing garbage (but not trailing blanks).

`find_option_end` also gained the TTY branch it had dropped (`vendor/option.c:92`),
so `&t_Co` is isolated as a whole 4-byte name instead of the 1-byte alphabetic run
`t`. `find_tty_option_end`'s body is not in `vendor/`, so its accepted shapes were
measured rather than transcribed: `nvim --clean --headless` gives
`exists('&t_ZZ')` 1, `exists('&t_z')` 0, `exists('&t_ZZZ')` 0, `exists('&t_')` 0,
`exists('&term')` 1, `exists('&ttytype')` 1. `term`/`ttytype` are matched as whole
strings, never as prefixes, so `&termguicolors` is not clipped to `&term`.

Covered by `tests/parity_cases/option_exists.vim` (recorded from vim) and
`option_optval::tests::find_option_end_isolates_tty_names_without_clipping`.

## R29-2. Five option reads returned "" where both engines have a value — ✅ FIXED

`&encoding`, `&fileformat`, `&iskeyword`, `&isprint` and `&isfname` were not in
the option table, so `get_option_value` returned the empty string for all five.

| option | vim / nvim | vimlrs (before) |
|---|---|---|
| `&encoding` | `utf-8` | `` |
| `&fileformat` | `unix` | `` |
| `&iskeyword` | `@,48-57,_,192-255` | `` |
| `&isprint` | `@,161-255` | `` |
| `&isfname` | `@,48-57,/,.,-,_,+,,,#,$,%,~,=` | `` |

These five and no others, because these five are **startup-invariant**: each reads
back identical under `vim -es -u NONE -i NONE`, `vim -N -es -u NONE -i NONE`,
`vim --clean -es` and `nvim --clean --headless`. See R29-O1 for the ones that are
not, and why hard-coding those would pin the engine to a startup artifact.

## R29-3. Three `tests/dap.rs` reference blocks quoted a script that is not the one under test — ✅ FIXED (docs only)

The round-28 DAP work documented its vim reference output as measured, but the
transcripts attached to `dap_stack_trace_reports_every_frame`, `dap_step_in_…`
and `dap_next_…` came from a different probe script — one with `let y = 2` in
`Bar` and `return x` on `Foo`'s line 3 — while all three tests run
[`NESTED_SCRIPT`], whose `Bar` body is `echo "in bar"` and whose `Foo` body is one
line. So the blocks claimed `script bt.vim[11]` and `function Foo[2]` for a
9-line-deep call in a 10-line script.

Re-measured on the script the tests actually use:

```
$ printf 'backtrace\ncont\n' | vim -es -u NONE -i NONE \
    -c 'breakadd func Bar' -c 'verbose source bt.vim' -c 'qa!'
  3 command line
  2 script bt.vim[9]
  1 function Foo[1]
->0 Bar
```

`[9]` (`echo Foo()`), not `[11]`; `Foo[1]` (`return Bar() + 1`, file line 6), not
`Foo[2]`. **The assertions were right all along** — `("Bar",2)`, `("Foo",6)`,
`("script",9)` is exactly this backtrace mapped from body-relative to
file-absolute lines — so no expectation changed and no coverage moved; only the
transcripts did. `dap_step_out_…`'s stated deviation (vim's `finish` makes one
extra stop on `line 2: End of function`) was re-measured and confirmed verbatim.

The `FUNCREF_SCRIPT` block cited `vim -N -u NONE -es …` with no `-i NONE`. That
command reads `~/.viminfo`, because a nocompatible vim has a non-empty `'viminfo'`
where a compatible one does not:

```
$ vim -N -u NONE -es -c 'redir! > o' \
    -c 'echo strlen(getreg(34)) histnr("cmd") len(v:oldfiles)' -c 'redir END' -c 'qa!'
18 100 100
$ vim -N -u NONE -i NONE -es …          # same command, -i NONE added
0 -1 0
```

`FUNCREF_SCRIPT` reads no register, so its recorded output was unaffected — it is
byte-identical under the pinned command (`xxd`: `0a68 6920 610a 6869 2062 0a65 6e64`).
The command in the doc block is now the pinned one and the hazard is stated on it.

## Still open

### R29-O1. Every option outside the ported table reads "" instead of its value, and E113 is still deferred

`eval_option` returns the empty string for an unknown name where vim raises
`E113: Unknown option: …` and exits 1:

```
$ printf 'echo &nosuchoptionxyz\necho "after"\n' > unk.vim
$ bash scripts/parity.sh unk.vim
--- vim      1 / E113: Unknown option: nosuchoptionxyz / after
+++ viml     0 /                                       / after
```

E113 is *not* being turned on yet, and that is the finding rather than an
omission. The option table holds 22 rows; vim's namespace is 922 names and
Neovim's 767 (measured by running `exists('&'.n)` over every `'name'` tag in both
runtimes' `doc/tags` plus both `getcompletion('','option')` lists — 952 in union).
Raising E113 against a 22-row table would convert a silent wrong *value* on ~930
real options into a spurious wrong *error*, which is the worse trade.

Closing this needs `options[]` filled out, and the defaults are the hard part —
they are not machine-readable from `doc/options.txt` (the "(default …)" column is
prose), and recording them from a live editor bakes in that editor's startup
state. `'cpoptions'` is the sharp case: `aAbBcCdDeEfFgHiIjJkKlLmMnoOpPqrRsStuvwWxXyZz$!%*-+<>;`
under `-u NONE` and `aABceFs` under `-N`.

### R29-O2. Startup-state dependence of the reference editor, and what the harnesses pin

`scripts/parity.sh` runs `vim -es -u NONE -i NONE`, i.e. **compatible mode** — no
`-N`. Probing 159 observables under eight entry points, against that one:

| entry point | used by | observables differing |
|---|---|---|
| `-es -u NONE -i NONE -c 'verbose source F'` | `parity.sh`, `gen_builtin_signatures.sh`, `BUGS.md` `vimref()` | — (baseline) |
| `-es -u NONE -i NONE -S F` | `fuzz_parity.rs` expression oracle | 1 (`&verbose`) |
| `-es -u NONE -c …` (no `-i NONE`) | `src/vimstr.rs` doc | 0 |
| `-e -s --not-a-term -u NONE -i NONE` | — | 0 |
| `-es -u NONE -i NONE -N` | `fuzz_parity.rs` DAP oracle (`run_program_vim`) | 14 |
| `-N -u NONE -es` (no `-i NONE`) | `tests/dap.rs` doc block (now fixed) | 21 |
| `-es -u NORC -i NONE` | — | 3 |
| `-es -i NONE` (this machine's real vimrc) | — | 3 |
| `--clean -es` | — | 29 |

(`-N` and `--cmd 'set nocp'` were both measured and give byte-identical output.)

The 14 that move on `-N` alone: `compatible`, `cpoptions`, `fileformats`,
`backspace`, `whichwrap`, `history`, `viminfo`, `formatoptions`, `modeline`,
`shortmess`, `more`, `esckeys`, `ruler`, `showcmd`. Dropping `-i NONE` from a
nocompatible vim adds seven more read straight off the developer's disk:
`&verbose`, `v:oldfiles`, `getreg('0')`, `getreg('"')`, `getreg('/')`,
`histnr('cmd')`, `histnr('search')`.

That last row is not only a read. A nocompatible vim with no `-i NONE` also
*writes* `~/.viminfo` on exit, so probing through that entry point mutates the
state the next probe will read — the `getreg('"')` length measured through it
changed between two runs of this audit for exactly that reason. Every command
recorded in this file that is meant to be reproducible therefore carries
`-i NONE`.

Nothing pinned today is contaminated: no harness inherits the user's vimrc, and
re-recording all 34 round-28 parity cases under `-N`, under `--cmd 'set cpo&vim'`
and under `--clean` produces files byte-identical to the committed ones. The
corpus simply does not read a compat-sensitive observable yet.

The latent problem is `fuzz_parity.rs` running three different `cpoptions` states
across its own call sites — `set cpo&vim` in the expression driver, `-N` in
`run_program_vim`, neither in `parity.sh`. A recommendation, deliberately NOT
applied here because it edits the measuring tool and fixes nothing currently
broken: add `-N` to `parity.sh`'s `run_vim`, so the oracle is Vim-defaults rather
than compatible-mode, matching the Neovim engine this crate ports. Proven safe —
all 34 expectations are unchanged by it — but it should land as its own reviewed
commit touching only the script.

### R29-O3. `exists(':cmd')` and `exists('##event')`

`exists(':echo')` is **2** in vim (`cmd_exists` returns 2 on an exact match, so the
result is not a boolean), and 0 here. Neither `cmd_exists` (c:1388) nor
`autocmd_supported` (c:1391) has a body in `vendor/`, and both need an ex-command
name table this crate does not model, so both branches fall through to the
variable lookup. Not portable faithfully until that table exists.

### R28-O1, R27-O1, R27-O2, R26-O1, R26-O2, R26-O4, R26-O5, R26-O6, R24-O5, R22-O3, R23-O1, R25-O1..O7 — unchanged

`tests/data/fake_fn_allowlist.txt` was not touched this round
(`git diff --exit-code` clean). The two suites that are red on `main` were red
before this round and are the two already tracked here: `ported_fn_names_match_c`
reports the three standing R22-O3 names (`f_typename`, `type_name_of`,
`member_of`), and `fusevm_bridge::tests::vim_vars` is R26-O6. Both were re-checked
against a clean `197578457f` worktree and fail identically there.

---

# Round 30

## R30-0. The parity oracle ran in COMPATIBLE mode — ✅ FIXED (harness only, isolated commit)

`scripts/parity.sh` drove vim as `-es -u NONE -i NONE`. `-u NONE` alone leaves
vim in **compatible** mode, which is not the dialect this crate ports: the engine
is Neovim-derived and Neovim has no compatible mode at all. Every
nocompatible-only behaviour was therefore invisible to the harness *by
construction* — not unreported, unreportable. This was measured and recommended
in R29-O2 and deliberately deferred there; it landed this round as its own
commit touching only the script, with no parity work bundled into it.

Re-verified against vim 9.2.0900 before landing. All 35 then-committed
expectations are byte-identical when re-recorded under each of:

| entry point | identical | differ |
|---|---|---|
| `-es -u NONE -i NONE` (the old baseline) | 35 | 0 |
| `-es -u NONE -i NONE -N` | 35 | 0 |
| `-es -u NONE -i NONE --cmd 'set cpo&vim'` | 35 | 0 |
| `--clean -es` | 35 | 0 |

so no expectation was accepted or rewritten by the change. `-i NONE` is retained:
a nocompatible vim without it reads *and writes* `~/.viminfo`, so the developer's
registers and histories would leak into the oracle and one probe would mutate
what the next reads.

## R30-1. 91 options read "" and did not exist — ✅ FIXED

Two DIFFERENT tables answered the two questions anyone asks about an option:

* `&opt` reads and `:set` resolve through `option::findoption` (`option.rs`).
* `exists('&opt')` and `:let &opt` resolve through `option_optval::find_option`
  (`option_optval.rs`).

They had different membership, so the two answers contradicted each other.
'runtimepath' was in the first only:

```
$ vim -es -u NONE -i NONE -N -c "verbose source probe.vim" -c 'qa!'
runtimepath exists=1 val='/…/vim92,…'
$ viml probe.vim
runtimepath exists=0 val=''            # exists() says no, the read resolves
```

and 90 more options were in neither, so they read `""` where both engines have a
value (R29-O1). Probing 133 option observables through
`vim -N -es -u NONE -i NONE` and `target/debug/viml`:

| | diverging from vim |
|---|---|
| before (`ad2bf3e16a`, clean worktree) | 96 / 133 |
| after | 29 / 133 |

Both tables now carry the same 112 rows, and two new unit tests keep them that
way: `option::tests::option_tables_agree` compares names, abbreviations, kinds
AND defaults across the two modules, and `option::tests::option_names_are_unique`
rejects a duplicate name or abbreviation (`findoption` is a linear scan, so a
duplicate silently shadows a later row — the same failure mode the builtin-id
guard in `tests/opcodes.rs` exists for).

The membership bar for a seeded default is that `vim -N -es -u NONE -i NONE` and
`nvim --clean --headless` report the **same** value. `scripts/parity.sh` pins the
oracle to `-N` as of R30-0, which is the state these were measured in; six of
them ('compatible', 'backspace', 'whichwrap', 'more', 'ruler', 'showcmd') read
differently under the old compatible-mode oracle, so this fix could not have been
verified before that commit landed.

`tests/parity_cases/option_defaults.vim` records the result. Re-recording the
whole corpus after adding it rewrote only the new file — the other 35
`.expected` files are byte-identical, which is the proof that no existing
expectation was moved to accommodate this change.

The 29 that still diverge are all deliberate exclusions, and each is named in the
table comment: 27 where the engines genuinely disagree ('cpoptions'
`aABceFsz`/`aABceFs_`, 'formatoptions' `tcq`/`tcqj`, 'history' 200/10000,
'shortmess', 'path', 'complete', 'listchars', 'fillchars', 'laststatus',
'startofline', 'joinspaces', 'hidden', 'autoread', 'background', 'mouse',
'display', 'switchbuf', 'sidescroll', 'ttimeoutlen', 'diffopt', 'sessionoptions',
'viewoptions', 'nrformats', 'commentstring', 'define', 'include', 'esckeys',
'foldcolumn', 'shellslash', 'maxcombine', 'tabpagemax'), and two that are
LOCALE-derived — see R30-3.

## R30-2. `strftime()` answered in the "C" locale until a `sort(…,'l')` ran — ✅ FIXED

`init_locale()` (`src/ported/os/lang.rs`) is a faithful port of the C's
`setlocale(LC_ALL, "")` + `setlocale(LC_NUMERIC, "C")`, and its doc comment said
it was "invoked lazily by the locale-dependent callers … so every entry point
gets the same locale state". It had exactly ONE caller —
`item_compare()`'s `strcoll` branch, i.e. `sort(…, 'l')`
(`src/ported/eval/typval.rs:2441`) — so every OTHER locale-dependent libc call
ran in the process's default `"C"` locale until a locale-collating sort happened
to occur. The state was not merely wrong, it was *order-dependent*:

```vim
echo "before=[" . strftime('%x %A', 0) . "]"
call sort(['b','a'], 'l')
echo "after =[" . strftime('%x %A', 0) . "]"
```

`TZ=UTC LC_ALL=de_DE.UTF-8`, before the fix:

```
vim  : before=[01.01.1970 Donnerstag]   after =[01.01.1970 Donnerstag]
viml : before=[01/01/70 Thursday]       after =[01.01.1970 Donnerstag]
```

The C calls `init_locale()` from `main()`, before anything is evaluated. It is
now called from the once-per-thread startup block in `fusevm_bridge::install`,
next to `eval_init()` — the same placement. After:

| observable | LC_ALL=C | en_US.UTF-8 | de_DE.UTF-8 | fr_FR.UTF-8 |
|---|---|---|---|---|
| `strftime('%c %x %X %A %B %p', 0)` | match | match | match | match |
| `strptime('%d %B %Y', '01 January 1970')` | match | match | match | match |
| `printf('%f %g %e %.2f')`, `string(1.5)`, `str2float` | match | match | match | match |
| `sort(…,'l')` collation | match | match | match | match |

("match" = viml byte-identical to vim 9.2.0900 at that locale.) `%f` and friends
are unmoved because `init_locale()` forces `LC_NUMERIC` back to `"C"`, which is
the regression the C guards against with the same second call.

`tests/parity_cases/locale_strftime.vim` pins the INVARIANT, not a locale-derived
string: no harness sets `LC_ALL`/`LANG`/`LC_TIME`, so recording `Donnerstag` or
`01/01/1970` would be recording the machine that ran the recorder. It records
that the same call answers the same thing throughout one script — verified
byte-identical against vim under `LC_ALL=C`, `en_US.UTF-8`, `de_DE.UTF-8` and
`fr_FR.UTF-8`.

## R30-3. `:retu`, `:th`, `:brea`, `:con` were not commands — ✅ FIXED

`canon_block_kw` (`src/viml_parser.rs`) resolves the BLOCK keywords'
abbreviations and its sets were re-verified this round against vim 9.2.0900's
`fullcommand()` — all 60 spellings checked, all correct. The statement dispatcher
beside it matched `:return`, `:throw`, `:break` and `:continue` by their FULL
spelling only. The failure was silent and misattributed: `retu 42` parsed as a
bare expression, which made the whole enclosing `:function` fail to parse, so the
error the user saw was `E117: Unknown function: Foo` at the CALL site.

```
$ cat abbr.vim                     vim 9.2.0900        viml (before)
function! R1()
  retu 42                          retu -> 42          E117: Unknown function: R1
endfunction
...
  if i == 2 | con | endif          i= 1 / i= 3         E121: Undefined variable: con
  if i == 2 | brea | endif         b= 1                E121: Undefined variable: brea
try | th 'x' | catch | …           caught x            (nothing)
```

Accepted sets, read out of `fullcommand()` and written as explicit sets rather
than prefix tests, because the spelling one character shorter is a DIFFERENT
command every time: `retu`/`retur`/`return` (`ret` is `:retab`),
`th`/`thr`/`thro`/`throw`, `brea`/`break` (`bre` is `:brewind`),
`con`/`cont`/`conti`/`continu`/`continue` (`co` is `:copy`),
`fini`/`finis`/`finish` (`fin` is `:find`).

`tests/parity_cases/cmd_abbreviations.vim` records every accepted spelling plus
the negative case `:ret`.

## R30-4. `:source` recursion allowed one level more than vim — ✅ FIXED

`src/fusevm_bridge.rs`'s `run_source_nested` guard read `depth >= 200`. The C
increments the counter inside `do_cmdline` itself, and the command line that
started everything (`-c 'source F'`, `-S F`, `viml F`) is already one
`do_cmdline` frame before the first `:source` nests. This port counts only the
nested calls, so it permitted exactly one more level. Measured with a
self-sourcing script:

```
$ vim -es -u NONE -i NONE -N -c "silent! source rec.vim" \
      -c "call writefile(['vim depth=' . g:d], out)" -c 'qa!'   → vim depth=199
$ viml -c "silent! source rec.vim" …                            → viml depth=200
```

Now `depth + 1 >= 200`, and both reach 199. The doc comment above the constant
already stated vim's number correctly; the code did not implement it.

## R30-5. Harness blind-spot census

What each harness is *structurally incapable* of reporting — not "has not
reported yet", but cannot, because the generator never emits it, the comparison
discards it, or an axis is pinned to a constant. Every row was checked against
the code, and the ones with a measurement carry it.

### `scripts/parity.sh` + `tests/parity_cases.rs`

| axis | status | evidence |
|---|---|---|
| **compatible vs nocompatible** | CLOSED this round | was `-u NONE` (compatible); 14 observables were invisible. R30-0. |
| **which stream a message went to** | blind | `2>&1` (`parity.sh` `run_vim`/`run_viml`) and one dup'd fd (`parity_cases.rs`). viml really does split them — `viml ln.vim 2>/dev/null` prints `a b c d f`, `2>&1 1>/dev/null` prints `E121: …` — and the harness compares only the merge. A message moved from stderr to stdout compares EQUAL. |
| **the line number an error is reported at** | blind | the normaliser drops `/^line\s+\d+:$/`, and viml emits no locator at all. vim prints `line    5:` for an error on line 5; viml prints nothing there. A wrong line number cannot fail a case. |
| **the `Error detected while processing …` preamble** | blind, by design | dropped because it embeds the case's absolute path. Its absence is therefore also unobservable. |
| **a carriage return inside a message** | blind | CR is stripped from vim's stream. Already tracked as R24-O5. |
| **trailing newlines** | blind | `$(...)` strips them on both sides in `parity.sh`; `parity_cases.rs` trims explicitly (documented at its line 96). |
| **locale / `TZ`** | PINNED TO THE DEVELOPER'S MACHINE | `grep -cE 'LC_ALL\|LANG=\|LC_[A-Z]+\|TZ=' scripts/parity.sh tests/parity_cases.rs src/bin/fuzz_parity.rs` → `0 0 0`. Both engines inherit the ambient locale, and 17 of the 38 committed records contain a value that moves with it. R30-O4. |
| **screen state** | blind | `-es` is silent Ex mode: no grid, no highlighting, no `:redraw`, no `:messages` history, no modes other than Ex. Every buffer/window/mapping behaviour is out of reach of this harness by construction. |
| **timing** | blind | nothing is timed. |
| **stdin** | blind | never fed; scripts cannot be interactive. |
| **what is generated** | nothing is | there is no generator. Coverage is exactly the 38 hand-written cases. |

### `fuzz_parity.rs` — expression mode

| axis | status | evidence |
|---|---|---|
| **error message prose** | blind, by design | compared by E-number only (`enumber`, c. line 1085). Documented and correct — the number is the contract. |
| **an exception with no E-number** | blind, and NOT by design | `enumber` returns the literal `"E?"` when no `E<digits>:` is found, so `:throw 'a'` and `:throw 'b'` produce the same outcome and compare EQUAL. Every user `:throw` in the corpus is one bucket. |
| **which value an errored expression still returns** | blind | an expression that both raises and yields a value reports the error only. That is exactly the class R30-O5 lives in. |
| **anything printed** | blind | the oracles run with `stdout`/`stderr` at `Stdio::null()`; results arrive only through `writefile()`. |
| **exit status** | blind | never compared. |
| **cross-expression state** | blind, by design | the `PRELUDE` is re-established before every expression. |
| **impure builtins** | never generated, by design | `FUNCS` admits only pure, deterministic, non-blocking names — no clock, filesystem, process table, RNG or buffer. That whole surface is unfuzzable here. |
| **top-level statement semantics** | mostly blind | statements ride the expression pipeline wrapped in `execute('…')`, and the oracle additionally wraps every expression in `try`/`catch`. Abort-the-rest-of-the-script behaviour cannot be observed through either wrapper. |
| **scopes other than `g:`** | never generated | the `PRELUDE` defines `g:` variables only. |

### `fuzz_parity.rs` — `--dap` mode

| axis | status | evidence |
|---|---|---|
| **error output** | discarded from BOTH sides | `out_lines` filters any line matching `E<digits>:`, the `line N:` locator, and the preamble. Documented, and the reason (redir folds messages into the output stream, viml writes them to stderr) is sound — but it means no DAP-mode finding can ever be about an error. |
| **blank lines** | discarded | `out_lines` drops empty lines, so the `:echo ''` column model is invisible in this mode. |
| **stderr of the plain run** | discarded | `run_program_plain` uses `Stdio::null()` for stderr. |
| **exit status** | blind | never compared in this mode. |
| **what is generated** | narrow, by design | "literals and arithmetic on them, nothing that raises" — so no DAP session ever steps through an error, a `try`, a dict, a string builtin, or a lambda. |
| **DAP surface beyond stepping** | never generated | one breakpoint at the first line, then `stepIn`/`next`/`stepOut`/`continue`. No conditional breakpoints, no `evaluate`, no variable inspection, no `setVariable`, no exception filters. |

### `tests/examples.rs`

Two criteria only: exit code zero, and no `E<num>:` on stderr. So it cannot see a
wrong ANSWER unless the script asserts on it — a script that prints garbage and
asserts nothing passes. It also cannot see stream identity, ordering, or the
specific exit code. Its module doc block was corrected this round to say so.

### What was closed

The `-N` gap (R30-0) and, downstream of it, 67 of the 96 diverging option
observables (R30-1) — which is the point of the census: the axis had to become
visible before the divergence on it could be found at all. `locale_strftime` and
`cmd_abbreviations` add two more cases. The remaining rows above are recorded
rather than closed, and the reason is given in each.

## Still open

### R30-O1. `source_tolerant()` discards every parse error

A script with a syntax error prints nothing about it and exits 0, where vim
reports it and exits 1:

```
$ printf 'echo "before"\necho ((1)\necho "after"\n' > probe.vim
$ viml probe.vim ; echo rc=$?
before
after
rc=0
$ vim -es -u NONE -i NONE -N -c 'verbose source probe.vim' -c 'qa!' ; echo rc=$?
before
Error detected while processing …probe.vim:
line    2:
E110: Missing ')'
after
rc=1
```

Implemented and REVERTED this round. The fallback fires for two different
reasons and nothing distinguishes them: (1) a real syntax error, which vim also
reports; (2) a construct vim accepts and this parser cannot read yet — a
curly-brace function name (`open_{pos}`), for instance, which vim parses lazily
inside a legacy function body and never complains about. Emitting the collected
list made `examples/tolerant_block_no_leak.vim` and `examples/registers.vim`
print an `E15:` that vim does not print for them. Trading a missed real error for
a spurious error on valid vim source is the worse of the two, and this fallback
exists precisely to source a real `~/.vimrc` full of case 2.

Two things must exist first: a "this line is invalid VimL" vs "this parser cannot
read it yet" signal out of the tolerant parser, and E-numbers that match vim's
(R30-O5). A third, independent cause of the exit status is recorded in the
function's doc comment: each statement runs as its own chunk and `run_chunk`
opens with `reset_run()`, which zeroes `did_emsg`, so even a reported error is
erased by the next statement starting.

### R30-O2. A byte slice that splits a character renders as U+FFFD, not `<c3>`

```
$ printf "echo 'héllo'[1]\n" > b8.vim
$ viml b8.vim | xxd      → 00000000: efbf bd0a   ....
$ vim … b8.vim | xxd     → 00000000: 3c63 333e   <c3>
```

vim routes the raw byte through `transchar_byte_buf`, which renders it as the
four ASCII characters `<c3>`; this port substitutes U+FFFD. `message.rs`'s
`msg_outtrans_len` port already documents that distinction — the index path does
not use it. The old `#8` entry claimed the two "render identically"; corrected in
place above.

### R30-O3. `:bre` (`:brewind`) and the rest of the unmodelled ex-commands

`try | bre | catch | echo v:exception | endtry` answers
`Vim(try):E121: Undefined variable: bre` here and prints nothing in vim, which
rewinds its (empty) buffer list. Any ex-command with no model reaches the
expression path and is reported as an undefined variable rather than as an
unknown command. Same root cause as R29-O3 (`exists(':cmd')` needs an ex-command
name table). Deliberately excluded from
`tests/parity_cases/cmd_abbreviations.vim`, which says so.

### R30-O4. 17 of the 38 committed parity records hold a locale-dependent value

Neither harness pins a locale (`grep` above: zero matches). Replaying the corpus
through vim under `LC_ALL=C`, `de_DE.UTF-8` and `tr_TR.UTF-8` and applying
`scripts/parity.sh`'s own normaliser:

* **`LC_CTYPE`-dependent — 7 files.** `LC_ALL=C` flips vim to `encoding=latin1`,
  which changes every byte-level answer: `option_exists` (lines 20, 25),
  `list2str_bytes`, `list2str_nul`, `match_start_bytes`,
  `regex_composing_start`, `echo_transchar`, `string_builtins`.
* **`LC_MESSAGES`-dependent — 11 files.** vim ships de/tr translations of its
  diagnostics; this port never translates. `dict_key_e716`, `exception_tags`,
  `function_forward`, `list_index_e684`, `reverse_argcheck`, `setreg_dict`,
  `string_builtins`, `ternary_e109`, `throwpoint`, `unlet_bar_try`,
  `unlet_e108`. Secondary effect: the normaliser drops only the ENGLISH preamble,
  so under `de_DE` the untranslated-match adds two more lines per file.

Control: at the ambient `en_US.UTF-8`, 0 of 38 differ, which is what confirms the
records were taken there. Nothing is wrong today — the exposure is entirely on
the RE-RECORDING side, on a contributor whose machine is set differently. The fix
is to pin `LC_ALL` (and `TZ`) in `run_vim`/`run_viml`, which is a
measurement-tool edit and therefore deliberately not bundled here; it is the
direct analogue of the `-N` recommendation, and should land the same way.

No committed record's *viml* side moves: running all 163 `examples/*.vim` and all
38 `tests/parity_cases/*.vim` through `target/debug/viml` under `LC_ALL=C`,
`en_US.UTF-8`, `tr_TR.UTF-8` and `de_DE.UTF-8` (with `TZ=UTC`, stdin from
`tests/fixtures/*.in` where one exists) gives 201 files per locale and
`diff -rq` reports 0 differences against `en_US.UTF-8` for each of the other
three. That is not because viml ignores the locale — after R30-2 it tracks it,
which is the point — but because no committed script reads a locale-derived
observable. `locale_strftime.vim` is written to keep it that way: it pins the
invariant, never the string.

### R30-O5. Parse-error text is Rust internals, and the E-number is wrong

`E15` is emitted with a `Debug`-formatted token where vim quotes the offending
source text, and the E-number is `E15` where vim picks a specific one:

| input | vim 9.2.0900 | viml |
|---|---|---|
| `echo ((1)` | `E110: Missing ')'` | `E15: expected RParen, found Eof` |
| `echo eval("1 +")` | `E15: Invalid expression: "1 +"` | `E15: Invalid expression: unexpected Eof` |
| `echo eval("]")` | `E15: Invalid expression: "]"` | `E15: Invalid expression: unexpected RBracket` |
| `echo 'a' .. 1.0e300` | `E15: Invalid expression: "0e300"` | `E15: Invalid expression: 0e300` (no quotes) |

The correctly-quoted form already exists at `ex_eval.rs:48`, `eval.rs:801/870`
and elsewhere; the parser and lexer paths do not use it. Blocks R30-O1: reporting
parse errors is only worth doing once the reported text is vim's.

### R30-O6. `getbufvar('%', '&opt')` reads empty

```
set tabstop=7
echo &tabstop                   " vim 7   viml 7
echo getbufvar('%', '&tabstop') " vim 7   viml (empty)
```

The buffer-scoped read does not reach either option store. Found while checking
R30-1; not fixed there because it is a buffer-variable path, not a table gap.

### R30-O7 — ✅ FIXED before the round closed (kept here as the record)

`f_toupper`/`f_tolower` (`src/ported/strings.rs`) used `str::to_uppercase()` /
`str::to_lowercase()`, and the `\U`/`\L`/`\u`/`\l` substitute escapes
(`src/viml_regex.rs`, `SubCase::push`) used the `char` equivalents. Those are
Unicode's FULL mappings and can turn one character into several; Vim's tables
hold only the SIMPLE (1:1) entries.

| input | vim 9.2.0900 | before |
|---|---|---|
| `toupper('ß')` | `ß` | `SS` |
| `toupper('ﬁ')` | `ﬁ` | `FI` |
| `tolower('İ')` | `i` | `i` + U+0307 |
| `substitute('straße','\(.*\)','\U\1','')` | `STRAßE` | `STRASSE` |

`mb_toupper`/`mb_tolower` were already taking the first codepoint of the full
mapping, which is why they were described as "identical over the simple
mappings". That is true for lowercase and FALSE for uppercase: the first
codepoint of `ß`'s full uppercase is `S`, which is not a case conversion of
anything. Where the full UPPERCASE mapping expands there is no simple mapping and
Vim leaves the character alone; lowercase keeps the first-codepoint rule
(`U+0130` really does lowercase to `i` in Vim, while its full mapping is
`i` + U+0307).

Both rules were checked by sweep rather than by argument: 2321 codepoints
(U+0020..U+05FF, Latin Extended Additional, Letterlike, Alphabetic Presentation
Forms, Fullwidth, Deseret, Cyrillic Extended-C, Latin Extended-C) through
`toupper()` and `tolower()` in both engines — byte-identical.
`tests/parity_cases/case_mapping.vim` records the hand-picked characters plus a
smaller in-script sweep so a future table change fails the case.

Locale-independent throughout: identical at `C`, `en_US.UTF-8`, `tr_TR.UTF-8`
and `de_DE.UTF-8`, in both engines. Neither implements the Turkish dotted-I rule.

### R30-O8. `v:lang`, `v:lc_time`, `v:ctype`, `v:collate` are always empty

vim returns the live locale string at every locale (`C`, `en_US.UTF-8`,
`de_DE.UTF-8`, `tr_TR.UTF-8`); this port returns `""` at all four. The C sets
them from `set_lang_var()` in `os/lang.c`, which is NOT in `vendor/` — the same
situation as R29-O3's `cmd_exists`. Not portable faithfully without a spec to
port from, and inventing one is the wrong answer.

### R29-O1, R29-O3, R28-O1, R27-O1, R27-O2, R26-O1, R26-O2, R26-O4, R26-O5, R26-O6, R24-O5, R22-O3, R23-O1, R25-O1..O7 — unchanged

R29-O1 is substantially narrowed by R30-1 (96 → 29 diverging option observables,
every remainder named and justified) but stays open: E113 for an unknown option
is still deferred, and the 27 engine-split options still read `""`.

`tests/data/fake_fn_allowlist.txt` was not touched this round
(`git diff --exit-code` clean, checked before every commit). The two suites red
on `main` were red before this round: `ported_fn_names_match_c` reports the three
standing R22-O3 names, and `fusevm_bridge::tests::vim_vars` is R26-O6. Neither
was weakened, and neither was cheap to close this round — R22-O3 needs vim's C
vendored (already CALLED against in round 26) and R26-O6 needs the `v:` table
audit its own entry describes.

---

## R31-0. The parity oracle inherited the developer's locale — ✅ FIXED (harness only, isolated commit)

R30-O4, recommended and declined last round because only the `-N` change was in
scope. Authorized this round and landed the same way, alone in one commit that
touches nothing it measures. Replaying the corpus through vim under different
ambient settings and diffing against the committed records:

| ambient | before | after |
|---|---|---|
| `LC_ALL=en_US.UTF-8 TZ=America/New_York` | 0/41 move | 0/41 |
| `LC_ALL=C TZ=UTC` | **9/41 move** | 0/41 |
| `LC_ALL=de_DE.UTF-8 TZ=Europe/Berlin` | **13/41 move** | 0/41 |
| `LC_ALL=tr_TR.UTF-8 TZ=Europe/Istanbul` | **13/41 move** | 0/41 |
| `LC_ALL=C.UTF-8 TZ=UTC` | 0/41 move | 0/41 |

The 9 are `LC_CTYPE`: `C`/`POSIX` gives vim `&encoding=latin1`, moving every
byte-level record. The 13 are `LC_MESSAGES`: vim translates its diagnostics from
`$VIMRUNTIME/lang` and this port never does, so under `de_DE` a record reads
`E979: Blobindex außerhalb des Bereichs: 7`.

Pinned to `LC_ALL=C.UTF-8 LANG=C.UTF-8 LC_MESSAGES=C.UTF-8 LANGUAGE= TZ=UTC`,
with `LC_CTYPE`/`LC_COLLATE`/`LC_TIME` removed. `LANGUAGE` is CLEARED, not set:
gettext reads it first and it is a colon-list, not a locale name. `C.UTF-8`
rather than `en_US.UTF-8` because it needs no generating and still gives
`utf-8` with untranslated messages. Re-recording all 41 under the pin changed no
record — the pin removes the dependence without moving an expectation.

**`$VIM` is now unset for the child too, and that is not cosmetic.**
`scripts/parity.sh`'s own binary-selection variable is named `VIM`, which is
also how vim locates its runtime. Exporting `VIM=<path to the binary>` makes vim
fail to find `$VIMRUNTIME/lang` and silently stop translating — measured while
taking the numbers above, it took the de_DE figure from 13/41 back to 0/41 and
looked exactly like a correctly pinned locale.

A pin that does not take is worse than none, because a missing locale falls back
to `C` in silence. The harness now probes `&encoding` through the same `pinned()`
before recording anything and refuses to run if it is not `utf-8`; verified to
discriminate (`utf-8` under the pin, `latin1` under `LC_ALL=C`).

## R31-1. Hardcoded-reference-string audit: 51 of 363 `E<number>` literals matched no engine — ✅ FIXED (34 of them)

Round 6's theme, applied mechanically. Every `E<number>` string literal under
`src/` (363) was matched against BOTH catalogues: vim 9.2.0900's own source,
fetched at tag `v9.2.0900`, and the vendored Neovim C that README:135 names as
the porting spec. A literal counts as agreeing if it is an INSTANTIATION of a
reference format string (`%s`/`%d` already filled in at the call site).

| | count |
|---|---|
| literals checked | 363 |
| agree with both engines | 230 |
| agree with the Neovim spec only (vim words it differently) | 74 |
| agree with vim only | 11 |
| **matched NEITHER** | **51** |

Re-running the same audit after this round: 340 literals, 20 matching neither —
so 31 of the 51 were closed. Of the 20 left, four are the `:Intercept` codes
below (a deliberate extension), one is a `#[test]` fixture string and one is the
unreachable default arm of `e_unmatched_block`, and the rest are R31-O1/O3.

The reachable ones, each re-measured against both engines after the fix:

| code | was | now |
|---|---|---|
| E1109/E1110/E1111/E1112/E1113/E1114 | invented text, no item index, no `0x80` floor, no sort, no overlap check | a port of `vendor/mbyte.c:2899` |
| E714 | `E1109: List required` | `E714: List required` (`e_listreq`) |
| E979 | no index | `Blob index out of range: <idx>` |
| E689 / E709 | E709 for a bad BASE | E689 base (`vendor/eval.c:1035`), E709 value (`c:1096`) |
| E799 | no constraint | `… (must be greater than or equal to 1)` |
| E1211 | `List required` | `List required for argument 1` |
| E715 | `E1206: Dictionary required` | `E715` per entry, and `sign_define([…])` answers a List |
| E364 | no `()` | `Library call failed for "f()"` |
| E685 | `Internal error` | `using an invalid value as a Number` (`typval.c:4097`) |
| E685 | `E473: Internal error: …` | `E685:` — `e_intern2` is E685 in Neovim |
| E474 | `E491: JSON decode error` | `E474: Failed to parse %.*s`, answering 0 |
| E80 | constant lost its `%s` | restored |
| E46 | read-only `v:` declined in SILENCE | `E46: Cannot change read-only variable "%s"` |

The parse-time ones are R31-2. Still open as **R31-O1**: `E5004` (needs the
encoder's `mpstack` path — Neovim says `Error while dumping msgpackdump()
argument, index 0, key 'a': …` and this port has no path context), the three
`E474`/one `E475` in `src/intercepts.rs` (vim codes borrowed for the vimlrs-only
`:Intercept` command — a deliberate extension, recorded rather than changed),
and `E116` from a call on a non-name callee, which cannot name the function.

## R31-2. Parse-error text was Rust internals — ✅ FIXED (R30-O5)

Thirty-two malformed expressions through `eval()`, measured against vim 9.2.0900
and nvim 0.12.4 (which agree on all 32). Thirty lines disagreed with this port;
eight still do, and none of the eight is a wording difference.

```
((1)  (1        E110: Missing ')'
[1  [1,2        E696: Missing comma in List:            (empty argument)
[1 2]           E696: Missing comma in List: 2]
{'a' 1} #{a 1}  E720: Missing colon in Dictionary: 1}
{'a': 1 #{a: 1  E722: Missing comma in Dictionary:
{ x -> x        E451: Expected }:
'abc  "abc      E115 / E114: Missing quote: 'abc
f(  f(1  f(1,   E116: Invalid arguments for function f
string(1e3)     E15: Invalid expression: …              (not E116)
]  }  )         E15: Invalid expression: "]"
1 +  1 .  1 ?   E15: Invalid expression: "1 +"          (the WHOLE input)
```

Three pieces of machinery, not a table of guesses: `Parser::rest()` (the source
still unread, which is what every vim diagnostic quotes), `VimlError::silent()`
(a FAIL with nothing to say — `vendor/eval.c:5604`, and the reason `eval('1 +')`
prints one E15 while `eval(']')` prints two), and `lex_prefix` returning the
error that stopped it instead of discarding it.

Statement-level codes fixed in the same sweep, each measured one probe per
keyword: the eight block terminators (`E580`/`E581`/`E582`/`E588`/`E602`/`E603`/
`E606`, which this port collapsed into one invented E580), `E600: Missing
:endtry` (was E170, which is the `:endwhile`/`:endfor` message), `E124`/`E125`
on `:function` and `:def`, `E193: defer not inside a function` (was E1298),
`E129: Function name required` for `defer 5` (was E1300), `E740` for a call with
more arguments than the bytecode operand holds (was an `E118` naming this
crate's own phase numbering), and the swapped `E1278`/`E1279` pair.

## R31-3. `vim_str2nr()` accepted a leading `+` — ✅ FIXED

The name-lookalike sweep's one find. `vendor/charset.c:1228` is
`const bool negative = (ptr[0] == '-')` and nothing else; `str2nr('+42')` is 42
only because `f_str2nr` strips the sign before calling in. Eleven observables
moved to agree with both engines, `'+7' + 0` from 7 to 0 among them. Pinned by
`tests/parity_cases/coerce_leading_sign.vim`.

The rest of the sweep — `/` and `%` on negatives and zero, `float2nr` at
1e18/1e30/inf/nan, `round`/`floor`/`ceil`/`trunc` value and type, `max`/`min` on
empty/mixed/Dict, `sort()`'s default being STRING order, `str2nr` bases, every
`printf` conversion, the case-mapping family, `strlen` vs `strchars` vs
`strcharlen` vs `strwidth` vs `strdisplaywidth`, negative index/slice, the
coercion edges, `stridx`/`match`/`count`, `repeat`/`join`/`split`/`reverse`,
`trim`/`substitute`/`escape`, `has_key`/`get`/`extend`/`empty`, `type()`, the
bitwise builtins on negatives, `index`/`insert`/`remove` — already agreed.

## R31-4. Six assertions that could not fail — ✅ FIXED

`assert_true(auto > 0)` on the auto-allocated match id (passes for the 1 a naive
counter hands out; both engines answer 1000 then 1001 from a reserved range, and
the counter does not rewind on delete), `len(getcompletion('', 'file')) > 0`,
`len(expand('examples/*.vim', 0, 1)) >= 10`, `hostname() != ''`,
`len(env['PATH']) > 0`, `len(ParseKV('a=b')) == 2`. All strengthened against
measured behaviour; none deleted.

`tests/parity_cases.rs` gains `every_record_can_fail`, which rejects a record
holding only the exit-status line and rejects two cases with byte-identical
records. Verified it catches one: a probe whose script was only `let x = 1` was
recorded and the gate failed on it, then the probe was removed.

## Still open

### R31-O1. Four `E<number>` literals still match no engine, by choice or by depth

`E5004` needs the msgpack encoder's `mpstack` path; the three `E474` and one
`E475` in `src/intercepts.rs` are vim codes borrowed for the vimlrs-only
`:Intercept` command and are recorded rather than changed; `E116` from a call on
a non-name callee cannot name the function the way both engines do.

### R31-O2. The corrected parse diagnostics are still unreachable from a script

R30-O1 is unchanged: `source_tolerant()` discards every parse error, so
`endif` on its own line still prints nothing where both engines print
`E580: :endif without :if: endif`. The strings are now right; reporting them is
the separate fix. `eval()` is the one path that reaches the parser at run time,
which is why `tests/parity_cases/parse_error_text.vim` goes through it.

### R31-O3. Five parse failures differ structurally, not in wording

`{x y -> x}` (vim evaluates the dict KEY first and reports E121; this reports
E720 at parse time), `&` and `$` alone (this port does not fail on them at all),
`x->` (vim evaluates the base first), and `string(1e3)`, where the E15 argument
starts at `e3` here and at `1e3` in vim.

### R31-O4. `setcellwidths()` does not check 'listchars'/'fillchars'

`check_chars_options()` (`optionstr.c:2574`) runs after the table is installed
and reverts it when a listed character would no longer occupy one cell. Measured:
`set listchars=eol:¬` then `setcellwidths([[0xac, 0xac, 2]])` is
`E834: Conflicts with value of 'listchars'` with an EMPTY table afterwards in
both engines, and is accepted here. Closing it needs `set_chars_option()`, i.e.
the option-character parser, which this port does not have; a partial
reimplementation would be an ad-hoc replacement, not a port.

### R31-N1. Engine splits this port resolves in Neovim's favour, deliberately

Recorded so a future reader does not "fix" them toward vim. Each was measured in
both engines this round.

| observable | vim 9.2.0900 | Neovim (followed here) |
|---|---|---|
| `toupper('ß')` | `ß` | `ẞ` — vim uses its own 1:1 table, Neovim delegates to utf8proc (`vendor/mbyte.c:1414`). **This port follows VIM here**, per round 5's 2321-codepoint measurement and `case_mapping.vim`. |
| `setcellwidths(1)` | `E1211: List required for argument 1` | `E714: List required` |
| `let n[0:1] = …` on a Number | `E689: Index not allowed after a number: …` | `E689: Can only index a List, Dictionary or Blob` |
| unterminated quote | `Missing single/double quote` | `Missing quote` |
| `f(` | `E116: … for function f(` | `E116: … for function f` |
| `printf` arity | `E767: … for printf()` | `E767: … to printf()` |
| `json_decode('[')` | `E491: JSON decode error at '['` | `E474: Failed to parse [` |
| `str2float('inf')` | `inf` | `str2float('inf')` |
| `v:version` | 902 | 801 |
| `v:count1` under `-es` | 0 | 1 |

### R30-O1, R30-O2, R30-O3, R30-O5 (superseded by R31-2), R30-O6, R30-O8, R29-O1, R29-O3, R28-O1, R27-O1, R27-O2, R26-O1, R26-O2, R26-O4, R26-O5, R24-O5, R22-O3, R23-O1, R25-O1..O7 — unchanged

R30-O4 is closed by R31-0 and R26-O6 by R31-1's E46 work.

`tests/data/fake_fn_allowlist.txt` was not touched this round
(`git diff --exit-code` clean, checked before every commit). `cargo test --lib`
is fully green for the first time since round 26 (413 passed, 0 failed).
`ported_fn_names_match_c` still reports the three standing R22-O3 names
(`f_typename`, `type_name_of`, `member_of`) and was not weakened: `typename()`
is a vim9 builtin with no Neovim counterpart, so its C name cannot appear in a
list generated from `vendor/`, and closing it means either vendoring vim's C
into a corpus defined as Neovim's or adding to the allowlist. Neither was done.
