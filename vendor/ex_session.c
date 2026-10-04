// Vendored SUBSET of Neovim v0.12.4's src/nvim/ex_session.c — the part a
// Vimscript engine owns when an editor writes a session: the global variables
// :mksession saves under 'sessionoptions' "globals". This is the spec for
// `src/ported/ex_session.rs`. Copied verbatim (ex_session.c:534-585).

static int store_session_globals(FILE *fd)
{
  TV_DICT_ITER(get_globvar_dict(), this_var, {
    if ((this_var->di_tv.v_type == VAR_NUMBER
         || this_var->di_tv.v_type == VAR_STRING)
        && var_flavour(this_var->di_key) == VAR_FLAVOUR_SESSION) {
      // Escape special characters with a backslash.  Turn a LF and
      // CR into \n and \r.
      char *const p = vim_strsave_escaped(tv_get_string(&this_var->di_tv), "\\\"\n\r");
      for (char *t = p; *t != NUL; t++) {
        if (*t == '\n') {
          *t = 'n';
        } else if (*t == '\r') {
          *t = 'r';
        }
      }
      if ((fprintf(fd, "let %s = %c%s%c",
                   this_var->di_key,
                   ((this_var->di_tv.v_type == VAR_STRING) ? '"' : ' '),
                   p,
                   ((this_var->di_tv.v_type == VAR_STRING) ? '"' : ' ')) < 0)
          || put_eol(fd) == FAIL) {
        xfree(p);
        return FAIL;
      }
      xfree(p);
    } else if (this_var->di_tv.v_type == VAR_FLOAT
               && var_flavour(this_var->di_key) == VAR_FLAVOUR_SESSION) {
      float_T f = this_var->di_tv.vval.v_float;
      int sign = ' ';

      if (f < 0) {
        f = -f;
        sign = '-';
      }
      if ((fprintf(fd, "let %s = %c%f", this_var->di_key, sign, f) < 0)
          || put_eol(fd) == FAIL) {
        return FAIL;
      }
    }
  });
  return OK;
}

/// Writes commands for restoring the current buffers, for :mksession.
///
/// Legacy 'sessionoptions'/'viewoptions' flags kOptSsopFlagUnix, kOptSsopFlagSlash are
/// always enabled.
///
/// @param dirnow  Current directory name
/// @param fd  File descriptor to write to
///
