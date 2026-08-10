" `exists()` over the option namespace, and the option reads whose value is the
" same under every startup entry point.
"
" `exists('&opt')` is `eval_option(&p, NULL, true) == OK` in the C: a QUERY, so
" the unknown-option branch answers 0 instead of raising E113. Before this case
" existed the whole `&`/`+` branch was missing and every line below answered 0.

" Full name, abbreviation, and the `+` (has-the-feature) spelling.
echo exists('&ignorecase')
echo exists('&ic')
echo exists('+tabstop')
echo exists('+ts')

" Scope prefixes resolve to the same option.
echo exists('&g:ignorecase')
echo exists('&l:ignorecase')

" TTY options have no table index and are still recognised.
echo exists('&t_Co')
echo exists('&term')
echo exists('&ttytype')

" A name that is no option at all, and the two malformed spellings.
echo exists('&nosuchoptionxyz')
echo exists('&')
echo exists('&123')

" Trailing whitespace is skipped; anything else after the name disqualifies.
echo exists('&ic ')
echo exists('&ic   ')
echo exists('&ic x')
echo exists('&ic.')

" The `$` form, which shared the same gap.
echo exists('$HOME')
echo exists('$NOSUCHENVVAR_VIMLRS_XYZ')

" Values that do not depend on how the editor was started: identical under
" `-u NONE` (compatible), `-N`, `--clean`, and in Neovim.
echo &encoding
echo &fileformat
echo &iskeyword
echo &isprint
echo &isfname

" ... and the same options read through their abbreviations.
echo &enc
echo &ff
echo &isk
echo &isp
echo &isf
