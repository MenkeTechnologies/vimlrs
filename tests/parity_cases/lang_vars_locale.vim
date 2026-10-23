" v:ctype, v:lang, v:lc_time and v:collate name the locale the editor adopted
" at startup (c: set_lang_var, locale.c) — C.UTF-8 under the harness's pin.
echo v:ctype
echo v:lang
echo v:lc_time
echo v:collate
echo type(v:lang) exists('v:collate')
