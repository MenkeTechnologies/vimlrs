" c: the `*regparse == '\'` arm of the collection loop (regexp_nfa.c:1937). A
" backslash is an escape inside `[]` only when what follows is in
" REGEXP_INRANGE ("]^-n\") or REGEXP_ABBR ("nrtebdoxuU") — regexp.c:182 —
" and `\d \o \x \u \U` then read a number through coll_get_char(), which
" falls back to a literal backslash when the digit run is empty.
echo '<'.substitute('[','[1-\x43]','X','').'>'
echo match('[', '[1-\x43]')
echo match('A', '[1-\x43]')
echo match('[', '[1-C]')
echo match('[', '[\x43]')
echo match('[', '[1-\x5b]')
echo match('C', '[\x43]')
echo match('a', '[\d97]')
echo match('a', '[\o141]')
echo match("é", '[é]')
echo match("\t", '[\t]')
echo match('r', '[\r]')
echo match("\r", '[\r]')
echo match(']', '[\]]')
echo match('-', '[a\-z]')
echo match('b', '[a\-z]')
echo match('\', '[\\]')
echo match('q', '[\q]')
echo match('\', '[\q]')
echo match('x', '[1-\xzz]')
echo match('\', '[1-\xzz]')
echo match('a', '[\x61-\x63]')
echo match('z', '[\x61-\x63]')
echo match('n', '[\n]')
echo match('e', '[\e]')
