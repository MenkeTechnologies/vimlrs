" After an empty match do_string_sub advances by utfc_ptr2len: a character
" together with its composing marks.
echo substitute("e\u301x", '\%C', '-', 'g')
echo substitute("e\u301x", '\%C', '', 'g')
echo substitute("e\u301\u302x", '\%C', '|', 'g')
echo substitute("ae\u301x", '\%C', '-', 'g')
echo substitute("e\u301x", '', '-', 'g')
