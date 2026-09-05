" A group's end position is a CHOICE POINT: when what follows the group fails,
" the group has to be retried with a shorter (or empty) match. The engine used to
" commit to the group's first successful end and give up.
echo matchstr('12c3', '\(\_[a-z]\?\)[a-z]')
echo matchstr('12c3', '\(\\\{-}\_[a-z]\?\)[a-z]')
echo matchlist('12c3', '\(\\\{-}\_[a-z]\?\)[a-z]\(^{}\)\?')
echo split('faz', '\(\([[:alpha:][[:punct:]]\)*\|.\)\_.\{2}\|\(\,}\)*')
" And a repetition that is tried and then backed off must not leave its `\zs` /
" `\ze` marks or its captures behind.
echo matchend('ng', '\(.\ze=\)\?')
echo matchend('n=g', '\(.\ze=\)\{-,}')
echo matchstr('ng', '\%(.\ze=\)\?')
echo substitute('ng', '\(.\ze=\)\?', 'X', '')
