// memline_defs.h: VENDORED SUBSET of Neovim src/nvim/memline_defs.h
// Only the memline_T fields the vimlrs eval layer reads are shown. The block
// tree bookkeeping (ml_stack, ml_locked, ml_chunksize, cached-line state) is
// listed verbatim for reference but is NOT modeled: src/ported/buffer.rs backs
// the line store with a Vec<String> (RUST-PORT NOTE there).

/// memline structure: the contents of a buffer.
typedef struct {
  linenr_T ml_line_count;       // number of lines in the buffer

  memfile_T *ml_mfp;          // pointer to associated memfile

  infoptr_T *ml_stack;        // stack of pointer blocks (array of IPTRs)
  int ml_stack_top;             // current top of ml_stack
  int ml_stack_size;            // total number of entries in ml_stack

  int ml_flags;

  colnr_T ml_line_textlen;      // length of the cached line + NUL
  linenr_T ml_line_lnum;        // line number of cached line, 0 if not valid
  char *ml_line_ptr;            // pointer to cached line
  size_t ml_line_offset;        // cached byte offset of ml_line_lnum
  int ml_line_offset_ff;        // fileformat of cached line
} memline_T;
