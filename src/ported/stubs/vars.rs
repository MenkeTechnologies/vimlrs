//! STUB surface for `csrc/eval/vars.c` — generated, do not edit.
#![allow(dead_code, unused_variables, non_snake_case, clippy::all)]

/// Port of `evalvars_clear()` — `csrc/eval/vars.c:361`. STUB: not yet ported.
/// C: `void evalvars_clear(void)`
pub fn evalvars_clear() {
    unimplemented!("STUB evalvars_clear — csrc/eval/vars.c:361")
}

/// Port of `garbage_collect_globvars()` — `csrc/eval/vars.c:392`. STUB: not yet ported.
/// C: `int garbage_collect_globvars(int copyID)`
pub fn garbage_collect_globvars() {
    unimplemented!("STUB garbage_collect_globvars — csrc/eval/vars.c:392")
}

/// Port of `garbage_collect_vimvars()` — `csrc/eval/vars.c:397`. STUB: not yet ported.
/// C: `bool garbage_collect_vimvars(int copyID)`
pub fn garbage_collect_vimvars() {
    unimplemented!("STUB garbage_collect_vimvars — csrc/eval/vars.c:397")
}

/// Port of `garbage_collect_scriptvars()` — `csrc/eval/vars.c:402`. STUB: not yet ported.
/// C: `bool garbage_collect_scriptvars(int copyID)`
pub fn garbage_collect_scriptvars() {
    unimplemented!("STUB garbage_collect_scriptvars — csrc/eval/vars.c:402")
}

/// Port of `set_internal_string_var()` — `csrc/eval/vars.c:415`. STUB: not yet ported.
/// C: `void set_internal_string_var(const char *name, char *value)  // NOLINT(readability-non-const-parameter)`
pub fn set_internal_string_var() {
    unimplemented!("STUB set_internal_string_var — csrc/eval/vars.c:415")
}

/// Port of `eval_charconvert()` — `csrc/eval/vars.c:426`. STUB: not yet ported.
/// C: `int eval_charconvert(const char *const enc_from, const char *const enc_to,`
pub fn eval_charconvert() {
    unimplemented!("STUB eval_charconvert — csrc/eval/vars.c:426")
}

/// Port of `eval_diff()` — `csrc/eval/vars.c:457`. STUB: not yet ported.
/// C: `void eval_diff(const char *const origfile, const char *const newfile, const char *const outfile)`
pub fn eval_diff() {
    unimplemented!("STUB eval_diff — csrc/eval/vars.c:457")
}

/// Port of `eval_patch()` — `csrc/eval/vars.c:479`. STUB: not yet ported.
/// C: `void eval_patch(const char *const origfile, const char *const difffile, const char *const outfile)`
pub fn eval_patch() {
    unimplemented!("STUB eval_patch — csrc/eval/vars.c:479")
}

/// Port of `eval_spell_expr()` — `csrc/eval/vars.c:505`. STUB: not yet ported.
/// C: `list_T *eval_spell_expr(char *badword, char *expr)`
pub fn eval_spell_expr() {
    unimplemented!("STUB eval_spell_expr — csrc/eval/vars.c:505")
}

/// Port of `get_spellword()` — `csrc/eval/vars.c:559`. STUB: not yet ported.
/// C: `int get_spellword(list_T *const list, const char **ret_word)`
pub fn get_spellword() {
    unimplemented!("STUB get_spellword — csrc/eval/vars.c:559")
}

/// Port of `prepare_vimvar()` — `csrc/eval/vars.c:576`. STUB: not yet ported.
/// C: `void prepare_vimvar(int idx, typval_T *save_tv)`
pub fn prepare_vimvar() {
    unimplemented!("STUB prepare_vimvar — csrc/eval/vars.c:576")
}

/// Port of `restore_vimvar()` — `csrc/eval/vars.c:588`. STUB: not yet ported.
/// C: `void restore_vimvar(int idx, typval_T *save_tv)`
pub fn restore_vimvar() {
    unimplemented!("STUB restore_vimvar — csrc/eval/vars.c:588")
}

/// Port of `list_vim_vars()` — `csrc/eval/vars.c:604`. STUB: not yet ported.
/// C: `static void list_vim_vars(int *first)`
pub fn list_vim_vars() {
    unimplemented!("STUB list_vim_vars — csrc/eval/vars.c:604")
}

/// Port of `list_script_vars()` — `csrc/eval/vars.c:610`. STUB: not yet ported.
/// C: `static void list_script_vars(int *first)`
pub fn list_script_vars() {
    unimplemented!("STUB list_script_vars — csrc/eval/vars.c:610")
}

/// Port of `eval_one_expr_in_str()` — `csrc/eval/vars.c:621`. STUB: not yet ported.
/// C: `char *eval_one_expr_in_str(char *p, garray_T *gap, bool evaluate)`
pub fn eval_one_expr_in_str() {
    unimplemented!("STUB eval_one_expr_in_str — csrc/eval/vars.c:621")
}

/// Port of `eval_all_expr_in_str()` — `csrc/eval/vars.c:656`. STUB: not yet ported.
/// C: `static char *eval_all_expr_in_str(char *str)`
pub fn eval_all_expr_in_str() {
    unimplemented!("STUB eval_all_expr_in_str — csrc/eval/vars.c:656")
}

/// Port of `heredoc_get()` — `csrc/eval/vars.c:724`. STUB: not yet ported.
/// C: `list_T *heredoc_get(exarg_T *eap, char *cmd, bool script_get)`
pub fn heredoc_get() {
    unimplemented!("STUB heredoc_get — csrc/eval/vars.c:724")
}

/// Port of `ex_let()` — `csrc/eval/vars.c:916`. STUB: not yet ported.
/// C: `void ex_let(exarg_T *eap)`
pub fn ex_let() {
    unimplemented!("STUB ex_let — csrc/eval/vars.c:916")
}

/// Port of `ex_let_vars()` — `csrc/eval/vars.c:1021`. STUB: not yet ported.
/// C: `int ex_let_vars(char *arg_start, typval_T *tv, int copy, int semicolon, int var_count, int is_const,`
pub fn ex_let_vars() {
    unimplemented!("STUB ex_let_vars — csrc/eval/vars.c:1021")
}

/// Port of `skip_var_list()` — `csrc/eval/vars.c:1103`. STUB: not yet ported.
/// C: `const char *skip_var_list(const char *arg, int *var_count, int *semicolon, bool silent)`
pub fn skip_var_list() {
    unimplemented!("STUB skip_var_list — csrc/eval/vars.c:1103")
}

/// Port of `skip_var_one()` — `csrc/eval/vars.c:1145`. STUB: not yet ported.
/// C: `static const char *skip_var_one(const char *arg)`
pub fn skip_var_one() {
    unimplemented!("STUB skip_var_one — csrc/eval/vars.c:1145")
}

/// Port of `list_hashtable_vars()` — `csrc/eval/vars.c:1157`. STUB: not yet ported.
/// C: `void list_hashtable_vars(hashtab_T *ht, const char *prefix, int empty, int *first)`
pub fn list_hashtable_vars() {
    unimplemented!("STUB list_hashtable_vars — csrc/eval/vars.c:1157")
}

/// Port of `list_glob_vars()` — `csrc/eval/vars.c:1186`. STUB: not yet ported.
/// C: `static void list_glob_vars(int *first)`
pub fn list_glob_vars() {
    unimplemented!("STUB list_glob_vars — csrc/eval/vars.c:1186")
}

/// Port of `list_buf_vars()` — `csrc/eval/vars.c:1192`. STUB: not yet ported.
/// C: `static void list_buf_vars(int *first)`
pub fn list_buf_vars() {
    unimplemented!("STUB list_buf_vars — csrc/eval/vars.c:1192")
}

/// Port of `list_win_vars()` — `csrc/eval/vars.c:1198`. STUB: not yet ported.
/// C: `static void list_win_vars(int *first)`
pub fn list_win_vars() {
    unimplemented!("STUB list_win_vars — csrc/eval/vars.c:1198")
}

/// Port of `list_tab_vars()` — `csrc/eval/vars.c:1204`. STUB: not yet ported.
/// C: `static void list_tab_vars(int *first)`
pub fn list_tab_vars() {
    unimplemented!("STUB list_tab_vars — csrc/eval/vars.c:1204")
}

/// Port of `list_arg_vars()` — `csrc/eval/vars.c:1210`. STUB: not yet ported.
/// C: `static const char *list_arg_vars(exarg_T *eap, const char *arg, int *first)`
pub fn list_arg_vars() {
    unimplemented!("STUB list_arg_vars — csrc/eval/vars.c:1210")
}

/// Port of `ex_let_env()` — `csrc/eval/vars.c:1299`. STUB: not yet ported.
/// C: `static char *ex_let_env(char *arg, typval_T *const tv, const bool is_const,`
pub fn ex_let_env() {
    unimplemented!("STUB ex_let_env — csrc/eval/vars.c:1299")
}

/// Port of `ex_let_option()` — `csrc/eval/vars.c:1346`. STUB: not yet ported.
/// C: `static char *ex_let_option(char *arg, typval_T *const tv, const bool is_const,`
pub fn ex_let_option() {
    unimplemented!("STUB ex_let_option — csrc/eval/vars.c:1346")
}

/// Port of `ex_let_register()` — `csrc/eval/vars.c:1446`. STUB: not yet ported.
/// C: `static char *ex_let_register(char *arg, typval_T *const tv, const bool is_const,`
pub fn ex_let_register() {
    unimplemented!("STUB ex_let_register — csrc/eval/vars.c:1446")
}

/// Port of `ex_let_one()` — `csrc/eval/vars.c:1493`. STUB: not yet ported.
/// C: `static char *ex_let_one(char *arg, typval_T *const tv, const bool copy, const bool is_const,`
pub fn ex_let_one() {
    unimplemented!("STUB ex_let_one — csrc/eval/vars.c:1493")
}

/// Port of `ex_unlet()` — `csrc/eval/vars.c:1532`. STUB: not yet ported.
/// C: `void ex_unlet(exarg_T *eap)`
pub fn ex_unlet() {
    unimplemented!("STUB ex_unlet — csrc/eval/vars.c:1532")
}

/// Port of `ex_lockvar()` — `csrc/eval/vars.c:1538`. STUB: not yet ported.
/// C: `void ex_lockvar(exarg_T *eap)`
pub fn ex_lockvar() {
    unimplemented!("STUB ex_lockvar — csrc/eval/vars.c:1538")
}

/// Port of `ex_unletlock()` — `csrc/eval/vars.c:1562`. STUB: not yet ported.
/// C: `static void ex_unletlock(exarg_T *eap, char *argstart, int deep, int glv_flags,`
pub fn ex_unletlock() {
    unimplemented!("STUB ex_unletlock — csrc/eval/vars.c:1562")
}

/// Port of `do_unlet_var()` — `csrc/eval/vars.c:1626`. STUB: not yet ported.
/// C: `static int do_unlet_var(lval_T *lp, char *name_end, exarg_T *eap, int deep FUNC_ATTR_UNUSED)`
pub fn do_unlet_var() {
    unimplemented!("STUB do_unlet_var — csrc/eval/vars.c:1626")
}

/// Port of `tv_list_unlet_range()` — `csrc/eval/vars.c:1688`. STUB: not yet ported.
/// C: `static void tv_list_unlet_range(list_T *const l, listitem_T *const li_first, const int n1_arg,`
pub fn tv_list_unlet_range() {
    unimplemented!("STUB tv_list_unlet_range — csrc/eval/vars.c:1688")
}

/// Port of `do_unlet()` — `csrc/eval/vars.c:1713`. STUB: not yet ported.
/// C: `int do_unlet(const char *const name, const size_t name_len, const bool forceit)`
pub fn do_unlet() {
    unimplemented!("STUB do_unlet — csrc/eval/vars.c:1713")
}

/// Port of `do_lock_var()` — `csrc/eval/vars.c:1786`. STUB: not yet ported.
/// C: `static int do_lock_var(lval_T *lp, char *name_end FUNC_ATTR_UNUSED, exarg_T *eap, int deep)`
pub fn do_lock_var() {
    unimplemented!("STUB do_lock_var — csrc/eval/vars.c:1786")
}

/// Port of `del_menutrans_vars()` — `csrc/eval/vars.c:1843`. STUB: not yet ported.
/// C: `void del_menutrans_vars(void)`
pub fn del_menutrans_vars() {
    unimplemented!("STUB del_menutrans_vars — csrc/eval/vars.c:1843")
}

/// Port of `get_globvar_dict()` — `csrc/eval/vars.c:1855`. STUB: not yet ported.
/// C: `dict_T *get_globvar_dict(void)`
pub fn get_globvar_dict() {
    unimplemented!("STUB get_globvar_dict — csrc/eval/vars.c:1855")
}

/// Port of `get_globvar_ht()` — `csrc/eval/vars.c:1862`. STUB: not yet ported.
/// C: `hashtab_T *get_globvar_ht(void)`
pub fn get_globvar_ht() {
    unimplemented!("STUB get_globvar_ht — csrc/eval/vars.c:1862")
}

/// Port of `get_vimvar_dict()` — `csrc/eval/vars.c:1868`. STUB: not yet ported.
/// C: `dict_T *get_vimvar_dict(void)`
pub fn get_vimvar_dict() {
    unimplemented!("STUB get_vimvar_dict — csrc/eval/vars.c:1868")
}

/// Port of `cat_prefix_varname()` — `csrc/eval/vars.c:1945`. STUB: not yet ported.
/// C: `char *cat_prefix_varname(int prefix, const char *name)`
pub fn cat_prefix_varname() {
    unimplemented!("STUB cat_prefix_varname — csrc/eval/vars.c:1945")
}

/// Port of `get_user_var_name()` — `csrc/eval/vars.c:1964`. STUB: not yet ported.
/// C: `char *get_user_var_name(expand_T *xp, int idx)`
pub fn get_user_var_name() {
    unimplemented!("STUB get_user_var_name — csrc/eval/vars.c:1964")
}

/// Port of `set_reg_var()` — `csrc/eval/vars.c:2168`. STUB: not yet ported.
/// C: `void set_reg_var(int c)`
pub fn set_reg_var() {
    unimplemented!("STUB set_reg_var — csrc/eval/vars.c:2168")
}

/// Port of `v_exception()` — `csrc/eval/vars.c:2189`. STUB: not yet ported.
/// C: `char *v_exception(char *oldval)`
pub fn v_exception() {
    unimplemented!("STUB v_exception — csrc/eval/vars.c:2189")
}

/// Port of `set_cmdarg()` — `csrc/eval/vars.c:2204`. STUB: not yet ported.
/// C: `char *set_cmdarg(exarg_T *eap, char *oldarg)`
pub fn set_cmdarg() {
    unimplemented!("STUB set_cmdarg — csrc/eval/vars.c:2204")
}

/// Port of `v_throwpoint()` — `csrc/eval/vars.c:2322`. STUB: not yet ported.
/// C: `char *v_throwpoint(char *oldval)`
pub fn v_throwpoint() {
    unimplemented!("STUB v_throwpoint — csrc/eval/vars.c:2322")
}

/// Port of `set_vcount()` — `csrc/eval/vars.c:2336`. STUB: not yet ported.
/// C: `void set_vcount(int64_t count, int64_t count1, bool set_prevcount)`
pub fn set_vcount() {
    unimplemented!("STUB set_vcount — csrc/eval/vars.c:2336")
}

/// Port of `check_vars()` — `csrc/eval/vars.c:2382`. STUB: not yet ported.
/// C: `void check_vars(const char *name, size_t len)`
pub fn check_vars() {
    unimplemented!("STUB check_vars — csrc/eval/vars.c:2382")
}

/// Port of `find_var()` — `csrc/eval/vars.c:2404`. STUB: not yet ported.
/// C: `dictitem_T *find_var(const char *const name, const size_t name_len, hashtab_T **htp,`
pub fn find_var() {
    unimplemented!("STUB find_var — csrc/eval/vars.c:2404")
}

/// Port of `find_var_in_ht()` — `csrc/eval/vars.c:2439`. STUB: not yet ported.
/// C: `dictitem_T *find_var_in_ht(hashtab_T *const ht, int htname, const char *const varname,`
pub fn find_var_in_ht() {
    unimplemented!("STUB find_var_in_ht — csrc/eval/vars.c:2439")
}

/// Port of `find_var_ht_dict()` — `csrc/eval/vars.c:2498`. STUB: not yet ported.
/// C: `static hashtab_T *find_var_ht_dict(const char *name, const size_t name_len, const char **varname,`
pub fn find_var_ht_dict() {
    unimplemented!("STUB find_var_ht_dict — csrc/eval/vars.c:2498")
}

/// Port of `find_var_ht()` — `csrc/eval/vars.c:2577`. STUB: not yet ported.
/// C: `hashtab_T *find_var_ht(const char *name, const size_t name_len, const char **varname)`
pub fn find_var_ht() {
    unimplemented!("STUB find_var_ht — csrc/eval/vars.c:2577")
}

/// Port of `get_var_value()` — `csrc/eval/vars.c:2587`. STUB: not yet ported.
/// C: `char *get_var_value(const char *const name)`
pub fn get_var_value() {
    unimplemented!("STUB get_var_value — csrc/eval/vars.c:2587")
}

/// Port of `new_script_vars()` — `csrc/eval/vars.c:2600`. STUB: not yet ported.
/// C: `void new_script_vars(scid_T id)`
pub fn new_script_vars() {
    unimplemented!("STUB new_script_vars — csrc/eval/vars.c:2600")
}

/// Port of `init_var_dict()` — `csrc/eval/vars.c:2609`. STUB: not yet ported.
/// C: `void init_var_dict(dict_T *dict, ScopeDictDictItem *dict_var, ScopeType scope)`
pub fn init_var_dict() {
    unimplemented!("STUB init_var_dict — csrc/eval/vars.c:2609")
}

/// Port of `unref_var_dict()` — `csrc/eval/vars.c:2625`. STUB: not yet ported.
/// C: `void unref_var_dict(dict_T *dict)`
pub fn unref_var_dict() {
    unimplemented!("STUB unref_var_dict — csrc/eval/vars.c:2625")
}

/// Port of `vars_clear()` — `csrc/eval/vars.c:2636`. STUB: not yet ported.
/// C: `void vars_clear(hashtab_T *ht)`
pub fn vars_clear() {
    unimplemented!("STUB vars_clear — csrc/eval/vars.c:2636")
}

/// Port of `vars_clear_ext()` — `csrc/eval/vars.c:2642`. STUB: not yet ported.
/// C: `void vars_clear_ext(hashtab_T *ht, bool free_val)`
pub fn vars_clear_ext() {
    unimplemented!("STUB vars_clear_ext — csrc/eval/vars.c:2642")
}

/// Port of `delete_var()` — `csrc/eval/vars.c:2672`. STUB: not yet ported.
/// C: `static void delete_var(hashtab_T *ht, hashitem_T *hi)`
pub fn delete_var() {
    unimplemented!("STUB delete_var — csrc/eval/vars.c:2672")
}

/// Port of `list_one_var()` — `csrc/eval/vars.c:2682`. STUB: not yet ported.
/// C: `static void list_one_var(dictitem_T *v, const char *prefix, int *first)`
pub fn list_one_var() {
    unimplemented!("STUB list_one_var — csrc/eval/vars.c:2682")
}

/// Port of `list_one_var_a()` — `csrc/eval/vars.c:2693`. STUB: not yet ported.
/// C: `static void list_one_var_a(const char *prefix, const char *name, const ptrdiff_t name_len,`
pub fn list_one_var_a() {
    unimplemented!("STUB list_one_var_a — csrc/eval/vars.c:2693")
}

/// Port of `before_set_vvar()` — `csrc/eval/vars.c:2744`. STUB: not yet ported.
/// C: `bool before_set_vvar(const char *const varname, dictitem_T *const di, typval_T *const tv,`
pub fn before_set_vvar() {
    unimplemented!("STUB before_set_vvar — csrc/eval/vars.c:2744")
}

/// Port of `set_var_const()` — `csrc/eval/vars.c:2821`. STUB: not yet ported.
/// C: `void set_var_const(const char *name, const size_t name_len, typval_T *const tv, const bool copy,`
pub fn set_var_const() {
    unimplemented!("STUB set_var_const — csrc/eval/vars.c:2821")
}

/// Port of `var_check_ro()` — `csrc/eval/vars.c:2947`. STUB: not yet ported.
/// C: `bool var_check_ro(const int flags, const char *name, size_t name_len)`
pub fn var_check_ro() {
    unimplemented!("STUB var_check_ro — csrc/eval/vars.c:2947")
}

/// Port of `var_check_lock()` — `csrc/eval/vars.c:2974`. STUB: not yet ported.
/// C: `bool var_check_lock(const int flags, const char *name, size_t name_len)`
pub fn var_check_lock() {
    unimplemented!("STUB var_check_lock — csrc/eval/vars.c:2974")
}

/// Port of `var_check_fixed()` — `csrc/eval/vars.c:3010`. STUB: not yet ported.
/// C: `bool var_check_fixed(const int flags, const char *name, size_t name_len)`
pub fn var_check_fixed() {
    unimplemented!("STUB var_check_fixed — csrc/eval/vars.c:3010")
}

/// Port of `var_wrong_func_name()` — `csrc/eval/vars.c:3033`. STUB: not yet ported.
/// C: `bool var_wrong_func_name(const char *const name, const bool new_var)`
pub fn var_wrong_func_name() {
    unimplemented!("STUB var_wrong_func_name — csrc/eval/vars.c:3033")
}

/// Port of `valid_varname()` — `csrc/eval/vars.c:3060`. STUB: not yet ported.
/// C: `bool valid_varname(const char *varname)`
pub fn valid_varname() {
    unimplemented!("STUB valid_varname — csrc/eval/vars.c:3060")
}

/// Port of `get_var_from()` — `csrc/eval/vars.c:3081`. STUB: not yet ported.
/// C: `static void get_var_from(const char *varname, typval_T *rettv, typval_T *deftv, int htname,`
pub fn get_var_from() {
    unimplemented!("STUB get_var_from — csrc/eval/vars.c:3081")
}

/// Port of `getwinvar()` — `csrc/eval/vars.c:3172`. STUB: not yet ported.
/// C: `static void getwinvar(typval_T *argvars, typval_T *rettv, int off)`
pub fn getwinvar() {
    unimplemented!("STUB getwinvar — csrc/eval/vars.c:3172")
}

/// Port of `tv_to_optval()` — `csrc/eval/vars.c:3196`. STUB: not yet ported.
/// C: `static OptVal tv_to_optval(typval_T *tv, OptIndex opt_idx, const char *option, bool *error)`
pub fn tv_to_optval() {
    unimplemented!("STUB tv_to_optval — csrc/eval/vars.c:3196")
}

/// Port of `optval_as_tv()` — `csrc/eval/vars.c:3256`. STUB: not yet ported.
/// C: `typval_T optval_as_tv(OptVal value, bool numbool)`
pub fn optval_as_tv() {
    unimplemented!("STUB optval_as_tv — csrc/eval/vars.c:3256")
}

/// Port of `set_option_from_tv()` — `csrc/eval/vars.c:3286`. STUB: not yet ported.
/// C: `static void set_option_from_tv(const char *varname, typval_T *varp)`
pub fn set_option_from_tv() {
    unimplemented!("STUB set_option_from_tv — csrc/eval/vars.c:3286")
}

/// Port of `setwinvar()` — `csrc/eval/vars.c:3308`. STUB: not yet ported.
/// C: `static void setwinvar(typval_T *argvars, int off)`
pub fn setwinvar() {
    unimplemented!("STUB setwinvar — csrc/eval/vars.c:3308")
}

/// Port of `reset_v_option_vars()` — `csrc/eval/vars.c:3349`. STUB: not yet ported.
/// C: `void reset_v_option_vars(void)`
pub fn reset_v_option_vars() {
    unimplemented!("STUB reset_v_option_vars — csrc/eval/vars.c:3349")
}

/// Port of `var_exists()` — `csrc/eval/vars.c:3371`. STUB: not yet ported.
/// C: `bool var_exists(const char *var)`
pub fn var_exists() {
    unimplemented!("STUB var_exists — csrc/eval/vars.c:3371")
}

/// Port of `var_redir_start()` — `csrc/eval/vars.c:3413`. STUB: not yet ported.
/// C: `int var_redir_start(char *name, bool append)`
pub fn var_redir_start() {
    unimplemented!("STUB var_redir_start — csrc/eval/vars.c:3413")
}

/// Port of `var_redir_str()` — `csrc/eval/vars.c:3475`. STUB: not yet ported.
/// C: `void var_redir_str(const char *value, int value_len)`
pub fn var_redir_str() {
    unimplemented!("STUB var_redir_str — csrc/eval/vars.c:3475")
}

/// Port of `var_redir_stop()` — `csrc/eval/vars.c:3495`. STUB: not yet ported.
/// C: `void var_redir_stop(void)`
pub fn var_redir_stop() {
    unimplemented!("STUB var_redir_stop — csrc/eval/vars.c:3495")
}

/// Port of `f_gettabvar()` — `csrc/eval/vars.c:3523`. STUB: not yet ported.
pub fn f_gettabvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_gettabvar — csrc/eval/vars.c:3523")
}

/// Port of `f_gettabwinvar()` — `csrc/eval/vars.c:3537`. STUB: not yet ported.
pub fn f_gettabwinvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_gettabwinvar — csrc/eval/vars.c:3537")
}

/// Port of `f_getwinvar()` — `csrc/eval/vars.c:3543`. STUB: not yet ported.
pub fn f_getwinvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_getwinvar — csrc/eval/vars.c:3543")
}

/// Port of `f_getbufvar()` — `csrc/eval/vars.c:3549`. STUB: not yet ported.
pub fn f_getbufvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_getbufvar — csrc/eval/vars.c:3549")
}

/// Port of `f_settabvar()` — `csrc/eval/vars.c:3558`. STUB: not yet ported.
pub fn f_settabvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_settabvar — csrc/eval/vars.c:3558")
}

/// Port of `f_settabwinvar()` — `csrc/eval/vars.c:3593`. STUB: not yet ported.
pub fn f_settabwinvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_settabwinvar — csrc/eval/vars.c:3593")
}

/// Port of `f_setwinvar()` — `csrc/eval/vars.c:3599`. STUB: not yet ported.
pub fn f_setwinvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setwinvar — csrc/eval/vars.c:3599")
}

/// Port of `f_setbufvar()` — `csrc/eval/vars.c:3605`. STUB: not yet ported.
pub fn f_setbufvar(
    _argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    _rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    unimplemented!("STUB f_setbufvar — csrc/eval/vars.c:3605")
}
