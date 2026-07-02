// Vendored SUBSET of Neovim src/nvim/pos_defs.h — only the position/line/column
// types the window model needs. Verbatim from upstream; trimmed to the subset.
#pragma once

#include <inttypes.h>

/// Line number type
typedef int32_t linenr_T;

/// Column number type
typedef int colnr_T;

/// position in file or buffer
typedef struct {
  linenr_T lnum;        ///< line number
  colnr_T col;          ///< column number
  colnr_T coladd;
} pos_T;
