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

### 2. `substitute()` with `\=` expression using `.` concat loses the result
- `substitute('abc','.','\=submatch(0).submatch(0)','g')` → Vim `aabbcc`, vimlrs `` (empty)
- `substitute('abc','.','\=submatch(0).submatch(0)','')` → Vim `aabc`, vimlrs `bc`
- The `\=` expression evaluates to empty specifically when it uses the `.`
  concatenation operator. `\=toupper(submatch(0))` and `\=submatch(0)*2` both work.

### 3. `split()` with zero-width pattern `\zs` doesn't split
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
