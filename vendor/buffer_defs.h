// Vendored SUBSET of Neovim src/nvim/buffer_defs.h — only the win_T / tabpage_T
// structs (and the WinConfig fields eval reads) needed by the window model.
// Struct bodies are copied verbatim from upstream and trimmed to the eval-read
// subset; trimmed regions are marked "// … (trimmed)".
#pragma once

#include "pos_defs.h"
#include "types_defs.h"

// --- WinConfig (window_defs.h in some trees) --------------------------------
// Only the two fields win_has_winnr() reads are kept.
typedef struct {
  // … (trimmed: geometry / border / title / footer fields)
  bool focusable;
  // … (trimmed)
  bool hide;
  // … (trimmed)
} WinConfig;

// --- tabpage_T (buffer_defs.h:839) ------------------------------------------
typedef struct tabpage_S tabpage_T;
struct tabpage_S {
  handle_T handle;
  tabpage_T *tp_next;         ///< next tabpage or NULL
  win_T *tp_curwin;           ///< current window in this Tab page
  win_T *tp_prevwin;          ///< previous window in this Tab page
  win_T *tp_firstwin;         ///< first window in this Tab page
  win_T *tp_lastwin;          ///< last window in this Tab page
  // … (trimmed: frame/diff/snapshot/vars/localdir fields)
};

// --- win_T (buffer_defs.h:1102, struct window_S) ----------------------------
struct window_S {
  handle_T handle;                  ///< unique identifier for the window

  buf_T *w_buffer;            ///< buffer we are a window into (used
                              ///< often, keep it the first item!)

  // … (trimmed: highlight / namespace fields)

  win_T *w_prev;              ///< link to previous window
  win_T *w_next;              ///< link to next window

  // … (trimmed)

  pos_T w_cursor;                   ///< cursor position in buffer

  // … (trimmed: many display / option fields)

  bool w_p_pvw;                     ///< 'previewwindow' (w_onebuf_opt.wo_pvw)

  bool w_floating;                  ///< whether the window is floating
  WinConfig w_config;

  // … (trimmed)
};

// --- buf_T (buffer_defs.h, struct file_buffer) ------------------------------
// Only the fields the vimlrs eval layer reads are kept (verbatim). Every
// window/fold/mark/undo/syntax/option field the eval leaves never touch is
// omitted. This is the spec for the buf_T model in src/ported/buffer.rs.

// flags for b_flags
#define BF_RECOVERED    0x01    // buffer has been recovered
#define BF_CHECK_RO     0x02    // need to check readonly when loading file
#define BF_NEVERLOADED  0x04    // file has never been loaded into buffer
#define BF_NOTEDITED    0x08    // Set when file name is changed after editing
#define BF_NEW          0x10    // file didn't exist when editing started
#define BF_NEW_W        0x20    // Warned for BF_NEW and file created
#define BF_READERR      0x40    // got errors while reading the file
#define BF_DUMMY        0x80    // dummy buffer, only used internally

struct file_buffer {
  handle_T handle;              // unique id for the buffer (buffer number)
#define b_fnum handle

  memline_T b_ml;               // associated memline (also contains line count)

  buf_T *b_next;          // links in list of buffers
  buf_T *b_prev;

  int b_nwindows;               // nr of windows open on this buffer

  int b_flags;                  // various BF_ flags

  // b_ffname   has the full path of the file (NULL for no name).
  // b_sfname   is the name as the user typed it (or NULL).
  // b_fname    is the same as b_sfname, unless ":cd" has been done.
  char *b_ffname;          // full path file name, allocated
  char *b_sfname;          // short file name, allocated, may equal b_ffname
  char *b_fname;           // current file name, points to b_ffname or b_sfname

  int b_changed;                // 'modified'

  // Change-identifier incremented for each change, stored in b:changedtick.
  ChangedtickDictItem changedtick_di;

  bool terminal;                // non-NULL when this is a terminal buffer

  time_t b_last_used;           // time when the buffer was last used

  int b_p_bl;                   ///< 'buflisted'
  int b_p_ma;                   ///< 'modifiable'
  int b_p_ro;                   ///< 'readonly'
  char *b_p_bt;                 ///< 'buftype'

  bool b_modified_was_set;      // did ":set modified"

  bool b_help;                  // true for help file buffer

  dict_T *b_vars;  ///< b: scope Dict.
};
