// memline.c: VENDORED SUBSET of Neovim src/nvim/memline.c
// Only the line read/append/replace/delete entry points the vimlrs eval layer
// reaches are vendored, verbatim from Neovim. The memfile block-tree subsystem
// (ml_get_buf_impl / ml_append_int / ml_replace_buf_len / ml_delete_int / ml_find_line)
// is NOT ported: src/ported/buffer.rs backs b_ml with a Vec<String> line store
// (see the RUST-PORT NOTE there). ml_get_buf_impl is included for reference only.

#include "nvim/memline.h"
#include "nvim/buffer_defs.h"
#include "nvim/globals.h"

// ---- read path ----
char *ml_get(linenr_T lnum)
  FUNC_ATTR_NONNULL_RET
{
  return ml_get_buf_impl(curbuf, lnum, false);
}

char *ml_get_buf(buf_T *buf, linenr_T lnum)
  FUNC_ATTR_NONNULL_ALL FUNC_ATTR_NONNULL_RET
{
  return ml_get_buf_impl(buf, lnum, false);
}

char *ml_get_buf_mut(buf_T *buf, linenr_T lnum)
  FUNC_ATTR_NONNULL_ALL FUNC_ATTR_NONNULL_RET
{
  return ml_get_buf_impl(buf, lnum, true);
}

colnr_T ml_get_len(linenr_T lnum)
{
  return ml_get_buf_len(curbuf, lnum);
}

colnr_T ml_get_buf_len(buf_T *buf, linenr_T lnum)
{
  const char *line = ml_get_buf(buf, lnum);

  if (*line == NUL) {
    return 0;
  }

  assert(buf->b_ml.ml_line_textlen > 0);
  return buf->b_ml.ml_line_textlen - 1;
}

static char *ml_get_buf_impl(buf_T *buf, linenr_T lnum, bool will_change)
  FUNC_ATTR_NONNULL_ALL FUNC_ATTR_NONNULL_RET
{
  static int recursive = 0;
  static char questions[4];

  if (buf->b_ml.ml_mfp == NULL) {       // there are no lines
    buf->b_ml.ml_line_textlen = 1;
    return "";
  }

  if (lnum > buf->b_ml.ml_line_count) {  // invalid line number
    if (recursive == 0) {
      // Avoid giving this message for a recursive call, may happen when
      // the GUI redraws part of the text.
      recursive++;
      siemsg(_(e_ml_get_invalid_lnum_nr), (int64_t)lnum);
      recursive--;
    }
    ml_flush_line(buf, false);
errorret:
    STRCPY(questions, "???");
    buf->b_ml.ml_line_textlen = 4;
    buf->b_ml.ml_line_lnum = lnum;
    return questions;
  }
  lnum = MAX(lnum, 1);  // pretend line 0 is line 1

  // See if it is the same line as requested last time.
  // Otherwise may need to flush last used line.
  // Don't use the last used line when 'swapfile' is reset, need to load all
  // blocks.
  if (buf->b_ml.ml_line_lnum != lnum) {
    ml_flush_line(buf, false);

    // Find the data block containing the line.
    // This also fills the stack with the blocks from the root to the data
    // block and releases any locked block.
    bhdr_T *hp;
    if ((hp = ml_find_line(buf, lnum, ML_FIND)) == NULL) {
      if (recursive == 0) {
        // Avoid giving this message for a recursive call, may happen
        // when the GUI redraws part of the text.
        recursive++;
        get_trans_bufname(buf);
        shorten_dir(NameBuff);
        siemsg(_(e_ml_get_cannot_find_line_nr_in_buffer_nr_str),
               (int64_t)lnum, buf->b_fnum, NameBuff);
        recursive--;
      }
      goto errorret;
    }

    DataBlock *dp = hp->bh_data;

    int idx = lnum - buf->b_ml.ml_locked_low;
    unsigned start = (dp->db_index[idx] & DB_INDEX_MASK);
    // The text ends where the previous line starts.  The first line ends
    // at the end of the block.
    unsigned end = idx == 0 ? dp->db_txt_end : (dp->db_index[idx - 1] & DB_INDEX_MASK);

    buf->b_ml.ml_line_ptr = (char *)dp + start;
    buf->b_ml.ml_line_textlen = (colnr_T)(end - start);
    buf->b_ml.ml_line_lnum = lnum;
    buf->b_ml.ml_flags &= ~(ML_LINE_DIRTY | ML_ALLOCATED);
  }
  if (will_change) {
    buf->b_ml.ml_flags |= (ML_LOCKED_DIRTY | ML_LOCKED_POS);
#ifdef ML_GET_ALLOC_LINES
    if (buf->b_ml.ml_flags & ML_ALLOCATED) {
      // can't make the change in the data block
      buf->b_ml.ml_flags |= ML_LINE_DIRTY;
    }
#endif
    ml_add_deleted_len_buf(buf, buf->b_ml.ml_line_ptr, -1);
  }

#ifdef ML_GET_ALLOC_LINES
  if ((buf->b_ml.ml_flags & (ML_LINE_DIRTY | ML_ALLOCATED)) == 0) {
    // make sure the text is in allocated memory
    buf->b_ml.ml_line_ptr = xmemdup(buf->b_ml.ml_line_ptr,
                                    (size_t)buf->b_ml.ml_line_textlen);
    buf->b_ml.ml_flags |= ML_ALLOCATED;
    if (will_change) {
      // can't make the change in the data block
      buf->b_ml.ml_flags |= ML_LINE_DIRTY;
    }
  }
#endif
  return buf->b_ml.ml_line_ptr;
}

// ---- append path ----
int ml_append(linenr_T lnum, char *line, colnr_T len, bool newfile)
{
  return ml_append_flags(lnum, line, len, newfile ? ML_APPEND_NEW : 0);
}

int ml_append_flags(linenr_T lnum, char *line, colnr_T len, int flags)
{
  // When starting up, we might still need to create the memfile
  if (curbuf->b_ml.ml_mfp == NULL && open_buffer(false, NULL, 0) == FAIL) {
    return FAIL;
  }

  return ml_append_flush(curbuf, lnum, line, len, flags);
}

int ml_append_buf(buf_T *buf, linenr_T lnum, char *line, colnr_T len, bool newfile)
  FUNC_ATTR_NONNULL_ARG(1)
{
  if (buf->b_ml.ml_mfp == NULL) {
    return FAIL;
  }

  return ml_append_flush(buf, lnum, line, len, newfile ? ML_APPEND_NEW : 0);
}

// ---- replace path ----
int ml_replace(linenr_T lnum, char *line, bool copy)
{
  return ml_replace_buf(curbuf, lnum, line, copy, false);
}

int ml_replace_buf(buf_T *buf, linenr_T lnum, char *line, bool copy, bool noalloc)
  FUNC_ATTR_NONNULL_ARG(1)
{
  size_t len = line != NULL ? strlen(line) : (size_t)-1;
  return ml_replace_buf_len(buf, lnum, line, len, copy, noalloc);
}

// ---- delete path ----
int ml_delete(linenr_T lnum)
{
  return ml_delete_flags(lnum, 0);
}

int ml_delete_flags(linenr_T lnum, int flags)
{
  ml_flush_line(curbuf, false);
  if (lnum < 1 || lnum > curbuf->b_ml.ml_line_count) {
    return FAIL;
  }

  return ml_delete_int(curbuf, lnum, flags);
}

