//! STUB surface for `csrc/eval/funcs.c` — generated, do not edit.
#![allow(dead_code, unused_variables, non_snake_case, clippy::all)]

/// Port of `get_function_name()` — `csrc/eval/funcs.c:178`. STUB: not yet ported.
/// C: `char *get_function_name(expand_T *xp, int idx)`
pub fn get_function_name() {
    unimplemented!("STUB get_function_name — csrc/eval/funcs.c:178")
}

/// Port of `get_expr_name()` — `csrc/eval/funcs.c:214`. STUB: not yet ported.
/// C: `char *get_expr_name(expand_T *xp, int idx)`
pub fn get_expr_name() {
    unimplemented!("STUB get_expr_name — csrc/eval/funcs.c:214")
}

/// Port of `find_internal_func()` — `csrc/eval/funcs.c:235`. STUB: not yet ported.
/// C: `const EvalFuncDef *find_internal_func(const char *const name)`
pub fn find_internal_func() {
    unimplemented!("STUB find_internal_func — csrc/eval/funcs.c:235")
}

/// Port of `find_internal_func_lua()` — `csrc/eval/funcs.c:244`. STUB: not yet ported.
/// C: `const char *find_internal_func_lua(const char *const name)`
pub fn find_internal_func_lua() {
    unimplemented!("STUB find_internal_func_lua — csrc/eval/funcs.c:244")
}

/// Port of `check_internal_func()` — `csrc/eval/funcs.c:257`. STUB: not yet ported.
/// C: `int check_internal_func(const EvalFuncDef *const fdef, const int argcount)`
pub fn check_internal_func() {
    unimplemented!("STUB check_internal_func — csrc/eval/funcs.c:257")
}

/// Port of `call_internal_func()` — `csrc/eval/funcs.c:279`. STUB: not yet ported.
/// C: `int call_internal_func(const char *const fname, const int argcount, typval_T *const argvars,`
pub fn call_internal_func() {
    unimplemented!("STUB call_internal_func — csrc/eval/funcs.c:279")
}

/// Port of `call_internal_method()` — `csrc/eval/funcs.c:297`. STUB: not yet ported.
/// C: `int call_internal_method(const char *const fname, const int argcount, typval_T *const argvars,`
pub fn call_internal_method() {
    unimplemented!("STUB call_internal_method — csrc/eval/funcs.c:297")
}

/// Port of `non_zero_arg()` — `csrc/eval/funcs.c:328`. STUB: not yet ported.
/// C: `static bool non_zero_arg(typval_T *argvars)`
pub fn non_zero_arg() {
    unimplemented!("STUB non_zero_arg — csrc/eval/funcs.c:328")
}

/// Port of `api_wrapper()` — `csrc/eval/funcs.c:360`. STUB: not yet ported.
/// C: `static void api_wrapper(typval_T *argvars, typval_T *rettv, EvalFuncData fptr)`
pub fn api_wrapper() {
    unimplemented!("STUB api_wrapper — csrc/eval/funcs.c:360")
}

/// Port of `lua_wrapper()` — `csrc/eval/funcs.c:397`. STUB: not yet ported.
/// C: `static void lua_wrapper(typval_T *argvars, typval_T *rettv, EvalFuncData fptr)`
pub fn lua_wrapper() {
    unimplemented!("STUB lua_wrapper — csrc/eval/funcs.c:397")
}

/// Port of `f_api_info()` — `csrc/eval/funcs.c:451`. STUB: not yet ported.
pub fn f_api_info(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_api_info — csrc/eval/funcs.c:451")
}

/// Port of `tv_get_buf()` — `csrc/eval/funcs.c:471`. STUB: not yet ported.
/// C: `buf_T *tv_get_buf(typval_T *tv, int curtab_only)`
pub fn tv_get_buf() {
    unimplemented!("STUB tv_get_buf — csrc/eval/funcs.c:471")
}

/// Port of `tv_get_buf_from_arg()` — `csrc/eval/funcs.c:510`. STUB: not yet ported.
/// C: `buf_T *tv_get_buf_from_arg(typval_T *const tv) FUNC_ATTR_NONNULL_ALL`
pub fn tv_get_buf_from_arg() {
    unimplemented!("STUB tv_get_buf_from_arg — csrc/eval/funcs.c:510")
}

/// Port of `get_buf_arg()` — `csrc/eval/funcs.c:523`. STUB: not yet ported.
/// C: `buf_T *get_buf_arg(typval_T *arg)`
pub fn get_buf_arg() {
    unimplemented!("STUB get_buf_arg — csrc/eval/funcs.c:523")
}

/// Port of `f_call()` — `csrc/eval/funcs.c:547`. STUB: not yet ported.
pub fn f_call(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_call — csrc/eval/funcs.c:547")
}

/// Port of `f_changenr()` — `csrc/eval/funcs.c:604`. STUB: not yet ported.
pub fn f_changenr(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_changenr — csrc/eval/funcs.c:604")
}

/// Port of `f_chanclose()` — `csrc/eval/funcs.c:610`. STUB: not yet ported.
pub fn f_chanclose(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_chanclose — csrc/eval/funcs.c:610")
}

/// Port of `f_chansend()` — `csrc/eval/funcs.c:649`. STUB: not yet ported.
pub fn f_chansend(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_chansend — csrc/eval/funcs.c:649")
}

/// Port of `get_col()` — `csrc/eval/funcs.c:712`. STUB: not yet ported.
/// C: `static void get_col(typval_T *argvars, typval_T *rettv, bool charcol)`
pub fn get_col() {
    unimplemented!("STUB get_col — csrc/eval/funcs.c:712")
}

/// Port of `get_optional_window()` — `csrc/eval/funcs.c:769`. STUB: not yet ported.
/// C: `win_T *get_optional_window(typval_T *argvars, int idx)`
pub fn get_optional_window() {
    unimplemented!("STUB get_optional_window — csrc/eval/funcs.c:769")
}

/// Port of `f_ctxget()` — `csrc/eval/funcs.c:848`. STUB: not yet ported.
pub fn f_ctxget(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_ctxget — csrc/eval/funcs.c:848")
}

/// Port of `f_ctxpop()` — `csrc/eval/funcs.c:873`. STUB: not yet ported.
pub fn f_ctxpop(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_ctxpop — csrc/eval/funcs.c:873")
}

/// Port of `f_ctxpush()` — `csrc/eval/funcs.c:881`. STUB: not yet ported.
pub fn f_ctxpush(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_ctxpush — csrc/eval/funcs.c:881")
}

/// Port of `f_ctxset()` — `csrc/eval/funcs.c:912`. STUB: not yet ported.
pub fn f_ctxset(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_ctxset — csrc/eval/funcs.c:912")
}

/// Port of `f_ctxsize()` — `csrc/eval/funcs.c:956`. STUB: not yet ported.
pub fn f_ctxsize(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_ctxsize — csrc/eval/funcs.c:956")
}

/// Port of `set_cursorpos()` — `csrc/eval/funcs.c:965`. STUB: not yet ported.
/// C: `static void set_cursorpos(typval_T *argvars, typval_T *rettv, bool charcol)`
pub fn set_cursorpos() {
    unimplemented!("STUB set_cursorpos — csrc/eval/funcs.c:965")
}

/// Port of `f_cursor()` — `csrc/eval/funcs.c:1035`. STUB: not yet ported.
pub fn f_cursor(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_cursor — csrc/eval/funcs.c:1035")
}

/// Port of `f_debugbreak()` — `csrc/eval/funcs.c:1041`. STUB: not yet ported.
pub fn f_debugbreak(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_debugbreak — csrc/eval/funcs.c:1041")
}

/// Port of `f_eval()` — `csrc/eval/funcs.c:1233`. STUB: not yet ported.
pub fn f_eval(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_eval — csrc/eval/funcs.c:1233")
}

/// Port of `get_list_line()` — `csrc/eval/funcs.c:1264`. STUB: not yet ported.
/// C: `char *get_list_line(int c, void *cookie, int indent, bool do_concat)`
pub fn get_list_line() {
    unimplemented!("STUB get_list_line — csrc/eval/funcs.c:1264")
}

/// Port of `execute_common()` — `csrc/eval/funcs.c:1278`. STUB: not yet ported.
/// C: `void execute_common(typval_T *argvars, typval_T *rettv, int arg_off)`
pub fn execute_common() {
    unimplemented!("STUB execute_common — csrc/eval/funcs.c:1278")
}

/// Port of `f_execute()` — `csrc/eval/funcs.c:1357`. STUB: not yet ported.
pub fn f_execute(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_execute — csrc/eval/funcs.c:1357")
}

/// Port of `f_expand()` — `csrc/eval/funcs.c:1403`. STUB: not yet ported.
pub fn f_expand(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_expand — csrc/eval/funcs.c:1403")
}

/// Port of `f_menu_get()` — `csrc/eval/funcs.c:1478`. STUB: not yet ported.
pub fn f_menu_get(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_menu_get — csrc/eval/funcs.c:1478")
}

/// Port of `f_expandcmd()` — `csrc/eval/funcs.c:1491`. STUB: not yet ported.
pub fn f_expandcmd(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_expandcmd — csrc/eval/funcs.c:1491")
}

/// Port of `f_feedkeys()` — `csrc/eval/funcs.c:1588`. STUB: not yet ported.
pub fn f_feedkeys(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_feedkeys — csrc/eval/funcs.c:1588")
}

/// Port of `common_function()` — `csrc/eval/funcs.c:1654`. STUB: not yet ported.
/// C: `static void common_function(typval_T *argvars, typval_T *rettv, bool is_funcref)`
pub fn common_function() {
    unimplemented!("STUB common_function — csrc/eval/funcs.c:1654")
}

/// Port of `block_def2str()` — `csrc/eval/funcs.c:2164`. STUB: not yet ported.
/// C: `static String block_def2str(struct block_def *bd)`
pub fn block_def2str() {
    unimplemented!("STUB block_def2str — csrc/eval/funcs.c:2164")
}

/// Port of `getregionpos()` — `csrc/eval/funcs.c:2180`. STUB: not yet ported.
/// C: `static int getregionpos(typval_T *argvars, typval_T *rettv, pos_T *p1, pos_T *p2,`
pub fn getregionpos() {
    unimplemented!("STUB getregionpos — csrc/eval/funcs.c:2180")
}

/// Port of `f_getregion()` — `csrc/eval/funcs.c:2316`. STUB: not yet ported.
pub fn f_getregion(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_getregion — csrc/eval/funcs.c:2316")
}

/// Port of `add_regionpos_range()` — `csrc/eval/funcs.c:2355`. STUB: not yet ported.
/// C: `static void add_regionpos_range(typval_T *rettv, pos_T p1, pos_T p2)`
pub fn add_regionpos_range() {
    unimplemented!("STUB add_regionpos_range — csrc/eval/funcs.c:2355")
}

/// Port of `f_getregionpos()` — `csrc/eval/funcs.c:2378`. STUB: not yet ported.
pub fn f_getregionpos(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_getregionpos — csrc/eval/funcs.c:2378")
}

/// Port of `dummy_timer_due_cb()` — `csrc/eval/funcs.c:2568`. STUB: not yet ported.
/// C: `static void dummy_timer_due_cb(TimeWatcher *tw, void *data)`
pub fn dummy_timer_due_cb() {
    unimplemented!("STUB dummy_timer_due_cb — csrc/eval/funcs.c:2568")
}

/// Port of `dummy_timer_close_cb()` — `csrc/eval/funcs.c:2579`. STUB: not yet ported.
/// C: `static void dummy_timer_close_cb(TimeWatcher *tw, void *data)`
pub fn dummy_timer_close_cb() {
    unimplemented!("STUB dummy_timer_close_cb — csrc/eval/funcs.c:2579")
}

/// Port of `f_wait()` — `csrc/eval/funcs.c:2585`. STUB: not yet ported.
pub fn f_wait(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_wait — csrc/eval/funcs.c:2585")
}

/// Port of `has_wsl()` — `csrc/eval/funcs.c:2879`. STUB: not yet ported.
/// C: `static bool has_wsl(void)`
pub fn has_wsl() {
    unimplemented!("STUB has_wsl — csrc/eval/funcs.c:2879")
}

/// Port of `f_hlID()` — `csrc/eval/funcs.c:2894`. STUB: not yet ported.
pub fn f_hlID(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_hlID — csrc/eval/funcs.c:2894")
}

/// Port of `indexof_eval_expr()` — `csrc/eval/funcs.c:2983`. STUB: not yet ported.
/// C: `static varnumber_T indexof_eval_expr(typval_T *expr)`
pub fn indexof_eval_expr() {
    unimplemented!("STUB indexof_eval_expr — csrc/eval/funcs.c:2983")
}

/// Port of `indexof_blob()` — `csrc/eval/funcs.c:3005`. STUB: not yet ported.
/// C: `static varnumber_T indexof_blob(blob_T *b, varnumber_T startidx, typval_T *expr)`
pub fn indexof_blob() {
    unimplemented!("STUB indexof_blob — csrc/eval/funcs.c:3005")
}

/// Port of `indexof_list()` — `csrc/eval/funcs.c:3042`. STUB: not yet ported.
/// C: `static varnumber_T indexof_list(list_T *l, varnumber_T startidx, typval_T *expr)`
pub fn indexof_list() {
    unimplemented!("STUB indexof_list — csrc/eval/funcs.c:3042")
}

/// Port of `f_interrupt()` — `csrc/eval/funcs.c:3211`. STUB: not yet ported.
pub fn f_interrupt(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_interrupt — csrc/eval/funcs.c:3211")
}

/// Port of `f_islocked()` — `csrc/eval/funcs.c:3223`. STUB: not yet ported.
pub fn f_islocked(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_islocked — csrc/eval/funcs.c:3223")
}

/// Port of `f_jobpid()` — `csrc/eval/funcs.c:3291`. STUB: not yet ported.
pub fn f_jobpid(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_jobpid — csrc/eval/funcs.c:3291")
}

/// Port of `f_jobresize()` — `csrc/eval/funcs.c:3315`. STUB: not yet ported.
pub fn f_jobresize(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_jobresize — csrc/eval/funcs.c:3315")
}

/// Port of `create_environment()` — `csrc/eval/funcs.c:3382`. STUB: not yet ported.
/// C: `dict_T *create_environment(const dictitem_T *job_env, const bool clear_env, const bool pty,`
pub fn create_environment() {
    unimplemented!("STUB create_environment — csrc/eval/funcs.c:3382")
}

/// Port of `f_jobstart()` — `csrc/eval/funcs.c:3490`. STUB: not yet ported.
pub fn f_jobstart(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_jobstart — csrc/eval/funcs.c:3490")
}

/// Port of `f_jobstop()` — `csrc/eval/funcs.c:3718`. STUB: not yet ported.
pub fn f_jobstop(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_jobstop — csrc/eval/funcs.c:3718")
}

/// Port of `f_jobwait()` — `csrc/eval/funcs.c:3751`. STUB: not yet ported.
pub fn f_jobwait(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_jobwait — csrc/eval/funcs.c:3751")
}

/// Port of `f_keytrans()` — `csrc/eval/funcs.c:3899`. STUB: not yet ported.
pub fn f_keytrans(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_keytrans — csrc/eval/funcs.c:3899")
}

/// Port of `libcall_common()` — `csrc/eval/funcs.c:3940`. STUB: not yet ported.
/// C: `static void libcall_common(typval_T *argvars, typval_T *rettv, int out_type)`
pub fn libcall_common() {
    unimplemented!("STUB libcall_common — csrc/eval/funcs.c:3940")
}

/// Port of `f_libcall()` — `csrc/eval/funcs.c:3984`. STUB: not yet ported.
pub fn f_libcall(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_libcall — csrc/eval/funcs.c:3984")
}

/// Port of `f_libcallnr()` — `csrc/eval/funcs.c:3990`. STUB: not yet ported.
pub fn f_libcallnr(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_libcallnr — csrc/eval/funcs.c:3990")
}

/// Port of `f_luaeval()` — `csrc/eval/funcs.c:4049`. STUB: not yet ported.
pub fn f_luaeval(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_luaeval — csrc/eval/funcs.c:4049")
}

/// Port of `find_some_match()` — `csrc/eval/funcs.c:4060`. STUB: not yet ported.
/// C: `static void find_some_match(typval_T *const argvars, typval_T *const rettv,`
pub fn find_some_match() {
    unimplemented!("STUB find_some_match — csrc/eval/funcs.c:4060")
}

/// Port of `get_matches_in_str()` — `csrc/eval/funcs.c:4272`. STUB: not yet ported.
/// C: `static void get_matches_in_str(const char *str, regmatch_T *rmp, list_T *mlist, int idx,`
pub fn get_matches_in_str() {
    unimplemented!("STUB get_matches_in_str — csrc/eval/funcs.c:4272")
}

/// Port of `f_matchbufline()` — `csrc/eval/funcs.c:4322`. STUB: not yet ported.
pub fn f_matchbufline(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_matchbufline — csrc/eval/funcs.c:4322")
}

/// Port of `may_add_state_char()` — `csrc/eval/funcs.c:4589`. STUB: not yet ported.
/// C: `static void may_add_state_char(garray_T *gap, const char *include, uint8_t c)`
pub fn may_add_state_char() {
    unimplemented!("STUB may_add_state_char — csrc/eval/funcs.c:4589")
}

/// Port of `f_msgpackdump()` — `csrc/eval/funcs.c:4634`. STUB: not yet ported.
pub fn f_msgpackdump(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_msgpackdump — csrc/eval/funcs.c:4634")
}

/// Port of `emsg_mpack_error()` — `csrc/eval/funcs.c:4666`. STUB: not yet ported.
/// C: `static void emsg_mpack_error(int status)`
pub fn emsg_mpack_error() {
    unimplemented!("STUB emsg_mpack_error — csrc/eval/funcs.c:4666")
}

/// Port of `msgpackparse_unpack_list()` — `csrc/eval/funcs.c:4686`. STUB: not yet ported.
/// C: `static void msgpackparse_unpack_list(const list_T *const list, list_T *const ret_list)`
pub fn msgpackparse_unpack_list() {
    unimplemented!("STUB msgpackparse_unpack_list — csrc/eval/funcs.c:4686")
}

/// Port of `msgpackparse_unpack_blob()` — `csrc/eval/funcs.c:4750`. STUB: not yet ported.
/// C: `static void msgpackparse_unpack_blob(const blob_T *const blob, list_T *const ret_list)`
pub fn msgpackparse_unpack_blob() {
    unimplemented!("STUB msgpackparse_unpack_blob — csrc/eval/funcs.c:4750")
}

/// Port of `f_msgpackparse()` — `csrc/eval/funcs.c:4773`. STUB: not yet ported.
pub fn f_msgpackparse(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_msgpackparse — csrc/eval/funcs.c:4773")
}

/// Port of `f_prompt_getinput()` — `csrc/eval/funcs.c:4914`. STUB: not yet ported.
pub fn f_prompt_getinput(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_prompt_getinput — csrc/eval/funcs.c:4914")
}

/// Port of `f_py3eval()` — `csrc/eval/funcs.c:4949`. STUB: not yet ported.
pub fn f_py3eval(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_py3eval — csrc/eval/funcs.c:4949")
}

/// Port of `f_perleval()` — `csrc/eval/funcs.c:5092`. STUB: not yet ported.
pub fn f_perleval(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_perleval — csrc/eval/funcs.c:5092")
}

/// Port of `f_rubyeval()` — `csrc/eval/funcs.c:5098`. STUB: not yet ported.
pub fn f_rubyeval(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_rubyeval — csrc/eval/funcs.c:5098")
}

/// Port of `return_register()` — `csrc/eval/funcs.c:5193`. STUB: not yet ported.
/// C: `static void return_register(int regname, typval_T *rettv)`
pub fn return_register() {
    unimplemented!("STUB return_register — csrc/eval/funcs.c:5193")
}

/// Port of `repeat_list()` — `csrc/eval/funcs.c:5310`. STUB: not yet ported.
/// C: `static void repeat_list(list_T *l, varnumber_T n, typval_T *rettv)`
pub fn repeat_list() {
    unimplemented!("STUB repeat_list — csrc/eval/funcs.c:5310")
}

/// Port of `repeat_blob()` — `csrc/eval/funcs.c:5319`. STUB: not yet ported.
/// C: `static void repeat_blob(typval_T *blob_tv, varnumber_T n, typval_T *rettv)`
pub fn repeat_blob() {
    unimplemented!("STUB repeat_blob — csrc/eval/funcs.c:5319")
}

/// Port of `repeat_string()` — `csrc/eval/funcs.c:5356`. STUB: not yet ported.
/// C: `static void repeat_string(typval_T *str_tv, varnumber_T n, typval_T *rettv)`
pub fn repeat_string() {
    unimplemented!("STUB repeat_string — csrc/eval/funcs.c:5356")
}

/// Port of `get_search_arg()` — `csrc/eval/funcs.c:5593`. STUB: not yet ported.
/// C: `static int get_search_arg(typval_T *varp, int *flagsp)`
pub fn get_search_arg() {
    unimplemented!("STUB get_search_arg — csrc/eval/funcs.c:5593")
}

/// Port of `search_cmn()` — `csrc/eval/funcs.c:5653`. STUB: not yet ported.
/// C: `static int search_cmn(typval_T *argvars, pos_T *match_pos, int *flagsp)`
pub fn search_cmn() {
    unimplemented!("STUB search_cmn — csrc/eval/funcs.c:5653")
}

/// Port of `f_rpcnotify()` — `csrc/eval/funcs.c:5790`. STUB: not yet ported.
pub fn f_rpcnotify(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_rpcnotify — csrc/eval/funcs.c:5790")
}

/// Port of `f_rpcrequest()` — `csrc/eval/funcs.c:5829`. STUB: not yet ported.
pub fn f_rpcrequest(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_rpcrequest — csrc/eval/funcs.c:5829")
}

/// Port of `screenchar_adjust()` — `csrc/eval/funcs.c:5925`. STUB: not yet ported.
/// C: `static void screenchar_adjust(ScreenGrid **grid, int *row, int *col)`
pub fn screenchar_adjust() {
    unimplemented!("STUB screenchar_adjust — csrc/eval/funcs.c:5925")
}

/// Port of `searchpair_cmn()` — `csrc/eval/funcs.c:6064`. STUB: not yet ported.
/// C: `static int searchpair_cmn(typval_T *argvars, pos_T *match_pos)`
pub fn searchpair_cmn() {
    unimplemented!("STUB searchpair_cmn — csrc/eval/funcs.c:6064")
}

/// Port of `do_searchpair()` — `csrc/eval/funcs.c:6173`. STUB: not yet ported.
/// C: `int do_searchpair(const char *spat, const char *mpat, const char *epat, int dir,`
pub fn do_searchpair() {
    unimplemented!("STUB do_searchpair — csrc/eval/funcs.c:6173")
}

/// Port of `f_serverstart()` — `csrc/eval/funcs.c:6376`. STUB: not yet ported.
pub fn f_serverstart(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_serverstart — csrc/eval/funcs.c:6376")
}

/// Port of `f_serverstop()` — `csrc/eval/funcs.c:6420`. STUB: not yet ported.
pub fn f_serverstop(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_serverstop — csrc/eval/funcs.c:6420")
}

/// Port of `set_position()` — `csrc/eval/funcs.c:6442`. STUB: not yet ported.
/// C: `static void set_position(typval_T *argvars, typval_T *rettv, bool charpos)`
pub fn set_position() {
    unimplemented!("STUB set_position — csrc/eval/funcs.c:6442")
}

/// Port of `f_setcharpos()` — `csrc/eval/funcs.c:6481`. STUB: not yet ported.
pub fn f_setcharpos(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setcharpos — csrc/eval/funcs.c:6481")
}

/// Port of `f_setcharsearch()` — `csrc/eval/funcs.c:6486`. STUB: not yet ported.
pub fn f_setcharsearch(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setcharsearch — csrc/eval/funcs.c:6486")
}

/// Port of `f_setcursorcharpos()` — `csrc/eval/funcs.c:6515`. STUB: not yet ported.
pub fn f_setcursorcharpos(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setcursorcharpos — csrc/eval/funcs.c:6515")
}

/// Port of `f_setpos()` — `csrc/eval/funcs.c:6574`. STUB: not yet ported.
pub fn f_setpos(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setpos — csrc/eval/funcs.c:6574")
}

/// Port of `f_settagstack()` — `csrc/eval/funcs.c:6751`. STUB: not yet ported.
pub fn f_settagstack(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_settagstack — csrc/eval/funcs.c:6751")
}

/// Port of `f_sockconnect()` — `csrc/eval/funcs.c:6844`. STUB: not yet ported.
pub fn f_sockconnect(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_sockconnect — csrc/eval/funcs.c:6844")
}

/// Port of `f_stdioopen()` — `csrc/eval/funcs.c:6896`. STUB: not yet ported.
pub fn f_stdioopen(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_stdioopen — csrc/eval/funcs.c:6896")
}

/// Port of `f_spellbadword()` — `csrc/eval/funcs.c:6951`. STUB: not yet ported.
pub fn f_spellbadword(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_spellbadword — csrc/eval/funcs.c:6951")
}

/// Port of `f_spellsuggest()` — `csrc/eval/funcs.c:7014`. STUB: not yet ported.
pub fn f_spellsuggest(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_spellsuggest — csrc/eval/funcs.c:7014")
}

/// Port of `get_xdg_var_list()` — `csrc/eval/funcs.c:7140`. STUB: not yet ported.
/// C: `static void get_xdg_var_list(const XDGVarType xdg, typval_T *rettv)`
pub fn get_xdg_var_list() {
    unimplemented!("STUB get_xdg_var_list — csrc/eval/funcs.c:7140")
}

/// Port of `f_stdpath()` — `csrc/eval/funcs.c:7167`. STUB: not yet ported.
pub fn f_stdpath(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_stdpath — csrc/eval/funcs.c:7167")
}

/// Port of `f_submatch()` — `csrc/eval/funcs.c:7297`. STUB: not yet ported.
pub fn f_submatch(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_submatch — csrc/eval/funcs.c:7297")
}

/// Port of `f_swapfilelist()` — `csrc/eval/funcs.c:7357`. STUB: not yet ported.
pub fn f_swapfilelist(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_swapfilelist — csrc/eval/funcs.c:7357")
}

/// Port of `f_swapinfo()` — `csrc/eval/funcs.c:7364`. STUB: not yet ported.
pub fn f_swapinfo(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_swapinfo — csrc/eval/funcs.c:7364")
}

/// Port of `f_swapname()` — `csrc/eval/funcs.c:7371`. STUB: not yet ported.
pub fn f_swapname(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_swapname — csrc/eval/funcs.c:7371")
}

/// Port of `f_synID()` — `csrc/eval/funcs.c:7385`. STUB: not yet ported.
pub fn f_synID(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_synID — csrc/eval/funcs.c:7385")
}

/// Port of `f_synIDattr()` — `csrc/eval/funcs.c:7404`. STUB: not yet ported.
pub fn f_synIDattr(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_synIDattr — csrc/eval/funcs.c:7404")
}

/// Port of `f_synIDtrans()` — `csrc/eval/funcs.c:7502`. STUB: not yet ported.
pub fn f_synIDtrans(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_synIDtrans — csrc/eval/funcs.c:7502")
}

/// Port of `f_synconcealed()` — `csrc/eval/funcs.c:7516`. STUB: not yet ported.
pub fn f_synconcealed(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_synconcealed — csrc/eval/funcs.c:7516")
}

/// Port of `f_synstack()` — `csrc/eval/funcs.c:7556`. STUB: not yet ported.
pub fn f_synstack(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_synstack — csrc/eval/funcs.c:7556")
}

/// Port of `f_timer_info()` — `csrc/eval/funcs.c:7635`. STUB: not yet ported.
pub fn f_timer_info(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_timer_info — csrc/eval/funcs.c:7635")
}

/// Port of `f_timer_pause()` — `csrc/eval/funcs.c:7654`. STUB: not yet ported.
pub fn f_timer_pause(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_timer_pause — csrc/eval/funcs.c:7654")
}

/// Port of `f_timer_start()` — `csrc/eval/funcs.c:7675`. STUB: not yet ported.
pub fn f_timer_start(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_timer_start — csrc/eval/funcs.c:7675")
}

/// Port of `f_timer_stop()` — `csrc/eval/funcs.c:7706`. STUB: not yet ported.
pub fn f_timer_stop(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_timer_stop — csrc/eval/funcs.c:7706")
}

/// Port of `f_timer_stopall()` — `csrc/eval/funcs.c:7720`. STUB: not yet ported.
pub fn f_timer_stopall(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_timer_stopall — csrc/eval/funcs.c:7720")
}
