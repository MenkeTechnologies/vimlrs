// Vendored subset for vimlrs — the yank-register store + get/set register paths.
//
// SOURCE: In current Neovim the register store lives in `src/nvim/register.c`
// (+ `register.h`, `register_defs.h`) after the ops.c/register.c split; the
// `MotionType` enum lives in `src/nvim/normal_defs.h`. This file vendors that
// subset verbatim under the historical `ops.c` name (the register machinery was
// originally part of ops.c). Only the symbols needed for `:let @r`, `getreg()`,
// `setreg()`, `getregtype()` are included; unrelated ops.c code is omitted.

// ── src/nvim/normal_defs.h ──────────────────────────────────────────────────

typedef enum {
  kMTCharWise = 0,     ///< character-wise movement/register
  kMTLineWise = 1,     ///< line-wise movement/register
  kMTBlockWise = 2,    ///< block-wise movement/register
  kMTUnknown = -1,     ///< Unknown or invalid motion type
} MotionType;

// ── src/nvim/register_defs.h ────────────────────────────────────────────────

/// Registers:
///      0 = register for latest (unnamed) yank
///   1..9 = registers '1' to '9', for deletes
/// 10..35 = registers 'a' to 'z'
///     36 = delete register '-'
///     37 = selection register '*'
///     38 = clipboard register '+'
enum {
  DELETION_REGISTER   = 36,
  NUM_SAVED_REGISTERS = 37,
  // The following registers should not be saved in ShaDa file:
  STAR_REGISTER       = 37,
  PLUS_REGISTER       = 38,
  NUM_REGISTERS       = 39,
};

/// Flags for get_reg_contents().
enum GRegFlags {
  kGRegNoExpr  = 1,  ///< Do not allow expression register.
  kGRegExprSrc = 2,  ///< Return expression itself for "=" register.
  kGRegList    = 4,  ///< Return list.
};

/// Definition of one register
typedef struct {
  String *y_array;          ///< Pointer to an array of Strings.
  size_t y_size;            ///< Number of lines in y_array.
  MotionType y_type;        ///< Register type
  colnr_T y_width;          ///< Register width (only valid for y_type == kBlockWise).
  Timestamp timestamp;      ///< Time when register was last modified.
  AdditionalData *additional_data;  ///< Additional data from ShaDa file.
} yankreg_T;

/// Modes for get_yank_register()
typedef enum {
  YREG_PASTE,
  YREG_YANK,
  YREG_PUT,
} yreg_mode_t;

// ── src/nvim/register.h (inline helpers) ────────────────────────────────────

/// Convert register name into register index
///
/// @return Index in y_regs array or -1 if register name was not recognized.
static inline int op_reg_index(const int regname)
{
  if (ascii_isdigit(regname)) {
    return regname - '0';
  } else if (ASCII_ISLOWER(regname)) {
    return CHAR_ORD_LOW(regname) + 10;
  } else if (ASCII_ISUPPER(regname)) {
    return CHAR_ORD_UP(regname) + 10;
  } else if (regname == '-') {
    return DELETION_REGISTER;
  } else if (regname == '*') {
    return STAR_REGISTER;
  } else if (regname == '+') {
    return PLUS_REGISTER;
  } else {
    return -1;
  }
}

static inline bool is_append_register(int regname)
{
  return ASCII_ISUPPER(regname);
}

/// @return  the character name of the register with the given number
static inline int get_register_name(int num)
{
  if (num == -1) {
    return '"';
  } else if (num < 10) {
    return num + '0';
  } else if (num == DELETION_REGISTER) {
    return '-';
  } else if (num == STAR_REGISTER) {
    return '*';
  } else if (num == PLUS_REGISTER) {
    return '+';
  } else {
    return num + 'a' - 10;
  }
}

/// Check whether register is empty
static inline bool reg_empty(const yankreg_T *const reg)
{
  return (reg->y_array == NULL
          || reg->y_size == 0
          || (reg->y_size == 1
              && reg->y_type == kMTCharWise
              && reg->y_array[0].size == 0));
}

// ── src/nvim/register.c ─────────────────────────────────────────────────────

static yankreg_T y_regs[NUM_REGISTERS] = { 0 };

static yankreg_T *y_previous = NULL;  // ptr to last written yankreg

yankreg_T *get_y_register(int reg)
{
  return &y_regs[reg];
}

yankreg_T *get_y_previous(void)
{
  return y_previous;
}

/// @return  whether `regname` is a valid name of a yank register.
///
/// @note: There is no check for 0 (default register), caller should do this.
/// The black hole register '_' is regarded as valid.
///
/// @param regname name of register
/// @param writing allow only writable registers
bool valid_yank_reg(int regname, bool writing)
{
  if ((regname > 0 && ASCII_ISALNUM(regname))
      || (!writing && vim_strchr("/#.%:=", regname) != NULL)
      || regname == '"'
      || regname == '-'
      || regname == '_'
      || regname == '*'
      || regname == '+') {
    return true;
  }
  return false;
}

/// Get register with the given name
///
/// @return Pointer to the register contents or NULL.
const yankreg_T *op_reg_get(const char name)
{
  int i = op_reg_index(name);
  if (i == -1) {
    return NULL;
  }
  return &y_regs[i];
}

yankreg_T *get_yank_register(int regname, int mode)
{
  yankreg_T *reg;

  if ((mode == YREG_PASTE || mode == YREG_PUT)
      && get_clipboard(regname, &reg, false)) {
    // reg is set to clipboard contents.
    return reg;
  } else if (mode == YREG_PUT && (regname == '*' || regname == '+')) {
    // in case clipboard not available and we aren't actually pasting,
    // return an empty register
    static yankreg_T empty_reg = { .y_array = NULL };
    return &empty_reg;
  } else if (mode != YREG_YANK
             && (regname == 0 || regname == '"' || regname == '*' || regname == '+')
             && y_previous != NULL) {
    // in case clipboard not available, paste from previous used register
    return y_previous;
  }

  int i = op_reg_index(regname);
  // when not 0-9, a-z, A-Z or '-'/'+'/'*': use register 0
  if (i == -1) {
    i = 0;
  }
  reg = &y_regs[i];

  if (mode == YREG_YANK) {
    // remember the written register for unnamed paste
    y_previous = reg;
  }
  return reg;
}

void free_register(yankreg_T *reg)
{
  XFREE_CLEAR(reg->additional_data);
  if (reg->y_array == NULL) {
    return;
  }

  for (size_t i = reg->y_size; i-- > 0;) {  // from y_size - 1 to 0 included
    API_CLEAR_STRING(reg->y_array[i]);
  }
  XFREE_CLEAR(reg->y_array);
}

size_t format_reg_type(MotionType reg_type, colnr_T reg_width, char *buf, size_t bufsize)
{
  assert(bufsize > 1);
  switch (reg_type) {
  case kMTLineWise:
    buf[0] = 'V';
    buf[1] = NUL;
    return 1;
  case kMTCharWise:
    buf[0] = 'v';
    buf[1] = NUL;
    return 1;
  case kMTBlockWise:
    return vim_snprintf_safelen(buf, bufsize, CTRL_V_STR "%" PRIdCOLNR, reg_width + 1);
  case kMTUnknown:
    buf[0] = NUL;
    return 0;
  }
  abort();
}

MotionType get_reg_type(int regname, colnr_T *reg_width)
{
  switch (regname) {
  case '%':     // file name
  case '#':     // alternate file name
  case '=':     // expression
  case ':':     // last command line
  case '/':     // last search-pattern
  case '.':     // last inserted text
  case Ctrl_F:  // Filename under cursor
  case Ctrl_P:  // Path under cursor, expand via "path"
  case Ctrl_W:  // word under cursor
  case Ctrl_A:  // WORD (mnemonic All) under cursor
  case '_':     // black hole: always empty
    return kMTCharWise;
  }

  if (regname != NUL && !valid_yank_reg(regname, false)) {
    return kMTUnknown;
  }

  yankreg_T *reg = get_yank_register(regname, YREG_PASTE);

  if (reg->y_array != NULL) {
    if (reg_width != NULL && reg->y_type == kMTBlockWise) {
      *reg_width = reg->y_width;
    }
    return reg->y_type;
  }
  return kMTUnknown;
}

/// When `flags` has `kGRegList` return a list with text `s`.
/// Otherwise just return `s`.
///
/// @return  a void * for use in get_reg_contents().
static void *get_reg_wrap_one_line(char *s, int flags)
{
  if (!(flags & kGRegList)) {
    return s;
  }
  list_T *const list = tv_list_alloc(1);
  tv_list_append_allocated_string(list, s);
  return list;
}

/// Gets the contents of a register.
/// @remark Used for `@r` in expressions and for `getreg()`.
///
/// @param regname  The register.
/// @param flags    see @ref GRegFlags
///
/// @returns The contents of the register as an allocated string.
/// @returns A linked list when `flags` contains @ref kGRegList.
/// @returns NULL for error.
void *get_reg_contents(int regname, int flags)
{
  // Don't allow using an expression register inside an expression.
  if (regname == '=') {
    if (flags & kGRegNoExpr) {
      return NULL;
    }
    if (flags & kGRegExprSrc) {
      return get_reg_wrap_one_line(get_expr_line_src(), flags);
    }
    return get_reg_wrap_one_line(get_expr_line(), flags);
  }

  if (regname == '@') {     // "@@" is used for unnamed register
    regname = '"';
  }

  // check for valid regname
  if (regname != NUL && !valid_yank_reg(regname, false)) {
    return NULL;
  }

  char *retval;
  bool allocated;
  if (get_spec_reg(regname, &retval, &allocated, false)) {
    if (retval == NULL) {
      return NULL;
    }
    if (allocated) {
      return get_reg_wrap_one_line(retval, flags);
    }
    return get_reg_wrap_one_line(xstrdup(retval), flags);
  }

  yankreg_T *reg = get_yank_register(regname, YREG_PUT);
  if (reg->y_array == NULL) {
    return NULL;
  }

  if (flags & kGRegList) {
    list_T *const list = tv_list_alloc((ptrdiff_t)reg->y_size);
    for (size_t i = 0; i < reg->y_size; i++) {
      tv_list_append_string(list, reg->y_array[i].data, (int)reg->y_array[i].size);
    }

    return list;
  }

  // Compute length of resulting string.
  size_t len = 0;
  for (size_t i = 0; i < reg->y_size; i++) {
    len += reg->y_array[i].size;
    // Insert a newline between lines and after last line if y_type is kMTLineWise.
    if (reg->y_type == kMTLineWise || i < reg->y_size - 1) {
      len++;
    }
  }

  retval = xmalloc(len + 1);

  // Copy the lines of the yank register into the string.
  len = 0;
  for (size_t i = 0; i < reg->y_size; i++) {
    STRCPY(retval + len, reg->y_array[i].data);
    len += reg->y_array[i].size;

    // Insert a newline between lines and after the last line if y_type is kMTLineWise.
    if (reg->y_type == kMTLineWise || i < reg->y_size - 1) {
      retval[len++] = '\n';
    }
  }
  retval[len] = NUL;

  return retval;
}

static yankreg_T *init_write_reg(int name, yankreg_T **old_y_previous, bool must_append)
{
  if (!valid_yank_reg(name, true)) {  // check for valid reg name
    emsg_invreg(name);
    return NULL;
  }

  // Don't want to change the current (unnamed) register.
  *old_y_previous = y_previous;

  yankreg_T *reg = get_yank_register(name, YREG_YANK);
  if (!is_append_register(name) && !must_append) {
    free_register(reg);
  }
  return reg;
}

/// str_to_reg - Put a string into a register.
///
/// When the register is not empty, the string is appended.
///
/// @param y_ptr pointer to yank register
/// @param yank_type The motion type (kMTUnknown to auto detect)
/// @param str string or list of strings to put in register
/// @param len length of the string (Ignored when str_list=true.)
/// @param blocklen width of visual block, or -1 for "I don't know."
/// @param str_list True if str is `char **`.
static void str_to_reg(yankreg_T *y_ptr, MotionType yank_type, const char *str, size_t len,
                       colnr_T blocklen, bool str_list)
{
  if (y_ptr->y_array == NULL) {  // NULL means empty register
    y_ptr->y_size = 0;
  }

  if (yank_type == kMTUnknown) {
    yank_type = ((str_list
                  || (len > 0 && (str[len - 1] == NL || str[len - 1] == CAR)))
                 ? kMTLineWise : kMTCharWise);
  }

  size_t newlines = 0;
  bool extraline = false;  // extra line at the end
  bool append = false;     // append to last line in register

  // Count the number of lines within the string
  if (str_list) {
    for (char **ss = (char **)str; *ss != NULL; ss++) {
      newlines++;
    }
  } else {
    newlines = memcnt(str, '\n', len);
    if (yank_type == kMTCharWise || len == 0 || str[len - 1] != '\n') {
      extraline = 1;
      newlines++;         // count extra newline at the end
    }
    if (y_ptr->y_size > 0 && y_ptr->y_type == kMTCharWise) {
      append = true;
      newlines--;         // uncount newline when appending first line
    }
  }

  // Without any lines make the register empty.
  if (y_ptr->y_size + newlines == 0) {
    XFREE_CLEAR(y_ptr->y_array);
    return;
  }

  // Grow the register array to hold the pointers to the new lines.
  String *pp = xrealloc(y_ptr->y_array, (y_ptr->y_size + newlines) * sizeof(String));
  y_ptr->y_array = pp;

  size_t lnum = y_ptr->y_size;  // The current line number.

  // If called with `blocklen < 0`, we have to update the yank reg's width.
  size_t maxlen = 0;

  // Find the end of each line and save it into the array.
  if (str_list) {
    for (char **ss = (char **)str; *ss != NULL; ss++, lnum++) {
      pp[lnum] = cstr_to_string(*ss);
      if (yank_type == kMTBlockWise) {
        size_t charlen = mb_string2cells(*ss);
        maxlen = MAX(maxlen, charlen);
      }
    }
  } else {
    size_t line_len;
    for (const char *start = str, *end = str + len;
         start < end + extraline;
         start += line_len + 1, lnum++) {
      int charlen = 0;

      const char *line_end = start;
      while (line_end < end) {  // find the end of the line
        if (*line_end == '\n') {
          break;
        }
        if (yank_type == kMTBlockWise) {
          charlen += utf_ptr2cells_len(line_end, (int)(end - line_end));
        }

        if (*line_end == NUL) {
          line_end++;  // registers can have NUL chars
        } else {
          line_end += utf_ptr2len_len(line_end, (int)(end - line_end));
        }
      }
      assert(line_end - start >= 0);
      line_len = (size_t)(line_end - start);
      maxlen = MAX(maxlen, (size_t)charlen);

      // When appending, copy the previous line and free it after.
      size_t extra = append ? pp[--lnum].size : 0;
      char *s = xmallocz(line_len + extra);
      if (extra > 0) {
        memcpy(s, pp[lnum].data, extra);
      }
      if (line_len > 0) {
        memcpy(s + extra, start, line_len);
      }
      size_t s_len = extra + line_len;

      if (append) {
        xfree(pp[lnum].data);
        append = false;  // only first line is appended
      }
      pp[lnum] = cbuf_as_string(s, s_len);

      // Convert NULs to '\n' to prevent truncation.
      memchrsub(pp[lnum].data, NUL, '\n', s_len);
    }
  }
  y_ptr->y_type = yank_type;
  y_ptr->y_size = lnum;
  XFREE_CLEAR(y_ptr->additional_data);
  y_ptr->timestamp = os_time();
  if (yank_type == kMTBlockWise) {
    y_ptr->y_width = (blocklen == -1 ? (colnr_T)maxlen - 1 : blocklen);
  } else {
    y_ptr->y_width = 0;
  }
}

static void finish_write_reg(int name, yankreg_T *reg, yankreg_T *old_y_previous)
{
  // Send text of clipboard register to the clipboard.
  set_clipboard(name, reg);

  // ':let @" = "val"' should change the meaning of the "" register
  if (name != '"') {
    y_previous = old_y_previous;
  }
}

/// store `str` in register `name`
///
/// @see write_reg_contents_ex
void write_reg_contents(int name, const char *str, ssize_t len, int must_append)
{
  write_reg_contents_ex(name, str, len, must_append, kMTUnknown, 0);
}

void write_reg_contents_lst(int name, char **strings, bool must_append, MotionType yank_type,
                            colnr_T block_len)
{
  if (name == '/' || name == '=' || name == '#') {
    char *s = strings[0];
    if (strings[0] == NULL) {
      s = "";
    } else if (strings[1] != NULL) {
      semsg(_(e_register_char_cannot_contain_multiple_lines), name);
      return;
    }
    write_reg_contents_ex(name, s, -1, must_append, yank_type, block_len);
    return;
  }

  // black hole: nothing to do
  if (name == '_') {
    return;
  }

  yankreg_T *old_y_previous, *reg;
  if (!(reg = init_write_reg(name, &old_y_previous, must_append))) {
    return;
  }

  str_to_reg(reg, yank_type, (char *)strings, strlen((char *)strings),
             block_len, true);
  finish_write_reg(name, reg, old_y_previous);
}

/// write_reg_contents_ex - store `str` in register `name`
///
/// If `str` ends in '\n' or '\r', use linewise, otherwise use charwise.
///
/// @param name The name of the register
/// @param str The contents to write
/// @param len If >= 0, write `len` bytes of `str`. Otherwise, write
///               `strlen(str)` bytes.
/// @param must_append If true, append the contents of `str` to the current
///                    contents of the register.
/// @param yank_type The motion type (kMTUnknown to auto detect)
/// @param block_len width of visual block
void write_reg_contents_ex(int name, const char *str, ssize_t len, bool must_append,
                           MotionType yank_type, colnr_T block_len)
{
  if (len < 0) {
    len = (ssize_t)strlen(str);
  }

  // Special case: '/' search pattern
  if (name == '/') {
    set_last_search_pat(str, RE_SEARCH, true, true);
    return;
  }

  if (name == '#') {
    // ... altfile handling (buffer substrate) ...
    return;
  }

  if (name == '=') {
    // ... expr_line handling ...
    return;
  }

  if (name == '_') {        // black hole: nothing to do
    return;
  }

  yankreg_T *old_y_previous, *reg;
  if (!(reg = init_write_reg(name, &old_y_previous, must_append))) {
    return;
  }
  str_to_reg(reg, yank_type, str, (size_t)len, block_len, false);
  finish_write_reg(name, reg, old_y_previous);
}
