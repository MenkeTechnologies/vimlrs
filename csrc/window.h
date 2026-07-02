// Vendored SUBSET of Neovim src/nvim/window.h — only the constant and the
// prototypes the window model needs. Verbatim from upstream; trimmed.
#pragma once

#include "buffer_defs.h"
#include "types_defs.h"

enum {
  /// Lowest number used for window ID. Cannot have this many windows per tab.
  LOWEST_WIN_ID = 1000,
};

tabpage_T *find_tabpage(int n);
void win_get_tabwin(handle_T id, int *tabnr, int *winnr);
