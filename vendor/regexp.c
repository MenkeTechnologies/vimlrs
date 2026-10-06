// Vendored SUBSET of Neovim v0.12.4's src/nvim/regexp.c — the Funcref {sub}
// path of substitute(): vim_regsub_both() calls the function once per match and
// fill_submatch_list() hands it the submatches. This is the spec for the
// Funcref branch of `f_substitute()` in `src/ported/eval/funcs.rs`. Copied
// verbatim (regexp.c:1974-2010, then the `expr != NULL` branch of
// vim_regsub_both() at regexp.c:2168-2208).

/// Put the submatches in "argv[argskip]" which is a list passed into
/// call_func() by vim_regsub_both().
static int fill_submatch_list(int argc FUNC_ATTR_UNUSED, typval_T *argv, int argskip, ufunc_T *fp)
  FUNC_ATTR_NONNULL_ALL
{
  typval_T *listarg = argv + argskip;

  if (!fp->uf_varargs && fp->uf_args.ga_len <= argskip) {
    // called function doesn't take a submatches argument
    return argskip;
  }

  // Relies on sl_list to be the first item in staticList10_T.
  tv_list_init_static10((staticList10_T *)listarg->vval.v_list);

  // There are always 10 list items in staticList10_T.
  listitem_T *li = tv_list_first(listarg->vval.v_list);
  for (int i = 0; i < 10; i++) {
    char *s = rsm.sm_match->startp[i];
    if (s == NULL || rsm.sm_match->endp[i] == NULL) {
      s = NULL;
    } else {
      s = xstrnsave(s, (size_t)(rsm.sm_match->endp[i] - s));
    }
    TV_LIST_ITEM_TV(li)->v_type = VAR_STRING;
    TV_LIST_ITEM_TV(li)->vval.v_string = s;
    li = TV_LIST_ITEM_NEXT(argv->vval.v_list, li);
  }
  return argskip + 1;
}

static void clear_submatch_list(staticList10_T *sl)
{
  TV_LIST_ITER(&sl->sl_list, li, {
    xfree(TV_LIST_ITEM_TV(li)->vval.v_string);
  });
}

// ── vim_regsub_both(), regexp.c:2168-2208 (fragment of the function body) ──

        typval_T argv[2];
        typval_T rettv;
        staticList10_T matchList = TV_LIST_STATIC10_INIT;
        rettv.v_type = VAR_STRING;
        rettv.vval.v_string = NULL;
        argv[0].v_type = VAR_LIST;
        argv[0].vval.v_list = &matchList.sl_list;
        funcexe_T funcexe = FUNCEXE_INIT;
        funcexe.fe_argv_func = fill_submatch_list;
        funcexe.fe_evaluate = true;
        if (expr->v_type == VAR_FUNC) {
          s = expr->vval.v_string;
          call_func(s, -1, &rettv, 1, argv, &funcexe);
        } else if (expr->v_type == VAR_PARTIAL) {
          partial_T *partial = expr->vval.v_partial;

          s = partial_name(partial);
          funcexe.fe_partial = partial;
          call_func(s, -1, &rettv, 1, argv, &funcexe);
        }
        if (tv_list_len(&matchList.sl_list) > 0) {
          // fill_submatch_list() was called.
          clear_submatch_list(&matchList);
        }
        if (rettv.v_type == VAR_UNKNOWN) {
          // something failed, no need to report another error
          eval_result[nested] = NULL;
        } else {
          char buf[NUMBUFLEN];
          eval_result[nested] = (char *)tv_get_string_buf_chk(&rettv, buf);
          if (eval_result[nested] != NULL) {
            eval_result[nested] = xstrdup(eval_result[nested]);
          }
        }
        tv_clear(&rettv);
      } else {
        eval_result[nested] = eval_to_string(source + 2, true, false);
      }
      nesting--;

      if (eval_result[nested] != NULL) {
