// Vendored SUBSET of Neovim src/nvim/window.c — only the window/tabpage list
// helpers the eval window layer calls (find_tabpage, win_get_tabwin) plus the
// globals/macros they read. Function bodies are copied verbatim from upstream;
// the file is trimmed to the eval-reachable subset.

#include <assert.h>
#include <inttypes.h>

#include "buffer_defs.h"
#include "types_defs.h"
#include "window.h"

// From globals.h — the window / tabpage lists. (globals.h:355-390)
// All windows are linked in a list. firstwin points to the first entry,
// lastwin to the last entry (can be the same as firstwin) and curwin to the
// currently active window.
EXTERN win_T *firstwin;              // first window
EXTERN win_T *lastwin;               // last window
EXTERN win_T *curwin;               // currently active window

// Tab pages are alternative topframes.  "first_tabpage" points to the first
// one in the list, "curtab" is the current one.
EXTERN tabpage_T *first_tabpage;
EXTERN tabpage_T *curtab;

// Iterates over all tabs in the tab list
#define FOR_ALL_TABS(tp) for (tabpage_T *(tp) = first_tabpage; (tp) != NULL; (tp) = (tp)->tp_next)

#define FOR_ALL_WINDOWS_IN_TAB(wp, tp) \
  for (win_T *wp = ((tp) == curtab) ? firstwin : (tp)->tp_firstwin; \
       wp != NULL; wp = wp->w_next)

// Find tab page "n" (first one is 1).  Returns NULL when not found.
tabpage_T *find_tabpage(int n)
{
  tabpage_T *tp;
  int i = 1;

  if (n == 0) {
    return curtab;
  }

  for (tp = first_tabpage; tp != NULL && i != n; tp = tp->tp_next) {
    i++;
  }
  return tp;
}

void win_get_tabwin(handle_T id, int *tabnr, int *winnr)
{
  *tabnr = 0;
  *winnr = 0;

  int tnum = 1;
  int wnum = 1;
  FOR_ALL_TABS(tp) {
    FOR_ALL_WINDOWS_IN_TAB(wp, tp) {
      if (wp->handle == id) {
        if (win_has_winnr(wp, tp)) {
          *winnr = wnum;
          *tabnr = tnum;
        }
        return;
      }
      wnum += win_has_winnr(wp, tp);
    }
    tnum++;
    wnum = 1;
  }
}
