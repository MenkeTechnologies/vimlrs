//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. The vimlrs analogue of zshrs's
//! `src/fusevm_bridge.rs`: pure fusevm plumbing, no operator logic. It only
//!   1. converts between [`typval_T`] and `fusevm::Value` (the refpool smuggle),
//!   2. holds the per-run bridge state (refpool, last result, echo sink),
//!   3. registers the `VIML_*` builtin handlers, each of which pops operands and
//!      calls the CANONICAL PORTS in `crate::ported::*` (never reimplementing
//!      VimL semantics here).
//!
//! Where a handler reconstructs logic the bytecode replaced (the `eval5`/`eval6`
//! arithmetic dispatch, the `eval7_leader` unary application, the `eval_index`
//! subscripts) it cites the `eval.c` lines, since those tree-walkers are not
//! ported (vimlrs replaces them with bytecode).
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::cell::RefCell;

use fusevm::{Value, VM};

use crate::compile_viml::compile_program;
use crate::ported::eval::encode::{encode_tv2echo, encode_tv2string};
use crate::ported::eval::fs::{
    f_browse, f_browsedir, f_chdir, f_delete, f_executable, f_exepath, f_expand, f_expandcmd,
    f_filecopy, f_filereadable, f_filewritable, f_finddir, f_findfile, f_fnamemodify, f_getcwd,
    f_getfperm, f_getfsize, f_getftime, f_getftype, f_glob, f_glob2regpat, f_globpath,
    f_haslocaldir, f_isabsolutepath, f_isdirectory, f_mkdir, f_pathshorten, f_readblob, f_readdir,
    f_readfile, f_rename, f_resolve, f_setfperm, f_simplify, f_tempname, f_writefile,
};
use crate::ported::eval::funcs::{
    f_abs, f_add, f_and, f_api_info, f_append, f_appendbufline, f_assert_equal, f_assert_exception,
    f_assert_false, f_assert_inrange, f_assert_match, f_assert_notequal, f_assert_notmatch,
    f_assert_report, f_assert_true, f_atan2, f_bufadd, f_bufexists, f_buflisted, f_bufload,
    f_bufloaded, f_bufname, f_bufnr, f_bufwinid, f_bufwinnr, f_byte2line, f_chanclose, f_changenr,
    f_chansend, f_char2nr, f_charcol, f_col, f_confirm, f_copy, f_ctxget, f_ctxpop, f_ctxpush,
    f_ctxset, f_ctxsize, f_cursor, f_debugbreak, f_deepcopy, f_deletebufline, f_dictwatcheradd,
    f_dictwatcherdel, f_did_filetype, f_empty, f_environ, f_escape, f_eventhandler, f_exists,
    f_feedkeys, f_flatten, f_flattennew, f_float2nr, f_fmod, f_fnameescape, f_foreground,
    f_funcref, f_function, f_garbagecollect, f_get, f_getbufinfo, f_getbufline, f_getbufoneline,
    f_getbufvar, f_getchangelist, f_getcharpos, f_getcharsearch, f_getcmdwintype, f_getcurpos,
    f_getcursorcharpos, f_getenv, f_getfontname, f_getjumplist, f_getline, f_getmarklist, f_getpid,
    f_getpos, f_getreg, f_getreginfo, f_getregion, f_getregionpos, f_getregtype, f_gettabinfo,
    f_gettabvar, f_gettabwinvar, f_gettagstack, f_gettext, f_getwininfo, f_getwinpos, f_getwinposx,
    f_getwinposy, f_getwinvar, f_has, f_has_key, f_hlID, f_hlexists, f_id, f_index, f_indexof,
    f_input, f_inputdialog, f_inputlist, f_inputrestore, f_inputsave, f_inputsecret, f_insert,
    f_interrupt, f_invert, f_isinf, f_islocked, f_isnan, f_items, f_jobpid, f_jobresize,
    f_jobstart, f_jobstop, f_jobwait, f_json_decode, f_json_encode, f_keys, f_keytrans,
    f_last_buffer_nr, f_len, f_libcall, f_libcallnr, f_line, f_line2byte, f_list2str, f_localtime,
    f_luaeval, f_match, f_matchbufline, f_matchend, f_matchlist, f_matchstr, f_matchstrlist,
    f_matchstrpos, f_max, f_menu_get, f_min, f_mode, f_msgpackdump, f_msgpackparse, f_nextnonblank,
    f_nr2char, f_or, f_perleval, f_pow, f_prevnonblank, f_printf, f_prompt_appendbuf,
    f_prompt_getinput, f_prompt_getprompt, f_prompt_setcallback, f_prompt_setinterrupt,
    f_prompt_setprompt, f_pum_getpos, f_pumvisible, f_py3eval, f_rand, f_range, f_reduce,
    f_reg_executing, f_reg_recorded, f_reg_recording, f_reltime, f_reltimefloat, f_reltimestr,
    f_repeat, f_reverse, f_rpcnotify, f_rpcrequest, f_rpcstart, f_rpcstop, f_rubyeval,
    f_screenattr, f_screenchar, f_screenchars, f_screencol, f_screenrow, f_screenstring, f_search,
    f_searchdecl, f_searchpair, f_searchpairpos, f_searchpos, f_serverlist, f_serverstart,
    f_serverstop, f_setbufline, f_setbufvar, f_setcharpos, f_setcharsearch, f_setcursorcharpos,
    f_setenv, f_setline, f_setpos, f_setreg, f_settabvar, f_settabwinvar, f_settagstack,
    f_setwinvar, f_sha256, f_shellescape, f_shiftwidth, f_sockconnect, f_soundfold, f_spellbadword,
    f_spellsuggest, f_split, f_srand, f_state, f_stdioopen, f_stdpath, f_str2float, f_strftime,
    f_strptime, f_submatch, f_substitute, f_swapfilelist, f_swapinfo, f_swapname, f_synID,
    f_synIDattr, f_synIDtrans, f_synconcealed, f_synstack, f_system, f_systemlist,
    f_tabpagebuflist, f_tabpagenr, f_tabpagewinnr, f_tagfiles, f_taglist, f_termopen, f_timer_info,
    f_timer_pause, f_timer_start, f_timer_stop, f_timer_stopall, f_type, f_values, f_virtcol,
    f_visualmode, f_wait, f_wildmenumode, f_win_execute, f_win_findbuf, f_win_getid, f_win_gettype,
    f_win_gotoid, f_win_id2tabwin, f_win_id2win, f_win_move_separator, f_win_move_statusline,
    f_win_screenpos, f_win_splitmove, f_winbufnr, f_wincol, f_windowsversion, f_winheight,
    f_winlayout, f_winline, f_winnr, f_winrestcmd, f_winrestview, f_winsaveview, f_winwidth,
    f_wordcount, f_xor, float_op_wrapper,
};
use crate::ported::eval::funcs::{
    f_argc, f_argidx, f_arglistid, f_argv, f_assert_equalfile, f_cindent, f_clearmatches,
    f_cmdcomplete_info, f_complete, f_complete_add, f_complete_check, f_complete_info,
    f_diff_filler, f_diff_hlID, f_digraph_get, f_digraph_getlist, f_digraph_set, f_digraph_setlist,
    f_foldclosed, f_foldclosedend, f_foldlevel, f_foldtext, f_foldtextresult, f_fullcommand,
    f_getchar, f_getcharmod, f_getcharstr, f_getcmdcomplpat, f_getcmdcompltype, f_getcmdline,
    f_getcmdpos, f_getcmdprompt, f_getcmdscreenpos, f_getcmdtype, f_getcompletion,
    f_getcompletiontype, f_getloclist, f_getmatches, f_getmousepos, f_getqflist, f_getscriptinfo,
    f_getstacktrace, f_hasmapto, f_highlight_exists, f_histadd, f_histdel, f_histget, f_histnr,
    f_hostname, f_iconv, f_indent, f_lispindent, f_maparg, f_mapcheck, f_maplist, f_mapset,
    f_matchadd, f_matchaddpos, f_matcharg, f_matchdelete, f_matchfuzzy, f_matchfuzzypos,
    f_menu_info, f_preinserted, f_pyeval, f_pyxeval, f_screenpos, f_searchcount, f_setcmdline,
    f_setcmdpos, f_setloclist, f_setmatches, f_setqflist, f_sign_define, f_sign_getdefined,
    f_sign_getplaced, f_sign_jump, f_sign_place, f_sign_placelist, f_sign_undefine, f_sign_unplace,
    f_sign_unplacelist, f_test_garbagecollect_now, f_test_write_list_log, f_undofile, f_undotree,
    f_virtcol2col, f_wildtrigger,
};
use crate::ported::eval::list::{
    f_count, f_extend, f_extendnew, f_filter, f_foreach, f_map, f_mapnew, f_remove,
    FILTER_MAP_CMD_HOOK, FILTER_MAP_EVAL_HOOK,
};
use crate::ported::eval::typval::{
    f_blob2list, f_join, f_list2blob, f_sort, f_uniq, tv_list_slice_or_index, CALL_FUNC_HOOK,
    FUNC_EXISTS_HOOK, SORT_FUNCREF_HOOK,
};
use crate::ported::eval::typval::{
    tv_get_float, tv_get_number_chk, tv_get_string, tv_list_alloc, tv_list_append_tv,
};
use crate::ported::eval::typval_defs_h::{
    blob_T, listitem_T, typval_T, typval_vval_union::*, varnumber_T, SpecialVarValue::*,
    VarLockStatus::VAR_UNLOCKED, VarType::*,
};
use crate::ported::eval::vars::{eval_variable, set_var, set_vim_var_string, vv::VV_EXCEPTION};
use crate::ported::eval_h::exprtype_T::{self, *};
use crate::ported::message;
use crate::ported::strings::{
    f_byteidx, f_byteidxcomp, f_charclass, f_charidx, f_getcellwidths, f_setcellwidths, f_slice,
    f_str2list, f_str2nr, f_strcharlen, f_strcharpart, f_strchars, f_strdisplaywidth, f_strgetchar,
    f_stridx, f_string, f_strlen, f_strpart, f_strridx, f_strtrans, f_strutf16len, f_strwidth,
    f_tolower, f_toupper, f_tr, f_trim, f_utf16idx,
};
use crate::viml_ast::Stmt;
use crate::viml_lexer::{CaseFlag, CmpOp, VimlError};
use crate::viml_parser::parse_expr;

// ── small typval_T constructors (carve-out helpers; not ported C) ──

fn tv_num(n: varnumber_T) -> typval_T {
    typval_T {
        v_type: VAR_NUMBER,
        v_lock: VAR_UNLOCKED,
        vval: v_number(n),
    }
}
fn tv_flt(f: f64) -> typval_T {
    typval_T {
        v_type: VAR_FLOAT,
        v_lock: VAR_UNLOCKED,
        vval: v_float(f),
    }
}
fn tv_str(s: String) -> typval_T {
    typval_T {
        v_type: VAR_STRING,
        v_lock: VAR_UNLOCKED,
        vval: v_string(s),
    }
}
fn tv_special() -> typval_T {
    typval_T {
        v_type: VAR_SPECIAL,
        v_lock: VAR_UNLOCKED,
        vval: v_special(kSpecialVarNull),
    }
}

// ── builtin op IDs (fusevm CallBuiltin space; AWK uses 1000–2999, VimL 3000+) ──

/// `getvar`: pop name → value.
pub const VIML_GETVAR: u16 = 3000;
/// `setvar`: pop name, value → store.
pub const VIML_SETVAR: u16 = 3001;
/// `truthy`: pop value → `Bool`.
pub const VIML_TRUTHY: u16 = 3002;
/// `boolnum`: pop value → `Int(0/1)`.
pub const VIML_BOOLNUM: u16 = 3003;
/// Coerce the value on top of the stack to an integer (`tv_get_number`), as
/// `range()`/`f_range` coerce their arguments. Used to hoist a non-literal
/// `range()` bound into a native counter loop.
pub const VIML_TONUMBER: u16 = 3004;
/// `+`
pub const VIML_ADD: u16 = 3010;
/// `-`
pub const VIML_SUB: u16 = 3011;
/// `*`
pub const VIML_MUL: u16 = 3012;
/// `/`
pub const VIML_DIV: u16 = 3013;
/// `%`
pub const VIML_MOD: u16 = 3014;
/// `.` / `..`
pub const VIML_CONCAT: u16 = 3015;
/// unary `-`
pub const VIML_NEG: u16 = 3016;
/// unary `+`
pub const VIML_UPLUS: u16 = 3017;
/// unary `!`
pub const VIML_NOT: u16 = 3018;
/// comparison id base; per-operator offset via [`cmp_id`].
pub const VIML_CMP_BASE: u16 = 3020;
// Ignore-case comparison ids = base + op + this offset. The match-case ids
// occupy 3020..=3029; this offset places the ic ids in the reserved gap
// 3030..=3039 (below the op cluster at 3050+ and the builtin-function ids at
// 3100+). Earlier values collided: `0x20` overlapped VIML_INDEX/SLICE/ECHO and
// `0x200` (=3532+) overlapped VIML_FN_GETCHAR…, so `==?` dispatched to those
// instead of comparing — leaving 3030..=3039 reserved for the ic family.
const VIML_CMP_IC_OFFSET: u16 = 10;
/// list constructor (argc = element count).
pub const VIML_MAKE_LIST: u16 = 3050;
/// dict constructor (argc = 2 × pairs).
pub const VIML_MAKE_DICT: u16 = 3051;
/// `base[index]`
pub const VIML_INDEX: u16 = 3052;
/// `base[from:to]`
pub const VIML_SLICE: u16 = 3053;
/// `let base[index] = value` — pop index, base, value; set base[index]=value.
pub const VIML_SETINDEX: u16 = 3054;
/// `let base[idx1:idx2] = list` — pop idx2, idx1, base, value; range-assign.
pub const VIML_SETRANGE: u16 = 3055;
/// `:echo`
pub const VIML_ECHO: u16 = 3060;
/// `:echon`
pub const VIML_ECHON: u16 = 3061;
/// store the bare-expression result.
pub const VIML_SET_RESULT: u16 = 3062;
/// `$ENV`
pub const VIML_GETENV: u16 = 3063;
/// `&option`
pub const VIML_GETOPT: u16 = 3064;
/// `@reg`
pub const VIML_GETREG: u16 = 3065;
/// `:let @reg = …`
pub const VIML_SETREG: u16 = 3570;
/// `:let &opt = …`
pub const VIML_SETOPT: u16 = 3571;
/// `:let $ENV = …`
pub const VIML_SETENV: u16 = 3066;
/// `len()`
pub const VIML_FN_LEN: u16 = 3100;
/// `type()`
pub const VIML_FN_TYPE: u16 = 3101;
/// `string()`
pub const VIML_FN_STRING: u16 = 3102;
/// `empty()`
pub const VIML_FN_EMPTY: u16 = 3103;
/// `abs()`
pub const VIML_FN_ABS: u16 = 3104;
/// `str2nr()`
pub const VIML_FN_STR2NR: u16 = 3105;
/// `str2float()`
pub const VIML_FN_STR2FLOAT: u16 = 3106;
/// `float2nr()`
pub const VIML_FN_FLOAT2NR: u16 = 3107;
/// Second builtin-function batch (`funcs.c`): ids `3108..=3129`.
pub const VIML_FN_STRLEN: u16 = 3108;
/// `tolower()`
pub const VIML_FN_TOLOWER: u16 = 3109;
/// `toupper()`
pub const VIML_FN_TOUPPER: u16 = 3110;
/// `char2nr()`
pub const VIML_FN_CHAR2NR: u16 = 3111;
/// `nr2char()`
pub const VIML_FN_NR2CHAR: u16 = 3112;
/// `repeat()`
pub const VIML_FN_REPEAT: u16 = 3113;
/// `split()`
pub const VIML_FN_SPLIT: u16 = 3114;
/// `join()`
pub const VIML_FN_JOIN: u16 = 3115;
/// `range()`
pub const VIML_FN_RANGE: u16 = 3116;
/// `add()`
pub const VIML_FN_ADD: u16 = 3117;
/// `reverse()`
pub const VIML_FN_REVERSE: u16 = 3118;
/// `get()`
pub const VIML_FN_GET: u16 = 3119;
/// `has_key()`
pub const VIML_FN_HAS_KEY: u16 = 3120;
/// `keys()`
pub const VIML_FN_KEYS: u16 = 3121;
/// `values()`
pub const VIML_FN_VALUES: u16 = 3122;
/// `max()`
pub const VIML_FN_MAX: u16 = 3123;
/// `min()`
pub const VIML_FN_MIN: u16 = 3124;
/// `count()`
pub const VIML_FN_COUNT: u16 = 3125;
/// `index()`
pub const VIML_FN_INDEX: u16 = 3126;
/// `has()`
pub const VIML_FN_HAS: u16 = 3127;
/// `exists()`
pub const VIML_FN_EXISTS: u16 = 3128;
/// `printf()`
pub const VIML_FN_PRINTF: u16 = 3129;
/// `map()` — callback per element (string expr binds `v:val`/`v:key`, or funcref).
pub const VIML_FN_MAP: u16 = 3130;
/// `filter()` — keep elements whose callback is truthy.
pub const VIML_FN_FILTER: u16 = 3131;
/// `sort()` — default string sort, `'n'` numeric.
pub const VIML_FN_SORT: u16 = 3132;
/// `call()` — invoke a funcref/name with an argument list.
pub const VIML_FN_CALL: u16 = 3133;
/// `function()` — a Funcref to a named function.
pub const VIML_FN_FUNCTION: u16 = 3134;
/// Third builtin batch (`funcs.c`): float math, bitwise, string, list/dict —
/// ids `3135..=3158`.
pub const VIML_FN_SQRT: u16 = 3135;
/// `floor()`
pub const VIML_FN_FLOOR: u16 = 3136;
/// `ceil()`
pub const VIML_FN_CEIL: u16 = 3137;
/// `round()`
pub const VIML_FN_ROUND: u16 = 3138;
/// `trunc()`
pub const VIML_FN_TRUNC: u16 = 3139;
/// `log()`
pub const VIML_FN_LOG: u16 = 3140;
/// `exp()`
pub const VIML_FN_EXP: u16 = 3141;
/// `sin()`
pub const VIML_FN_SIN: u16 = 3142;
/// `cos()`
pub const VIML_FN_COS: u16 = 3143;
/// `pow()`
pub const VIML_FN_POW: u16 = 3144;
/// `and()`
pub const VIML_FN_AND: u16 = 3145;
/// `or()`
pub const VIML_FN_OR: u16 = 3146;
/// `xor()`
pub const VIML_FN_XOR: u16 = 3147;
/// `invert()`
pub const VIML_FN_INVERT: u16 = 3148;
/// `strchars()`
pub const VIML_FN_STRCHARS: u16 = 3149;
/// `strpart()`
pub const VIML_FN_STRPART: u16 = 3150;
/// `stridx()`
pub const VIML_FN_STRIDX: u16 = 3151;
/// `trim()`
pub const VIML_FN_TRIM: u16 = 3152;
/// `insert()`
pub const VIML_FN_INSERT: u16 = 3153;
/// `remove()`
pub const VIML_FN_REMOVE: u16 = 3154;
/// `extend()`
pub const VIML_FN_EXTEND: u16 = 3155;
/// `copy()`
pub const VIML_FN_COPY: u16 = 3156;
/// `items()`
pub const VIML_FN_ITEMS: u16 = 3157;
/// `uniq()`
pub const VIML_FN_UNIQ: u16 = 3158;
/// `matchstr()` — Vim-regex matched substring.
pub const VIML_FN_MATCHSTR: u16 = 3159;
/// `match()` — Vim-regex match index.
pub const VIML_FN_MATCH: u16 = 3160;
/// `substitute()` — Vim-regex replace.
pub const VIML_FN_SUBSTITUTE: u16 = 3161;
/// `matchlist()`
pub const VIML_FN_MATCHLIST: u16 = 3162;
/// `matchend()`
pub const VIML_FN_MATCHEND: u16 = 3163;
/// `strridx()`
pub const VIML_FN_STRRIDX: u16 = 3164;
/// `escape()`
pub const VIML_FN_ESCAPE: u16 = 3165;
/// `tr()`
pub const VIML_FN_TR: u16 = 3166;
/// `str2list()`
pub const VIML_FN_STR2LIST: u16 = 3167;
/// `list2str()`
pub const VIML_FN_LIST2STR: u16 = 3168;
/// `flatten()`
pub const VIML_FN_FLATTEN: u16 = 3169;
/// `reduce()` — fold a List with a funcref `f(acc, val)` (callback, bridge-side).
pub const VIML_FN_REDUCE: u16 = 3170;
/// `eval()` — evaluate a string as an expression (bridge-side).
pub const VIML_FN_EVAL: u16 = 3171;
/// `execute()` — run ex commands and capture their output (bridge-side).
pub const VIML_FN_EXECUTE: u16 = 3172;
/// `deepcopy()`
pub const VIML_FN_DEEPCOPY: u16 = 3173;
/// `fmod()`
pub const VIML_FN_FMOD: u16 = 3174;
/// `atan2()`
pub const VIML_FN_ATAN2: u16 = 3175;
/// `tan()`
pub const VIML_FN_TAN: u16 = 3176;
/// `atan()`
pub const VIML_FN_ATAN: u16 = 3177;
/// `asin()`
pub const VIML_FN_ASIN: u16 = 3178;
/// `acos()`
pub const VIML_FN_ACOS: u16 = 3179;
/// `sinh()`
pub const VIML_FN_SINH: u16 = 3180;
/// `cosh()`
pub const VIML_FN_COSH: u16 = 3181;
/// `tanh()`
pub const VIML_FN_TANH: u16 = 3182;
/// `log10()`
pub const VIML_FN_LOG10: u16 = 3183;
/// `:execute` statement: pop argc values, join with spaces, run as a command.
pub const VIML_EXEC_STMT: u16 = 3184;
/// `:set` statement: pop the argument string, apply via `option::do_set`.
pub const VIML_SET: u16 = 3185;
/// `:map`-family statement: pop the raw command line, apply via `do_map`.
pub const VIML_MAP: u16 = 3562;
/// `:command` statement: pop the args, define a user command via `ex_command`.
pub const VIML_COMMAND: u16 = 3563;
/// `:delcommand` statement: pop the name, delete via `ex_delcommand`.
pub const VIML_DELCOMMAND: u16 = 3564;
/// User-command invocation: pop the raw line, expand + run the replacement.
pub const VIML_USERCMD: u16 = 3565;
/// `:autocmd` statement: pop the args, register via `do_autocmd`.
pub const VIML_AUTOCMD: u16 = 3566;
/// `:augroup` statement: pop the name, set the group via `do_augroup`.
pub const VIML_AUGROUP: u16 = 3567;
/// `:doautocmd` statement: pop the args, fire matching autocommands.
pub const VIML_DOAUTOCMD: u16 = 3568;
/// `:[range]cmd` statement: pop the raw line, run it against the buffer.
pub const VIML_EXCMD: u16 = 3569;
/// `:source {file}`: pop the filename, read and run it in the current scope.
pub const VIML_SOURCE: u16 = 3500;
/// `:unlet {name}`: pop the name, delete the variable.
pub const VIML_UNLET: u16 = 3501;
/// `json_encode()`
pub const VIML_FN_JSON_ENCODE: u16 = 3186;
/// `json_decode()`
pub const VIML_FN_JSON_DECODE: u16 = 3187;
/// `strgetchar()`
pub const VIML_FN_STRGETCHAR: u16 = 3188;
/// `strcharpart()`
pub const VIML_FN_STRCHARPART: u16 = 3189;
/// `byteidx()`
pub const VIML_FN_BYTEIDX: u16 = 3190;
/// `charidx()`
pub const VIML_FN_CHARIDX: u16 = 3191;
/// `matchstrpos()`
pub const VIML_FN_MATCHSTRPOS: u16 = 3192;
/// `extendnew()`
pub const VIML_FN_EXTENDNEW: u16 = 3193;
/// `getenv()`
pub const VIML_FN_GETENV: u16 = 3194;
/// `setenv()`
pub const VIML_FN_SETENV: u16 = 3195;
/// `shellescape()`
pub const VIML_FN_SHELLESCAPE: u16 = 3196;
/// `isinf()`
pub const VIML_FN_ISINF: u16 = 3197;
/// `isnan()`
pub const VIML_FN_ISNAN: u16 = 3198;
/// `getpid()`
pub const VIML_FN_GETPID: u16 = 3199;
/// `localtime()`
pub const VIML_FN_LOCALTIME: u16 = 3200;
/// `soundfold()`
pub const VIML_FN_SOUNDFOLD: u16 = 3201;
/// `byteidxcomp()`
pub const VIML_FN_BYTEIDXCOMP: u16 = 3202;
/// `reltime()`
pub const VIML_FN_RELTIME: u16 = 3203;
/// `reltimestr()`
pub const VIML_FN_RELTIMESTR: u16 = 3204;
/// `reltimefloat()`
pub const VIML_FN_RELTIMEFLOAT: u16 = 3205;
/// `rand()`
pub const VIML_FN_RAND: u16 = 3206;
/// `srand()`
pub const VIML_FN_SRAND: u16 = 3207;
/// `strftime()`
pub const VIML_FN_STRFTIME: u16 = 3208;
/// `strptime()`
pub const VIML_FN_STRPTIME: u16 = 3209;
/// `pathshorten()`
pub const VIML_FN_PATHSHORTEN: u16 = 3210;
/// eval/fs.c filesystem builtins.
pub const VIML_FN_ISABSOLUTEPATH: u16 = 3219;
/// `simplify()`
pub const VIML_FN_SIMPLIFY: u16 = 3220;
/// `filereadable()`
pub const VIML_FN_FILEREADABLE: u16 = 3221;
/// `filewritable()`
pub const VIML_FN_FILEWRITABLE: u16 = 3222;
/// `isdirectory()`
pub const VIML_FN_ISDIRECTORY: u16 = 3223;
/// `getfsize()`
pub const VIML_FN_GETFSIZE: u16 = 3224;
/// `getftype()`
pub const VIML_FN_GETFTYPE: u16 = 3225;
/// `getftime()`
pub const VIML_FN_GETFTIME: u16 = 3226;
/// `getfperm()`
pub const VIML_FN_GETFPERM: u16 = 3227;
/// `setfperm()`
pub const VIML_FN_SETFPERM: u16 = 3228;
/// `getcwd()`
pub const VIML_FN_GETCWD: u16 = 3229;
/// `chdir()`
pub const VIML_FN_CHDIR: u16 = 3230;
/// `executable()`
pub const VIML_FN_EXECUTABLE: u16 = 3231;
/// `exepath()`
pub const VIML_FN_EXEPATH: u16 = 3232;
/// `tempname()`
pub const VIML_FN_TEMPNAME: u16 = 3233;
/// `mkdir()`
pub const VIML_FN_MKDIR: u16 = 3234;
/// `delete()`
pub const VIML_FN_DELETE: u16 = 3235;
/// `rename()`
pub const VIML_FN_RENAME: u16 = 3236;
/// `readfile()`
pub const VIML_FN_READFILE: u16 = 3237;
/// `writefile()`
pub const VIML_FN_WRITEFILE: u16 = 3238;
/// `fnamemodify()`
pub const VIML_FN_FNAMEMODIFY: u16 = 3239;
/// `filecopy()`
pub const VIML_FN_FILECOPY: u16 = 3240;
/// `haslocaldir()`
pub const VIML_FN_HASLOCALDIR: u16 = 3241;
/// `resolve()`
pub const VIML_FN_RESOLVE: u16 = 3242;
/// `glob2regpat()`
pub const VIML_FN_GLOB2REGPAT: u16 = 3243;
/// `readdir()`
pub const VIML_FN_READDIR: u16 = 3244;
/// `readblob()`
pub const VIML_FN_READBLOB: u16 = 3245;
/// `getreg()`
pub const VIML_FN_GETREG: u16 = 3246;
/// `getregtype()`
pub const VIML_FN_GETREGTYPE: u16 = 3247;
/// `getreginfo()`
pub const VIML_FN_GETREGINFO: u16 = 3248;
/// `setreg()`
pub const VIML_FN_SETREG: u16 = 3249;
/// `reg_recording()`
pub const VIML_FN_REG_RECORDING: u16 = 3250;
/// `reg_executing()`
pub const VIML_FN_REG_EXECUTING: u16 = 3251;
/// `reg_recorded()`
pub const VIML_FN_REG_RECORDED: u16 = 3252;
/// `gettext()`
pub const VIML_FN_GETTEXT: u16 = 3253;
/// `garbagecollect()`
pub const VIML_FN_GARBAGECOLLECT: u16 = 3254;
/// `funcref()`
pub const VIML_FN_FUNCREF: u16 = 3255;
/// `id()`
pub const VIML_FN_ID: u16 = 3256;
/// `indexof()`
pub const VIML_FN_INDEXOF: u16 = 3257;
/// `matchstrlist()`
pub const VIML_FN_MATCHSTRLIST: u16 = 3258;
/// `fnameescape()`
pub const VIML_FN_FNAMEESCAPE: u16 = 3259;
/// `shiftwidth()`
pub const VIML_FN_SHIFTWIDTH: u16 = 3260;
/// `mode()`
pub const VIML_FN_MODE: u16 = 3261;
/// `state()`
pub const VIML_FN_STATE: u16 = 3262;
/// `visualmode()`
pub const VIML_FN_VISUALMODE: u16 = 3263;
/// `pumvisible()`
pub const VIML_FN_PUMVISIBLE: u16 = 3264;
/// `wildmenumode()`
pub const VIML_FN_WILDMENUMODE: u16 = 3265;
/// `did_filetype()`
pub const VIML_FN_DID_FILETYPE: u16 = 3266;
/// `eventhandler()`
pub const VIML_FN_EVENTHANDLER: u16 = 3267;
/// `hlexists()`
pub const VIML_FN_HLEXISTS: u16 = 3268;
/// `windowsversion()`
pub const VIML_FN_WINDOWSVERSION: u16 = 3269;
/// `getfontname()`
pub const VIML_FN_GETFONTNAME: u16 = 3270;
/// `foreground()`
pub const VIML_FN_FOREGROUND: u16 = 3271;
/// `prompt_getprompt()`
pub const VIML_FN_PROMPT_GETPROMPT: u16 = 3272;
/// `pum_getpos()`
pub const VIML_FN_PUM_GETPOS: u16 = 3273;
/// `serverlist()`
pub const VIML_FN_SERVERLIST: u16 = 3274;
/// `getpos()`
pub const VIML_FN_GETPOS: u16 = 3275;
/// `getcharpos()`
pub const VIML_FN_GETCHARPOS: u16 = 3276;
/// `getcurpos()`
pub const VIML_FN_GETCURPOS: u16 = 3277;
/// `getcursorcharpos()`
pub const VIML_FN_GETCURSORCHARPOS: u16 = 3278;
/// `col()`
pub const VIML_FN_COL: u16 = 3279;
/// `charcol()`
pub const VIML_FN_CHARCOL: u16 = 3280;
/// `line()`
pub const VIML_FN_LINE: u16 = 3281;
/// `virtcol()`
pub const VIML_FN_VIRTCOL: u16 = 3282;
/// `screenrow()`
pub const VIML_FN_SCREENROW: u16 = 3283;
/// `screencol()`
pub const VIML_FN_SCREENCOL: u16 = 3284;
/// `screenchar()`
pub const VIML_FN_SCREENCHAR: u16 = 3285;
/// `screenattr()`
pub const VIML_FN_SCREENATTR: u16 = 3286;
/// `screenchars()`
pub const VIML_FN_SCREENCHARS: u16 = 3287;
/// `screenstring()`
pub const VIML_FN_SCREENSTRING: u16 = 3288;
/// `line2byte()`
pub const VIML_FN_LINE2BYTE: u16 = 3289;
/// `byte2line()`
pub const VIML_FN_BYTE2LINE: u16 = 3290;
/// `nextnonblank()`
pub const VIML_FN_NEXTNONBLANK: u16 = 3291;
/// `prevnonblank()`
pub const VIML_FN_PREVNONBLANK: u16 = 3292;
/// `wordcount()`
pub const VIML_FN_WORDCOUNT: u16 = 3293;
/// `getjumplist()`
pub const VIML_FN_GETJUMPLIST: u16 = 3294;
/// `getchangelist()`
pub const VIML_FN_GETCHANGELIST: u16 = 3295;
/// `getmarklist()`
pub const VIML_FN_GETMARKLIST: u16 = 3296;
/// `gettagstack()`
pub const VIML_FN_GETTAGSTACK: u16 = 3297;
/// `tagfiles()`
pub const VIML_FN_TAGFILES: u16 = 3298;
/// `taglist()`
pub const VIML_FN_TAGLIST: u16 = 3299;
/// `tabpagebuflist()`
pub const VIML_FN_TABPAGEBUFLIST: u16 = 3300;
/// `search()`
pub const VIML_FN_SEARCH: u16 = 3301;
/// `searchpos()`
pub const VIML_FN_SEARCHPOS: u16 = 3302;
/// `searchpair()`
pub const VIML_FN_SEARCHPAIR: u16 = 3303;
/// `searchpairpos()`
pub const VIML_FN_SEARCHPAIRPOS: u16 = 3304;
/// `searchdecl()`
pub const VIML_FN_SEARCHDECL: u16 = 3305;
/// `getcharsearch()`
pub const VIML_FN_GETCHARSEARCH: u16 = 3306;
/// `input()`
pub const VIML_FN_INPUT: u16 = 3307;
/// `inputsecret()`
pub const VIML_FN_INPUTSECRET: u16 = 3308;
/// `inputdialog()`
pub const VIML_FN_INPUTDIALOG: u16 = 3309;
/// `inputlist()`
pub const VIML_FN_INPUTLIST: u16 = 3310;
/// `inputsave()`
pub const VIML_FN_INPUTSAVE: u16 = 3311;
/// `inputrestore()`
pub const VIML_FN_INPUTRESTORE: u16 = 3312;
/// `confirm()`
pub const VIML_FN_CONFIRM: u16 = 3313;
/// `synID()`
pub const VIML_FN_SYNID: u16 = 3314;
/// `synIDtrans()`
pub const VIML_FN_SYNIDTRANS: u16 = 3315;
/// `synIDattr()`
pub const VIML_FN_SYNIDATTR: u16 = 3316;
/// `synstack()`
pub const VIML_FN_SYNSTACK: u16 = 3317;
/// `synconcealed()`
pub const VIML_FN_SYNCONCEALED: u16 = 3318;
/// `changenr()`
pub const VIML_FN_CHANGENR: u16 = 3319;
/// `swapname()`
pub const VIML_FN_SWAPNAME: u16 = 3320;
/// `swapfilelist()`
pub const VIML_FN_SWAPFILELIST: u16 = 3321;
/// `spellbadword()`
pub const VIML_FN_SPELLBADWORD: u16 = 3322;
/// `spellsuggest()`
pub const VIML_FN_SPELLSUGGEST: u16 = 3323;
/// `getregion()`
pub const VIML_FN_GETREGION: u16 = 3324;
/// `getregionpos()`
pub const VIML_FN_GETREGIONPOS: u16 = 3325;
/// `matchbufline()`
pub const VIML_FN_MATCHBUFLINE: u16 = 3326;
/// `menu_get()`
pub const VIML_FN_MENU_GET: u16 = 3327;
/// `timer_info()`
pub const VIML_FN_TIMER_INFO: u16 = 3328;
/// `timer_start()`
pub const VIML_FN_TIMER_START: u16 = 3329;
/// `timer_stop()`
pub const VIML_FN_TIMER_STOP: u16 = 3330;
/// `timer_pause()`
pub const VIML_FN_TIMER_PAUSE: u16 = 3331;
/// `timer_stopall()`
pub const VIML_FN_TIMER_STOPALL: u16 = 3332;
/// `setpos()`
pub const VIML_FN_SETPOS: u16 = 3333;
/// `setcharpos()`
pub const VIML_FN_SETCHARPOS: u16 = 3334;
/// `cursor()`
pub const VIML_FN_CURSOR: u16 = 3335;
/// `setcursorcharpos()`
pub const VIML_FN_SETCURSORCHARPOS: u16 = 3336;
/// `setcharsearch()`
pub const VIML_FN_SETCHARSEARCH: u16 = 3337;
/// `settagstack()`
pub const VIML_FN_SETTAGSTACK: u16 = 3338;
/// `assert_equal()`
pub const VIML_FN_ASSERT_EQUAL: u16 = 3339;
/// `assert_notequal()`
pub const VIML_FN_ASSERT_NOTEQUAL: u16 = 3340;
/// `assert_true()`
pub const VIML_FN_ASSERT_TRUE: u16 = 3341;
/// `assert_false()`
pub const VIML_FN_ASSERT_FALSE: u16 = 3342;
/// `assert_match()`
pub const VIML_FN_ASSERT_MATCH: u16 = 3343;
/// `assert_notmatch()`
pub const VIML_FN_ASSERT_NOTMATCH: u16 = 3344;
/// `assert_report()`
pub const VIML_FN_ASSERT_REPORT: u16 = 3345;
/// `assert_inrange()`
pub const VIML_FN_ASSERT_INRANGE: u16 = 3346;
/// `assert_exception()`
pub const VIML_FN_ASSERT_EXCEPTION: u16 = 3347;
/// `assert_fails()`
pub const VIML_FN_ASSERT_FAILS: u16 = 3348;
/// `system()`
pub const VIML_FN_SYSTEM: u16 = 3349;
/// `systemlist()`
pub const VIML_FN_SYSTEMLIST: u16 = 3350;
/// `environ()`
pub const VIML_FN_ENVIRON: u16 = 3351;
/// `slice()`
pub const VIML_FN_SLICE: u16 = 3352;
/// `strcharlen()`
pub const VIML_FN_STRCHARLEN: u16 = 3353;
/// `strtrans()`
pub const VIML_FN_STRTRANS: u16 = 3354;
/// `strwidth()`
pub const VIML_FN_STRWIDTH: u16 = 3355;
/// `strdisplaywidth()`
pub const VIML_FN_STRDISPLAYWIDTH: u16 = 3356;
/// `charclass()`
pub const VIML_FN_CHARCLASS: u16 = 3357;
/// `glob()`
pub const VIML_FN_GLOB: u16 = 3358;
/// `globpath()`
pub const VIML_FN_GLOBPATH: u16 = 3359;
/// `strutf16len()`
pub const VIML_FN_STRUTF16LEN: u16 = 3360;
/// `utf16idx()`
pub const VIML_FN_UTF16IDX: u16 = 3361;
/// `bufnr()`
pub const VIML_FN_BUFNR: u16 = 3362;
/// `bufexists()`
pub const VIML_FN_BUFEXISTS: u16 = 3363;
/// `buflisted()`
pub const VIML_FN_BUFLISTED: u16 = 3364;
/// `bufloaded()`
pub const VIML_FN_BUFLOADED: u16 = 3365;
/// `bufname()`
pub const VIML_FN_BUFNAME: u16 = 3366;
/// `bufwinnr()`
pub const VIML_FN_BUFWINNR: u16 = 3367;
/// `bufwinid()`
pub const VIML_FN_BUFWINID: u16 = 3368;
/// `winnr()`
pub const VIML_FN_WINNR: u16 = 3369;
/// `winbufnr()`
pub const VIML_FN_WINBUFNR: u16 = 3370;
/// `winwidth()`
pub const VIML_FN_WINWIDTH: u16 = 3371;
/// `winheight()`
pub const VIML_FN_WINHEIGHT: u16 = 3372;
/// `winlayout()`
pub const VIML_FN_WINLAYOUT: u16 = 3373;
/// `winline()`
pub const VIML_FN_WINLINE: u16 = 3374;
/// `wincol()`
pub const VIML_FN_WINCOL: u16 = 3375;
/// `winrestcmd()`
pub const VIML_FN_WINRESTCMD: u16 = 3376;
/// `tabpagenr()`
pub const VIML_FN_TABPAGENR: u16 = 3377;
/// `tabpagewinnr()`
pub const VIML_FN_TABPAGEWINNR: u16 = 3378;
/// `getline()`
pub const VIML_FN_GETLINE: u16 = 3379;
/// `getbufline()`
pub const VIML_FN_GETBUFLINE: u16 = 3380;
/// `getbufoneline()`
pub const VIML_FN_GETBUFONELINE: u16 = 3381;
/// `getbufinfo()`
pub const VIML_FN_GETBUFINFO: u16 = 3382;
/// `setline()`
pub const VIML_FN_SETLINE: u16 = 3383;
/// `setbufline()`
pub const VIML_FN_SETBUFLINE: u16 = 3384;
/// `append()`
pub const VIML_FN_APPEND: u16 = 3385;
/// `appendbufline()`
pub const VIML_FN_APPENDBUFLINE: u16 = 3386;
/// `deletebufline()`
pub const VIML_FN_DELETEBUFLINE: u16 = 3387;
/// `getwininfo()`
pub const VIML_FN_GETWININFO: u16 = 3388;
/// `gettabinfo()`
pub const VIML_FN_GETTABINFO: u16 = 3389;
/// `getwinpos()`
pub const VIML_FN_GETWINPOS: u16 = 3390;
/// `getwinposx()`
pub const VIML_FN_GETWINPOSX: u16 = 3391;
/// `getwinposy()`
pub const VIML_FN_GETWINPOSY: u16 = 3392;
/// `win_getid()`
pub const VIML_FN_WIN_GETID: u16 = 3393;
/// `win_id2win()`
pub const VIML_FN_WIN_ID2WIN: u16 = 3394;
/// `win_findbuf()`
pub const VIML_FN_WIN_FINDBUF: u16 = 3395;
/// `win_gotoid()`
pub const VIML_FN_WIN_GOTOID: u16 = 3396;
/// `win_gettype()`
pub const VIML_FN_WIN_GETTYPE: u16 = 3397;
/// `win_screenpos()`
pub const VIML_FN_WIN_SCREENPOS: u16 = 3398;
/// `expand()`
pub const VIML_FN_EXPAND: u16 = 3399;
/// `expandcmd()`
pub const VIML_FN_EXPANDCMD: u16 = 3400;
/// `win_id2tabwin()`
pub const VIML_FN_WIN_ID2TABWIN: u16 = 3401;
/// `win_splitmove()`
pub const VIML_FN_WIN_SPLITMOVE: u16 = 3402;
/// `win_move_separator()`
pub const VIML_FN_WIN_MOVE_SEPARATOR: u16 = 3403;
/// `win_move_statusline()`
pub const VIML_FN_WIN_MOVE_STATUSLINE: u16 = 3404;
/// `getcmdwintype()`
pub const VIML_FN_GETCMDWINTYPE: u16 = 3405;
/// `winrestview()`
pub const VIML_FN_WINRESTVIEW: u16 = 3406;
/// `winsaveview()`
pub const VIML_FN_WINSAVEVIEW: u16 = 3407;
/// `bufload()`
pub const VIML_FN_BUFLOAD: u16 = 3408;
/// `prompt_getinput()`
pub const VIML_FN_PROMPT_GETINPUT: u16 = 3409;
/// `prompt_setprompt()`
pub const VIML_FN_PROMPT_SETPROMPT: u16 = 3410;
/// `prompt_setcallback()`
pub const VIML_FN_PROMPT_SETCALLBACK: u16 = 3411;
/// `prompt_setinterrupt()`
pub const VIML_FN_PROMPT_SETINTERRUPT: u16 = 3412;
/// `interrupt()`
pub const VIML_FN_INTERRUPT: u16 = 3413;
/// `debugbreak()`
pub const VIML_FN_DEBUGBREAK: u16 = 3414;
/// `api_info()`
pub const VIML_FN_API_INFO: u16 = 3415;
/// `swapinfo()`
pub const VIML_FN_SWAPINFO: u16 = 3416;
/// `serverstart()`
pub const VIML_FN_SERVERSTART: u16 = 3417;
/// `serverstop()`
pub const VIML_FN_SERVERSTOP: u16 = 3418;
/// `getbufvar()`
pub const VIML_FN_GETBUFVAR: u16 = 3419;
/// `getwinvar()`
pub const VIML_FN_GETWINVAR: u16 = 3420;
/// `gettabvar()`
pub const VIML_FN_GETTABVAR: u16 = 3421;
/// `gettabwinvar()`
pub const VIML_FN_GETTABWINVAR: u16 = 3422;
/// `setbufvar()`
pub const VIML_FN_SETBUFVAR: u16 = 3423;
/// `setwinvar()`
pub const VIML_FN_SETWINVAR: u16 = 3424;
/// `settabvar()`
pub const VIML_FN_SETTABVAR: u16 = 3425;
/// `settabwinvar()`
pub const VIML_FN_SETTABWINVAR: u16 = 3426;
/// `jobstart()`
pub const VIML_FN_JOBSTART: u16 = 3427;
/// `jobpid()`
pub const VIML_FN_JOBPID: u16 = 3428;
/// `jobstop()`
pub const VIML_FN_JOBSTOP: u16 = 3429;
/// `jobwait()`
pub const VIML_FN_JOBWAIT: u16 = 3430;
/// `jobresize()`
pub const VIML_FN_JOBRESIZE: u16 = 3431;
/// `chanclose()`
pub const VIML_FN_CHANCLOSE: u16 = 3432;
/// `chansend()`
pub const VIML_FN_CHANSEND: u16 = 3433;
/// `feedkeys()`
pub const VIML_FN_FEEDKEYS: u16 = 3434;
/// `wait()`
pub const VIML_FN_WAIT: u16 = 3435;
/// `sockconnect()`
pub const VIML_FN_SOCKCONNECT: u16 = 3436;
/// `win_execute()`
pub const VIML_FN_WIN_EXECUTE: u16 = 3437;
/// `bufadd()`
pub const VIML_FN_BUFADD: u16 = 3438;
/// `ctxget()`
pub const VIML_FN_CTXGET: u16 = 3439;
/// `ctxpop()`
pub const VIML_FN_CTXPOP: u16 = 3440;
/// `ctxpush()`
pub const VIML_FN_CTXPUSH: u16 = 3441;
/// `ctxset()`
pub const VIML_FN_CTXSET: u16 = 3442;
/// `ctxsize()`
pub const VIML_FN_CTXSIZE: u16 = 3443;
/// `islocked()`
pub const VIML_FN_ISLOCKED: u16 = 3444;
/// `last_buffer_nr()`
pub const VIML_FN_LAST_BUFFER_NR: u16 = 3445;
/// `libcall()`
pub const VIML_FN_LIBCALL: u16 = 3446;
/// `libcallnr()`
pub const VIML_FN_LIBCALLNR: u16 = 3447;
/// `msgpackdump()`
pub const VIML_FN_MSGPACKDUMP: u16 = 3448;
/// `msgpackparse()`
pub const VIML_FN_MSGPACKPARSE: u16 = 3449;
/// `rpcnotify()`
pub const VIML_FN_RPCNOTIFY: u16 = 3450;
/// `rpcrequest()`
pub const VIML_FN_RPCREQUEST: u16 = 3451;
/// `rpcstart()`
pub const VIML_FN_RPCSTART: u16 = 3452;
/// `rpcstop()`
pub const VIML_FN_RPCSTOP: u16 = 3453;
/// `stdioopen()`
pub const VIML_FN_STDIOOPEN: u16 = 3454;
/// `submatch()`
pub const VIML_FN_SUBMATCH: u16 = 3455;
/// `prompt_appendbuf()`
pub const VIML_FN_PROMPT_APPENDBUF: u16 = 3456;
/// `py3eval()`
pub const VIML_FN_PY3EVAL: u16 = 3457;
/// `perleval()`
pub const VIML_FN_PERLEVAL: u16 = 3458;
/// `stdpath()`
pub const VIML_FN_STDPATH: u16 = 3459;
/// `keytrans()`
pub const VIML_FN_KEYTRANS: u16 = 3460;
/// `luaeval()`
pub const VIML_FN_LUAEVAL: u16 = 3461;
/// `rubyeval()`
pub const VIML_FN_RUBYEVAL: u16 = 3462;
/// `termopen()`
pub const VIML_FN_TERMOPEN: u16 = 3463;
/// `browse()`
pub const VIML_FN_BROWSE: u16 = 3464;
/// `browsedir()`
pub const VIML_FN_BROWSEDIR: u16 = 3465;
/// `finddir()`
pub const VIML_FN_FINDDIR: u16 = 3466;
/// `findfile()`
pub const VIML_FN_FINDFILE: u16 = 3467;
/// `matchfuzzy()`
pub const VIML_FN_MATCHFUZZY: u16 = 3468;
/// `matchfuzzypos()`
pub const VIML_FN_MATCHFUZZYPOS: u16 = 3469;
/// `histadd()`
pub const VIML_FN_HISTADD: u16 = 3470;
/// `histget()`
pub const VIML_FN_HISTGET: u16 = 3471;
/// `histnr()`
pub const VIML_FN_HISTNR: u16 = 3472;
/// `histdel()`
pub const VIML_FN_HISTDEL: u16 = 3473;
/// `digraph_get()`
pub const VIML_FN_DIGRAPH_GET: u16 = 3474;
/// `digraph_set()`
pub const VIML_FN_DIGRAPH_SET: u16 = 3475;
/// `digraph_getlist()`
pub const VIML_FN_DIGRAPH_GETLIST: u16 = 3476;
/// `digraph_setlist()`
pub const VIML_FN_DIGRAPH_SETLIST: u16 = 3477;
/// `setcellwidths()`
pub const VIML_FN_SETCELLWIDTHS: u16 = 3478;
/// `getcellwidths()`
pub const VIML_FN_GETCELLWIDTHS: u16 = 3479;
/// `hostname()`
pub const VIML_FN_HOSTNAME: u16 = 3480;
/// `iconv()`
pub const VIML_FN_ICONV: u16 = 3481;
/// `argc()`
pub const VIML_FN_ARGC: u16 = 3482;
/// `argidx()`
pub const VIML_FN_ARGIDX: u16 = 3483;
/// `argv()`
pub const VIML_FN_ARGV: u16 = 3484;
/// `assert_equalfile()`
pub const VIML_FN_ASSERT_EQUALFILE: u16 = 3485;
/// `arglistid()`
pub const VIML_FN_ARGLISTID: u16 = 3486;
/// `foldlevel()`
pub const VIML_FN_FOLDLEVEL: u16 = 3487;
/// `matchadd()`
pub const VIML_FN_MATCHADD: u16 = 3488;
/// `matchaddpos()`
pub const VIML_FN_MATCHADDPOS: u16 = 3489;
/// `matchdelete()`
pub const VIML_FN_MATCHDELETE: u16 = 3490;
/// `getmatches()`
pub const VIML_FN_GETMATCHES: u16 = 3491;
/// `setmatches()`
pub const VIML_FN_SETMATCHES: u16 = 3492;
/// `clearmatches()`
pub const VIML_FN_CLEARMATCHES: u16 = 3493;
/// `matcharg()`
pub const VIML_FN_MATCHARG: u16 = 3494;
/// `sign_define()`
pub const VIML_FN_SIGN_DEFINE: u16 = 3495;
/// `sign_getdefined()`
pub const VIML_FN_SIGN_GETDEFINED: u16 = 3496;
/// `sign_undefine()`
pub const VIML_FN_SIGN_UNDEFINE: u16 = 3497;
/// `foldclosed()`
pub const VIML_FN_FOLDCLOSED: u16 = 3498;
/// `foldclosedend()`
pub const VIML_FN_FOLDCLOSEDEND: u16 = 3499;
/// `hasmapto()`
pub const VIML_FN_HASMAPTO: u16 = 3503;
/// `maparg()`
pub const VIML_FN_MAPARG: u16 = 3504;
/// `mapcheck()`
pub const VIML_FN_MAPCHECK: u16 = 3505;
/// `maplist()`
pub const VIML_FN_MAPLIST: u16 = 3506;
/// `setcmdline()`
pub const VIML_FN_SETCMDLINE: u16 = 3507;
/// `getcmdline()`
pub const VIML_FN_GETCMDLINE: u16 = 3508;
/// `setcmdpos()`
pub const VIML_FN_SETCMDPOS: u16 = 3509;
/// `getcmdpos()`
pub const VIML_FN_GETCMDPOS: u16 = 3510;
/// `getcmdtype()`
pub const VIML_FN_GETCMDTYPE: u16 = 3511;
/// `sign_place()`
pub const VIML_FN_SIGN_PLACE: u16 = 3512;
/// `sign_getplaced()`
pub const VIML_FN_SIGN_GETPLACED: u16 = 3513;
/// `sign_unplace()`
pub const VIML_FN_SIGN_UNPLACE: u16 = 3514;
/// `sign_placelist()`
pub const VIML_FN_SIGN_PLACELIST: u16 = 3515;
/// `sign_unplacelist()`
pub const VIML_FN_SIGN_UNPLACELIST: u16 = 3516;
/// `sign_jump()`
pub const VIML_FN_SIGN_JUMP: u16 = 3517;
/// `indent()`
pub const VIML_FN_INDENT: u16 = 3518;
/// `foldtext()`
pub const VIML_FN_FOLDTEXT: u16 = 3519;
/// `foldtextresult()`
pub const VIML_FN_FOLDTEXTRESULT: u16 = 3520;
/// `highlight_exists()`
pub const VIML_FN_HIGHLIGHT_EXISTS: u16 = 3521;
/// `diff_filler()`
pub const VIML_FN_DIFF_FILLER: u16 = 3522;
/// `virtcol2col()`
pub const VIML_FN_VIRTCOL2COL: u16 = 3523;
/// `hlID()` (and its deprecated alias `highlightID()`)
pub const VIML_FN_HLID: u16 = 3572;
/// `diff_hlID()`
pub const VIML_FN_DIFF_HLID: u16 = 3573;
/// `wildtrigger()`
pub const VIML_FN_WILDTRIGGER: u16 = 3524;
/// `searchcount()`
pub const VIML_FN_SEARCHCOUNT: u16 = 3525;
/// `complete_info()`
pub const VIML_FN_COMPLETE_INFO: u16 = 3526;
/// `setqflist()`
pub const VIML_FN_SETQFLIST: u16 = 3527;
/// `getqflist()`
pub const VIML_FN_GETQFLIST: u16 = 3528;
/// `setloclist()`
pub const VIML_FN_SETLOCLIST: u16 = 3529;
/// `getloclist()`
pub const VIML_FN_GETLOCLIST: u16 = 3530;
/// `getcompletion()`
pub const VIML_FN_GETCOMPLETION: u16 = 3531;
/// `getchar()`
pub const VIML_FN_GETCHAR: u16 = 3532;
/// `getcharstr()`
pub const VIML_FN_GETCHARSTR: u16 = 3533;
/// `getcharmod()`
pub const VIML_FN_GETCHARMOD: u16 = 3534;
/// `getcmdprompt()`
pub const VIML_FN_GETCMDPROMPT: u16 = 3535;
/// `getcmdscreenpos()`
pub const VIML_FN_GETCMDSCREENPOS: u16 = 3536;
/// `getcmdcompltype()`
pub const VIML_FN_GETCMDCOMPLTYPE: u16 = 3537;
/// `getcmdcomplpat()`
pub const VIML_FN_GETCMDCOMPLPAT: u16 = 3538;
/// `cindent()`
pub const VIML_FN_CINDENT: u16 = 3539;
/// `lispindent()`
pub const VIML_FN_LISPINDENT: u16 = 3540;
/// `complete_add()`
pub const VIML_FN_COMPLETE_ADD: u16 = 3541;
/// `complete_check()`
pub const VIML_FN_COMPLETE_CHECK: u16 = 3542;
/// `cmdcomplete_info()`
pub const VIML_FN_CMDCOMPLETE_INFO: u16 = 3543;
/// `menu_info()`
pub const VIML_FN_MENU_INFO: u16 = 3544;
/// `test_garbagecollect_now()`
pub const VIML_FN_TEST_GARBAGECOLLECT_NOW: u16 = 3545;
/// `test_write_list_log()`
pub const VIML_FN_TEST_WRITE_LIST_LOG: u16 = 3546;
/// `pyeval()`
pub const VIML_FN_PYEVAL: u16 = 3547;
/// `pyxeval()`
pub const VIML_FN_PYXEVAL: u16 = 3548;
/// `undofile()`
pub const VIML_FN_UNDOFILE: u16 = 3549;
/// `undotree()`
pub const VIML_FN_UNDOTREE: u16 = 3550;
/// `getmousepos()`
pub const VIML_FN_GETMOUSEPOS: u16 = 3551;
/// `screenpos()`
pub const VIML_FN_SCREENPOS: u16 = 3552;
/// `getcompletiontype()`
pub const VIML_FN_GETCOMPLETIONTYPE: u16 = 3553;
/// `mapset()`
pub const VIML_FN_MAPSET: u16 = 3554;
/// `complete()`
pub const VIML_FN_COMPLETE: u16 = 3555;
/// `preinserted()`
pub const VIML_FN_PREINSERTED: u16 = 3556;
/// `getscriptinfo()`
pub const VIML_FN_GETSCRIPTINFO: u16 = 3557;
/// `getstacktrace()`
pub const VIML_FN_GETSTACKTRACE: u16 = 3558;
/// `fullcommand()`
pub const VIML_FN_FULLCOMMAND: u16 = 3559;
/// `assert_beeps()`
pub const VIML_FN_ASSERT_BEEPS: u16 = 3560;
/// `assert_nobeep()`
pub const VIML_FN_ASSERT_NOBEEP: u16 = 3561;
/// `flattennew()`
pub const VIML_FN_FLATTENNEW: u16 = 3211;
/// `sha256()`
pub const VIML_FN_SHA256: u16 = 3212;
/// `blob2list()`
pub const VIML_FN_BLOB2LIST: u16 = 3213;
/// `list2blob()`
pub const VIML_FN_LIST2BLOB: u16 = 3214;
/// `mapnew()`
pub const VIML_FN_MAPNEW: u16 = 3215;
/// `foreach()`
pub const VIML_FN_FOREACH: u16 = 3216;
/// `dictwatcheradd()`
pub const VIML_FN_DICTWATCHERADD: u16 = 3217;
/// `dictwatcherdel()`
pub const VIML_FN_DICTWATCHERDEL: u16 = 3218;
/// Debug line marker: pop a line number → notify the DAP `check_line` hook
/// (emitted before each statement only in debug-compiled chunks).
pub const VIML_SET_LINENO: u16 = 3070;
/// User-function call: pop `argc` args then the name → run the function.
pub const VIML_CALL_USER: u16 = 3071;
/// Funcref-value call: pop `argc` args then a Funcref/Partial value → call it.
pub const VIML_CALL_FUNCREF: u16 = 3574;
/// `:return {expr}`: pop the value → store it as the current call's result.
pub const VIML_SET_RETURN: u16 = 3072;
/// `:throw {expr}`: pop the value → raise it as the pending exception.
pub const VIML_THROW: u16 = 3073;
/// Push `Bool(an exception is pending)` — the per-statement unwind check.
pub const VIML_CHECK_EXC: u16 = 3074;
/// `:catch /{pat}/`: pop the pattern → if it matches the pending exception,
/// catch it (clear pending, keep `v:exception`) and push `Bool(true)`.
pub const VIML_CATCH_MATCH: u16 = 3075;
/// At program end: if an exception is still pending, report `E605` and clear it.
pub const VIML_REPORT_UNCAUGHT: u16 = 3076;

/// Builtin id for comparison `(op, ignore_case)`.
pub fn cmp_id(op: CmpOp, ic: bool) -> u16 {
    let off = match op {
        CmpOp::Equal => 0,
        CmpOp::NotEqual => 1,
        CmpOp::Match => 2,
        CmpOp::NoMatch => 3,
        CmpOp::Greater => 4,
        CmpOp::GreaterEqual => 5,
        CmpOp::Less => 6,
        CmpOp::LessEqual => 7,
        CmpOp::Is => 8,
        CmpOp::IsNot => 9,
    };
    VIML_CMP_BASE + off + if ic { VIML_CMP_IC_OFFSET } else { 0 }
}

/// Resolve the comparison ignore-case flag from a `==#`/`==?` suffix. Phase 3
/// has no `'ignorecase'` option, so `Default` is match-case.
pub fn ic_flag(case: CaseFlag) -> bool {
    matches!(case, CaseFlag::IgnoreCase)
}

/// Map a lexer [`CmpOp`] to the ported [`exprtype_T`] `typval_compare` expects.
fn cmp_to_exprtype(op: CmpOp) -> exprtype_T {
    match op {
        CmpOp::Equal => EXPR_EQUAL,
        CmpOp::NotEqual => EXPR_NEQUAL,
        CmpOp::Match => EXPR_MATCH,
        CmpOp::NoMatch => EXPR_NOMATCH,
        CmpOp::Greater => EXPR_GREATER,
        CmpOp::GreaterEqual => EXPR_GEQUAL,
        CmpOp::Less => EXPR_SMALLER,
        CmpOp::LessEqual => EXPR_SEQUAL,
        CmpOp::Is => EXPR_IS,
        CmpOp::IsNot => EXPR_ISNOT,
    }
}

// ── per-run bridge state (carve-out; smuggles compounds + holds the run result
//    and echo sink — no C analog) ──

thread_local! {
    /// Compound `typval_T` handles smuggled across the fusevm `Value` boundary
    /// (see file header). Cleared between runs; `Rc` keeps live values alive.
    static REFPOOL: RefCell<Vec<typval_T>> = const { RefCell::new(Vec::new()) };
    /// Value of the last bare-expression statement (REPL `-e` result).
    static LAST_RESULT: RefCell<Option<typval_T>> = const { RefCell::new(None) };
    /// `:echo` sink: `Some(buf)` captures (tests/embedding), `None` is stdout.
    static ECHO_SINK: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Registry of user-defined functions, by name (populated from a compiled
    /// program before its `main` chunk runs).
    static FUNCTIONS: RefCell<std::collections::HashMap<String, crate::compile_viml::UserFuncDef>> =
        RefCell::new(std::collections::HashMap::new());
    /// Stack of pending function return values (one per active call); the
    /// `VIML_SET_RETURN` handler writes the top.
    static RETURN_STACK: RefCell<Vec<Option<typval_T>>> = const { RefCell::new(Vec::new()) };
    /// The currently-raised, not-yet-caught exception value (`:throw`). `Some`
    /// while unwinding toward a `:catch`. Models `ex_eval.c`'s `current_exception`.
    static PENDING_EXC: RefCell<Option<String>> = const { RefCell::new(None) };
    /// `v:exception` — the most recently caught exception text.
    static V_EXCEPTION: RefCell<String> = const { RefCell::new(String::new()) };
    /// `v:val` — the current element during `map()`/`filter()`/`sort()`.
    static V_VAL: RefCell<Option<typval_T>> = const { RefCell::new(None) };
    /// `v:key` — the current key/index during `map()`/`filter()`.
    static V_KEY: RefCell<Option<typval_T>> = const { RefCell::new(None) };
}

// ── Typval ↔ fusevm::Value bridge ──

fn tv_to_value(tv: typval_T) -> Value {
    match (tv.v_type, tv.vval) {
        (VAR_NUMBER, v_number(n)) => Value::Int(n),
        (VAR_FLOAT, v_float(f)) => Value::Float(f),
        (VAR_STRING, v_string(s)) => Value::str(s),
        (VAR_FUNC, v_string(s)) => Value::str(format!("\u{1}func\u{1}{s}")),
        (VAR_BOOL, v_bool(b)) => {
            Value::Bool(b == crate::ported::eval::typval_defs_h::BoolVarValue::kBoolVarTrue)
        }
        (VAR_SPECIAL, _) | (VAR_UNKNOWN, _) => Value::Undef,
        // Compound: stash the whole typval (keeps v_type) and tag with the index.
        (vt, vv) => {
            let idx = REFPOOL.with(|p| {
                let mut p = p.borrow_mut();
                p.push(typval_T {
                    v_type: vt,
                    v_lock: VAR_UNLOCKED,
                    vval: vv,
                });
                p.len() - 1
            });
            Value::Ref(Box::new(Value::Int(idx as i64)))
        }
    }
}

fn value_to_tv(v: &Value) -> typval_T {
    match v {
        Value::Int(n) => tv_num(*n),
        Value::Float(f) => tv_flt(*f),
        Value::Bool(b) => typval_T {
            v_type: VAR_BOOL,
            v_lock: VAR_UNLOCKED,
            vval: v_bool(if *b {
                crate::ported::eval::typval_defs_h::BoolVarValue::kBoolVarTrue
            } else {
                crate::ported::eval::typval_defs_h::BoolVarValue::kBoolVarFalse
            }),
        },
        Value::Undef => tv_special(),
        Value::Status(c) => tv_num(*c as varnumber_T),
        Value::Str(s) => match s.strip_prefix("\u{1}func\u{1}") {
            Some(name) => typval_T {
                v_type: VAR_FUNC,
                v_lock: VAR_UNLOCKED,
                vval: v_string(name.to_string()),
            },
            None => tv_str(s.to_string()),
        },
        Value::Ref(inner) => match inner.as_ref() {
            Value::Int(idx) => REFPOOL.with(|p| {
                p.borrow()
                    .get(*idx as usize)
                    .cloned()
                    .unwrap_or_else(tv_special)
            }),
            _ => tv_special(),
        },
        _ => tv_special(),
    }
}

fn pop_tv(vm: &mut VM) -> typval_T {
    let v = vm.pop();
    value_to_tv(&v)
}

// ── handlers ──

fn b_throw(vm: &mut VM, _: u8) -> Value {
    let v = tv_get_string(&pop_tv(vm));
    // c: throw_exception — set current_exception and v:exception.
    V_EXCEPTION.with(|e| *e.borrow_mut() = v.clone());
    set_vim_var_string(VV_EXCEPTION, &v);
    PENDING_EXC.with(|p| *p.borrow_mut() = Some(v));
    Value::Undef
}

fn b_check_exc(_vm: &mut VM, _: u8) -> Value {
    Value::Bool(PENDING_EXC.with(|p| p.borrow().is_some()))
}

fn b_catch_match(vm: &mut VM, _: u8) -> Value {
    let pat = tv_get_string(&pop_tv(vm));
    let pending = PENDING_EXC.with(|p| p.borrow().clone());
    let Some(exc) = pending else {
        return Value::Bool(false);
    };
    // Empty pattern = catch-all; otherwise the catch pattern is a Vim regex.
    let matched = pat.is_empty() || crate::viml_regex::regex_match(&pat, &exc, false);
    if matched {
        // c: caught — clear the pending exception; v:exception already set.
        PENDING_EXC.with(|p| *p.borrow_mut() = None);
        set_vim_var_string(VV_EXCEPTION, &exc);
        V_EXCEPTION.with(|e| *e.borrow_mut() = exc);
    }
    Value::Bool(matched)
}

fn b_report_uncaught(_vm: &mut VM, _: u8) -> Value {
    if let Some(exc) = PENDING_EXC.with(|p| p.borrow_mut().take()) {
        // c: E605 when an exception reaches the top level uncaught.
        message::semsg(&format!("E605: Exception not caught: {exc}"));
    }
    Value::Undef
}

fn b_getvar(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    // Dynamic v: state (not fixed v: constants).
    if name == "v:exception" {
        return Value::str(V_EXCEPTION.with(|e| e.borrow().clone()));
    }
    if name == "v:val" {
        return V_VAL
            .with(|v| v.borrow().clone())
            .map_or(Value::Int(0), tv_to_value);
    }
    if name == "v:key" {
        return V_KEY
            .with(|v| v.borrow().clone())
            .map_or(Value::Int(0), tv_to_value);
    }
    match eval_variable(&name) {
        Some(tv) => tv_to_value(tv),
        None => {
            message::semsg(&format!("E121: Undefined variable: {name}"));
            Value::Undef
        }
    }
}

fn b_setvar(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    let val = pop_tv(vm);
    set_var(&name, name.len(), val, false);
    Value::Undef
}

fn b_setenv(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    let val = tv_get_string(&pop_tv(vm));
    std::env::set_var(name, val);
    Value::Undef
}

fn b_truthy(vm: &mut VM, _: u8) -> Value {
    // VimL truthiness: tv_get_number(tv) != 0 (the `:if`/`!`/`&&`/`||` test).
    Value::Bool(tv_get_number_chk(&pop_tv(vm), None) != 0)
}

fn b_boolnum(vm: &mut VM, _: u8) -> Value {
    Value::Int((tv_get_number_chk(&pop_tv(vm), None) != 0) as varnumber_T)
}

fn b_tonumber(vm: &mut VM, _: u8) -> Value {
    // c: range() coerces each argument with tv_get_number before counting.
    Value::Int(tv_get_number_chk(&pop_tv(vm), None))
}

fn b_add(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval5 — `+` is List+List / Blob+Blob concat, else numeric.
    if let (VAR_LIST, VAR_LIST) = (a.v_type, b.v_type) {
        return tv_to_value(list_concat(&a, &b));
    }
    if let (VAR_BLOB, VAR_BLOB) = (a.v_type, b.v_type) {
        return tv_to_value(blob_concat(&a, &b));
    }
    if a.v_type == VAR_FLOAT || b.v_type == VAR_FLOAT {
        tv_to_value(tv_flt(tv_get_float(&a) + tv_get_float(&b)))
    } else {
        tv_to_value(tv_num(
            tv_get_number_chk(&a, None).wrapping_add(tv_get_number_chk(&b, None)),
        ))
    }
}

fn b_sub(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval5 — numeric subtraction (float if either is Float).
    if a.v_type == VAR_FLOAT || b.v_type == VAR_FLOAT {
        tv_to_value(tv_flt(tv_get_float(&a) - tv_get_float(&b)))
    } else {
        tv_to_value(tv_num(
            tv_get_number_chk(&a, None).wrapping_sub(tv_get_number_chk(&b, None)),
        ))
    }
}

fn b_mul(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval6 — `*`.
    if a.v_type == VAR_FLOAT || b.v_type == VAR_FLOAT {
        tv_to_value(tv_flt(tv_get_float(&a) * tv_get_float(&b)))
    } else {
        tv_to_value(tv_num(
            tv_get_number_chk(&a, None).wrapping_mul(tv_get_number_chk(&b, None)),
        ))
    }
}

fn b_div(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval6 — `/`: float division, else num_divide.
    if a.v_type == VAR_FLOAT || b.v_type == VAR_FLOAT {
        tv_to_value(tv_flt(tv_get_float(&a) / tv_get_float(&b)))
    } else {
        tv_to_value(tv_num(crate::ported::eval::num_divide(
            tv_get_number_chk(&a, None),
            tv_get_number_chk(&b, None),
        )))
    }
}

fn b_mod(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval6 — `%` is integer-only; a Float operand is an error (unlike `*`/`/`
    // which fall back to float arithmetic).
    if a.v_type == VAR_FLOAT || b.v_type == VAR_FLOAT {
        message::emsg("E804: Cannot use '%' with Float");
        return tv_to_value(tv_num(0));
    }
    tv_to_value(tv_num(crate::ported::eval::num_modulus(
        tv_get_number_chk(&a, None),
        tv_get_number_chk(&b, None),
    )))
}

fn b_concat(vm: &mut VM, _: u8) -> Value {
    let b = pop_tv(vm);
    let a = pop_tv(vm);
    // c: eval5 — `.`/`..` string concatenation.
    tv_to_value(tv_str(tv_get_string(&a) + &tv_get_string(&b)))
}

fn b_neg(vm: &mut VM, _: u8) -> Value {
    let v = pop_tv(vm);
    // c: eval7_leader — `-`: negate Float, else negate Number.
    match v.v_type {
        VAR_FLOAT => tv_to_value(tv_flt(-tv_get_float(&v))),
        _ => tv_to_value(tv_num(tv_get_number_chk(&v, None).wrapping_neg())),
    }
}

fn b_uplus(vm: &mut VM, _: u8) -> Value {
    let v = pop_tv(vm);
    // c: eval7_leader — `+`: coerce to Float/Number, no value change.
    match v.v_type {
        VAR_FLOAT => tv_to_value(tv_flt(tv_get_float(&v))),
        _ => tv_to_value(tv_num(tv_get_number_chk(&v, None))),
    }
}

fn b_not(vm: &mut VM, _: u8) -> Value {
    let v = pop_tv(vm);
    // c: eval7_leader — `!`: logical NOT of tv_get_number.
    tv_to_value(tv_num((tv_get_number_chk(&v, None) == 0) as varnumber_T))
}

/// Shared comparison body: `typval_compare(&mut a, &b, type, ic)` writes the
/// boolean result into `a`.
fn do_compare(vm: &mut VM, op: CmpOp, ic: bool) -> Value {
    let b = pop_tv(vm);
    let mut a = pop_tv(vm);
    crate::ported::eval::typval_compare(&mut a, &b, cmp_to_exprtype(op), ic);
    tv_to_value(a)
}

// One thin handler per comparison id (fusevm `register_builtin` takes plain
// `fn`s; the `(op, ic)` pair is compile-time known).
macro_rules! cmp_handlers {
    ($(($name:ident, $op:expr, $ic:literal)),+ $(,)?) => {
        $(fn $name(vm: &mut VM, _: u8) -> Value { do_compare(vm, $op, $ic) })+
        fn register_cmp_handlers(vm: &mut VM) {
            $(vm.register_builtin(cmp_id($op, $ic), $name);)+
        }
    };
}
cmp_handlers! {
    (cmp_eq,     CmpOp::Equal,        false), (cmp_eq_ic,     CmpOp::Equal,        true),
    (cmp_ne,     CmpOp::NotEqual,     false), (cmp_ne_ic,     CmpOp::NotEqual,     true),
    (cmp_match,  CmpOp::Match,        false), (cmp_match_ic,  CmpOp::Match,        true),
    (cmp_nomatch,CmpOp::NoMatch,      false), (cmp_nomatch_ic,CmpOp::NoMatch,      true),
    (cmp_gt,     CmpOp::Greater,      false), (cmp_gt_ic,     CmpOp::Greater,      true),
    (cmp_ge,     CmpOp::GreaterEqual, false), (cmp_ge_ic,     CmpOp::GreaterEqual, true),
    (cmp_lt,     CmpOp::Less,         false), (cmp_lt_ic,     CmpOp::Less,         true),
    (cmp_le,     CmpOp::LessEqual,    false), (cmp_le_ic,     CmpOp::LessEqual,    true),
    (cmp_is,     CmpOp::Is,           false), (cmp_is_ic,     CmpOp::Is,           true),
    (cmp_isnot,  CmpOp::IsNot,        false), (cmp_isnot_ic,  CmpOp::IsNot,        true),
}

fn b_make_list(vm: &mut VM, argc: u8) -> Value {
    let mut items = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        items.push(pop_tv(vm));
    }
    items.reverse();
    tv_to_value(new_list(items))
}

fn b_make_dict(vm: &mut VM, argc: u8) -> Value {
    let n = argc as usize / 2;
    let mut pairs = Vec::with_capacity(n);
    for _ in 0..n {
        let val = pop_tv(vm);
        let key = tv_get_string(&pop_tv(vm));
        pairs.push((key, val));
    }
    pairs.reverse();
    let d = crate::ported::eval::typval::tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        for (k, v) in pairs {
            crate::ported::eval::typval::tv_dict_add_tv(&mut db, &k, v);
        }
    }
    tv_to_value(typval_T {
        v_type: VAR_DICT,
        v_lock: VAR_UNLOCKED,
        vval: v_dict(Some(d)),
    })
}

fn b_index(vm: &mut VM, _: u8) -> Value {
    let index = pop_tv(vm);
    let base = pop_tv(vm);
    tv_to_value(index_value(&base, &index))
}

fn b_slice(vm: &mut VM, _: u8) -> Value {
    let to = pop_tv(vm);
    let from = pop_tv(vm);
    let base = pop_tv(vm);
    tv_to_value(slice_value(&base, &from, &to))
}

/// `let base[index] = value` — set a List/Dict/Blob element in place. For a Dict
/// this also fires any registered watchers (the add/change side).
fn b_setindex(vm: &mut VM, _: u8) -> Value {
    let index = pop_tv(vm);
    let base = pop_tv(vm);
    let value = pop_tv(vm);
    match (base.v_type, &base.vval) {
        (VAR_DICT, v_dict(Some(d))) => {
            let key = tv_get_string(&index);
            let old = d.borrow().dv_hashtab.get(&key).cloned();
            d.borrow_mut().dv_hashtab.insert(key.clone(), value.clone());
            crate::ported::eval::typval::tv_dict_watcher_notify(
                d,
                &key,
                Some(&value),
                old.as_ref(),
            );
        }
        (VAR_LIST, v_list(Some(l))) => {
            let len = l.borrow().lv_len as varnumber_T;
            let mut i = tv_get_number_chk(&index, None);
            if i < 0 {
                i += len;
            }
            if i >= 0 && i < len {
                l.borrow_mut().lv_items[i as usize].li_tv = value;
            } else {
                message::emsg("E684: list index out of range");
            }
        }
        (VAR_BLOB, v_blob(Some(b))) => {
            let len = crate::ported::eval::typval::tv_blob_len(&b.borrow()) as varnumber_T;
            let mut i = tv_get_number_chk(&index, None);
            if i < 0 {
                i += len;
            }
            if i >= 0 && i < len {
                crate::ported::eval::typval::tv_blob_set(
                    &mut b.borrow_mut(),
                    i as i32,
                    tv_get_number_chk(&value, None) as u8,
                );
            } else {
                message::emsg("E979: Blob index out of range");
            }
        }
        _ => message::emsg("E689: Can only index a List, Dictionary or Blob"),
    }
    Value::Undef
}

/// `let base[idx1:idx2] = list` — list range assignment via the ported
/// `tv_list_assign_range`. Stack (top→bottom): idx2, idx1, base, value. An
/// `idx2` of Undef (the compiler's `l[i:]` marker) means "to the end".
fn b_setrange(vm: &mut VM, _: u8) -> Value {
    use crate::ported::eval::typval::{
        tv_list_assign_range, tv_list_check_range_index_one, tv_list_check_range_index_two,
        tv_list_copy,
    };
    let idx2_tv = pop_tv(vm);
    let idx1_tv = pop_tv(vm);
    let base = pop_tv(vm);
    let value = pop_tv(vm);
    let dest = match (base.v_type, &base.vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            message::emsg("E709: [:] requires a List value");
            return Value::Undef;
        }
    };
    // c: the source must be a List (E709 otherwise). NULL list → empty.
    let src = match (value.v_type, &value.vval) {
        (VAR_LIST, v_list(Some(l))) => tv_list_copy(l, false),
        (VAR_LIST, v_list(None)) => crate::ported::eval::typval::tv_list_alloc(0),
        _ => {
            message::emsg("E709: [:] requires a List value");
            return Value::Undef;
        }
    };
    // The compiler emits LoadUndef (→ VAR_SPECIAL/VAR_UNKNOWN) for an omitted
    // second index (`l[i:]`), meaning "to the end".
    let empty_idx2 = matches!(idx2_tv.v_type, VAR_SPECIAL | VAR_UNKNOWN);
    let mut n1 = tv_get_number_chk(&idx1_tv, None) as i32;
    let mut n2 = if empty_idx2 {
        0
    } else {
        tv_get_number_chk(&idx2_tv, None) as i32
    };
    let pos1 = match tv_list_check_range_index_one(&dest.borrow(), &mut n1, false) {
        Some(p) => p,
        None => return Value::Undef, // E684 already emitted
    };
    if !empty_idx2
        && tv_list_check_range_index_two(&dest.borrow(), &mut n1, pos1, &mut n2, false)
            == crate::ported::eval_h::FAIL
    {
        return Value::Undef;
    }
    tv_list_assign_range(&dest, &src.borrow(), n1, n2, empty_idx2, "=", "");
    Value::Undef
}

fn b_echo(vm: &mut VM, argc: u8) -> Value {
    echo_impl(vm, argc, true);
    Value::Undef
}
fn b_echon(vm: &mut VM, argc: u8) -> Value {
    echo_impl(vm, argc, false);
    Value::Undef
}

fn echo_impl(vm: &mut VM, argc: u8, newline: bool) {
    let mut parts = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        parts.push(pop_tv(vm));
    }
    parts.reverse();
    let sep = if newline { " " } else { "" };
    let rendered: Vec<String> = parts.iter().map(encode_tv2echo).collect();
    let mut line = rendered.join(sep);
    if newline {
        line.push('\n');
    }
    echo_write(&line);
}

fn b_set_result(vm: &mut VM, _: u8) -> Value {
    let v = pop_tv(vm);
    LAST_RESULT.with(|r| *r.borrow_mut() = Some(v));
    Value::Undef
}

fn b_getenv(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    Value::str(std::env::var(&name).unwrap_or_default())
}
fn b_exec_stmt(vm: &mut VM, argc: u8) -> Value {
    let mut parts = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        parts.push(tv_get_string(&pop_tv(vm)));
    }
    parts.reverse();
    let _ = run_source_nested(&parts.join(" "));
    Value::Undef
}

/// `:source {file}` — read the file and run it in the current (shared) scope,
/// so functions and globals it defines persist. `~`/`$VAR` in the path expand.
fn b_source(vm: &mut VM, _: u8) -> Value {
    let path = tv_get_string(&pop_tv(vm));
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        match std::env::var("HOME") {
            Ok(h) => format!("{h}/{rest}"),
            Err(_) => path.clone(),
        }
    } else {
        path.clone()
    };
    match std::fs::read_to_string(&expanded) {
        Ok(src) => {
            let _ = run_source_nested(&src);
        }
        Err(_) => message::semsg(&format!("E484: Can't open file {path}")),
    }
    Value::Undef
}

/// `:unlet {name}` — delete a variable (forceit: missing is not an error here).
fn b_unlet(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    crate::ported::eval::vars::do_unlet(&name, name.len(), true);
    Value::Undef
}

fn b_set_lineno(vm: &mut VM, _: u8) -> Value {
    let line = tv_get_number_chk(&pop_tv(vm), None);
    crate::dap::check_line(line as u32);
    Value::Undef
}

thread_local! {
    /// Autoload files already sourced this run (by resolved path), so a missing
    /// `pkg#func` is not re-sourced on every call.
    static AUTOLOADED: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Try to autoload the package for `name` (`pkg#func`, `a#b#func` → `a/b`):
/// source `autoload/{path}.vim` (relative to the cwd) once. Returns true if a
/// file was sourced (so the call should be retried). Port of `script_autoload`.
fn try_autoload(name: &str) -> bool {
    let parts: Vec<&str> = name.split('#').collect();
    if parts.len() < 2 {
        return false;
    }
    let rel = format!("autoload/{}.vim", parts[..parts.len() - 1].join("/"));
    let already = AUTOLOADED.with(|s| s.borrow().contains(&rel));
    if already {
        return false;
    }
    AUTOLOADED.with(|s| {
        s.borrow_mut().insert(rel.clone());
    });
    match std::fs::read_to_string(&rel) {
        Ok(src) => {
            let _ = run_source_nested(&src);
            true
        }
        Err(_) => false,
    }
}

/// Direct call of a funcref VALUE: stack is `[funcref, arg0, …, arg{argc-1}]`
/// (the callee pushed first). Backs `expr(args)` for a funcref-valued `expr`.
fn b_call_funcref(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let callee = pop_tv(vm);
    if !matches!(callee.v_type, VAR_FUNC | VAR_PARTIAL) {
        message::emsg("E15: not a function");
        return Value::Undef;
    }
    match call_funcref(&callee, args) {
        Some(rettv) => tv_to_value(rettv),
        None => Value::Undef,
    }
}

fn b_call_user(vm: &mut VM, argc: u8) -> Value {
    // Stack: [name, arg0, …, arg{argc-1}] (name pushed first).
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let name = tv_get_string(&pop_tv(vm));
    if let Some(rettv) = call_user_function(&name, args.clone()) {
        return tv_to_value(rettv);
    }
    // Autoload: an undefined `pkg#func` sources `autoload/pkg.vim` (which should
    // define it), then the call is retried once.
    if name.contains('#') && try_autoload(&name) {
        if let Some(rettv) = call_user_function(&name, args.clone()) {
            return tv_to_value(rettv);
        }
    }
    // Fallback: `F(args)` where `F` is a variable holding a Funcref/Partial
    // (e.g. a lambda stored in a variable). Call through the funcref value.
    if let Some(v) = eval_variable(&name) {
        if matches!(v.v_type, VAR_FUNC | VAR_PARTIAL) {
            return match call_funcref(&v, args) {
                Some(rettv) => tv_to_value(rettv),
                None => {
                    message::semsg(&format!("E117: Unknown function: {name}"));
                    Value::Undef
                }
            };
        }
    }
    message::semsg(&format!("E117: Unknown function: {name}"));
    Value::Undef
}

fn b_set_return(vm: &mut VM, _: u8) -> Value {
    let v = pop_tv(vm);
    RETURN_STACK.with(|r| {
        if let Some(top) = r.borrow_mut().last_mut() {
            *top = Some(v);
        }
    });
    Value::Undef
}

/// Invoke a user function: bind `a:` args, push the `l:`/`a:` scope, run the
/// body chunk on a nested VM, and return the result (`0` if no `:return`).
fn call_user_function(name: &str, args: Vec<typval_T>) -> Option<typval_T> {
    let func = FUNCTIONS.with(|f| f.borrow().get(name).cloned())?;

    // Bind named parameters into the a: scope; extras into a:0 / a:000. A `...`
    // entry marks the varargs boundary: params before it are named, every arg
    // from that position on goes to a:000 (so `F(...)` collects all args).
    let mut avars = crate::ported::eval::typval_defs_h::dict_T::default();
    let nfixed = func
        .params
        .iter()
        .position(|p| p == "...")
        .unwrap_or(func.params.len());
    for (i, p) in func.params.iter().take(nfixed).enumerate() {
        let v = args.get(i).cloned().unwrap_or_else(tv_special);
        crate::ported::eval::typval::tv_dict_add_tv(&mut avars, p, v);
    }
    let extra: Vec<typval_T> = if args.len() > nfixed {
        args[nfixed..].to_vec()
    } else {
        Vec::new()
    };
    crate::ported::eval::typval::tv_dict_add_tv(
        &mut avars,
        "0",
        tv_num(extra.len() as varnumber_T),
    );
    crate::ported::eval::typval::tv_dict_add_tv(&mut avars, "000", new_list(extra));

    crate::ported::eval::vars::funccal_stack.with(|s| {
        s.borrow_mut().push(crate::ported::eval::vars::FuncScope {
            fc_l_vars: crate::ported::eval::typval_defs_h::dict_T::default(),
            fc_l_avars: avars,
        })
    });
    RETURN_STACK.with(|r| r.borrow_mut().push(None));

    run_chunk_nested(func.chunk.clone());

    let ret = RETURN_STACK.with(|r| r.borrow_mut().pop().flatten());
    crate::ported::eval::vars::funccal_stack.with(|s| {
        s.borrow_mut().pop();
    });
    // c: a function with no :return yields 0.
    Some(ret.unwrap_or_else(|| tv_num(0)))
}

/// Run a chunk on a nested VM **without** resetting the refpool (a user-function
/// call happens mid-outer-run; clearing the refpool would corrupt the outer
/// VM's live compound values). The outer `last_result` is saved and restored.
fn run_chunk_nested(chunk: fusevm::Chunk) {
    let saved = LAST_RESULT.with(|r| r.borrow_mut().take());
    let mut vm = VM::new(chunk);
    install(&mut vm);
    let _ = vm.run();
    LAST_RESULT.with(|r| *r.borrow_mut() = saved);
}

/// Run a nested chunk and capture its bare-expression result (the per-element
/// expression eval used by `map`/`filter`). Refpool-safe like
/// [`run_chunk_nested`]; restores the outer `last_result`.
fn run_chunk_capture(chunk: fusevm::Chunk) -> Option<typval_T> {
    let saved = LAST_RESULT.with(|r| r.borrow_mut().take());
    let mut vm = VM::new(chunk);
    install(&mut vm);
    let _ = vm.run();
    let result = LAST_RESULT.with(|r| r.borrow_mut().take());
    LAST_RESULT.with(|r| *r.borrow_mut() = saved);
    result
}

/// Compile a single VimL expression string to a runnable chunk (for the
/// `map()`/`filter()` string-expression callback form).
fn compile_expr_chunk(src: &str) -> Result<fusevm::Chunk, VimlError> {
    let e = parse_expr(src)?;
    Ok(crate::compile_viml::compile_program(&[Stmt::Expr(e)])?.main)
}

/// Parse + compile + run VimL source on a nested VM (refpool-safe — used by
/// `execute()`, which runs commands mid-outer-run). Functions defined by the
/// source register globally.
fn run_source_nested(src: &str) -> Result<(), VimlError> {
    let prog = crate::compile_viml::compile_program(&crate::viml_parser::parse_program(src)?)?;
    FUNCTIONS.with(|f| {
        let mut f = f.borrow_mut();
        for func in prog.funcs {
            f.insert(func.name.clone(), func);
        }
    });
    run_chunk_nested(prog.main);
    Ok(())
}

fn b_eval(vm: &mut VM, _: u8) -> Value {
    let src = tv_get_string(&pop_tv(vm));
    match compile_expr_chunk(&src) {
        Ok(chunk) => run_chunk_capture(chunk).map_or(Value::Int(0), tv_to_value),
        Err(e) => {
            message::semsg(&format!("{e}"));
            Value::Undef
        }
    }
}

/// The `\=` substitute-expression evaluator (installed into the regex engine's
/// `SUBST_EXPR_HOOK`): compile + run the expression, return its string value.
fn subst_expr_eval(expr: &str) -> String {
    match compile_expr_chunk(expr) {
        Ok(chunk) => run_chunk_capture(chunk)
            .map(|tv| tv_get_string(&tv))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// `execute({command} [, {silent}])` — run ex command(s) (a string or a List of
/// strings) and return their captured `:echo`/message output.
fn b_execute(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let cmds: Vec<String> = match (args[0].v_type, &args[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.borrow().lv_items.iter().map(tv_string_item).collect(),
        _ => vec![tv_get_string(&args[0])],
    };
    // Redirect output into a fresh capture buffer, run, then restore the sink.
    let saved = ECHO_SINK.with(|s| s.borrow_mut().replace(String::new()));
    for cmd in cmds {
        let _ = run_source_nested(&cmd);
    }
    let out = ECHO_SINK.with(|s| s.borrow_mut().take().unwrap_or_default());
    ECHO_SINK.with(|s| *s.borrow_mut() = saved);
    tv_to_value(tv_str(out))
}

/// `tv_get_string` of a list item (helper for `execute`'s List form).
fn tv_string_item(it: &listitem_T) -> String {
    tv_get_string(&it.li_tv)
}

/// `assert_fails({cmd} [, {error} [, {msg}]])` — run `{cmd}` and record a
/// failure in `v:errors` if it does NOT error, or if `{error}` is not found in
/// the reported error. `{error}` is a literal substring (String) or a List of
/// up to two regex patterns matched against the first / last reported error.
///
/// Bridge-level (like `execute()`): it must run a command and observe the error
/// machinery — `did_emsg`, the captured error text, and a thrown exception
/// (reported by the nested top level). Spec: `csrc/eval.lua`; the implementation
/// follows Neovim's `testing.c`, which is not part of the vendored eval tree.
/// Appends through the ported `assert_error` (`csrc/eval/vars.c`).
fn b_assert_fails(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let cmd = tv_get_string(&args[0]);
    let opt_msg = args.get(2).filter(|m| m.v_type != VAR_UNKNOWN);
    let prefix = opt_msg
        .map(|m| format!("{}: ", encode_tv2echo(m)))
        .unwrap_or_default();

    // Run the command with errors captured (suppressed) instead of printed.
    // The command under test is *expected* to error, so its errors must not
    // count against the surrounding script — save and restore did_emsg and
    // v:exception around the run (as Vim resets called_emsg / restores
    // v:exception in assert_fails).
    let before = message::did_emsg.with(|d| d.get());
    let saved_exc = V_EXCEPTION.with(|e| e.borrow().clone());
    message::capture_errors_begin();
    let parse_err = run_source_nested(&cmd).err();
    // A throw that unwound to the nested top level is reported + cleared there;
    // clear any residue so it cannot escape into the caller's script.
    PENDING_EXC.with(|p| *p.borrow_mut() = None);
    let mut errs = message::capture_errors_take();
    if let Some(e) = &parse_err {
        errs.insert(0, e.to_string());
    }
    let failed = parse_err.is_some() || message::did_emsg.with(|d| d.get()) > before;
    message::did_emsg.with(|d| d.set(before));
    set_vim_var_string(VV_EXCEPTION, &saved_exc);
    V_EXCEPTION.with(|e| *e.borrow_mut() = saved_exc);

    if !failed {
        set_var_errors(&format!("{prefix}command did not fail: {cmd}"));
        return Value::Int(1);
    }

    // Match {error} against the reported error(s), if given.
    if let Some(want) = args.get(1).filter(|w| w.v_type != VAR_UNKNOWN) {
        let first = errs.first().cloned().unwrap_or_default();
        let last = errs.last().cloned().unwrap_or_default();
        let ok = match (want.v_type, &want.vval) {
            (VAR_LIST, v_list(Some(l))) => {
                let items = &l.borrow().lv_items;
                let pat0 = items
                    .first()
                    .map(|it| tv_get_string(&it.li_tv))
                    .unwrap_or_default();
                let m0 = pat0.is_empty() || crate::viml_regex::regex_match(&pat0, &first, false);
                let m1 = match items.get(1) {
                    Some(it) => {
                        crate::viml_regex::regex_match(&tv_get_string(&it.li_tv), &last, false)
                    }
                    None => true,
                };
                m0 && m1
            }
            _ => first.contains(&tv_get_string(want)),
        };
        if !ok {
            set_var_errors(&format!(
                "{prefix}Expected {} but got '{first}'",
                encode_tv2string(want)
            ));
            return Value::Int(1);
        }
    }
    Value::Int(0)
}

/// Append a message to `v:errors` (thin wrapper over the ported `assert_error`).
fn set_var_errors(msg: &str) {
    crate::ported::eval::vars::assert_error(msg);
}

/// Run `{cmd}` (errors captured) and report whether it "rang the bell": an Ex
/// command that errors triggers a beep in Vim, so a reported error (parse error
/// or a raised `emsg`) models the beep. Shared by `assert_beeps()`/
/// `assert_nobeep()`. Restores `did_emsg`/`v:exception` around the run.
fn assert_beeps_run(cmd: &str) -> bool {
    let before = message::did_emsg.with(|d| d.get());
    let saved_exc = V_EXCEPTION.with(|e| e.borrow().clone());
    message::capture_errors_begin();
    let parse_err = run_source_nested(cmd).err();
    PENDING_EXC.with(|p| *p.borrow_mut() = None);
    let _ = message::capture_errors_take();
    let beeped = parse_err.is_some() || message::did_emsg.with(|d| d.get()) > before;
    message::did_emsg.with(|d| d.set(before));
    set_vim_var_string(VV_EXCEPTION, &saved_exc);
    V_EXCEPTION.with(|e| *e.borrow_mut() = saved_exc);
    beeped
}

/// `assert_beeps({cmd})` — run `{cmd}` and record a failure in `v:errors` if it
/// does NOT cause a beep. Bridge-level (runs a command, like `assert_fails()`);
/// spec in `csrc/eval.lua`, implementation follows Neovim's testing.c.
fn b_assert_beeps(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let cmd = tv_get_string(&args[0]);
    if assert_beeps_run(&cmd) {
        Value::Int(0)
    } else {
        set_var_errors(&format!("command did not beep: {cmd}"));
        Value::Int(1)
    }
}

/// `assert_nobeep({cmd})` — run `{cmd}` and record a failure in `v:errors` if it
/// DOES cause a beep. The complement of `assert_beeps()`.
fn b_assert_nobeep(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let cmd = tv_get_string(&args[0]);
    if assert_beeps_run(&cmd) {
        set_var_errors(&format!("command did beep: {cmd}"));
        Value::Int(1)
    } else {
        Value::Int(0)
    }
}

/// Evaluate a `map`/`filter` callback for one element: either a string
/// expression (with `v:val`/`v:key` bound) or a funcref called as `f(key, val)`.
fn eval_callback(
    callback: &typval_T,
    chunk: &Option<fusevm::Chunk>,
    key: &typval_T,
    val: &typval_T,
) -> typval_T {
    V_VAL.with(|v| *v.borrow_mut() = Some(val.clone()));
    V_KEY.with(|v| *v.borrow_mut() = Some(key.clone()));
    match callback.v_type {
        VAR_FUNC | VAR_PARTIAL => {
            call_funcref(callback, vec![key.clone(), val.clone()]).unwrap_or_else(|| tv_num(0))
        }
        _ => chunk
            .as_ref()
            .and_then(|c| run_chunk_capture(c.clone()))
            .unwrap_or_else(|| tv_num(0)),
    }
}

// map()/filter()/mapnew()/foreach() are ported faithfully in
// `src/ported/eval/list.rs`; the per-item evaluation comes back here through the
// `FILTER_MAP_EVAL_HOOK`/`FILTER_MAP_CMD_HOOK` (installed in `install()`).

// `reduce()` is ported faithfully in `src/ported/eval/funcs.rs` (uses CALL_FUNC_HOOK).

/// The `sort()`/`uniq()` `{func}` comparator hook (installed into the value
/// layer's `SORT_FUNCREF_HOOK`): call the user function with the two items and
/// read its result as a Number. `None` signals a call/type error.
fn sort_compare_funcref(name: &str, a: &typval_T, b: &typval_T) -> Option<varnumber_T> {
    let r = call_user_function(name, vec![a.clone(), b.clone()])?;
    Some(tv_get_number_chk(&r, None))
}

/// The generic "call user function" hook (installed into `CALL_FUNC_HOOK`), used
/// by `reduce()`.
fn call_func_hook(funcref: &typval_T, args: &[typval_T]) -> Option<typval_T> {
    call_funcref(funcref, args.to_vec())
}

/// The map()/filter()/foreach() per-item evaluator hook (installed into the
/// value layer's `FILTER_MAP_EVAL_HOOK`): set v:key/v:val and evaluate the expr
/// (string) or call the funcref → result.
fn filter_map_eval(expr: &typval_T, key: &typval_T, val: &typval_T) -> Option<typval_T> {
    let chunk = if expr.v_type == VAR_STRING {
        match compile_expr_chunk(&tv_get_string(expr)) {
            Ok(c) => Some(c),
            Err(_) => return None,
        }
    } else {
        None
    };
    Some(eval_callback(expr, &chunk, key, val))
}

/// The foreach() command hook (`do_cmdline_cmd`): set v:key/v:val and run the
/// string as a command line.
fn filter_map_cmd(cmd: &str, key: &typval_T, val: &typval_T) -> bool {
    V_VAL.with(|v| *v.borrow_mut() = Some(val.clone()));
    V_KEY.with(|v| *v.borrow_mut() = Some(key.clone()));
    run_source_nested(cmd).is_ok()
}

/// Call a Funcref/Partial typval with `extra` args. A Partial prepends its bound
/// `pt_argv` (its `self` dict is not modeled).
fn call_funcref(funcref: &typval_T, extra: Vec<typval_T>) -> Option<typval_T> {
    match (funcref.v_type, &funcref.vval) {
        (VAR_PARTIAL, v_partial(Some(p))) => {
            let mut args = p.pt_argv.clone();
            args.extend(extra);
            call_named(&p.pt_name, args)
        }
        _ => call_named(&tv_get_string(funcref), extra),
    }
}

/// `exists("*name")` — true if `name` is a defined user function or a ported
/// builtin. Installed into `FUNC_EXISTS_HOOK` for `f_exists`. A leading `g:`
/// scope prefix on the function name is ignored, as in Vim.
fn func_exists_hook(name: &str) -> bool {
    let name = name.strip_prefix("g:").unwrap_or(name);
    FUNCTIONS.with(|f| f.borrow().contains_key(name))
        || crate::compile_viml::builtin_fn_id(name).is_some()
}

/// Resolve a function name to either a user `:function` or a ported builtin and
/// call it. Vim's `call()`/funcrefs accept builtin names (`call('printf', […])`,
/// `function('substitute')`), not just user functions — a user function takes
/// precedence when both exist.
fn call_named(name: &str, args: Vec<typval_T>) -> Option<typval_T> {
    if FUNCTIONS.with(|f| f.borrow().contains_key(name)) {
        return call_user_function(name, args);
    }
    if crate::compile_viml::builtin_fn_id(name).is_some() {
        return call_builtin_by_name(name, args);
    }
    // Neither yet — fall back to the user-function path (handles autoload-loaded
    // names registered after the initial check).
    call_user_function(name, args)
}

/// Invoke a ported builtin by name with already-evaluated argument typvals.
/// Builds a one-instruction `CallBuiltin` chunk, pushes the args onto a fresh
/// VM, and runs it — reusing every registered handler. The REFPOOL is a shared
/// thread_local, so converting compound args across the nested VM is safe.
fn call_builtin_by_name(name: &str, args: Vec<typval_T>) -> Option<typval_T> {
    let id = crate::compile_viml::builtin_fn_id(name)?;
    let argc = args.len() as u8;
    let mut b = fusevm::ChunkBuilder::new();
    b.emit(fusevm::Op::CallBuiltin(id, argc), 0);
    let mut vm = VM::new(b.build());
    install(&mut vm);
    for a in args {
        vm.push(tv_to_value(a));
    }
    // `run()` returns the top-of-stack as its result (it does not leave it on the
    // stack), so read the builtin's value from there.
    match vm.run() {
        fusevm::VMResult::Ok(v) => Some(value_to_tv(&v)),
        _ => None,
    }
}

fn b_call(vm: &mut VM, argc: u8) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    // The arg list is the second argument (a List).
    let call_args: Vec<typval_T> = match args.get(1).map(|a| (a.v_type, &a.vval)) {
        Some((VAR_LIST, v_list(Some(l)))) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| it.li_tv.clone())
            .collect(),
        _ => Vec::new(),
    };
    match call_funcref(&args[0], call_args) {
        Some(rettv) => tv_to_value(rettv),
        None => {
            message::semsg(&format!(
                "E117: Unknown function: {}",
                tv_get_string(&args[0])
            ));
            Value::Undef
        }
    }
}

fn b_getopt(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    // The option name may carry a `&l:`/`&g:` scope prefix; strip it.
    let name = name
        .strip_prefix("l:")
        .or_else(|| name.strip_prefix("g:"))
        .unwrap_or(&name);
    tv_to_value(crate::ported::option::get_option_value(name))
}

fn b_set(vm: &mut VM, _: u8) -> Value {
    let args = tv_get_string(&pop_tv(vm));
    crate::ported::option::do_set(&args);
    Value::Undef
}

/// `:let &opt = value` — apply via `option::do_set` as `name=value`.
fn b_setopt(vm: &mut VM, _: u8) -> Value {
    let val = tv_get_string(&pop_tv(vm));
    let name = tv_get_string(&pop_tv(vm));
    crate::ported::option::do_set(&format!("{name}={val}"));
    Value::Undef
}

/// `:map`-family statement: pop the raw command line, split off its command
/// word, and apply via the ported `get_map_mode()` + `do_map()`.
fn b_map(vm: &mut VM, _: u8) -> Value {
    let line = tv_get_string(&pop_tv(vm));
    let line = line.trim();
    // The command word is the leading run of letters plus an optional `!`.
    let alpha_end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    let cmd_end = if line[alpha_end..].starts_with('!') {
        alpha_end + 1
    } else {
        alpha_end
    };
    let cmd = &line[..cmd_end];
    let rest = &line[cmd_end..];
    if let Some((mode, unmap, clear, noremap)) = crate::ported::eval::funcs::get_map_mode(cmd) {
        crate::ported::eval::funcs::do_map(rest, mode, unmap, clear, noremap);
    }
    Value::Undef
}

/// `:command` statement: pop the argument text, define a user command.
fn b_command(vm: &mut VM, _: u8) -> Value {
    let args = tv_get_string(&pop_tv(vm));
    crate::ported::eval::funcs::ex_command(&args);
    Value::Undef
}

/// `:delcommand` statement: pop the name, delete the user command.
fn b_delcommand(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    crate::ported::eval::funcs::ex_delcommand(&name);
    Value::Undef
}

/// `:autocmd` statement: pop the args, register the autocommand.
fn b_autocmd(vm: &mut VM, _: u8) -> Value {
    let args = tv_get_string(&pop_tv(vm));
    crate::ported::eval::funcs::do_autocmd(&args);
    Value::Undef
}

/// `:augroup` statement: pop the name, set the active autocommand group.
fn b_augroup(vm: &mut VM, _: u8) -> Value {
    let name = tv_get_string(&pop_tv(vm));
    crate::ported::eval::funcs::do_augroup(&name);
    Value::Undef
}

/// `:doautocmd` statement: pop the args, run every matching autocommand.
fn b_doautocmd(vm: &mut VM, _: u8) -> Value {
    let args = tv_get_string(&pop_tv(vm));
    for cmd in crate::ported::eval::funcs::do_doautocmd(&args) {
        let _ = run_source_nested(&cmd);
    }
    Value::Undef
}

/// Run one Ex command against the buffer; an unrecognized command (or a
/// `:global` sub-command that isn't an Ex command) runs as an ordinary
/// statement. Used by `b_excmd` and for each `:global`-matched line.
fn exec_ex_or_stmt(line: &str) {
    use crate::ported::eval::funcs::{do_excmd, ExCmdResult};
    match do_excmd(line) {
        ExCmdResult::Handled => {}
        ExCmdResult::NotEx => {
            // Strip a leading ':' so `:echo …` runs as the `echo` statement.
            let stmt = line.trim().strip_prefix(':').unwrap_or(line.trim());
            let _ = run_source_nested(stmt);
        }
        ExCmdResult::Global(mut lines, cmd) => {
            // Run `cmd` on each matched line, highest first so deletions above
            // don't shift the lines still to process.
            lines.sort_unstable();
            for lnum in lines.into_iter().rev() {
                crate::ported::eval::funcs::set_cursorpos(lnum, 1);
                exec_ex_or_stmt(&cmd);
            }
        }
    }
}

/// `:[range]cmd` statement: pop the raw line and run it against the buffer.
fn b_excmd(vm: &mut VM, _: u8) -> Value {
    let line = tv_get_string(&pop_tv(vm));
    exec_ex_or_stmt(&line);
    Value::Undef
}

/// User-command invocation: pop the raw line (`Name[!] args`), expand the
/// command's replacement and run it; error E492 if there is no such command.
fn b_usercmd(vm: &mut VM, _: u8) -> Value {
    let line = tv_get_string(&pop_tv(vm));
    let line = line.trim();
    let alpha_end = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(line.len());
    let bang = line[alpha_end..].starts_with('!');
    let name = &line[..alpha_end];
    let args_start = alpha_end + if bang { 1 } else { 0 };
    let args = line[args_start..].trim();
    match crate::ported::eval::funcs::do_ucmd(name, args, bang) {
        Some(expanded) => {
            let _ = run_source_nested(&expanded);
        }
        None => message::semsg(&format!("E492: Not an editor command: {name}")),
    }
    Value::Undef
}

/// Dispatch a Phase-3 builtin function: pop `argc` args, call the ported
/// `f_<name>` with a pre-initialized `VAR_NUMBER` rettv, push the result.
fn call_func(vm: &mut VM, argc: u8, f: fn(&[typval_T], &mut typval_T)) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    // c: call_func pre-initializes rettv to VAR_NUMBER / 0.
    let mut rettv = tv_num(0);
    f(&args, &mut rettv);
    tv_to_value(rettv)
}

/// Dispatch a single-argument float builtin (`sqrt`/`floor`/`sin`/…) through the
/// real `float_op_wrapper` with the C `func_float` (here a Rust `fn(f64)->f64`).
/// Mirrors Neovim's eval.lua `func_float` routing — there is no `f_sqrt` etc.
fn call_float_op(vm: &mut VM, argc: u8, op: fn(f64) -> f64) -> Value {
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        args.push(pop_tv(vm));
    }
    args.reverse();
    let mut rettv = tv_num(0);
    float_op_wrapper(&args, &mut rettv, op);
    tv_to_value(rettv)
}

fn b_fn_len(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_len)
}
fn b_fn_type(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_type)
}
fn b_fn_string(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_string)
}
fn b_fn_empty(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_empty)
}
fn b_fn_abs(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_abs)
}
fn b_fn_str2nr(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_str2nr)
}
fn b_fn_str2float(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_str2float)
}
fn b_fn_float2nr(vm: &mut VM, argc: u8) -> Value {
    call_func(vm, argc, f_float2nr)
}

// ── helpers reconstructing tree-walker logic (cite eval.c) ──

/// Build a `VAR_LIST` typval from items (the `eval_list` result shape).
fn new_list(items: Vec<typval_T>) -> typval_T {
    let l = tv_list_alloc(items.len() as isize);
    {
        let mut lb = l.borrow_mut();
        for it in items {
            tv_list_append_tv(&mut lb, it);
        }
    }
    typval_T {
        v_type: VAR_LIST,
        v_lock: VAR_UNLOCKED,
        vval: v_list(Some(l)),
    }
}

/// `eval5` List `+` List concatenation — a new list of both items' values.
fn list_concat(a: &typval_T, b: &typval_T) -> typval_T {
    let mut items = Vec::new();
    if let v_list(Some(la)) = &a.vval {
        items.extend(la.borrow().lv_items.iter().map(|it| it.li_tv.clone()));
    }
    if let v_list(Some(lb)) = &b.vval {
        items.extend(lb.borrow().lv_items.iter().map(|it| it.li_tv.clone()));
    }
    new_list(items)
}

/// `eval5` Blob `+` Blob concatenation.
fn blob_concat(a: &typval_T, b: &typval_T) -> typval_T {
    let mut data = Vec::new();
    if let v_blob(Some(ba)) = &a.vval {
        data.extend_from_slice(&ba.borrow().bv_ga);
    }
    if let v_blob(Some(bb)) = &b.vval {
        data.extend_from_slice(&bb.borrow().bv_ga);
    }
    let blob = std::rc::Rc::new(RefCell::new(blob_T {
        bv_ga: data,
        ..Default::default()
    }));
    typval_T {
        v_type: VAR_BLOB,
        v_lock: VAR_UNLOCKED,
        vval: v_blob(Some(blob)),
    }
}

/// `eval_index` subscript (`eval.c`) — Phase-3 String/List/Dict subset.
fn index_value(base: &typval_T, index: &typval_T) -> typval_T {
    match (base.v_type, &base.vval) {
        (VAR_LIST, v_list(Some(l))) => {
            // Faithful single-index via the ported value layer.
            let i = tv_get_number_chk(index, None);
            let mut rettv = base.clone();
            if tv_list_slice_or_index(l, false, i, 0, false, &mut rettv, true) == 0 {
                tv_special()
            } else {
                rettv
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let key = tv_get_string(index);
            match crate::ported::eval::typval::tv_dict_find(&d.borrow(), &key) {
                Some(v) => v.clone(),
                None => {
                    message::semsg(&format!("E716: Key not present in Dictionary: {key}"));
                    tv_special()
                }
            }
        }
        (VAR_STRING, v_string(s)) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as varnumber_T;
            let mut i = tv_get_number_chk(index, None);
            if i < 0 {
                i += len;
            }
            if i < 0 || i >= len {
                tv_str(String::new())
            } else {
                tv_str(chars[i as usize].to_string())
            }
        }
        (VAR_BLOB, v_blob(Some(b))) => {
            // Blob subscript → a single byte (Number), via the ported value layer.
            let i = tv_get_number_chk(index, None);
            let mut rettv = base.clone();
            let _ = crate::ported::eval::typval::tv_blob_slice_or_index(
                &b.borrow(),
                false,
                i,
                0,
                false,
                &mut rettv,
            );
            rettv
        }
        _ => {
            message::emsg("E909: Cannot index this type");
            tv_special()
        }
    }
}

/// `eval_index` slice branch (`eval.c`) — Phase-3 String/List subset.
fn slice_value(base: &typval_T, from: &typval_T, to: &typval_T) -> typval_T {
    let lower = |len: varnumber_T, t: &typval_T| -> varnumber_T {
        if t.v_type == VAR_SPECIAL {
            0
        } else {
            let mut i = tv_get_number_chk(t, None);
            if i < 0 {
                i += len;
            }
            i.max(0)
        }
    };
    let upper = |len: varnumber_T, t: &typval_T| -> varnumber_T {
        if t.v_type == VAR_SPECIAL {
            len - 1
        } else {
            let mut i = tv_get_number_chk(t, None);
            if i < 0 {
                i += len;
            }
            i.min(len - 1)
        }
    };
    match (base.v_type, &base.vval) {
        (VAR_LIST, v_list(Some(l))) => {
            // Faithful list slice via the ported value layer (inclusive bounds;
            // an omitted bound is the whole-start/whole-end default).
            let len = l.borrow().lv_len as varnumber_T;
            let n1 = if from.v_type == VAR_SPECIAL {
                0
            } else {
                tv_get_number_chk(from, None)
            };
            let n2 = if to.v_type == VAR_SPECIAL {
                len - 1
            } else {
                tv_get_number_chk(to, None)
            };
            let mut rettv = base.clone();
            let _ = tv_list_slice_or_index(l, true, n1, n2, false, &mut rettv, true);
            rettv
        }
        (VAR_STRING, v_string(s)) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as varnumber_T;
            let (lo, hi) = (lower(len, from), upper(len, to));
            if lo > hi || len == 0 {
                tv_str(String::new())
            } else {
                tv_str(
                    chars[lo as usize..=(hi as usize).min(chars.len() - 1)]
                        .iter()
                        .collect(),
                )
            }
        }
        (VAR_BLOB, v_blob(Some(b))) => {
            // Blob slice → a sub-blob, via the ported value layer (inclusive
            // bounds; an omitted bound is the whole-end default).
            let len = crate::ported::eval::typval::tv_blob_len(&b.borrow()) as varnumber_T;
            let n1 = if from.v_type == VAR_SPECIAL {
                0
            } else {
                tv_get_number_chk(from, None)
            };
            let n2 = if to.v_type == VAR_SPECIAL {
                len - 1
            } else {
                tv_get_number_chk(to, None)
            };
            let mut rettv = base.clone();
            let _ = crate::ported::eval::typval::tv_blob_slice_or_index(
                &b.borrow(),
                true,
                n1,
                n2,
                false,
                &mut rettv,
            );
            rettv
        }
        _ => {
            message::emsg("E909: Cannot slice this type");
            tv_special()
        }
    }
}

// ── echo sink + per-run lifecycle (carve-out) ──

fn echo_write(s: &str) {
    ECHO_SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        match sink.as_mut() {
            Some(buf) => buf.push_str(s),
            None => {
                use std::io::Write;
                let out = std::io::stdout();
                let _ = out.lock().write_all(s.as_bytes());
            }
        }
    });
}

/// Begin capturing `:echo` output into a buffer (tests / embedding).
pub fn capture_begin() {
    ECHO_SINK.with(|s| *s.borrow_mut() = Some(String::new()));
}

/// Take and clear the captured echo buffer, restoring stdout output.
pub fn capture_take() -> String {
    ECHO_SINK.with(|s| s.borrow_mut().take().unwrap_or_default())
}

/// Drain the captured echo buffer **without** disabling capture (the debugger
/// streams output progressively as DAP `output` events at each pause).
pub fn capture_drain() -> String {
    ECHO_SINK.with(|s| {
        let mut s = s.borrow_mut();
        match s.as_mut() {
            Some(buf) => std::mem::take(buf),
            None => String::new(),
        }
    })
}

/// Take the last bare-expression result.
pub fn take_last_result() -> Option<typval_T> {
    LAST_RESULT.with(|r| r.borrow_mut().take())
}

/// Reset per-run state (refpool, last result, `did_emsg`).
pub fn reset_run() {
    REFPOOL.with(|p| p.borrow_mut().clear());
    LAST_RESULT.with(|r| *r.borrow_mut() = None);
    PENDING_EXC.with(|p| *p.borrow_mut() = None);
    V_EXCEPTION.with(|e| e.borrow_mut().clear());
    message::did_emsg.with(|d| d.set(0));
}

/// Register every `VIML_*` builtin on a fresh VM before `vm.run()`.
pub fn install(vm: &mut VM) {
    // Turn on fusevm's tiered Cranelift JIT (Linear/Block/Tracing). Without
    // this the JIT never engages — `VM::new` defaults it off and the whole
    // tier dispatch is gated on it. Safe: only CallBuiltin-free chunks/loop
    // bodies are eligible; everything else runs on the interpreter exactly as
    // before, and the tracing tier deopts back to it on a slot-type guard miss.
    // `VIMLRS_NO_JIT` forces the interpreter (for benchmarking the baseline).
    if std::env::var_os("VIMLRS_NO_JIT").is_none() {
        vm.enable_tracing_jit();
    }
    // Seed the v: variable store (vimvars[]) before any script runs.
    crate::ported::eval::vars::evalvars_init();
    vm.register_builtin(VIML_GETVAR, b_getvar);
    vm.register_builtin(VIML_SETVAR, b_setvar);
    vm.register_builtin(VIML_SETENV, b_setenv);
    vm.register_builtin(VIML_TRUTHY, b_truthy);
    vm.register_builtin(VIML_BOOLNUM, b_boolnum);
    vm.register_builtin(VIML_TONUMBER, b_tonumber);
    vm.register_builtin(VIML_ADD, b_add);
    vm.register_builtin(VIML_SUB, b_sub);
    vm.register_builtin(VIML_MUL, b_mul);
    vm.register_builtin(VIML_DIV, b_div);
    vm.register_builtin(VIML_MOD, b_mod);
    vm.register_builtin(VIML_CONCAT, b_concat);
    vm.register_builtin(VIML_NEG, b_neg);
    vm.register_builtin(VIML_UPLUS, b_uplus);
    vm.register_builtin(VIML_NOT, b_not);
    register_cmp_handlers(vm);
    vm.register_builtin(VIML_MAKE_LIST, b_make_list);
    vm.register_builtin(VIML_MAKE_DICT, b_make_dict);
    vm.register_builtin(VIML_INDEX, b_index);
    vm.register_builtin(VIML_SLICE, b_slice);
    vm.register_builtin(VIML_SETINDEX, b_setindex);
    vm.register_builtin(VIML_SETRANGE, b_setrange);
    vm.register_builtin(VIML_ECHO, b_echo);
    vm.register_builtin(VIML_ECHON, b_echon);
    vm.register_builtin(VIML_SET_RESULT, b_set_result);
    vm.register_builtin(VIML_GETENV, b_getenv);
    vm.register_builtin(VIML_GETOPT, b_getopt);
    vm.register_builtin(VIML_GETREG, |vm, n| call_func(vm, n, f_getreg));
    vm.register_builtin(VIML_SETREG, |vm, n| call_func(vm, n, f_setreg));
    vm.register_builtin(VIML_SETOPT, b_setopt);
    vm.register_builtin(VIML_FN_LEN, b_fn_len);
    vm.register_builtin(VIML_FN_TYPE, b_fn_type);
    vm.register_builtin(VIML_FN_STRING, b_fn_string);
    vm.register_builtin(VIML_FN_EMPTY, b_fn_empty);
    vm.register_builtin(VIML_FN_ABS, b_fn_abs);
    vm.register_builtin(VIML_FN_STR2NR, b_fn_str2nr);
    vm.register_builtin(VIML_FN_STR2FLOAT, b_fn_str2float);
    vm.register_builtin(VIML_FN_FLOAT2NR, b_fn_float2nr);
    // Second builtin batch — non-capturing closures coerce to the `fn` handler.
    vm.register_builtin(VIML_FN_STRLEN, |vm, n| call_func(vm, n, f_strlen));
    vm.register_builtin(VIML_FN_TOLOWER, |vm, n| call_func(vm, n, f_tolower));
    vm.register_builtin(VIML_FN_TOUPPER, |vm, n| call_func(vm, n, f_toupper));
    vm.register_builtin(VIML_FN_CHAR2NR, |vm, n| call_func(vm, n, f_char2nr));
    vm.register_builtin(VIML_FN_NR2CHAR, |vm, n| call_func(vm, n, f_nr2char));
    vm.register_builtin(VIML_FN_REPEAT, |vm, n| call_func(vm, n, f_repeat));
    vm.register_builtin(VIML_FN_SPLIT, |vm, n| call_func(vm, n, f_split));
    vm.register_builtin(VIML_FN_JOIN, |vm, n| call_func(vm, n, f_join));
    vm.register_builtin(VIML_FN_RANGE, |vm, n| call_func(vm, n, f_range));
    vm.register_builtin(VIML_FN_ADD, |vm, n| call_func(vm, n, f_add));
    vm.register_builtin(VIML_FN_REVERSE, |vm, n| call_func(vm, n, f_reverse));
    vm.register_builtin(VIML_FN_GET, |vm, n| call_func(vm, n, f_get));
    vm.register_builtin(VIML_FN_HAS_KEY, |vm, n| call_func(vm, n, f_has_key));
    vm.register_builtin(VIML_FN_KEYS, |vm, n| call_func(vm, n, f_keys));
    vm.register_builtin(VIML_FN_VALUES, |vm, n| call_func(vm, n, f_values));
    vm.register_builtin(VIML_FN_MAX, |vm, n| call_func(vm, n, f_max));
    vm.register_builtin(VIML_FN_MIN, |vm, n| call_func(vm, n, f_min));
    vm.register_builtin(VIML_FN_COUNT, |vm, n| call_func(vm, n, f_count));
    vm.register_builtin(VIML_FN_INDEX, |vm, n| call_func(vm, n, f_index));
    vm.register_builtin(VIML_FN_HAS, |vm, n| call_func(vm, n, f_has));
    vm.register_builtin(VIML_FN_EXISTS, |vm, n| call_func(vm, n, f_exists));
    vm.register_builtin(VIML_FN_PRINTF, |vm, n| call_func(vm, n, f_printf));
    // Let the value layer's map/filter/foreach evaluate per-item callbacks.
    FILTER_MAP_EVAL_HOOK.with(|h| *h.borrow_mut() = Some(filter_map_eval));
    FILTER_MAP_CMD_HOOK.with(|h| *h.borrow_mut() = Some(filter_map_cmd));
    vm.register_builtin(VIML_FN_MAP, |vm, n| call_func(vm, n, f_map));
    vm.register_builtin(VIML_FN_FILTER, |vm, n| call_func(vm, n, f_filter));
    vm.register_builtin(VIML_FN_MAPNEW, |vm, n| call_func(vm, n, f_mapnew));
    vm.register_builtin(VIML_FN_FOREACH, |vm, n| call_func(vm, n, f_foreach));
    vm.register_builtin(VIML_FN_DICTWATCHERADD, |vm, n| {
        call_func(vm, n, f_dictwatcheradd)
    });
    vm.register_builtin(VIML_FN_DICTWATCHERDEL, |vm, n| {
        call_func(vm, n, f_dictwatcherdel)
    });
    // Let the value layer's sort()/uniq() call user comparator functions.
    SORT_FUNCREF_HOOK.with(|h| *h.borrow_mut() = Some(sort_compare_funcref));
    vm.register_builtin(VIML_FN_SORT, |vm, n| call_func(vm, n, f_sort));
    vm.register_builtin(VIML_FN_CALL, b_call);
    vm.register_builtin(VIML_FN_FUNCTION, |vm, n| call_func(vm, n, f_function));
    vm.register_builtin(VIML_FN_SQRT, |vm, n| call_float_op(vm, n, f64::sqrt));
    vm.register_builtin(VIML_FN_FLOOR, |vm, n| call_float_op(vm, n, f64::floor));
    vm.register_builtin(VIML_FN_CEIL, |vm, n| call_float_op(vm, n, f64::ceil));
    vm.register_builtin(VIML_FN_ROUND, |vm, n| call_float_op(vm, n, f64::round));
    vm.register_builtin(VIML_FN_TRUNC, |vm, n| call_float_op(vm, n, f64::trunc));
    vm.register_builtin(VIML_FN_LOG, |vm, n| call_float_op(vm, n, f64::ln));
    vm.register_builtin(VIML_FN_EXP, |vm, n| call_float_op(vm, n, f64::exp));
    vm.register_builtin(VIML_FN_SIN, |vm, n| call_float_op(vm, n, f64::sin));
    vm.register_builtin(VIML_FN_COS, |vm, n| call_float_op(vm, n, f64::cos));
    vm.register_builtin(VIML_FN_POW, |vm, n| call_func(vm, n, f_pow));
    vm.register_builtin(VIML_FN_AND, |vm, n| call_func(vm, n, f_and));
    vm.register_builtin(VIML_FN_OR, |vm, n| call_func(vm, n, f_or));
    vm.register_builtin(VIML_FN_XOR, |vm, n| call_func(vm, n, f_xor));
    vm.register_builtin(VIML_FN_INVERT, |vm, n| call_func(vm, n, f_invert));
    vm.register_builtin(VIML_FN_STRCHARS, |vm, n| call_func(vm, n, f_strchars));
    vm.register_builtin(VIML_FN_STRPART, |vm, n| call_func(vm, n, f_strpart));
    vm.register_builtin(VIML_FN_STRIDX, |vm, n| call_func(vm, n, f_stridx));
    vm.register_builtin(VIML_FN_TRIM, |vm, n| call_func(vm, n, f_trim));
    vm.register_builtin(VIML_FN_INSERT, |vm, n| call_func(vm, n, f_insert));
    vm.register_builtin(VIML_FN_REMOVE, |vm, n| call_func(vm, n, f_remove));
    vm.register_builtin(VIML_FN_EXTEND, |vm, n| call_func(vm, n, f_extend));
    vm.register_builtin(VIML_FN_COPY, |vm, n| call_func(vm, n, f_copy));
    vm.register_builtin(VIML_FN_ITEMS, |vm, n| call_func(vm, n, f_items));
    vm.register_builtin(VIML_FN_UNIQ, |vm, n| call_func(vm, n, f_uniq));
    vm.register_builtin(VIML_FN_MATCHSTR, |vm, n| call_func(vm, n, f_matchstr));
    vm.register_builtin(VIML_FN_MATCH, |vm, n| call_func(vm, n, f_match));
    vm.register_builtin(VIML_FN_SUBSTITUTE, |vm, n| call_func(vm, n, f_substitute));
    vm.register_builtin(VIML_FN_MATCHLIST, |vm, n| call_func(vm, n, f_matchlist));
    vm.register_builtin(VIML_FN_MATCHEND, |vm, n| call_func(vm, n, f_matchend));
    vm.register_builtin(VIML_FN_STRRIDX, |vm, n| call_func(vm, n, f_strridx));
    vm.register_builtin(VIML_FN_ESCAPE, |vm, n| call_func(vm, n, f_escape));
    vm.register_builtin(VIML_FN_TR, |vm, n| call_func(vm, n, f_tr));
    vm.register_builtin(VIML_FN_STR2LIST, |vm, n| call_func(vm, n, f_str2list));
    vm.register_builtin(VIML_FN_LIST2STR, |vm, n| call_func(vm, n, f_list2str));
    vm.register_builtin(VIML_FN_FLATTEN, |vm, n| call_func(vm, n, f_flatten));
    CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(call_func_hook));
    FUNC_EXISTS_HOOK.with(|h| *h.borrow_mut() = Some(func_exists_hook));
    vm.register_builtin(VIML_FN_REDUCE, |vm, n| call_func(vm, n, f_reduce));
    vm.register_builtin(VIML_FN_EVAL, b_eval);
    crate::viml_regex::SUBST_EXPR_HOOK.with(|h| *h.borrow_mut() = Some(subst_expr_eval));
    vm.register_builtin(VIML_FN_EXECUTE, b_execute);
    vm.register_builtin(VIML_FN_DEEPCOPY, |vm, n| call_func(vm, n, f_deepcopy));
    vm.register_builtin(VIML_FN_FMOD, |vm, n| call_func(vm, n, f_fmod));
    vm.register_builtin(VIML_FN_ATAN2, |vm, n| call_func(vm, n, f_atan2));
    vm.register_builtin(VIML_FN_TAN, |vm, n| call_float_op(vm, n, f64::tan));
    vm.register_builtin(VIML_FN_ATAN, |vm, n| call_float_op(vm, n, f64::atan));
    vm.register_builtin(VIML_FN_ASIN, |vm, n| call_float_op(vm, n, f64::asin));
    vm.register_builtin(VIML_FN_ACOS, |vm, n| call_float_op(vm, n, f64::acos));
    vm.register_builtin(VIML_FN_SINH, |vm, n| call_float_op(vm, n, f64::sinh));
    vm.register_builtin(VIML_FN_COSH, |vm, n| call_float_op(vm, n, f64::cosh));
    vm.register_builtin(VIML_FN_TANH, |vm, n| call_float_op(vm, n, f64::tanh));
    vm.register_builtin(VIML_FN_LOG10, |vm, n| call_float_op(vm, n, f64::log10));
    vm.register_builtin(VIML_EXEC_STMT, b_exec_stmt);
    vm.register_builtin(VIML_SOURCE, b_source);
    vm.register_builtin(VIML_UNLET, b_unlet);
    vm.register_builtin(VIML_SET, b_set);
    vm.register_builtin(VIML_MAP, b_map);
    vm.register_builtin(VIML_COMMAND, b_command);
    vm.register_builtin(VIML_DELCOMMAND, b_delcommand);
    vm.register_builtin(VIML_USERCMD, b_usercmd);
    vm.register_builtin(VIML_AUTOCMD, b_autocmd);
    vm.register_builtin(VIML_AUGROUP, b_augroup);
    vm.register_builtin(VIML_DOAUTOCMD, b_doautocmd);
    vm.register_builtin(VIML_EXCMD, b_excmd);
    vm.register_builtin(VIML_FN_JSON_ENCODE, |vm, n| call_func(vm, n, f_json_encode));
    vm.register_builtin(VIML_FN_JSON_DECODE, |vm, n| call_func(vm, n, f_json_decode));
    vm.register_builtin(VIML_FN_STRGETCHAR, |vm, n| call_func(vm, n, f_strgetchar));
    vm.register_builtin(VIML_FN_STRCHARPART, |vm, n| call_func(vm, n, f_strcharpart));
    vm.register_builtin(VIML_FN_BYTEIDX, |vm, n| call_func(vm, n, f_byteidx));
    vm.register_builtin(VIML_FN_CHARIDX, |vm, n| call_func(vm, n, f_charidx));
    vm.register_builtin(VIML_FN_MATCHSTRPOS, |vm, n| call_func(vm, n, f_matchstrpos));
    vm.register_builtin(VIML_FN_EXTENDNEW, |vm, n| call_func(vm, n, f_extendnew));
    vm.register_builtin(VIML_FN_GETENV, |vm, n| call_func(vm, n, f_getenv));
    vm.register_builtin(VIML_FN_SETENV, |vm, n| call_func(vm, n, f_setenv));
    vm.register_builtin(VIML_FN_SHELLESCAPE, |vm, n| call_func(vm, n, f_shellescape));
    vm.register_builtin(VIML_FN_ISINF, |vm, n| call_func(vm, n, f_isinf));
    vm.register_builtin(VIML_FN_ISNAN, |vm, n| call_func(vm, n, f_isnan));
    vm.register_builtin(VIML_FN_GETPID, |vm, n| call_func(vm, n, f_getpid));
    vm.register_builtin(VIML_FN_LOCALTIME, |vm, n| call_func(vm, n, f_localtime));
    vm.register_builtin(VIML_FN_SOUNDFOLD, |vm, n| call_func(vm, n, f_soundfold));
    vm.register_builtin(VIML_FN_BYTEIDXCOMP, |vm, n| call_func(vm, n, f_byteidxcomp));
    vm.register_builtin(VIML_FN_RELTIME, |vm, n| call_func(vm, n, f_reltime));
    vm.register_builtin(VIML_FN_RELTIMESTR, |vm, n| call_func(vm, n, f_reltimestr));
    vm.register_builtin(VIML_FN_RELTIMEFLOAT, |vm, n| {
        call_func(vm, n, f_reltimefloat)
    });
    vm.register_builtin(VIML_FN_RAND, |vm, n| call_func(vm, n, f_rand));
    vm.register_builtin(VIML_FN_SRAND, |vm, n| call_func(vm, n, f_srand));
    vm.register_builtin(VIML_FN_STRFTIME, |vm, n| call_func(vm, n, f_strftime));
    vm.register_builtin(VIML_FN_STRPTIME, |vm, n| call_func(vm, n, f_strptime));
    vm.register_builtin(VIML_FN_PATHSHORTEN, |vm, n| call_func(vm, n, f_pathshorten));
    vm.register_builtin(VIML_FN_ISABSOLUTEPATH, |vm, n| {
        call_func(vm, n, f_isabsolutepath)
    });
    vm.register_builtin(VIML_FN_SIMPLIFY, |vm, n| call_func(vm, n, f_simplify));
    vm.register_builtin(VIML_FN_FILEREADABLE, |vm, n| {
        call_func(vm, n, f_filereadable)
    });
    vm.register_builtin(VIML_FN_FILEWRITABLE, |vm, n| {
        call_func(vm, n, f_filewritable)
    });
    vm.register_builtin(VIML_FN_ISDIRECTORY, |vm, n| call_func(vm, n, f_isdirectory));
    vm.register_builtin(VIML_FN_GETFSIZE, |vm, n| call_func(vm, n, f_getfsize));
    vm.register_builtin(VIML_FN_GETFTYPE, |vm, n| call_func(vm, n, f_getftype));
    vm.register_builtin(VIML_FN_GETFTIME, |vm, n| call_func(vm, n, f_getftime));
    vm.register_builtin(VIML_FN_GETFPERM, |vm, n| call_func(vm, n, f_getfperm));
    vm.register_builtin(VIML_FN_SETFPERM, |vm, n| call_func(vm, n, f_setfperm));
    vm.register_builtin(VIML_FN_GETCWD, |vm, n| call_func(vm, n, f_getcwd));
    vm.register_builtin(VIML_FN_CHDIR, |vm, n| call_func(vm, n, f_chdir));
    vm.register_builtin(VIML_FN_EXECUTABLE, |vm, n| call_func(vm, n, f_executable));
    vm.register_builtin(VIML_FN_EXEPATH, |vm, n| call_func(vm, n, f_exepath));
    vm.register_builtin(VIML_FN_TEMPNAME, |vm, n| call_func(vm, n, f_tempname));
    vm.register_builtin(VIML_FN_MKDIR, |vm, n| call_func(vm, n, f_mkdir));
    vm.register_builtin(VIML_FN_DELETE, |vm, n| call_func(vm, n, f_delete));
    vm.register_builtin(VIML_FN_RENAME, |vm, n| call_func(vm, n, f_rename));
    vm.register_builtin(VIML_FN_READFILE, |vm, n| call_func(vm, n, f_readfile));
    vm.register_builtin(VIML_FN_WRITEFILE, |vm, n| call_func(vm, n, f_writefile));
    vm.register_builtin(VIML_FN_FNAMEMODIFY, |vm, n| call_func(vm, n, f_fnamemodify));
    vm.register_builtin(VIML_FN_FILECOPY, |vm, n| call_func(vm, n, f_filecopy));
    vm.register_builtin(VIML_FN_HASLOCALDIR, |vm, n| call_func(vm, n, f_haslocaldir));
    vm.register_builtin(VIML_FN_RESOLVE, |vm, n| call_func(vm, n, f_resolve));
    vm.register_builtin(VIML_FN_GLOB2REGPAT, |vm, n| call_func(vm, n, f_glob2regpat));
    vm.register_builtin(VIML_FN_READDIR, |vm, n| call_func(vm, n, f_readdir));
    vm.register_builtin(VIML_FN_READBLOB, |vm, n| call_func(vm, n, f_readblob));
    vm.register_builtin(VIML_FN_GETREG, |vm, n| call_func(vm, n, f_getreg));
    vm.register_builtin(VIML_FN_GETREGTYPE, |vm, n| call_func(vm, n, f_getregtype));
    vm.register_builtin(VIML_FN_GETREGINFO, |vm, n| call_func(vm, n, f_getreginfo));
    vm.register_builtin(VIML_FN_SETREG, |vm, n| call_func(vm, n, f_setreg));
    vm.register_builtin(VIML_FN_REG_RECORDING, |vm, n| {
        call_func(vm, n, f_reg_recording)
    });
    vm.register_builtin(VIML_FN_REG_EXECUTING, |vm, n| {
        call_func(vm, n, f_reg_executing)
    });
    vm.register_builtin(VIML_FN_REG_RECORDED, |vm, n| {
        call_func(vm, n, f_reg_recorded)
    });
    vm.register_builtin(VIML_FN_GETTEXT, |vm, n| call_func(vm, n, f_gettext));
    vm.register_builtin(VIML_FN_GARBAGECOLLECT, |vm, n| {
        call_func(vm, n, f_garbagecollect)
    });
    vm.register_builtin(VIML_FN_FUNCREF, |vm, n| call_func(vm, n, f_funcref));
    vm.register_builtin(VIML_FN_ID, |vm, n| call_func(vm, n, f_id));
    vm.register_builtin(VIML_FN_INDEXOF, |vm, n| call_func(vm, n, f_indexof));
    vm.register_builtin(VIML_FN_MATCHSTRLIST, |vm, n| {
        call_func(vm, n, f_matchstrlist)
    });
    vm.register_builtin(VIML_FN_FNAMEESCAPE, |vm, n| call_func(vm, n, f_fnameescape));
    vm.register_builtin(VIML_FN_SHIFTWIDTH, |vm, n| call_func(vm, n, f_shiftwidth));
    vm.register_builtin(VIML_FN_MODE, |vm, n| call_func(vm, n, f_mode));
    vm.register_builtin(VIML_FN_STATE, |vm, n| call_func(vm, n, f_state));
    vm.register_builtin(VIML_FN_VISUALMODE, |vm, n| call_func(vm, n, f_visualmode));
    vm.register_builtin(VIML_FN_PUMVISIBLE, |vm, n| call_func(vm, n, f_pumvisible));
    vm.register_builtin(VIML_FN_WILDMENUMODE, |vm, n| {
        call_func(vm, n, f_wildmenumode)
    });
    vm.register_builtin(VIML_FN_DID_FILETYPE, |vm, n| {
        call_func(vm, n, f_did_filetype)
    });
    vm.register_builtin(VIML_FN_EVENTHANDLER, |vm, n| {
        call_func(vm, n, f_eventhandler)
    });
    vm.register_builtin(VIML_FN_HLEXISTS, |vm, n| call_func(vm, n, f_hlexists));
    vm.register_builtin(VIML_FN_WINDOWSVERSION, |vm, n| {
        call_func(vm, n, f_windowsversion)
    });
    vm.register_builtin(VIML_FN_GETFONTNAME, |vm, n| call_func(vm, n, f_getfontname));
    vm.register_builtin(VIML_FN_FOREGROUND, |vm, n| call_func(vm, n, f_foreground));
    vm.register_builtin(VIML_FN_PROMPT_GETPROMPT, |vm, n| {
        call_func(vm, n, f_prompt_getprompt)
    });
    vm.register_builtin(VIML_FN_PUM_GETPOS, |vm, n| call_func(vm, n, f_pum_getpos));
    vm.register_builtin(VIML_FN_SERVERLIST, |vm, n| call_func(vm, n, f_serverlist));
    vm.register_builtin(VIML_FN_GETPOS, |vm, n| call_func(vm, n, f_getpos));
    vm.register_builtin(VIML_FN_GETCHARPOS, |vm, n| call_func(vm, n, f_getcharpos));
    vm.register_builtin(VIML_FN_GETCURPOS, |vm, n| call_func(vm, n, f_getcurpos));
    vm.register_builtin(VIML_FN_GETCURSORCHARPOS, |vm, n| {
        call_func(vm, n, f_getcursorcharpos)
    });
    vm.register_builtin(VIML_FN_COL, |vm, n| call_func(vm, n, f_col));
    vm.register_builtin(VIML_FN_CHARCOL, |vm, n| call_func(vm, n, f_charcol));
    vm.register_builtin(VIML_FN_LINE, |vm, n| call_func(vm, n, f_line));
    vm.register_builtin(VIML_FN_VIRTCOL, |vm, n| call_func(vm, n, f_virtcol));
    vm.register_builtin(VIML_FN_SCREENROW, |vm, n| call_func(vm, n, f_screenrow));
    vm.register_builtin(VIML_FN_SCREENCOL, |vm, n| call_func(vm, n, f_screencol));
    vm.register_builtin(VIML_FN_SCREENCHAR, |vm, n| call_func(vm, n, f_screenchar));
    vm.register_builtin(VIML_FN_SCREENATTR, |vm, n| call_func(vm, n, f_screenattr));
    vm.register_builtin(VIML_FN_SCREENCHARS, |vm, n| call_func(vm, n, f_screenchars));
    vm.register_builtin(VIML_FN_SCREENSTRING, |vm, n| {
        call_func(vm, n, f_screenstring)
    });
    vm.register_builtin(VIML_FN_LINE2BYTE, |vm, n| call_func(vm, n, f_line2byte));
    vm.register_builtin(VIML_FN_BYTE2LINE, |vm, n| call_func(vm, n, f_byte2line));
    vm.register_builtin(VIML_FN_NEXTNONBLANK, |vm, n| {
        call_func(vm, n, f_nextnonblank)
    });
    vm.register_builtin(VIML_FN_PREVNONBLANK, |vm, n| {
        call_func(vm, n, f_prevnonblank)
    });
    vm.register_builtin(VIML_FN_WORDCOUNT, |vm, n| call_func(vm, n, f_wordcount));
    vm.register_builtin(VIML_FN_GETJUMPLIST, |vm, n| call_func(vm, n, f_getjumplist));
    vm.register_builtin(VIML_FN_GETCHANGELIST, |vm, n| {
        call_func(vm, n, f_getchangelist)
    });
    vm.register_builtin(VIML_FN_GETMARKLIST, |vm, n| call_func(vm, n, f_getmarklist));
    vm.register_builtin(VIML_FN_GETTAGSTACK, |vm, n| call_func(vm, n, f_gettagstack));
    vm.register_builtin(VIML_FN_TAGFILES, |vm, n| call_func(vm, n, f_tagfiles));
    vm.register_builtin(VIML_FN_TAGLIST, |vm, n| call_func(vm, n, f_taglist));
    vm.register_builtin(VIML_FN_TABPAGEBUFLIST, |vm, n| {
        call_func(vm, n, f_tabpagebuflist)
    });
    vm.register_builtin(VIML_FN_SEARCH, |vm, n| call_func(vm, n, f_search));
    vm.register_builtin(VIML_FN_SEARCHPOS, |vm, n| call_func(vm, n, f_searchpos));
    vm.register_builtin(VIML_FN_SEARCHPAIR, |vm, n| call_func(vm, n, f_searchpair));
    vm.register_builtin(VIML_FN_SEARCHPAIRPOS, |vm, n| {
        call_func(vm, n, f_searchpairpos)
    });
    vm.register_builtin(VIML_FN_SEARCHDECL, |vm, n| call_func(vm, n, f_searchdecl));
    vm.register_builtin(VIML_FN_GETCHARSEARCH, |vm, n| {
        call_func(vm, n, f_getcharsearch)
    });
    vm.register_builtin(VIML_FN_INPUT, |vm, n| call_func(vm, n, f_input));
    vm.register_builtin(VIML_FN_INPUTSECRET, |vm, n| call_func(vm, n, f_inputsecret));
    vm.register_builtin(VIML_FN_INPUTDIALOG, |vm, n| call_func(vm, n, f_inputdialog));
    vm.register_builtin(VIML_FN_INPUTLIST, |vm, n| call_func(vm, n, f_inputlist));
    vm.register_builtin(VIML_FN_INPUTSAVE, |vm, n| call_func(vm, n, f_inputsave));
    vm.register_builtin(VIML_FN_INPUTRESTORE, |vm, n| {
        call_func(vm, n, f_inputrestore)
    });
    vm.register_builtin(VIML_FN_CONFIRM, |vm, n| call_func(vm, n, f_confirm));
    vm.register_builtin(VIML_FN_SYNID, |vm, n| call_func(vm, n, f_synID));
    vm.register_builtin(VIML_FN_SYNIDTRANS, |vm, n| call_func(vm, n, f_synIDtrans));
    vm.register_builtin(VIML_FN_SYNIDATTR, |vm, n| call_func(vm, n, f_synIDattr));
    vm.register_builtin(VIML_FN_SYNSTACK, |vm, n| call_func(vm, n, f_synstack));
    vm.register_builtin(VIML_FN_SYNCONCEALED, |vm, n| {
        call_func(vm, n, f_synconcealed)
    });
    vm.register_builtin(VIML_FN_CHANGENR, |vm, n| call_func(vm, n, f_changenr));
    vm.register_builtin(VIML_FN_SWAPNAME, |vm, n| call_func(vm, n, f_swapname));
    vm.register_builtin(VIML_FN_SWAPFILELIST, |vm, n| {
        call_func(vm, n, f_swapfilelist)
    });
    vm.register_builtin(VIML_FN_SPELLBADWORD, |vm, n| {
        call_func(vm, n, f_spellbadword)
    });
    vm.register_builtin(VIML_FN_SPELLSUGGEST, |vm, n| {
        call_func(vm, n, f_spellsuggest)
    });
    vm.register_builtin(VIML_FN_GETREGION, |vm, n| call_func(vm, n, f_getregion));
    vm.register_builtin(VIML_FN_GETREGIONPOS, |vm, n| {
        call_func(vm, n, f_getregionpos)
    });
    vm.register_builtin(VIML_FN_MATCHBUFLINE, |vm, n| {
        call_func(vm, n, f_matchbufline)
    });
    vm.register_builtin(VIML_FN_MENU_GET, |vm, n| call_func(vm, n, f_menu_get));
    vm.register_builtin(VIML_FN_TIMER_INFO, |vm, n| call_func(vm, n, f_timer_info));
    vm.register_builtin(VIML_FN_TIMER_START, |vm, n| call_func(vm, n, f_timer_start));
    vm.register_builtin(VIML_FN_TIMER_STOP, |vm, n| call_func(vm, n, f_timer_stop));
    vm.register_builtin(VIML_FN_TIMER_PAUSE, |vm, n| call_func(vm, n, f_timer_pause));
    vm.register_builtin(VIML_FN_TIMER_STOPALL, |vm, n| {
        call_func(vm, n, f_timer_stopall)
    });
    vm.register_builtin(VIML_FN_SETPOS, |vm, n| call_func(vm, n, f_setpos));
    vm.register_builtin(VIML_FN_SETCHARPOS, |vm, n| call_func(vm, n, f_setcharpos));
    vm.register_builtin(VIML_FN_CURSOR, |vm, n| call_func(vm, n, f_cursor));
    vm.register_builtin(VIML_FN_SETCURSORCHARPOS, |vm, n| {
        call_func(vm, n, f_setcursorcharpos)
    });
    vm.register_builtin(VIML_FN_SETCHARSEARCH, |vm, n| {
        call_func(vm, n, f_setcharsearch)
    });
    vm.register_builtin(VIML_FN_SETTAGSTACK, |vm, n| call_func(vm, n, f_settagstack));
    vm.register_builtin(VIML_FN_ASSERT_EQUAL, |vm, n| {
        call_func(vm, n, f_assert_equal)
    });
    vm.register_builtin(VIML_FN_ASSERT_NOTEQUAL, |vm, n| {
        call_func(vm, n, f_assert_notequal)
    });
    vm.register_builtin(VIML_FN_ASSERT_TRUE, |vm, n| call_func(vm, n, f_assert_true));
    vm.register_builtin(VIML_FN_ASSERT_FALSE, |vm, n| {
        call_func(vm, n, f_assert_false)
    });
    vm.register_builtin(VIML_FN_ASSERT_MATCH, |vm, n| {
        call_func(vm, n, f_assert_match)
    });
    vm.register_builtin(VIML_FN_ASSERT_NOTMATCH, |vm, n| {
        call_func(vm, n, f_assert_notmatch)
    });
    vm.register_builtin(VIML_FN_ASSERT_REPORT, |vm, n| {
        call_func(vm, n, f_assert_report)
    });
    vm.register_builtin(VIML_FN_ASSERT_INRANGE, |vm, n| {
        call_func(vm, n, f_assert_inrange)
    });
    vm.register_builtin(VIML_FN_ASSERT_EXCEPTION, |vm, n| {
        call_func(vm, n, f_assert_exception)
    });
    vm.register_builtin(VIML_FN_ASSERT_FAILS, b_assert_fails);
    vm.register_builtin(VIML_FN_SYSTEM, |vm, n| call_func(vm, n, f_system));
    vm.register_builtin(VIML_FN_SYSTEMLIST, |vm, n| call_func(vm, n, f_systemlist));
    vm.register_builtin(VIML_FN_ENVIRON, |vm, n| call_func(vm, n, f_environ));
    vm.register_builtin(VIML_FN_SLICE, |vm, n| call_func(vm, n, f_slice));
    vm.register_builtin(VIML_FN_STRCHARLEN, |vm, n| call_func(vm, n, f_strcharlen));
    vm.register_builtin(VIML_FN_STRTRANS, |vm, n| call_func(vm, n, f_strtrans));
    vm.register_builtin(VIML_FN_STRWIDTH, |vm, n| call_func(vm, n, f_strwidth));
    vm.register_builtin(VIML_FN_STRDISPLAYWIDTH, |vm, n| {
        call_func(vm, n, f_strdisplaywidth)
    });
    vm.register_builtin(VIML_FN_CHARCLASS, |vm, n| call_func(vm, n, f_charclass));
    vm.register_builtin(VIML_FN_GLOB, |vm, n| call_func(vm, n, f_glob));
    vm.register_builtin(VIML_FN_GLOBPATH, |vm, n| call_func(vm, n, f_globpath));
    vm.register_builtin(VIML_FN_STRUTF16LEN, |vm, n| call_func(vm, n, f_strutf16len));
    vm.register_builtin(VIML_FN_UTF16IDX, |vm, n| call_func(vm, n, f_utf16idx));
    vm.register_builtin(VIML_FN_BUFNR, |vm, n| call_func(vm, n, f_bufnr));
    vm.register_builtin(VIML_FN_BUFEXISTS, |vm, n| call_func(vm, n, f_bufexists));
    vm.register_builtin(VIML_FN_BUFLISTED, |vm, n| call_func(vm, n, f_buflisted));
    vm.register_builtin(VIML_FN_BUFLOADED, |vm, n| call_func(vm, n, f_bufloaded));
    vm.register_builtin(VIML_FN_BUFNAME, |vm, n| call_func(vm, n, f_bufname));
    vm.register_builtin(VIML_FN_BUFWINNR, |vm, n| call_func(vm, n, f_bufwinnr));
    vm.register_builtin(VIML_FN_BUFWINID, |vm, n| call_func(vm, n, f_bufwinid));
    vm.register_builtin(VIML_FN_WINNR, |vm, n| call_func(vm, n, f_winnr));
    vm.register_builtin(VIML_FN_WINBUFNR, |vm, n| call_func(vm, n, f_winbufnr));
    vm.register_builtin(VIML_FN_WINWIDTH, |vm, n| call_func(vm, n, f_winwidth));
    vm.register_builtin(VIML_FN_WINHEIGHT, |vm, n| call_func(vm, n, f_winheight));
    vm.register_builtin(VIML_FN_WINLAYOUT, |vm, n| call_func(vm, n, f_winlayout));
    vm.register_builtin(VIML_FN_WINLINE, |vm, n| call_func(vm, n, f_winline));
    vm.register_builtin(VIML_FN_WINCOL, |vm, n| call_func(vm, n, f_wincol));
    vm.register_builtin(VIML_FN_WINRESTCMD, |vm, n| call_func(vm, n, f_winrestcmd));
    vm.register_builtin(VIML_FN_TABPAGENR, |vm, n| call_func(vm, n, f_tabpagenr));
    vm.register_builtin(VIML_FN_TABPAGEWINNR, |vm, n| {
        call_func(vm, n, f_tabpagewinnr)
    });
    vm.register_builtin(VIML_FN_GETLINE, |vm, n| call_func(vm, n, f_getline));
    vm.register_builtin(VIML_FN_GETBUFLINE, |vm, n| call_func(vm, n, f_getbufline));
    vm.register_builtin(VIML_FN_GETBUFONELINE, |vm, n| {
        call_func(vm, n, f_getbufoneline)
    });
    vm.register_builtin(VIML_FN_GETBUFINFO, |vm, n| call_func(vm, n, f_getbufinfo));
    vm.register_builtin(VIML_FN_SETLINE, |vm, n| call_func(vm, n, f_setline));
    vm.register_builtin(VIML_FN_SETBUFLINE, |vm, n| call_func(vm, n, f_setbufline));
    vm.register_builtin(VIML_FN_APPEND, |vm, n| call_func(vm, n, f_append));
    vm.register_builtin(VIML_FN_APPENDBUFLINE, |vm, n| {
        call_func(vm, n, f_appendbufline)
    });
    vm.register_builtin(VIML_FN_DELETEBUFLINE, |vm, n| {
        call_func(vm, n, f_deletebufline)
    });
    vm.register_builtin(VIML_FN_GETWININFO, |vm, n| call_func(vm, n, f_getwininfo));
    vm.register_builtin(VIML_FN_GETTABINFO, |vm, n| call_func(vm, n, f_gettabinfo));
    vm.register_builtin(VIML_FN_GETWINPOS, |vm, n| call_func(vm, n, f_getwinpos));
    vm.register_builtin(VIML_FN_GETWINPOSX, |vm, n| call_func(vm, n, f_getwinposx));
    vm.register_builtin(VIML_FN_GETWINPOSY, |vm, n| call_func(vm, n, f_getwinposy));
    vm.register_builtin(VIML_FN_WIN_GETID, |vm, n| call_func(vm, n, f_win_getid));
    vm.register_builtin(VIML_FN_WIN_ID2WIN, |vm, n| call_func(vm, n, f_win_id2win));
    vm.register_builtin(VIML_FN_WIN_FINDBUF, |vm, n| call_func(vm, n, f_win_findbuf));
    vm.register_builtin(VIML_FN_WIN_GOTOID, |vm, n| call_func(vm, n, f_win_gotoid));
    vm.register_builtin(VIML_FN_WIN_GETTYPE, |vm, n| call_func(vm, n, f_win_gettype));
    vm.register_builtin(VIML_FN_WIN_SCREENPOS, |vm, n| {
        call_func(vm, n, f_win_screenpos)
    });
    vm.register_builtin(VIML_FN_EXPAND, |vm, n| call_func(vm, n, f_expand));
    vm.register_builtin(VIML_FN_EXPANDCMD, |vm, n| call_func(vm, n, f_expandcmd));
    vm.register_builtin(VIML_FN_WIN_ID2TABWIN, |vm, n| {
        call_func(vm, n, f_win_id2tabwin)
    });
    vm.register_builtin(VIML_FN_WIN_SPLITMOVE, |vm, n| {
        call_func(vm, n, f_win_splitmove)
    });
    vm.register_builtin(VIML_FN_WIN_MOVE_SEPARATOR, |vm, n| {
        call_func(vm, n, f_win_move_separator)
    });
    vm.register_builtin(VIML_FN_WIN_MOVE_STATUSLINE, |vm, n| {
        call_func(vm, n, f_win_move_statusline)
    });
    vm.register_builtin(VIML_FN_GETCMDWINTYPE, |vm, n| {
        call_func(vm, n, f_getcmdwintype)
    });
    vm.register_builtin(VIML_FN_WINRESTVIEW, |vm, n| call_func(vm, n, f_winrestview));
    vm.register_builtin(VIML_FN_WINSAVEVIEW, |vm, n| call_func(vm, n, f_winsaveview));
    vm.register_builtin(VIML_FN_BUFLOAD, |vm, n| call_func(vm, n, f_bufload));
    vm.register_builtin(VIML_FN_PROMPT_GETINPUT, |vm, n| {
        call_func(vm, n, f_prompt_getinput)
    });
    vm.register_builtin(VIML_FN_PROMPT_SETPROMPT, |vm, n| {
        call_func(vm, n, f_prompt_setprompt)
    });
    vm.register_builtin(VIML_FN_PROMPT_SETCALLBACK, |vm, n| {
        call_func(vm, n, f_prompt_setcallback)
    });
    vm.register_builtin(VIML_FN_PROMPT_SETINTERRUPT, |vm, n| {
        call_func(vm, n, f_prompt_setinterrupt)
    });
    vm.register_builtin(VIML_FN_INTERRUPT, |vm, n| call_func(vm, n, f_interrupt));
    vm.register_builtin(VIML_FN_DEBUGBREAK, |vm, n| call_func(vm, n, f_debugbreak));
    vm.register_builtin(VIML_FN_API_INFO, |vm, n| call_func(vm, n, f_api_info));
    vm.register_builtin(VIML_FN_SWAPINFO, |vm, n| call_func(vm, n, f_swapinfo));
    vm.register_builtin(VIML_FN_SERVERSTART, |vm, n| call_func(vm, n, f_serverstart));
    vm.register_builtin(VIML_FN_SERVERSTOP, |vm, n| call_func(vm, n, f_serverstop));
    vm.register_builtin(VIML_FN_GETBUFVAR, |vm, n| call_func(vm, n, f_getbufvar));
    vm.register_builtin(VIML_FN_GETWINVAR, |vm, n| call_func(vm, n, f_getwinvar));
    vm.register_builtin(VIML_FN_GETTABVAR, |vm, n| call_func(vm, n, f_gettabvar));
    vm.register_builtin(VIML_FN_GETTABWINVAR, |vm, n| {
        call_func(vm, n, f_gettabwinvar)
    });
    vm.register_builtin(VIML_FN_SETBUFVAR, |vm, n| call_func(vm, n, f_setbufvar));
    vm.register_builtin(VIML_FN_SETWINVAR, |vm, n| call_func(vm, n, f_setwinvar));
    vm.register_builtin(VIML_FN_SETTABVAR, |vm, n| call_func(vm, n, f_settabvar));
    vm.register_builtin(VIML_FN_SETTABWINVAR, |vm, n| {
        call_func(vm, n, f_settabwinvar)
    });
    vm.register_builtin(VIML_FN_JOBSTART, |vm, n| call_func(vm, n, f_jobstart));
    vm.register_builtin(VIML_FN_JOBPID, |vm, n| call_func(vm, n, f_jobpid));
    vm.register_builtin(VIML_FN_JOBSTOP, |vm, n| call_func(vm, n, f_jobstop));
    vm.register_builtin(VIML_FN_JOBWAIT, |vm, n| call_func(vm, n, f_jobwait));
    vm.register_builtin(VIML_FN_JOBRESIZE, |vm, n| call_func(vm, n, f_jobresize));
    vm.register_builtin(VIML_FN_CHANCLOSE, |vm, n| call_func(vm, n, f_chanclose));
    vm.register_builtin(VIML_FN_CHANSEND, |vm, n| call_func(vm, n, f_chansend));
    vm.register_builtin(VIML_FN_FEEDKEYS, |vm, n| call_func(vm, n, f_feedkeys));
    vm.register_builtin(VIML_FN_WAIT, |vm, n| call_func(vm, n, f_wait));
    vm.register_builtin(VIML_FN_SOCKCONNECT, |vm, n| call_func(vm, n, f_sockconnect));
    vm.register_builtin(VIML_FN_WIN_EXECUTE, |vm, n| call_func(vm, n, f_win_execute));
    vm.register_builtin(VIML_FN_BUFADD, |vm, n| call_func(vm, n, f_bufadd));
    vm.register_builtin(VIML_FN_CTXGET, |vm, n| call_func(vm, n, f_ctxget));
    vm.register_builtin(VIML_FN_CTXPOP, |vm, n| call_func(vm, n, f_ctxpop));
    vm.register_builtin(VIML_FN_CTXPUSH, |vm, n| call_func(vm, n, f_ctxpush));
    vm.register_builtin(VIML_FN_CTXSET, |vm, n| call_func(vm, n, f_ctxset));
    vm.register_builtin(VIML_FN_CTXSIZE, |vm, n| call_func(vm, n, f_ctxsize));
    vm.register_builtin(VIML_FN_ISLOCKED, |vm, n| call_func(vm, n, f_islocked));
    vm.register_builtin(VIML_FN_LAST_BUFFER_NR, |vm, n| {
        call_func(vm, n, f_last_buffer_nr)
    });
    vm.register_builtin(VIML_FN_LIBCALL, |vm, n| call_func(vm, n, f_libcall));
    vm.register_builtin(VIML_FN_LIBCALLNR, |vm, n| call_func(vm, n, f_libcallnr));
    vm.register_builtin(VIML_FN_MSGPACKDUMP, |vm, n| call_func(vm, n, f_msgpackdump));
    vm.register_builtin(VIML_FN_MSGPACKPARSE, |vm, n| {
        call_func(vm, n, f_msgpackparse)
    });
    vm.register_builtin(VIML_FN_RPCNOTIFY, |vm, n| call_func(vm, n, f_rpcnotify));
    vm.register_builtin(VIML_FN_RPCREQUEST, |vm, n| call_func(vm, n, f_rpcrequest));
    vm.register_builtin(VIML_FN_RPCSTART, |vm, n| call_func(vm, n, f_rpcstart));
    vm.register_builtin(VIML_FN_RPCSTOP, |vm, n| call_func(vm, n, f_rpcstop));
    vm.register_builtin(VIML_FN_STDIOOPEN, |vm, n| call_func(vm, n, f_stdioopen));
    vm.register_builtin(VIML_FN_SUBMATCH, |vm, n| call_func(vm, n, f_submatch));
    vm.register_builtin(VIML_FN_PROMPT_APPENDBUF, |vm, n| {
        call_func(vm, n, f_prompt_appendbuf)
    });
    vm.register_builtin(VIML_FN_PY3EVAL, |vm, n| call_func(vm, n, f_py3eval));
    vm.register_builtin(VIML_FN_PERLEVAL, |vm, n| call_func(vm, n, f_perleval));
    vm.register_builtin(VIML_FN_STDPATH, |vm, n| call_func(vm, n, f_stdpath));
    vm.register_builtin(VIML_FN_KEYTRANS, |vm, n| call_func(vm, n, f_keytrans));
    vm.register_builtin(VIML_FN_LUAEVAL, |vm, n| call_func(vm, n, f_luaeval));
    vm.register_builtin(VIML_FN_RUBYEVAL, |vm, n| call_func(vm, n, f_rubyeval));
    vm.register_builtin(VIML_FN_TERMOPEN, |vm, n| call_func(vm, n, f_termopen));
    vm.register_builtin(VIML_FN_BROWSE, |vm, n| call_func(vm, n, f_browse));
    vm.register_builtin(VIML_FN_BROWSEDIR, |vm, n| call_func(vm, n, f_browsedir));
    vm.register_builtin(VIML_FN_FINDDIR, |vm, n| call_func(vm, n, f_finddir));
    vm.register_builtin(VIML_FN_FINDFILE, |vm, n| call_func(vm, n, f_findfile));
    vm.register_builtin(VIML_FN_FLATTENNEW, |vm, n| call_func(vm, n, f_flattennew));
    vm.register_builtin(VIML_FN_SHA256, |vm, n| call_func(vm, n, f_sha256));
    vm.register_builtin(VIML_FN_BLOB2LIST, |vm, n| call_func(vm, n, f_blob2list));
    vm.register_builtin(VIML_FN_LIST2BLOB, |vm, n| call_func(vm, n, f_list2blob));
    vm.register_builtin(VIML_FN_MATCHFUZZY, |vm, n| call_func(vm, n, f_matchfuzzy));
    vm.register_builtin(VIML_FN_MATCHFUZZYPOS, |vm, n| {
        call_func(vm, n, f_matchfuzzypos)
    });
    vm.register_builtin(VIML_FN_HISTADD, |vm, n| call_func(vm, n, f_histadd));
    vm.register_builtin(VIML_FN_HISTGET, |vm, n| call_func(vm, n, f_histget));
    vm.register_builtin(VIML_FN_HISTNR, |vm, n| call_func(vm, n, f_histnr));
    vm.register_builtin(VIML_FN_HISTDEL, |vm, n| call_func(vm, n, f_histdel));
    vm.register_builtin(VIML_FN_DIGRAPH_GET, |vm, n| call_func(vm, n, f_digraph_get));
    vm.register_builtin(VIML_FN_DIGRAPH_SET, |vm, n| call_func(vm, n, f_digraph_set));
    vm.register_builtin(VIML_FN_DIGRAPH_GETLIST, |vm, n| {
        call_func(vm, n, f_digraph_getlist)
    });
    vm.register_builtin(VIML_FN_DIGRAPH_SETLIST, |vm, n| {
        call_func(vm, n, f_digraph_setlist)
    });
    vm.register_builtin(VIML_FN_SETCELLWIDTHS, |vm, n| {
        call_func(vm, n, f_setcellwidths)
    });
    vm.register_builtin(VIML_FN_GETCELLWIDTHS, |vm, n| {
        call_func(vm, n, f_getcellwidths)
    });
    vm.register_builtin(VIML_FN_HOSTNAME, |vm, n| call_func(vm, n, f_hostname));
    vm.register_builtin(VIML_FN_ICONV, |vm, n| call_func(vm, n, f_iconv));
    vm.register_builtin(VIML_FN_ARGC, |vm, n| call_func(vm, n, f_argc));
    vm.register_builtin(VIML_FN_ARGIDX, |vm, n| call_func(vm, n, f_argidx));
    vm.register_builtin(VIML_FN_ARGV, |vm, n| call_func(vm, n, f_argv));
    vm.register_builtin(VIML_FN_ASSERT_EQUALFILE, |vm, n| {
        call_func(vm, n, f_assert_equalfile)
    });
    vm.register_builtin(VIML_FN_ARGLISTID, |vm, n| call_func(vm, n, f_arglistid));
    vm.register_builtin(VIML_FN_FOLDLEVEL, |vm, n| call_func(vm, n, f_foldlevel));
    vm.register_builtin(VIML_FN_MATCHADD, |vm, n| call_func(vm, n, f_matchadd));
    vm.register_builtin(VIML_FN_MATCHADDPOS, |vm, n| call_func(vm, n, f_matchaddpos));
    vm.register_builtin(VIML_FN_MATCHDELETE, |vm, n| call_func(vm, n, f_matchdelete));
    vm.register_builtin(VIML_FN_GETMATCHES, |vm, n| call_func(vm, n, f_getmatches));
    vm.register_builtin(VIML_FN_SETMATCHES, |vm, n| call_func(vm, n, f_setmatches));
    vm.register_builtin(VIML_FN_CLEARMATCHES, |vm, n| {
        call_func(vm, n, f_clearmatches)
    });
    vm.register_builtin(VIML_FN_MATCHARG, |vm, n| call_func(vm, n, f_matcharg));
    vm.register_builtin(VIML_FN_SIGN_DEFINE, |vm, n| call_func(vm, n, f_sign_define));
    vm.register_builtin(VIML_FN_SIGN_GETDEFINED, |vm, n| {
        call_func(vm, n, f_sign_getdefined)
    });
    vm.register_builtin(VIML_FN_SIGN_UNDEFINE, |vm, n| {
        call_func(vm, n, f_sign_undefine)
    });
    vm.register_builtin(VIML_FN_FOLDCLOSED, |vm, n| call_func(vm, n, f_foldclosed));
    vm.register_builtin(VIML_FN_FOLDCLOSEDEND, |vm, n| {
        call_func(vm, n, f_foldclosedend)
    });
    vm.register_builtin(VIML_FN_HASMAPTO, |vm, n| call_func(vm, n, f_hasmapto));
    vm.register_builtin(VIML_FN_MAPARG, |vm, n| call_func(vm, n, f_maparg));
    vm.register_builtin(VIML_FN_MAPCHECK, |vm, n| call_func(vm, n, f_mapcheck));
    vm.register_builtin(VIML_FN_MAPLIST, |vm, n| call_func(vm, n, f_maplist));
    vm.register_builtin(VIML_FN_SETCMDLINE, |vm, n| call_func(vm, n, f_setcmdline));
    vm.register_builtin(VIML_FN_GETCMDLINE, |vm, n| call_func(vm, n, f_getcmdline));
    vm.register_builtin(VIML_FN_SETCMDPOS, |vm, n| call_func(vm, n, f_setcmdpos));
    vm.register_builtin(VIML_FN_GETCMDPOS, |vm, n| call_func(vm, n, f_getcmdpos));
    vm.register_builtin(VIML_FN_GETCMDTYPE, |vm, n| call_func(vm, n, f_getcmdtype));
    vm.register_builtin(VIML_FN_SIGN_PLACE, |vm, n| call_func(vm, n, f_sign_place));
    vm.register_builtin(VIML_FN_SIGN_GETPLACED, |vm, n| {
        call_func(vm, n, f_sign_getplaced)
    });
    vm.register_builtin(VIML_FN_SIGN_UNPLACE, |vm, n| {
        call_func(vm, n, f_sign_unplace)
    });
    vm.register_builtin(VIML_FN_SIGN_PLACELIST, |vm, n| {
        call_func(vm, n, f_sign_placelist)
    });
    vm.register_builtin(VIML_FN_SIGN_UNPLACELIST, |vm, n| {
        call_func(vm, n, f_sign_unplacelist)
    });
    vm.register_builtin(VIML_FN_SIGN_JUMP, |vm, n| call_func(vm, n, f_sign_jump));
    vm.register_builtin(VIML_FN_INDENT, |vm, n| call_func(vm, n, f_indent));
    vm.register_builtin(VIML_FN_FOLDTEXT, |vm, n| call_func(vm, n, f_foldtext));
    vm.register_builtin(VIML_FN_FOLDTEXTRESULT, |vm, n| {
        call_func(vm, n, f_foldtextresult)
    });
    vm.register_builtin(VIML_FN_HIGHLIGHT_EXISTS, |vm, n| {
        call_func(vm, n, f_highlight_exists)
    });
    vm.register_builtin(VIML_FN_DIFF_FILLER, |vm, n| call_func(vm, n, f_diff_filler));
    vm.register_builtin(VIML_FN_HLID, |vm, n| call_func(vm, n, f_hlID));
    vm.register_builtin(VIML_FN_DIFF_HLID, |vm, n| call_func(vm, n, f_diff_hlID));
    vm.register_builtin(VIML_FN_VIRTCOL2COL, |vm, n| call_func(vm, n, f_virtcol2col));
    vm.register_builtin(VIML_FN_WILDTRIGGER, |vm, n| call_func(vm, n, f_wildtrigger));
    vm.register_builtin(VIML_FN_SEARCHCOUNT, |vm, n| call_func(vm, n, f_searchcount));
    vm.register_builtin(VIML_FN_COMPLETE_INFO, |vm, n| {
        call_func(vm, n, f_complete_info)
    });
    vm.register_builtin(VIML_FN_SETQFLIST, |vm, n| call_func(vm, n, f_setqflist));
    vm.register_builtin(VIML_FN_GETQFLIST, |vm, n| call_func(vm, n, f_getqflist));
    vm.register_builtin(VIML_FN_SETLOCLIST, |vm, n| call_func(vm, n, f_setloclist));
    vm.register_builtin(VIML_FN_GETLOCLIST, |vm, n| call_func(vm, n, f_getloclist));
    vm.register_builtin(VIML_FN_GETCOMPLETION, |vm, n| {
        call_func(vm, n, f_getcompletion)
    });
    vm.register_builtin(VIML_FN_GETCHAR, |vm, n| call_func(vm, n, f_getchar));
    vm.register_builtin(VIML_FN_GETCHARSTR, |vm, n| call_func(vm, n, f_getcharstr));
    vm.register_builtin(VIML_FN_GETCHARMOD, |vm, n| call_func(vm, n, f_getcharmod));
    vm.register_builtin(VIML_FN_GETCMDPROMPT, |vm, n| {
        call_func(vm, n, f_getcmdprompt)
    });
    vm.register_builtin(VIML_FN_GETCMDSCREENPOS, |vm, n| {
        call_func(vm, n, f_getcmdscreenpos)
    });
    vm.register_builtin(VIML_FN_GETCMDCOMPLTYPE, |vm, n| {
        call_func(vm, n, f_getcmdcompltype)
    });
    vm.register_builtin(VIML_FN_GETCMDCOMPLPAT, |vm, n| {
        call_func(vm, n, f_getcmdcomplpat)
    });
    vm.register_builtin(VIML_FN_CINDENT, |vm, n| call_func(vm, n, f_cindent));
    vm.register_builtin(VIML_FN_LISPINDENT, |vm, n| call_func(vm, n, f_lispindent));
    vm.register_builtin(VIML_FN_COMPLETE_ADD, |vm, n| {
        call_func(vm, n, f_complete_add)
    });
    vm.register_builtin(VIML_FN_COMPLETE_CHECK, |vm, n| {
        call_func(vm, n, f_complete_check)
    });
    vm.register_builtin(VIML_FN_CMDCOMPLETE_INFO, |vm, n| {
        call_func(vm, n, f_cmdcomplete_info)
    });
    vm.register_builtin(VIML_FN_MENU_INFO, |vm, n| call_func(vm, n, f_menu_info));
    vm.register_builtin(VIML_FN_TEST_GARBAGECOLLECT_NOW, |vm, n| {
        call_func(vm, n, f_test_garbagecollect_now)
    });
    vm.register_builtin(VIML_FN_TEST_WRITE_LIST_LOG, |vm, n| {
        call_func(vm, n, f_test_write_list_log)
    });
    vm.register_builtin(VIML_FN_PYEVAL, |vm, n| call_func(vm, n, f_pyeval));
    vm.register_builtin(VIML_FN_PYXEVAL, |vm, n| call_func(vm, n, f_pyxeval));
    vm.register_builtin(VIML_FN_UNDOFILE, |vm, n| call_func(vm, n, f_undofile));
    vm.register_builtin(VIML_FN_UNDOTREE, |vm, n| call_func(vm, n, f_undotree));
    vm.register_builtin(VIML_FN_GETMOUSEPOS, |vm, n| call_func(vm, n, f_getmousepos));
    vm.register_builtin(VIML_FN_SCREENPOS, |vm, n| call_func(vm, n, f_screenpos));
    vm.register_builtin(VIML_FN_GETCOMPLETIONTYPE, |vm, n| {
        call_func(vm, n, f_getcompletiontype)
    });
    vm.register_builtin(VIML_FN_MAPSET, |vm, n| call_func(vm, n, f_mapset));
    vm.register_builtin(VIML_FN_COMPLETE, |vm, n| call_func(vm, n, f_complete));
    vm.register_builtin(VIML_FN_PREINSERTED, |vm, n| call_func(vm, n, f_preinserted));
    vm.register_builtin(VIML_FN_GETSCRIPTINFO, |vm, n| {
        call_func(vm, n, f_getscriptinfo)
    });
    vm.register_builtin(VIML_FN_GETSTACKTRACE, |vm, n| {
        call_func(vm, n, f_getstacktrace)
    });
    vm.register_builtin(VIML_FN_FULLCOMMAND, |vm, n| call_func(vm, n, f_fullcommand));
    vm.register_builtin(VIML_FN_ASSERT_BEEPS, b_assert_beeps);
    vm.register_builtin(VIML_FN_ASSERT_NOBEEP, b_assert_nobeep);
    vm.register_builtin(VIML_SET_LINENO, b_set_lineno);
    vm.register_builtin(VIML_CALL_USER, b_call_user);
    vm.register_builtin(VIML_CALL_FUNCREF, b_call_funcref);
    vm.register_builtin(VIML_SET_RETURN, b_set_return);
    vm.register_builtin(VIML_THROW, b_throw);
    vm.register_builtin(VIML_CHECK_EXC, b_check_exc);
    vm.register_builtin(VIML_CATCH_MATCH, b_catch_match);
    vm.register_builtin(VIML_REPORT_UNCAUGHT, b_report_uncaught);
}

/// Register a compiled program's user functions, then run its `main` chunk.
pub fn run_compiled(prog: crate::compile_viml::CompiledProgram) {
    FUNCTIONS.with(|f| {
        let mut f = f.borrow_mut();
        for func in prog.funcs {
            f.insert(func.name.clone(), func);
        }
    });
    run_chunk(prog.main);
}

// ── debugger support (DAP) ──

/// Parse + debug-compile + run source with a `SET_LINENO` marker before each
/// statement, so the DAP `check_line` hook can pause at breakpoints.
pub fn eval_source_debug(src: &str) -> Result<(), VimlError> {
    let numbered = crate::viml_parser::parse_program_lines(src)?;
    run_chunk(crate::compile_viml::compile_program_debug(&numbered)?);
    Ok(())
}

/// Snapshot the `g:` scope for the debugger's variables view: `(name, rendered)`
/// pairs read from `globvardict` (no VM run).
pub fn dap_globals() -> Vec<(String, String)> {
    crate::ported::eval::vars::globvardict.with(|d| {
        d.borrow()
            .dv_hashtab
            .iter()
            .map(|(k, v)| (format!("g:{k}"), encode_tv2echo(v)))
            .collect()
    })
}

/// Evaluate a bare variable name for the debugger's `evaluate` request (reads
/// `eval_variable`, no nested VM run — avoids disturbing the paused executor).
pub fn dap_eval_var(name: &str) -> Option<String> {
    eval_variable(name).as_ref().map(encode_tv2echo)
}

// ── public driver: parse → compile → run on fusevm ──

/// Run an already-compiled `fusevm::Chunk` on a fresh VM with the VimL host
/// installed (the script-cache hit path — no lex/parse/compile).
pub fn run_chunk(chunk: fusevm::Chunk) {
    crate::fusevm_disasm::maybe_print_stdout("program", &chunk);
    reset_run();
    let stats = std::env::var_os("VIMLRS_JIT_STATS").is_some();
    let probe = if stats { Some(chunk.clone()) } else { None };
    let mut vm = VM::new(chunk);
    install(&mut vm);
    let _ = vm.run();
    if let Some(chunk) = probe {
        jit_stats_report(&chunk);
    }
}

/// Diagnostic (opt-in via `VIMLRS_JIT_STATS`): report how much fusevm
/// JIT-compiled to native machine code — across the main chunk AND every
/// user-function body (which run on nested VMs), counting any loop whose trace
/// compiled. Goes to stderr like `--doctor`.
fn jit_stats_report(main: &fusevm::Chunk) {
    use fusevm::Op;
    let jc = fusevm::JitCompiler::new();
    let count_traces = |chunk: &fusevm::Chunk| -> usize {
        chunk
            .ops
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o {
                Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) if *t < i => Some(*t),
                _ => None,
            })
            .filter(|&h| jc.trace_is_compiled(chunk, h))
            .count()
    };
    let mut traced = count_traces(main);
    FUNCTIONS.with(|f| {
        for func in f.borrow().values() {
            traced += count_traces(&func.chunk);
        }
    });
    eprintln!(
        "vimlrs: JIT — main block-compiled={}, loop traces compiled={}",
        jc.block_jit_is_compiled(main),
        traced
    );
}

/// Compile and run a statement list. `:echo` output and `emsg` errors happen as
/// side effects.
pub fn run_program(stmts: &[Stmt]) -> Result<(), VimlError> {
    run_compiled(compile_program(stmts)?);
    Ok(())
}

/// Parse + compile + run a block of VimL source. Block-structured (`:if`/
/// `:while`/`:for`/…) statements are parsed across lines into one chunk. Returns
/// the last bare-expression value.
pub fn eval_source(src: &str) -> Result<Option<typval_T>, VimlError> {
    run_program(&crate::viml_parser::parse_program(src)?)?;
    Ok(take_last_result())
}

/// Source a `.vim` file through the rkyv bytecode cache: a 2nd+ run with an
/// unchanged file and binary skips lex/parse/compile and runs the cached chunk.
pub fn eval_file(path: &std::path::Path) -> Result<(), VimlError> {
    if let Some(prog) = crate::script_cache::try_load(path) {
        run_compiled(prog);
        return Ok(());
    }
    let src = std::fs::read_to_string(path)
        .map_err(|e| VimlError::msg(format!("vimlrs: {}: {e}", path.display())))?;
    let prog = compile_program(&crate::viml_parser::parse_program(&src)?)?;
    crate::script_cache::store(path, &prog);
    run_compiled(prog);
    Ok(())
}

/// Parse + compile + run a single expression, returning its value.
pub fn eval_expr(src: &str) -> Result<typval_T, VimlError> {
    let e = parse_expr(src)?;
    run_program(&[Stmt::Expr(e)])?;
    Ok(take_last_result().unwrap_or_else(tv_special))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        capture_begin();
        eval_source(src).unwrap();
        capture_take()
    }

    /// Proof that a vimlrs numeric loop runs on fusevm's tracing JIT: a
    /// function with a constant-bound integer `while` loop lowers to a
    /// CallBuiltin-free native loop body (slots + compare + arith + jumps), and
    /// fusevm records and compiles a trace for it.
    #[test]
    fn numeric_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! Count()\n  let i = 0\n  while i < 1000\n    let i = i + 1\n  endwhile\n  return i\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "Count")
            .unwrap()
            .chunk
            .clone();

        // The loop lowers to native slot/compare/arith ops.
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::GetSlot(_))),
            "slotted reads"
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetSlot(_))),
            "slotted writes"
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::NumLt)),
            "native compare"
        );
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Add)), "native add");

        // The loop body (loop header .. backedge) is CallBuiltin-free → trace-eligible.
        let backedge = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        let (header, back) = backedge;
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        assert!(
            matches!(chunk.ops[back], Op::JumpIfTrue(t) if t == header),
            "loop closes with a conditional backward branch to the header"
        );

        // Drive the native loop hot in a single run (1000 backedges >> the
        // threshold of 50) and confirm fusevm records and compiles a trace for
        // it — vimlrs numeric loops execute as native machine code.
        let mut vm = VM::new(chunk.clone());
        install(&mut vm); // registers VIML_SET_RETURN + enables the JIT
        while vm.frames.last().unwrap().slots.is_empty() {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the native loop at header {header}"
        );
    }

    /// Proof that the idiomatic `for i in range(N)` loop runs on the tracing
    /// JIT: it compiles to a native integer counter loop (no list is built),
    /// and fusevm records and compiles a trace for it.
    #[test]
    fn range_for_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! S()\n  let s = 0\n  for i in range(1000)\n    let s = s + i\n  endfor\n  return s\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "S")
            .unwrap()
            .chunk
            .clone();

        // No list materialization — range() never calls VIML_FN_RANGE/VIML_INDEX.
        assert!(
            !chunk.ops.iter().any(|o| matches!(o, Op::CallBuiltin(id, _)
                if *id == VIML_FN_RANGE || *id == VIML_INDEX || *id == VIML_FN_LEN)),
            "range-for must not build a list"
        );
        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "range-for loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the native range-for loop"
        );
    }

    /// Proof that a SCRIPT-LEVEL (top-level, no function) numeric loop runs on
    /// the tracing JIT — the common shape of real `.vim` scripts.
    #[test]
    fn script_level_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let s = 0\nfor i in range(1000)\n  let s = s + i\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "script-level loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 3 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the script-level loop"
        );
    }

    /// End-to-end proof: a real script run through the SAME path as
    /// `vimlrs script.vim` (`run_chunk` → `install` → `run`, no manual slot
    /// pre-sizing) JIT-compiles its hot loop. This is the actual runtime path,
    /// not a hand-built harness.
    #[test]
    fn real_run_path_jit_compiles_hot_loop() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let s = 0\nfor i in range(2000)\n  let s = s + i\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let header = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(*t),
                _ => None,
            })
            .expect("loop backedge");

        // EXACTLY what the CLI does for a script: run_chunk installs builtins,
        // enables the JIT, and runs. No ensure_slots — `let s = 0` grows them.
        run_chunk(chunk.clone());

        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "the real `vimlrs script.vim` path must JIT-compile the hot loop"
        );
    }

    /// Proof that a FLOAT accumulator loop also trace-JITs (native `fadd` over a
    /// Float slot; int counter + float accumulator in the same trace).
    #[test]
    fn float_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let x = 0.0\nfor i in range(1000)\n  let x = x + 0.5\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let header = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(*t),
                _ => None,
            })
            .expect("loop backedge");
        run_chunk(chunk.clone());
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the native float loop"
        );
    }

    /// Proof that a loop with a COMPOUND condition (`&&`/`||` of native
    /// comparisons) still trace-JITs — short-circuit lowering stays
    /// CallBuiltin-free.
    #[test]
    fn compound_condition_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let i = 0\nlet s = 0\nwhile i < 5000 && s < 1000000000\n  let s = s + i\n  let i = i + 1\nendwhile";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let header = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(*t),
                _ => None,
            })
            .expect("loop backedge");
        run_chunk(chunk.clone());
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the compound-condition loop"
        );
    }

    /// Proof that a hot loop in a function that ALSO calls another function
    /// still trace-JITs: a callee can't see this function's `l:` locals, so the
    /// call no longer bails slotting (only at script scope, where bare = `g:`).
    #[test]
    fn loop_with_sibling_call_traces_in_function() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! Helper()\n  return 99\nendfunction\nfunction! F()\n  let s = 0\n  for i in range(1000)\n    let s = s + i\n  endfor\n  let x = Helper()\n  return s + x\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "F")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        // The loop body is CallBuiltin-free even though the chunk calls Helper().
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "loop body must be CallBuiltin-free despite the sibling call, got {:?}",
            &chunk.ops[header..=back]
        );
        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the loop despite the sibling call"
        );
    }

    /// Proof that `for i in range(<dynamic>)` trace-JITs: the bound need not be a
    /// compile-time integer. `range(a:n)` hoists `a:n` once, coerced with
    /// `tv_get_number` (`VIML_TONUMBER`) in the prologue, so the loop body is
    /// native and fusevm compiles a trace. Driven through the real call path.
    #[test]
    fn dynamic_range_bound_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! F(n)\n  let s = 0\n  for i in range(a:n)\n    let s += i\n  endfor\n  return s\nendfunction\ncall F(3000)";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "F")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        // The bound coercion is in the prologue; the loop body is native.
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::NumLt)),
            "native counter compare"
        );
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "dynamic-range loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        // Drive the function hot through the actual call path (a:n = 3000).
        run_compiled(prog);
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the dynamic-range loop"
        );
    }

    /// Proof that a bit-manipulation loop trace-JITs: `and()`/`or()`/`xor()`/
    /// `invert()` of integer args lower to native `Op::BitAnd`/`BitOr`/`BitXor`/
    /// `BitNot` (fusevm lowers all of these), so the loop body is CallBuiltin-free.
    #[test]
    fn bitwise_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! H()\n  let h = 0\n  for i in range(2000)\n    let h = xor(h, i)\n    let h = and(h, 65535)\n  endfor\n  return h\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "H")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::BitXor))
                && chunk.ops[header..=back]
                    .iter()
                    .any(|o| matches!(o, Op::BitAnd)),
            "loop body should use native bitwise ops"
        );
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "bitwise loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the bitwise loop"
        );
    }

    /// Proof that a numeric ternary in a loop trace-JITs: the `?:` test lowers
    /// through `cond()` (native compare), and a ternary with numeric branches is
    /// itself treated as a Number, so the enclosing `+=` stays native.
    #[test]
    fn ternary_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! F()\n  let s = 0\n  for i in range(2000)\n    let s += i % 2 == 0 ? i : 0\n  endfor\n  return s\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "F")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "ternary loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the ternary loop"
        );
    }

    /// Proof that a value-position comparison in a loop trace-JITs: `i > 500`
    /// lowers to a native compare reified to Number 0/1 with a branch, and a
    /// comparison counts as an integer, so `let s += i > 500` stays native.
    #[test]
    fn value_compare_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! F()\n  let s = 0\n  for i in range(2000)\n    let s += i > 500\n  endfor\n  return s\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "F")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::NumGt)),
            "value-position compare should use a native compare op"
        );
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "value-compare loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the value-compare loop"
        );
    }

    /// Proof that logical-not of an integer in a loop trace-JITs: `!x` lowers to
    /// a native `x == 0` reified to Number 0/1, so `let s += !(i % 2)` stays native.
    #[test]
    fn logical_not_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! F()\n  let s = 0\n  for i in range(2000)\n    let s += !(i % 2)\n  endfor\n  return s\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "F")
            .unwrap()
            .chunk
            .clone();

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "logical-not loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the logical-not loop"
        );
    }

    /// Proof that a loop using integer `%` (e.g. `if i % 2 == 0`) trace-JITs —
    /// native `Op::Mod` (identical to VimL's `num_modulus` for ints).
    #[test]
    fn modulo_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src =
            "let s = 0\nfor i in range(5000)\n  if i % 2 == 0\n    let s = s + i\n  endif\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::Mod)),
            "loop body should use native Op::Mod"
        );
        run_chunk(chunk.clone());
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the modulo loop"
        );
    }

    /// Proof that numeric negation lowers to native `Op::Negate` and keeps a
    /// loop trace-JIT-able.
    #[test]
    fn negate_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let s = 0\nfor i in range(2000)\n  let s = s + -i\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::Negate)),
            "loop body should use native Op::Negate"
        );
        run_chunk(chunk.clone());
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the negate loop"
        );
    }

    /// Proof that explicit `l:`-scoped references in a function trace-JIT.
    /// In a legacy function `l:name` IS bare `name` (same slot), so a loop using
    /// `l:` refs lowers to the same native slot ops and fusevm compiles a trace.
    #[test]
    fn local_ref_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "function! S()\n  let l:s = 0\n  for l:i in range(2000)\n    let l:s += l:i\n  endfor\n  return l:s\nendfunction";
        let prog = compile_program(&parse_program(src).unwrap()).unwrap();
        let chunk = prog
            .funcs
            .iter()
            .find(|f| f.name == "S")
            .unwrap()
            .chunk
            .clone();

        // Both the accumulator (`l:s`) and the induction var (`l:i`) are slotted.
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::GetSlot(_))),
            "slotted reads"
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetSlot(_))),
            "slotted writes"
        );

        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            !chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "l: loop body must be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );

        let mut vm = VM::new(chunk.clone());
        install(&mut vm);
        while vm.frames.last().unwrap().slots.len() < 2 {
            vm.frames
                .last_mut()
                .unwrap()
                .slots
                .push(fusevm::Value::Int(0));
        }
        let _ = vm.run();
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the l:-ref loop"
        );
    }

    /// Proof that a compound-assignment accumulator (`let s += i`) trace-JITs.
    /// `op=` desugars to `s = s + i`, the same store path as the plain form, so
    /// the hot loop body stays CallBuiltin-free and fusevm compiles a trace.
    #[test]
    fn compound_add_loop_traces_on_jit() {
        use crate::compile_viml::compile_program;
        use crate::viml_parser::parse_program;
        use fusevm::Op;

        let src = "let s = 0\nfor i in range(2000)\n  let s += i\nendfor";
        let chunk = compile_program(&parse_program(src).unwrap()).unwrap().main;
        let (header, back) = chunk
            .ops
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                Op::JumpIfTrue(t) if *t < i => Some(((*t), i)),
                _ => None,
            })
            .expect("loop backedge");
        assert!(
            chunk.ops[header..=back]
                .iter()
                .any(|o| matches!(o, Op::Add))
                && !chunk.ops[header..=back]
                    .iter()
                    .any(|o| matches!(o, Op::CallBuiltin(..) | Op::Extended(..))),
            "compound-add loop body must use native Op::Add and be CallBuiltin-free, got {:?}",
            &chunk.ops[header..=back]
        );
        run_chunk(chunk.clone());
        assert!(
            fusevm::JitCompiler::new().trace_is_compiled(&chunk, header),
            "fusevm must compile a trace for the `+=` loop"
        );
    }

    /// Proof that vimlrs bytecode actually runs on fusevm's Cranelift JIT:
    /// an integer expression lowers to a CallBuiltin-free native-op chunk, and
    /// fusevm block-JIT-compiles it to machine code after the warm-up threshold.
    #[test]
    fn integer_expr_runs_on_jit() {
        use crate::compile_viml::compile_expr_only;
        use crate::viml_parser::parse_expr;

        let e = parse_expr("2 + 3 * 4 - 1").unwrap();
        let chunk = compile_expr_only(&e).unwrap();

        // 1. The chunk is fully native — no builtin/extended dispatch, so it is
        //    eligible for every JIT tier.
        assert!(
            !chunk
                .ops
                .iter()
                .any(|op| matches!(op, fusevm::Op::CallBuiltin(..) | fusevm::Op::Extended(..))),
            "integer expr must lower to native ops only, got {:?}",
            chunk.ops
        );

        // 2. It computes the right value, and fusevm block-JIT compiles it after
        //    the 10-invocation warm-up (install() enables the JIT).
        let mut compiled = false;
        let mut last = Value::Undef;
        for _ in 0..15 {
            let mut vm = VM::new(chunk.clone());
            install(&mut vm);
            if let fusevm::VMResult::Ok(v) = vm.run() {
                last = v;
            }
            if fusevm::JitCompiler::new().block_jit_is_compiled(&chunk) {
                compiled = true;
            }
        }
        assert_eq!(last, Value::Int(13), "2 + 3*4 - 1 = 13");
        assert!(compiled, "fusevm must block-JIT-compile the native chunk");
    }

    #[test]
    fn echo_arithmetic() {
        assert_eq!(run("echo 1 + 2 * 3"), "7\n");
        assert_eq!(run("echo 10 / 3"), "3\n");
        assert_eq!(run("echo 10 % 3"), "1\n");
    }

    #[test]
    fn echo_float_and_concat() {
        assert_eq!(run("echo 1.5 + 1.5"), "3.0\n");
        assert_eq!(run(r#"echo "a" . "b" . 1"#), "ab1\n");
    }

    #[test]
    fn echo_collections() {
        assert_eq!(run("echo [1, 2, 3]"), "[1, 2, 3]\n");
        assert_eq!(run("echo ['a', 'b']"), "['a', 'b']\n");
        assert_eq!(run("echo {'k': 1}"), "{'k': 1}\n");
    }

    #[test]
    fn variables_and_logic() {
        assert_eq!(run("let x = 5\necho x + 1"), "6\n");
        assert_eq!(run("echo 1 == 1"), "1\n");
        assert_eq!(run(r#"echo 1 == "1""#), "1\n"); // Vim number/string coercion
        assert_eq!(run("echo 2 > 1 && 1 < 3"), "1\n");
        assert_eq!(run("echo 0 || 5"), "1\n");
    }

    #[test]
    fn scopes_and_options() {
        // s: script-local, visible inside functions.
        assert_eq!(
            run("let s:n = 100\nfunction G()\nreturn s:n\nendfunction\necho G()"),
            "100\n"
        );
        // b:/w:/t: scopes.
        assert_eq!(run("let b:x = 'B'\nlet w:y = 'W'\necho b:x . w:y"), "BW\n");
        // &opt + :set (default, set, abbreviation-off, number).
        assert_eq!(run("echo &ignorecase"), "0\n");
        assert_eq!(run("set ignorecase\necho &ic"), "1\n");
        assert_eq!(run("set ignorecase\nset noic\necho &ignorecase"), "0\n");
        assert_eq!(run("set tabstop=4\necho &tabstop"), "4\n");
        // 'ignorecase' flips =~ case sensitivity.
        assert_eq!(run("echo 'FOO' =~ 'foo'"), "0\n");
        assert_eq!(run("set ignorecase\necho 'FOO' =~ 'foo'"), "1\n");
    }

    #[test]
    fn statement_features() {
        // :execute builds and runs a command line.
        assert_eq!(run("let g:n = 5\nexecute 'echo' 'g:n * 2'"), "10\n");
        assert_eq!(run("execute 'let g:z = ' . (3 * 7)\necho g:z"), "21\n");
        // :let list-unpack (+ ;rest).
        assert_eq!(
            run("let [g:a, g:b] = [10, 20]\necho g:a . ',' . g:b"),
            "10,20\n"
        );
        assert_eq!(run("let [g:x; g:r] = [1, 2, 3]\necho g:r"), "[2, 3]\n");
        // :for destructuring over a list of pairs and over items().
        assert_eq!(
            run("for [x, y] in [[1, 2], [3, 4]]\necho x + y\nendfor"),
            "3\n7\n"
        );
    }

    #[test]
    fn compound_assignment() {
        // op= for every operator (ex_let's tv_op).
        assert_eq!(run("let a = 10\nlet a += 3\necho a"), "13\n");
        assert_eq!(run("let a = 10\nlet a -= 3\necho a"), "7\n");
        assert_eq!(run("let a = 10\nlet a *= 4\necho a"), "40\n");
        assert_eq!(run("let a = 20\nlet a /= 6\necho a"), "3\n"); // integer division
        assert_eq!(run("let a = 20\nlet a %= 6\necho a"), "2\n");
        assert_eq!(run("let s = 'foo'\nlet s .= 'bar'\necho s"), "foobar\n");
        // Float accumulation.
        assert_eq!(run("let f = 1.5\nlet f += 0.25\necho f"), "1.75\n");
        // Tight spacing (no blanks around `+=`).
        assert_eq!(run("let n=1\nlet n+=41\necho n"), "42\n");
        // Accumulation inside a loop — the common idiom that was silently a no-op.
        assert_eq!(
            run("let s = 0\nfor i in range(101)\n  let s += i\nendfor\necho s"),
            "5050\n"
        );
        // Scoped target.
        assert_eq!(run("let g:c = 5\nlet g:c += 10\necho g:c"), "15\n");
    }

    #[test]
    fn local_scope_refs() {
        // In a function, l:x is the SAME variable as bare x (both = the slot).
        assert_eq!(
            run("function! F()\nlet x = 1\nlet l:x += 2\nreturn l:x\nendfunction\necho F()"),
            "3\n"
        );
        assert_eq!(
            run("function! S()\nlet l:t = 0\nfor l:i in range(101)\nlet l:t += l:i\nendfor\nreturn l:t\nendfunction\necho S()"),
            "5050\n"
        );
        // Soundness: g: in a function is a DISTINCT store from bare (l:) — the
        // l:-slot optimization must not alias it.
        assert_eq!(
            run("let g:n = 100\nfunction! F()\nlet n = 1\nreturn g:n\nendfunction\necho F()"),
            "100\n"
        );
        // ...and writing bare n must not leak into g:n.
        assert_eq!(
            run("let g:n = 100\nfunction! F()\nlet n = 7\nendfunction\ncall F()\necho g:n"),
            "100\n"
        );
    }

    #[test]
    fn dynamic_range_bounds() {
        // `range()` bound from a parameter, a string (coerced), an expression,
        // and a two-arg inclusive range — all match Vim's tv_get_number coercion.
        assert_eq!(
            run("function! F(n)\nlet s=0\nfor i in range(a:n)\nlet s+=i\nendfor\nreturn s\nendfunction\necho F(101)"),
            "5050\n"
        );
        assert_eq!(
            run("function! F(n)\nlet s=0\nfor i in range(a:n)\nlet s+=i\nendfor\nreturn s\nendfunction\necho F('101')"),
            "5050\n"
        );
        assert_eq!(
            run("function! G(n)\nlet s=0\nfor i in range(2, a:n)\nlet s+=i\nendfor\nreturn s\nendfunction\necho G(10)"),
            "54\n"
        );
        assert_eq!(
            run("function! H(xs)\nlet s=0\nfor i in range(len(a:xs))\nlet s+=i\nendfor\nreturn s\nendfunction\necho H([5, 6, 7, 8])"),
            "6\n"
        );
    }

    #[test]
    fn strftime_builtin() {
        // TZ-independent: literal text passes through; `%%` → `%`.
        assert_eq!(run("echo strftime('hello world')"), "hello world\n");
        assert_eq!(run("echo strftime('100%%')"), "100%\n");
        // Format specifiers expand: `%Y-%m-%d` is always 10 chars (the exact date
        // is TZ-dependent, but its width is not), proving strftime() ran.
        assert_eq!(run("echo len(strftime('%Y-%m-%d', 0))"), "10\n");
    }

    #[test]
    fn strptime_builtin() {
        // Round-trip is TZ-independent: parse-local then format-local cancels the
        // zone offset, recovering the input date.
        assert_eq!(
            run("echo strftime('%Y-%m-%d', strptime('%Y-%m-%d %H:%M', '2020-06-15 12:00'))"),
            "2020-06-15\n"
        );
        // Unparseable input → 0 (strptime fails / mktime == -1).
        assert_eq!(run("echo strptime('%Y', 'not-a-year')"), "0\n");
    }

    #[test]
    fn logical_not_native() {
        // `!x` is VimL's logical not (Number 0/1), incl. double negation.
        assert_eq!(run("echo !0"), "1\n");
        assert_eq!(run("echo !5"), "0\n");
        assert_eq!(run("echo !(3 % 2)"), "0\n");
        assert_eq!(run("echo !!7"), "1\n");
        // In a loop (the native-lowered path): count even i in 0..9 → 5.
        assert_eq!(
            run("let s=0\nfor i in range(10)\nlet s += !(i % 2)\nendfor\necho s"),
            "5\n"
        );
    }

    #[test]
    fn value_position_compare() {
        // A comparison in value position yields VimL's Number 0/1 (verified vs nvim).
        assert_eq!(run("echo 5 > 3"), "1\n");
        assert_eq!(run("echo 3 > 5"), "0\n");
        assert_eq!(run("echo 2 <= 2"), "1\n");
        // Mixed Number/String still coerces via the builtin path (not native).
        assert_eq!(run("echo 1 == '1'"), "1\n");
        // Counting via a value-position compare in a loop (the native path).
        assert_eq!(
            run("let s=0\nfor i in range(10)\nlet s += i >= 5\nendfor\necho s"),
            "5\n" // i = 5,6,7,8,9
        );
    }

    #[test]
    fn ternary_native() {
        // Native-lowered ternary preserves VimL truthiness (incl. the string
        // truthiness fallback) and selects the right branch.
        assert_eq!(run("echo 5 > 3 ? 100 : 200"), "100\n");
        assert_eq!(run("echo 5 < 3 ? 100 : 200"), "200\n");
        assert_eq!(run("echo 'abc' ? 1 : 2"), "2\n"); // non-numeric string is falsy
        assert_eq!(run("echo '5' ? 1 : 2"), "1\n"); // numeric string is truthy
                                                    // Accumulating ternary in a loop (the JIT-lowered path).
        assert_eq!(
            run("let s=0\nfor i in range(10)\nlet s += i % 2 == 0 ? i : 0\nendfor\necho s"),
            "20\n" // 0+2+4+6+8
        );
    }

    #[test]
    fn flatten_and_flattennew() {
        // maxdepth-limited flatten (verified vs nvim).
        assert_eq!(run("echo flatten([1, [2, [3]]], 1)"), "[1, 2, [3]]\n");
        // flatten() mutates its argument in place...
        assert_eq!(
            run("let k=[1,[2,[3]]]\ncall flatten(k)\necho k"),
            "[1, 2, 3]\n"
        );
        // ...flattennew() returns a flattened copy, leaving the source intact.
        assert_eq!(
            run("let l=[1,[2,[3]]]\nlet m=flattennew(l)\necho l\necho m"),
            "[1, [2, [3]]]\n[1, 2, 3]\n"
        );
    }

    #[test]
    fn blob_builtins() {
        // list2blob/blob2list round-trip + native rendering (verified vs nvim).
        assert_eq!(
            run("echo blob2list(list2blob([10, 20, 30]))"),
            "[10, 20, 30]\n"
        );
        assert_eq!(run("echo list2blob([171, 205])"), "0zABCD\n");
        // Blob subscript → a byte; negative index from the end.
        assert_eq!(run("echo list2blob([10, 20, 30, 40])[2]"), "30\n");
        assert_eq!(run("echo list2blob([10, 20, 30, 40])[-1]"), "40\n");
        // Blob slice → a sub-blob (inclusive bounds).
        assert_eq!(
            run("echo blob2list(list2blob([1, 2, 3, 4, 5])[1:3])"),
            "[2, 3, 4]\n"
        );
    }

    #[test]
    fn count_builtin() {
        // String/List/Dict + case-insensitivity + start index (verified vs nvim).
        assert_eq!(run("echo count([1, 2, 2, 3], 2)"), "2\n");
        assert_eq!(run("echo count('banana', 'a')"), "3\n");
        assert_eq!(run("echo count({'a': 1, 'b': 2, 'c': 1}, 1)"), "2\n");
        assert_eq!(run("echo count([1, 2, 2, 3, 2], 2, 0, 2)"), "2\n");
        assert_eq!(run("echo count('aaa', 'aa')"), "1\n"); // non-overlapping
    }

    #[test]
    fn extend_builtins() {
        // List: append, and insert at an index (verified vs nvim).
        assert_eq!(
            run("let l=[1,2,3]\ncall extend(l,[4,5])\necho l"),
            "[1, 2, 3, 4, 5]\n"
        );
        assert_eq!(
            run("let m=[1,2,3]\ncall extend(m,[9],1)\necho m"),
            "[1, 9, 2, 3]\n"
        );
        // Dict: keep (don't overwrite) vs force (default, overwrite).
        assert_eq!(
            run("let d={'a':1,'b':2}\ncall extend(d,{'b':20,'c':3},'keep')\necho d"),
            "{'a': 1, 'b': 2, 'c': 3}\n"
        );
        assert_eq!(
            run("let e={'a':1}\ncall extend(e,{'a':99})\necho e"),
            "{'a': 99}\n"
        );
        // extendnew returns a new value, leaving the source intact.
        assert_eq!(
            run("let n=[1,2]\nlet p=extendnew(n,[3])\necho n\necho p"),
            "[1, 2]\n[1, 2, 3]\n"
        );
    }

    #[test]
    fn index_assignment() {
        // Dict key set (bracket + member), and overwrite — verified vs nvim.
        assert_eq!(
            run("let d={'a':1}\nlet d['b']=2\nlet d.c=3\nlet d['a']=99\necho d"),
            "{'a': 99, 'b': 2, 'c': 3}\n"
        );
        // List element set, including a negative index.
        assert_eq!(
            run("let l=[1,2,3]\nlet l[1]=20\nlet l[-1]=30\necho l"),
            "[1, 20, 30]\n"
        );
        // Nested subscript assignment (shared-Rc propagation through containers).
        assert_eq!(
            run("let d={'a':{'b':1},'l':[10,20]}\nlet d['a']['b']=99\nlet d['l'][0]=100\necho [d['a']['b'], d['l'][0]]"),
            "[99, 100]\n"
        );
        // Compound index-assign (desugars to d[k] = d[k] op rhs).
        assert_eq!(run("let d={'n':5}\nlet d['n']+=10\necho d['n']"), "15\n");
        // A dict-set fires a registered watcher (the add side).
        assert_eq!(
            run("let g:log=[]\nfunction! Cb(d,k,ch)\ncall add(g:log,a:k)\nendfunction\nlet w={}\ncall dictwatcheradd(w,'x',function('Cb'))\nlet w['x']=5\nlet w['y']=6\necho g:log"),
            "['x']\n"
        );
    }

    #[test]
    fn dictwatcher() {
        // A watcher fires only on a pattern-matched key change (here, remove()).
        assert_eq!(
            run("let g:log=[]\nfunction! Cb(d,k,ch)\ncall add(g:log,a:k)\nendfunction\nlet d={'a':1,'b':2}\ncall dictwatcheradd(d,'a',function('Cb'))\ncall remove(d,'a')\ncall remove(d,'b')\necho g:log"),
            "['a']\n"
        );
        // dictwatcherdel unregisters; a trailing '*' is a prefix match.
        assert_eq!(
            run("let g:log=[]\nfunction! Cb(d,k,ch)\ncall add(g:log,a:k)\nendfunction\nlet d={'foo':1}\ncall dictwatcheradd(d,'f*',function('Cb'))\ncall dictwatcherdel(d,'f*',function('Cb'))\ncall remove(d,'foo')\necho g:log"),
            "[]\n"
        );
    }

    #[test]
    fn reduce_builtin() {
        // List fold (no initial uses the first element; with initial).
        assert_eq!(
            run("function! Add(a,b)\nreturn a:a+a:b\nendfunction\necho reduce([1,2,3,4], function('Add'))"),
            "10\n"
        );
        assert_eq!(
            run("function! Add(a,b)\nreturn a:a+a:b\nendfunction\necho reduce([1,2,3,4], function('Add'), 100)"),
            "110\n"
        );
        // String fold and Blob fold.
        assert_eq!(
            run("function! Cat(a,b)\nreturn a:a . a:b\nendfunction\necho reduce('abc', function('Cat'), '>')"),
            ">abc\n"
        );
        assert_eq!(
            run("function! Add(a,b)\nreturn a:a+a:b\nendfunction\necho reduce(list2blob([1,2,3]), function('Add'), 0)"),
            "6\n"
        );
    }

    #[test]
    fn matchstrlist_fnameescape_shiftwidth() {
        // matchstrlist — content verified against nvim (key order is vimlrs's
        // deterministic IndexMap order; nvim's hashtab order differs cosmetically).
        assert_eq!(
            run("echo matchstrlist(['a1','b2','cc'], '\\d')"),
            "[{'idx': 0, 'byteidx': 1, 'text': '1'}, {'idx': 1, 'byteidx': 1, 'text': '2'}]\n"
        );
        // submatches padded to the 9 \1..\9 backrefs.
        assert_eq!(
            run("echo matchstrlist(['foobar'], '\\(o\\+\\)', {'submatches': v:true})[0]['submatches']"),
            "['oo', '', '', '', '', '', '', '', '']\n"
        );
        // fnameescape (verified vs nvim).
        assert_eq!(run("echo fnameescape('foo bar')"), "foo\\ bar\n");
        assert_eq!(run("echo fnameescape('a%b#c')"), "a\\%b\\#c\n");
        // shiftwidth: 'shiftwidth', or 'tabstop' when sw==0.
        assert_eq!(run("echo shiftwidth()"), "8\n");
        assert_eq!(run("set shiftwidth=4\necho shiftwidth()"), "4\n");
        assert_eq!(
            run("set shiftwidth=0\nset tabstop=2\necho shiftwidth()"),
            "2\n"
        );
    }

    #[test]
    fn editor_absent_builtins() {
        // A standalone interpreter has no editor UI / GUI / server (verified vs nvim).
        assert_eq!(run("echo visualmode()"), "\n");
        assert_eq!(run("echo pumvisible()"), "0\n");
        assert_eq!(run("echo wildmenumode()"), "0\n");
        assert_eq!(run("echo did_filetype()"), "0\n");
        assert_eq!(run("echo eventhandler()"), "0\n");
        assert_eq!(run("echo hlexists('Foo')"), "0\n");
        assert_eq!(run("echo windowsversion()"), "\n");
        assert_eq!(run("echo getfontname()"), "\n");
        assert_eq!(run("echo foreground()"), "0\n");
        assert_eq!(run("echo pum_getpos()"), "{}\n");
        assert_eq!(run("echo serverlist()"), "[]\n");
        assert_eq!(run("echo mode()"), "n\n");
    }

    #[test]
    fn registers() {
        // setreg/getreg/getregtype (values verified against `nvim --clean`).
        assert_eq!(run("call setreg('a','hello')\necho getreg('a')"), "hello\n");
        assert_eq!(run("call setreg('a','hello')\necho getregtype('a')"), "v\n");
        assert_eq!(
            run("call setreg('b',['x','y','z'])\necho getreg('b',1,1)"),
            "['x', 'y', 'z']\n"
        );
        assert_eq!(
            run("call setreg('b',['x','y'])\necho getregtype('b')"),
            "V\n"
        );
        // charwise append continues the last line.
        assert_eq!(
            run("call setreg('a','hello')\ncall setreg('a',' world','a')\necho getreg('a')"),
            "hello world\n"
        );
        // dict form + getreginfo (unset → {}); bracket access (the `.member`
        // concat ambiguity means tests use ['key']).
        assert_eq!(run("echo getreginfo('z')"), "{}\n");
        assert_eq!(
            run("call setreg('b',['x','y'])\necho getreginfo('b')['regcontents']"),
            "['x', 'y']\n"
        );
        assert_eq!(
            run("call setreg('e',{'regcontents':['p','q'],'regtype':'V'})\necho getreg('e',1,1)\necho getregtype('e')"),
            "['p', 'q']\nV\n"
        );
        // reg_recording/executing/recorded are empty standalone.
        assert_eq!(run("echo reg_recording()"), "\n");
    }

    #[test]
    fn misc_utilities() {
        assert_eq!(run("echo gettext('hello')"), "hello\n");
        // indexof: first item where the expr (v:val/v:key) is true, else -1.
        assert_eq!(run("echo indexof([1,2,3,4], 'v:val > 2')"), "2\n");
        assert_eq!(
            run("echo indexof(['a','bb','ccc'], 'len(v:val) == 2')"),
            "1\n"
        );
        assert_eq!(run("echo indexof([1,2,3], 'v:val > 9')"), "-1\n");
        // id() is non-empty and stable for a container, empty for a scalar.
        assert_eq!(run("let l=[1,2]\necho id(l) == id(l)"), "1\n");
        assert_eq!(run("echo id(5)"), "\n");
        assert_eq!(run("let a=[1]\nlet b=[1]\necho id(a) != id(b)"), "1\n");
        // garbagecollect() is a no-op that returns nothing.
        assert_eq!(run("call garbagecollect()\necho 'ok'"), "ok\n");
    }

    #[test]
    fn fnamemodify_and_glob() {
        // Filename modifiers (verified against `nvim --clean`).
        assert_eq!(
            run("echo fnamemodify('/home/u/file.txt.gz', ':t')"),
            "file.txt.gz\n"
        );
        assert_eq!(
            run("echo fnamemodify('/home/u/file.txt.gz', ':h')"),
            "/home/u\n"
        );
        assert_eq!(
            run("echo fnamemodify('/home/u/file.txt.gz', ':r')"),
            "/home/u/file.txt\n"
        );
        assert_eq!(run("echo fnamemodify('/home/u/file.txt.gz', ':e')"), "gz\n");
        assert_eq!(run("echo fnamemodify('a.b.c', ':e:e')"), "b.c\n");
        assert_eq!(run("echo fnamemodify('a.b.c', ':r:r')"), "a\n");
        assert_eq!(
            run("echo fnamemodify('path/to/this.file.ext', ':e:e')"),
            "file.ext\n"
        );
        assert_eq!(run("echo fnamemodify('foo/bar.txt', ':t:r')"), "bar\n");
        assert_eq!(
            run("echo fnamemodify('foo/bar.txt', ':s?bar?baz?')"),
            "foo/baz.txt\n"
        );
        // glob2regpat (pure string transform).
        assert_eq!(run("echo glob2regpat('*.txt')"), "\\.txt$\n");
        assert_eq!(run("echo glob2regpat('foo?bar')"), "^foo.bar$\n");
        assert_eq!(run("echo glob2regpat('a.b')"), "^a\\.b$\n");
        assert_eq!(run("echo haslocaldir()"), "0\n");
    }

    #[test]
    fn readdir_readblob_filecopy() {
        // Hermetic: build a dir under a tempname(), exercise readdir/readblob/
        // filecopy/readfile, then clean up. Headless-CI-safe.
        let src = "let d=tempname()\ncall mkdir(d)\n\
            call writefile(['AAA'], d.'/a.txt')\n\
            call writefile(['B'], d.'/b.log')\n\
            echo sort(readdir(d))\n\
            echo readdir(d, 'v:val =~ \"txt$\"')\n\
            echo readblob(d.'/a.txt')\n\
            call filecopy(d.'/a.txt', d.'/c.txt')\n\
            echo readfile(d.'/c.txt')\n\
            call delete(d,'rf')";
        assert_eq!(
            run(src),
            "['a.txt', 'b.log']\n['a.txt']\n0z4141410A\n['AAA']\n"
        );
    }

    #[test]
    fn fs_builtins() {
        // Pure path builtins (verified against `nvim --clean`).
        assert_eq!(run("echo isabsolutepath('/usr/bin')"), "1\n");
        assert_eq!(run("echo isabsolutepath('foo/bar')"), "0\n");
        assert_eq!(run("echo simplify('/a/b/../c')"), "/a/c\n");
        assert_eq!(run("echo simplify('a/./b//c')"), "a/b/c\n");
        // writefile → readfile round-trip + delete, through a system temp path
        // (hermetic: tempname() lives under the OS temp dir, fine in headless CI).
        assert_eq!(
            run("let f=tempname()\ncall writefile(['a','b','c'], f)\necho readfile(f)\necho getfsize(f)\ncall delete(f)\necho filereadable(f)"),
            "['a', 'b', 'c']\n6\n0\n"
        );
        // mkdir/isdirectory/delete on a temp directory.
        assert_eq!(
            run("let d=tempname()\necho mkdir(d)\necho isdirectory(d)\necho delete(d,'d')\necho isdirectory(d)"),
            "1\n1\n0\n0\n"
        );
    }

    #[test]
    fn list_range_assign() {
        // `let l[i:j] = list` — values verified against `nvim --clean`.
        assert_eq!(
            run("let l=[1,2,3,4,5]\nlet l[1:3]=[20,30,40]\necho l"),
            "[1, 20, 30, 40, 5]\n"
        );
        // `l[i:]` replaces from i to the end (grows when the source is longer).
        assert_eq!(
            run("let m=[0,0,0,0]\nlet m[1:]=[7,8,9]\necho m"),
            "[0, 7, 8, 9]\n"
        );
        assert_eq!(
            run("let g=[1,2]\nlet g[1:]=[5,6,7,8]\necho g"),
            "[1, 5, 6, 7, 8]\n"
        );
        // `l[:j]` replaces from the start; single-element range; negative index.
        assert_eq!(run("let p=[1,2,3]\nlet p[:1]=[8,9]\necho p"), "[8, 9, 3]\n");
        assert_eq!(
            run("let q=[1,2,3]\nlet q[1:1]=[99]\necho q"),
            "[1, 99, 3]\n"
        );
        assert_eq!(
            run("let l=[1,2,3,4]\nlet l[-2:]=[88,99]\necho l"),
            "[1, 2, 88, 99]\n"
        );
    }

    #[test]
    fn vim_vars() {
        // v: variable store (vimvars[]). Expected values verified against nvim.
        assert_eq!(run("echo v:version"), "801\n");
        assert_eq!(
            run(
                "echo v:t_number v:t_string v:t_func v:t_list v:t_dict v:t_float v:t_bool v:t_blob"
            ),
            "0 1 2 3 4 5 6 10\n"
        );
        assert_eq!(
            run("echo v:numbermax v:numbermin v:numbersize"),
            "9223372036854775807 -9223372036854775808 64\n"
        );
        assert_eq!(run("echo v:maxcol"), "2147483647\n");
        assert_eq!(run("echo v:true v:false v:null"), "v:true v:false v:null\n");
        assert_eq!(run("echo v:searchforward v:hlsearch v:count1"), "1 1 1\n");
        assert_eq!(run("echo type(v:msgpack_types)"), "4\n");
        assert_eq!(run("echo type(v:errors)"), "3\n");
        assert_eq!(run("echo v:register"), "\"\n");
        // Mutable v: var round-trips; read-only v: var declines assignment.
        assert_eq!(run("let v:errmsg = 'boom'\necho v:errmsg"), "boom\n");
        assert_eq!(run("echo v:errmsg"), "\n");
    }

    #[test]
    fn partials() {
        // function() with bound args → a Partial; call() prepends them. Each
        // assertion's expected value was verified against `nvim --clean`.
        let add = "function! Add(a,b)\nreturn a:a+a:b\nendfunction\n";
        let add3 = "function! Add3(x,a,b)\nreturn a:x+a:a+a:b\nendfunction\n";
        // call(partial, args): Add(10, 5).
        assert_eq!(
            run(&format!(
                "{add}let P=function('Add',[10])\necho call(P,[5])"
            )),
            "15\n"
        );
        // type() of a Partial is 2 (Funcref).
        assert_eq!(run(&format!("{add}echo type(function('Add',[10]))")), "2\n");
        // echo / string() render as function('name', [args]).
        assert_eq!(
            run(&format!("{add}echo function('Add',[10])")),
            "function('Add', [10])\n"
        );
        assert_eq!(
            run(&format!("{add}echo string(function('Add',[1,2]))")),
            "function('Add', [1, 2])\n"
        );
        // Partial honored in reduce() — Add3(100, acc, item).
        assert_eq!(
            run(&format!(
                "{add3}echo reduce([1,2,3], function('Add3',[100]), 0)"
            )),
            "306\n"
        );
        // Partial honored in map() — Add3(10, key, val).
        assert_eq!(
            run(&format!("{add3}echo map([1,2,3], function('Add3',[10]))")),
            "[11, 13, 15]\n"
        );
        // func_equal(): same name+args equal; differing args/arity not.
        assert_eq!(
            run(&format!(
                "{add}echo function('Add',[1,2])==function('Add',[1,2])"
            )),
            "1\n"
        );
        assert_eq!(
            run(&format!(
                "{add}echo function('Add',[1,2])==function('Add',[1,3])"
            )),
            "0\n"
        );
        assert_eq!(
            run(&format!("{add}echo function('Add',[1])==function('Add')")),
            "0\n"
        );
    }

    #[test]
    fn map_filter_foreach_mapnew() {
        // List map / filter.
        assert_eq!(run("echo map([1,2,3], 'v:val * 2')"), "[2, 4, 6]\n");
        assert_eq!(run("echo filter([1,2,3,4], 'v:val % 2 == 0')"), "[2, 4]\n");
        // Dict map.
        assert_eq!(
            run("echo map({'a':1,'b':2}, 'v:val + 10')"),
            "{'a': 11, 'b': 12}\n"
        );
        // String map / filter (new — char-by-char).
        assert_eq!(run("echo map('abc', 'toupper(v:val)')"), "ABC\n");
        assert_eq!(run("echo filter('hello', 'v:val != \"l\"')"), "heo\n");
        // Blob map.
        assert_eq!(
            run("echo blob2list(map(list2blob([1,2,3]), 'v:val + 1'))"),
            "[2, 3, 4]\n"
        );
        // mapnew leaves the source intact.
        assert_eq!(
            run("let l=[1,2,3]\nlet m=mapnew(l, 'v:val*10')\necho l\necho m"),
            "[1, 2, 3]\n[10, 20, 30]\n"
        );
        // foreach with a funcref (side effect).
        assert_eq!(
            run("let g:s=0\nfunction! Add(i,x)\nlet g:s+=a:x\nendfunction\ncall foreach([1,2,3,4], function('Add'))\necho g:s"),
            "10\n"
        );
        // map with a funcref.
        assert_eq!(
            run(
                "function! Dbl(k,v)\nreturn a:v*2\nendfunction\necho map([5,6,7], function('Dbl'))"
            ),
            "[10, 12, 14]\n"
        );
    }

    #[test]
    fn sort_and_uniq() {
        // Default (string) vs 'n' numeric vs 'i' ignorecase vs 'f' float.
        assert_eq!(run("echo sort([10,9,2,100])"), "[10, 100, 2, 9]\n");
        assert_eq!(run("echo sort([10,9,2,100], 'n')"), "[2, 9, 10, 100]\n");
        assert_eq!(run("echo sort(['B','a','C'], 'i')"), "['a', 'B', 'C']\n");
        assert_eq!(run("echo sort([3.5,1.2,2.8], 'f')"), "[1.2, 2.8, 3.5]\n");
        // uniq drops adjacent-equal (after sorting).
        assert_eq!(run("echo uniq(sort([3,1,2,2,1,3]))"), "[1, 2, 3]\n");
        // Funcref comparator (sort {func}) via the bridge hook.
        assert_eq!(
            run("function! Desc(a,b)\nreturn a:b - a:a\nendfunction\necho sort([1,5,3,2,4], 'Desc')"),
            "[5, 4, 3, 2, 1]\n"
        );
    }

    #[test]
    fn join_and_list_slice() {
        // join() renders items as `:echo` (nested structurally) — the prior
        // tv_get_string version dropped nested Lists. Verified vs nvim.
        assert_eq!(run("echo join([[1,2],3], '-')"), "[1, 2]-3\n");
        assert_eq!(run("echo join(['a','b','c'])"), "a b c\n");
        assert_eq!(run("echo join([1,2,3], ', ')"), "1, 2, 3\n");
        // List index/slice via the ported tv_list_slice_or_index.
        assert_eq!(run("echo [10,20,30,40][2]"), "30\n");
        assert_eq!(run("echo [10,20,30,40][1:2]"), "[20, 30]\n");
        assert_eq!(run("echo [10,20,30,40][-2:]"), "[30, 40]\n");
        assert_eq!(run("echo [10,20,30,40][1:]"), "[20, 30, 40]\n");
    }

    #[test]
    fn remove_builtin() {
        // List: single index (returns the item), and a range (returns a sub-list).
        assert_eq!(
            run("let l=[1,2,3,4]\necho remove(l, 1)\necho l"),
            "2\n[1, 3, 4]\n"
        );
        assert_eq!(
            run("let m=[10,20,30,40,50]\necho remove(m, 1, 3)\necho m"),
            "[20, 30, 40]\n[10, 50]\n"
        );
        // Dict: by key.
        assert_eq!(
            run("let d={'a':1,'b':2}\necho remove(d, 'a')\necho d"),
            "1\n{'b': 2}\n"
        );
        // Blob: single byte, and a range (returns a sub-blob).
        assert_eq!(
            run("let b=list2blob([1,2,3,4,5])\necho remove(b, 2)\necho blob2list(b)"),
            "3\n[1, 2, 4, 5]\n"
        );
        assert_eq!(
            run("echo blob2list(remove(list2blob([1,2,3,4,5]), 1, 2))"),
            "[2, 3]\n"
        );
    }

    #[test]
    fn sha256_builtin() {
        // FIPS-180-2 test vectors (also bit-exact vs Neovim).
        assert_eq!(
            run("echo sha256('abc')"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );
        assert_eq!(
            run("echo sha256('')"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n"
        );
        // > 64 bytes exercises the multi-block loop + length padding.
        assert_eq!(
            run("echo sha256(repeat('a', 200))"),
            "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5\n"
        );
    }

    #[test]
    fn pathshorten_builtin() {
        // Verified bit-exact against Neovim: each directory shortens to `len`
        // chars (keeping a leading `~`/`.`); the final component is untouched.
        assert_eq!(
            run("echo pathshorten('~/foo/bar/baz.vim')"),
            "~/f/b/baz.vim\n"
        );
        assert_eq!(
            run("echo pathshorten('/usr/local/bin/foo')"),
            "/u/l/b/foo\n"
        );
        assert_eq!(
            run("echo pathshorten('~/.config/nvim/init.vim', 2)"),
            "~/.co/nv/init.vim\n"
        );
        assert_eq!(run("echo pathshorten('noseps')"), "noseps\n");
    }

    #[test]
    fn bitwise_builtins() {
        // and/or/xor/invert — verified bit-exact against Neovim.
        assert_eq!(run("echo and(12, 10)"), "8\n");
        assert_eq!(run("echo or(12, 10)"), "14\n");
        assert_eq!(run("echo xor(12, 10)"), "6\n");
        assert_eq!(run("echo invert(0)"), "-1\n");
        // Nested + a slotted accumulator loop (the native-lowered path).
        assert_eq!(run("echo and(255, xor(170, 85))"), "255\n");
        assert_eq!(
            run("let h=0\nfor i in range(8)\nlet h=xor(h,i)\nendfor\necho h"),
            "0\n"
        );
    }

    #[test]
    fn rand_srand_match_neovim() {
        // Bit-exact against real Neovim (verified with `nvim --headless`):
        // seeded srand/rand are deterministic (xoshiro128** + splitmix32).
        assert_eq!(
            run("echo srand(123)"),
            "[1482537201, 2737842301, 2667502145, 3175280481]\n"
        );
        // rand(seed) advances the 4-number seed list in place, yielding a fixed
        // sequence and leaving the list at the advanced state.
        assert_eq!(
            run("let s = srand(123)\necho rand(s)\necho rand(s)\necho string(s)"),
            "3146710351\n1913850085\n[3045381587, 2236591504, 3326704989, 1565652893]\n"
        );
    }

    #[test]
    fn reltime_builtins() {
        // reltime() with no args → a 2-element [high, low] list.
        assert_eq!(run("echo len(reltime())"), "2\n");
        // Difference of a timestamp with itself is exactly zero, in both forms.
        assert_eq!(
            run("let s = reltime()\necho reltimefloat(reltime(s, s))"),
            "0.0\n"
        );
        // reltimestr formats seconds as %10.6f (right-justified width 10).
        assert_eq!(
            run("let s = reltime()\necho reltimestr(reltime(s, s))"),
            "  0.000000\n"
        );
    }

    #[test]
    fn eval_execute_dictmap() {
        assert_eq!(run("echo eval('1 + 2 * 3')"), "7\n");
        assert_eq!(
            run("echo map({'a': 1, 'b': 2}, 'v:val * 10')"),
            "{'a': 10, 'b': 20}\n"
        );
        assert_eq!(
            run("echo filter({'a': 1, 'b': 2, 'c': 3}, 'v:val > 1')"),
            "{'b': 2, 'c': 3}\n"
        );
        assert_eq!(run("echo execute('echo 41 + 1')"), "42\n\n"); // captured "42\n" + echo's \n
        assert_eq!(run("echo deepcopy([[1], [2]])"), "[[1], [2]]\n");
        assert_eq!(run("echo fmod(10.0, 3.0)"), "1.0\n");
    }

    #[test]
    fn call_resolves_builtins() {
        // call() / funcrefs accept builtin names, not just user functions.
        assert_eq!(run("echo call('printf', ['%d-%d', 3, 4])"), "3-4\n");
        assert_eq!(run("echo call('abs', [-5])"), "5\n");
        assert_eq!(run("echo call('len', [[1, 2, 3]])"), "3\n");
        assert_eq!(run("let F = function('toupper') | echo F('hi')"), "HI\n");
        // A Partial over a builtin binds leading args.
        assert_eq!(
            run("let G = function('substitute', ['axa']) | echo G('a', 'Z', 'g')"),
            "ZxZ\n"
        );
    }

    #[test]
    fn printf_sign_flags_and_byte_len() {
        assert_eq!(run("echo printf('%+d % d', 7, 7)"), "+7  7\n");
        assert_eq!(run("echo printf('%+05d', 7)"), "+0007\n");
        // len() of a String is its byte length.
        assert_eq!(run("echo len('héllo')"), "6\n");
    }

    #[test]
    fn trim_dir_and_scope_dicts() {
        assert_eq!(run("echo trim('  x  ', ' ', 1)"), "x  \n");
        assert_eq!(run("echo trim('  x  ', ' ', 2)"), "  x\n");
        // A bare scope sigil is a Dict of that scope.
        assert_eq!(run("let g:zz = 9 | echo get(g:, 'zz', -1)"), "9\n");
        assert_eq!(run("echo type(g:)"), "4\n");
        assert_eq!(run("let g:q = 1 | echo has_key(g:, 'q')"), "1\n");
    }

    #[test]
    fn printf_g_and_substitute_expr() {
        assert_eq!(
            run("echo printf('%g %g %g', 0.1, 1000000.0, 0.0001)"),
            "0.1 1e+06 0.0001\n"
        );
        assert_eq!(run("echo printf('%.3g', 3.14159)"), "3.14\n");
        // \= replacement expression with submatch().
        assert_eq!(
            run("echo substitute('abcABC', '[a-z]', '\\=toupper(submatch(0))', 'g')"),
            "ABCABC\n"
        );
        assert_eq!(
            run("echo substitute('x1y2', '\\d', '\\=submatch(0)+10', 'g')"),
            "x11y12\n"
        );
    }

    #[test]
    fn batch4_builtins() {
        assert_eq!(
            run("echo matchlist('a-b', '\\(\\w\\)-\\(\\w\\)')"),
            "['a-b', 'a', 'b', '', '', '', '', '', '', '']\n"
        );
        assert_eq!(run("echo matchend('abc123', '\\d\\+')"), "6\n");
        assert_eq!(run("echo strridx('a/b/c', '/')"), "3\n");
        assert_eq!(run("echo escape('a.b', '.')"), "a\\.b\n");
        assert_eq!(run("echo tr('hello', 'el', 'ip')"), "hippo\n");
        assert_eq!(run("echo str2list('AB')"), "[65, 66]\n");
        assert_eq!(run("echo list2str([72, 105])"), "Hi\n");
        assert_eq!(run("echo flatten([1, [2, [3]]])"), "[1, 2, 3]\n");
        assert_eq!(
            run("function A(a, b)\nreturn a:a + a:b\nendfunction\necho reduce([1, 2, 3], function('A'), 0)"),
            "6\n"
        );
    }

    #[test]
    fn regex_through_eval() {
        assert_eq!(run("echo 'foobar123' =~ '\\d\\+'"), "1\n");
        assert_eq!(run("echo 'hello' =~ '^h.*o$'"), "1\n");
        assert_eq!(run("echo matchstr('ab123cd', '\\d\\+')"), "123\n");
        assert_eq!(run("echo substitute('a-b-c', '-', '/', 'g')"), "a/b/c\n");
        assert_eq!(
            run("echo substitute('John Smith', '\\(\\w\\+\\) \\(\\w\\+\\)', '\\2 \\1', '')"),
            "Smith John\n"
        );
        assert_eq!(run("echo split('a1b22c', '\\d\\+')"), "['a', 'b', 'c']\n");
        // :catch with a regex pattern.
        assert_eq!(
            run("try\nthrow 'E42: bad'\ncatch /E\\d\\+/\necho 'caught'\nendtry"),
            "caught\n"
        );
    }

    #[test]
    fn math_string_list_builtins() {
        assert_eq!(
            run("echo float2nr(sqrt(16.0)) . float2nr(pow(2.0, 5.0))"),
            "432\n"
        );
        assert_eq!(run("echo and(12, 10) . or(12, 10) . xor(12, 10)"), "8146\n");
        assert_eq!(run("echo strpart('hello world', 6)"), "world\n");
        assert_eq!(run("echo stridx('abcabc', 'c', 3)"), "5\n");
        assert_eq!(run("echo trim('  hi  ')"), "hi\n");
        assert_eq!(run("echo insert([2, 3], 1)"), "[1, 2, 3]\n");
        assert_eq!(run("echo remove([10, 20, 30], 1)"), "20\n");
        assert_eq!(run("echo extend([1, 2], [3, 4])"), "[1, 2, 3, 4]\n");
        assert_eq!(run("echo uniq([1, 1, 2, 3, 3])"), "[1, 2, 3]\n");
        assert_eq!(run("echo sort([3.0, 1.0, 2.5], 'n')"), "[1.0, 2.5, 3.0]\n");
        assert_eq!(run("echo items({'a': 1})"), "[['a', 1]]\n");
    }

    #[test]
    fn callback_builtins() {
        // map with a string expression binding v:val.
        assert_eq!(run("echo map([1, 2, 3], 'v:val * 10')"), "[10, 20, 30]\n");
        // filter.
        assert_eq!(
            run("echo filter([1, 2, 3, 4], 'v:val % 2 == 0')"),
            "[2, 4]\n"
        );
        // sort: default string order vs numeric.
        assert_eq!(run("echo sort([10, 9, 2])"), "[10, 2, 9]\n");
        assert_eq!(run("echo sort([10, 9, 2], 'n')"), "[2, 9, 10]\n");
        // map with a funcref + call().
        assert_eq!(
            run("function D(k, v)\nreturn a:v + a:v\nendfunction\necho map([1, 2], function('D'))"),
            "[2, 4]\n"
        );
        assert_eq!(
            run("function S(a, b)\nreturn a:a + a:b\nendfunction\necho call('S', [3, 4])"),
            "7\n"
        );
        // chained.
        assert_eq!(
            run("echo sort(filter(map([5, 1, 4], 'v:val * v:val'), 'v:val > 4'), 'n')"),
            "[16, 25]\n"
        );
    }

    #[test]
    fn builtins_batch() {
        assert_eq!(run("echo join(split('a b c'), '-')"), "a-b-c\n");
        assert_eq!(run("echo range(2, 8, 2)"), "[2, 4, 6, 8]\n");
        assert_eq!(run("echo toupper('hi') . repeat('!', 3)"), "HI!!!\n");
        assert_eq!(run("echo max([3, 9, 2]) . '/' . min([3, 9, 2])"), "9/2\n");
        assert_eq!(run("echo has_key({'x': 1}, 'x')"), "1\n");
        assert_eq!(run("echo get([10, 20, 30], 1)"), "20\n");
        assert_eq!(run("echo printf('%s=%05X', 'n', 255)"), "n=000FF\n");
        assert_eq!(
            run("echo count([1, 2, 2, 3], 2) . index([1, 2, 3], 3)"),
            "22\n"
        );
        assert_eq!(run("echo reverse([1, 2, 3])"), "[3, 2, 1]\n");
    }

    #[test]
    fn exceptions() {
        // throw caught by matching pattern + v:exception + finally always runs.
        assert_eq!(
            run("try\nthrow 'boom'\necho 'skipped'\ncatch /boom/\necho 'caught ' . v:exception\nfinally\necho 'fin'\nendtry"),
            "caught boom\nfin\n"
        );
        // throw from a function propagates and is caught in the caller (no
        // spurious value consumed by the aborted command).
        assert_eq!(
            run("function R(n)\nif a:n < 0\nthrow 'neg'\nendif\nreturn a:n\nendfunction\ntry\necho R(7)\necho R(-1)\ncatch /neg/\necho 'caught'\nendtry"),
            "7\ncaught\n"
        );
    }

    #[test]
    fn user_functions() {
        // a: args + return.
        assert_eq!(
            run("function Add(a, b)\nreturn a:a + a:b\nendfunction\necho Add(3, 4)"),
            "7\n"
        );
        // recursion (scope stack + nested calls).
        assert_eq!(
            run("function Fact(n)\nif a:n <= 1\nreturn 1\nendif\nreturn a:n * Fact(a:n - 1)\nendfunction\necho Fact(5)"),
            "120\n"
        );
        // l: locals + call inside an expression.
        assert_eq!(
            run("function Inc(x)\nlet l:y = a:x + 1\nreturn l:y\nendfunction\necho Inc(10) * 2"),
            "22\n"
        );
    }

    #[test]
    fn control_flow() {
        assert_eq!(
            run("let g:x = 5\nif g:x > 10\necho 'big'\nelseif g:x > 3\necho 'mid'\nelse\necho 'small'\nendif"),
            "mid\n"
        );
        // while with break/continue: i=1 echo, i=2 continue, i=3 echo, i=4 break.
        assert_eq!(
            run("let g:i = 0\nwhile g:i < 9\nlet g:i = g:i + 1\nif g:i == 2\ncontinue\nendif\nif g:i == 4\nbreak\nendif\necho g:i\nendwhile"),
            "1\n3\n"
        );
        // for over a list.
        assert_eq!(run("for n in [10, 20, 30]\necho n\nendfor"), "10\n20\n30\n");
        // while accumulator.
        assert_eq!(
            run("let g:s = 0\nlet g:k = 1\nwhile g:k <= 4\nlet g:s = g:s + g:k\nlet g:k = g:k + 1\nendwhile\necho g:s"),
            "10\n"
        );
    }

    #[test]
    fn ternary_index_builtins() {
        assert_eq!(run("echo 1 ? 'y' : 'n'"), "y\n");
        assert_eq!(run("echo 0 ?? 7"), "7\n");
        assert_eq!(run("echo [10, 20, 30][-1]"), "30\n");
        assert_eq!(run("echo 'hello'[1:3]"), "ell\n");
        assert_eq!(run("echo len([1, 2, 3])"), "3\n");
        assert_eq!(run("echo abs(-5)"), "5\n");
        assert_eq!(run("echo [1, 2, 3]->len()"), "3\n");
    }
}
