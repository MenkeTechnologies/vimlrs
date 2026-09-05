" `:help /star` — "Exception: When "*" is used at the start of the pattern or
" just after "^" it matches the star character."
"
" The start-of-branch half already worked; the `^` half did not, and the star was
" attaching to the anchor as a quantifier ("zero or more starts of line"), which
" matches the empty string everywhere.
echo match('ab', '^*a')
echo match('*ab', '^*a')
echo matchstr('*ab', '^*a')
echo match('', '^*$')
echo match('*', '^*$')
echo match('**', '^**')
echo match('a', '^*')
echo match('*', '^*')
" `\_^` is NOT covered by the exception: there the star still repeats.
echo match('a', '\_^*')
" A non-star quantifier after `^` still applies to the anchor.
echo match('a', '^\{1}a')
