// Vendored SUBSET of Neovim v0.12.4's src/nvim/usercmd.c — the <f-args>
// splitter a user command's replacement text uses. This is the spec for
// `uc_split_args()` in `src/ported/eval/funcs.rs`. Copied verbatim
// (usercmd.c:1179-1292).

/// split and quote args for <f-args>
static char *uc_split_args(const char *arg, char **args, const size_t *arglens, size_t argc,
                           size_t *lenp)
{
  // Precalculate length
  int len = 2;   // Initial and final quotes
  if (args == NULL) {
    const char *p = arg;

    while (*p) {
      if (p[0] == '\\' && p[1] == '\\') {
        len += 2;
        p += 2;
      } else if (p[0] == '\\' && ascii_iswhite(p[1])) {
        len += 1;
        p += 2;
      } else if (*p == '\\' || *p == '"') {
        len += 2;
        p += 1;
      } else if (ascii_iswhite(*p)) {
        p = skipwhite(p);
        if (*p == NUL) {
          break;
        }
        len += 4;  // ", "
      } else {
        const int charlen = utfc_ptr2len(p);

        len += charlen;
        p += charlen;
      }
    }
  } else {
    for (size_t i = 0; i < argc; i++) {
      const char *p = args[i];
      const char *arg_end = args[i] + arglens[i];

      while (p < arg_end) {
        if (*p == '\\' || *p == '"') {
          len += 2;
          p += 1;
        } else {
          const int charlen = utfc_ptr2len(p);

          len += charlen;
          p += charlen;
        }
      }

      if (i != argc - 1) {
        len += 4;  // ", "
      }
    }
  }

  char *buf = xmalloc((size_t)len + 1);

  char *q = buf;
  *q++ = '"';

  if (args == NULL) {
    const char *p = arg;
    while (*p) {
      if (p[0] == '\\' && p[1] == '\\') {
        *q++ = '\\';
        *q++ = '\\';
        p += 2;
      } else if (p[0] == '\\' && ascii_iswhite(p[1])) {
        *q++ = p[1];
        p += 2;
      } else if (*p == '\\' || *p == '"') {
        *q++ = '\\';
        *q++ = *p++;
      } else if (ascii_iswhite(*p)) {
        p = skipwhite(p);
        if (*p == NUL) {
          break;
        }
        *q++ = '"';
        *q++ = ',';
        *q++ = ' ';
        *q++ = '"';
      } else {
        mb_copy_char(&p, &q);
      }
    }
  } else {
    for (size_t i = 0; i < argc; i++) {
      const char *p = args[i];
      const char *arg_end = args[i] + arglens[i];

      while (p < arg_end) {
        if (*p == '\\' || *p == '"') {
          *q++ = '\\';
          *q++ = *p++;
        } else {
          mb_copy_char(&p, &q);
        }
      }
      if (i != argc - 1) {
        *q++ = '"';
        *q++ = ',';
        *q++ = ' ';
        *q++ = '"';
      }
    }
  }

  *q++ = '"';
  *q = 0;

  *lenp = (size_t)len;
  return buf;
}
