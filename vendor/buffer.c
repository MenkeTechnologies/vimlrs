// buffer.c: VENDORED SUBSET of Neovim src/nvim/buffer.c
// Only the buffer-list lookup/creation functions the vimlrs eval layer reaches
// are vendored here, verbatim from Neovim. This is the spec for src/ported/buffer.rs.
// The full buffer subsystem (windows, folds, marks, autocmds, memfile) is NOT vendored.

#include "nvim/buffer.h"
#include "nvim/buffer_defs.h"
#include "nvim/globals.h"
#include "nvim/memline.h"

// ---- buflist_new() / curbuf_reusable() ----
buf_T *buflist_new(char *ffname_arg, char *sfname_arg, linenr_T lnum, int flags)
{
  char *ffname = ffname_arg;
  char *sfname = sfname_arg;
  buf_T *buf;

  fname_expand(curbuf, &ffname, &sfname);       // will allocate ffname

  // If the file name already exists in the list, update the entry.

  // We can use inode numbers when the file exists.  Works better
  // for hard links.
  FileID file_id;
  bool file_id_valid = (sfname != NULL && os_fileid(sfname, &file_id));
  if (ffname != NULL && !(flags & (BLN_DUMMY | BLN_NEW))
      && (buf = buflist_findname_file_id(ffname, &file_id, file_id_valid)) != NULL) {
    xfree(ffname);
    if (lnum != 0) {
      buflist_setfpos(buf, (flags & BLN_NOCURWIN) ? NULL : curwin,
                      lnum, 0, false);
    }
    if ((flags & BLN_NOOPT) == 0) {
      // Copy the options now, if 'cpo' doesn't have 's' and not done already.
      buf_copy_options(buf, 0);
    }
    if ((flags & BLN_LISTED) && !buf->b_p_bl) {
      buf->b_p_bl = true;
      bufref_T bufref;
      set_bufref(&bufref, buf);
      if (!(flags & BLN_DUMMY)) {
        if (apply_autocmds(EVENT_BUFADD, NULL, NULL, false, buf)
            && !bufref_valid(&bufref)) {
          return NULL;
        }
      }
    }
    return buf;
  }

  // If the current buffer has no name and no contents, use the current
  // buffer.    Otherwise: Need to allocate a new buffer structure.
  //
  // This is the ONLY place where a new buffer structure is allocated!
  // (A spell file buffer is allocated in spell.c, but that's not a normal
  // buffer.)
  buf = NULL;
  if ((flags & BLN_CURBUF) && curbuf_reusable()) {
    bufref_T bufref;

    assert(curbuf != NULL);
    buf = curbuf;
    set_bufref(&bufref, buf);
    trigger_undo_ftplugin(buf, curwin);
    // It's like this buffer is deleted.  Watch out for autocommands that
    // change curbuf!  If that happens, allocate a new buffer anyway.
    buf_freeall(buf, BFA_WIPE | BFA_DEL);
    if (aborting()) {           // autocmds may abort script processing
      xfree(ffname);
      return NULL;
    }
    if (!bufref_valid(&bufref)) {
      buf = NULL;  // buf was deleted; allocate a new buffer
    }
  }
  if (buf != curbuf || curbuf == NULL) {
    buf = xcalloc(1, sizeof(buf_T));
    // init b: variables
    buf->b_vars = tv_dict_alloc();
    init_var_dict(buf->b_vars, &buf->b_bufvar, VAR_SCOPE);
    buf_init_changedtick(buf);
  }

  if (ffname != NULL) {
    buf->b_ffname = ffname;
    buf->b_sfname = xstrdup(sfname);
  }

  clear_wininfo(buf);
  WinInfo *curwin_info = xcalloc(1, sizeof(WinInfo));
  kv_push(buf->b_wininfo, curwin_info);

  if (buf == curbuf) {
    free_buffer_stuff(buf, kBffInitChangedtick);  // delete local vars et al.

    // Init the options.
    buf->b_p_initialized = false;
    buf_copy_options(buf, BCO_ENTER);

    // need to reload lmaps and set b:keymap_name
    curbuf->b_kmap_state |= KEYMAP_INIT;
  } else {
    // put new buffer at the end of the buffer list
    buf->b_next = NULL;
    if (firstbuf == NULL) {             // buffer list is empty
      buf->b_prev = NULL;
      firstbuf = buf;
    } else {                            // append new buffer at end of list
      lastbuf->b_next = buf;
      buf->b_prev = lastbuf;
    }
    lastbuf = buf;

    buf->b_fnum = top_file_num++;
    pmap_put(int)(&buffer_handles, buf->b_fnum, buf);
    if (top_file_num < 0) {  // wrap around (may cause duplicates)
      emsg(_("W14: Warning: List of file names overflow"));
      if (emsg_silent == 0 && !in_assert_fails) {
        msg_delay(3001, true);  // make sure it is noticed
      }
      top_file_num = 1;
    }

    // Always copy the options from the current buffer.
    buf_copy_options(buf, BCO_ALWAYS);
  }

  curwin_info->wi_mark = (fmark_T)INIT_FMARK;
  curwin_info->wi_mark.mark.lnum = lnum;
  curwin_info->wi_win = curwin;

  hash_init(&buf->b_s.b_keywtab);
  hash_init(&buf->b_s.b_keywtab_ic);

  buf->b_fname = buf->b_sfname;
  if (!file_id_valid) {
    buf->file_id_valid = false;
  } else {
    buf->file_id_valid = true;
    buf->file_id = file_id;
  }
  buf->b_u_synced = true;
  buf->b_flags = BF_CHECK_RO | BF_NEVERLOADED;
  if (flags & BLN_DUMMY) {
    buf->b_flags |= BF_DUMMY;
  }
  buf_clear_file(buf);
  clrallmarks(buf, 0);                  // clear marks
  fmarks_check_names(buf);              // check file marks for this file
  buf->b_p_bl = (flags & BLN_LISTED) ? true : false;    // init 'buflisted'
  kv_destroy(buf->update_channels);
  kv_init(buf->update_channels);
  kv_destroy(buf->update_callbacks);
  kv_init(buf->update_callbacks);
  if (!(flags & BLN_DUMMY)) {
    // Tricky: these autocommands may change the buffer list.  They could also
    // split the window with re-using the one empty buffer. This may result in
    // unexpectedly losing the empty buffer.
    bufref_T bufref;
    set_bufref(&bufref, buf);
    if (apply_autocmds(EVENT_BUFNEW, NULL, NULL, false, buf)
        && !bufref_valid(&bufref)) {
      return NULL;
    }
    if ((flags & BLN_LISTED)
        && apply_autocmds(EVENT_BUFADD, NULL, NULL, false, buf)
        && !bufref_valid(&bufref)) {
      return NULL;
    }
    if (aborting()) {
      // Autocmds may abort script processing.
      return NULL;
    }
  }

  buf->b_prompt_callback.type = kCallbackNone;
  buf->b_prompt_interrupt.type = kCallbackNone;
  buf->b_prompt_text = NULL;
  buf->b_prompt_start = (fmark_T)INIT_FMARK;
  buf->b_prompt_start.mark.col = 2;  // default prompt is "% "
  buf->b_prompt_append_new_line = true;

  return buf;
}

bool curbuf_reusable(void)
{
  return (curbuf != NULL
          && curbuf->b_ffname == NULL
          && curbuf->b_nwindows <= 1
          && !curbuf->terminal
          && (curbuf->b_ml.ml_mfp == NULL || buf_is_empty(curbuf))
          && !bt_quickfix(curbuf)
          && !curbufIsChanged());
}

// ---- buflist_add() ----
int buflist_add(char *fname, int flags)
{
  buf_T *buf = buflist_new(fname, NULL, 0, flags);
  if (buf != NULL) {
    return buf->b_fnum;
  }
  return 0;
}

// ---- buflist_findname_exp / buflist_findname / buflist_findname_file_id ----
buf_T *buflist_findname_exp(char *fname)
{
  buf_T *buf = NULL;

  // First make the name into a full path name
  char *ffname = FullName_save(fname,
#ifdef UNIX
                               // force expansion, get rid of symbolic links
                               true
#else
                               false
#endif
                               );
  if (ffname != NULL) {
    buf = buflist_findname(ffname);
    xfree(ffname);
  }
  return buf;
}

buf_T *buflist_findname(char *ffname)
{
  FileID file_id;
  bool file_id_valid = os_fileid(ffname, &file_id);
  return buflist_findname_file_id(ffname, &file_id, file_id_valid);
}

static buf_T *buflist_findname_file_id(char *ffname, FileID *file_id, bool file_id_valid)
  FUNC_ATTR_PURE
{
  // Start at the last buffer, expect to find a match sooner.
  FOR_ALL_BUFFERS_BACKWARDS(buf) {
    if ((buf->b_flags & BF_DUMMY) == 0
        && !otherfile_buf(buf, ffname, file_id, file_id_valid)) {
      return buf;
    }
  }
  return NULL;
}

// ---- otherfile_buf() ----
static bool otherfile_buf(buf_T *buf, char *ffname, FileID *file_id_p, bool file_id_valid)
  FUNC_ATTR_PURE FUNC_ATTR_WARN_UNUSED_RESULT
{
  // no name is different
  if (ffname == NULL || *ffname == NUL || buf->b_ffname == NULL) {
    return true;
  }
  if (path_fnamecmp(ffname, buf->b_ffname) == 0) {
    return false;
  }
  {
    FileID file_id;
    // If no struct stat given, get it now
    if (file_id_p == NULL) {
      file_id_p = &file_id;
      file_id_valid = os_fileid(ffname, file_id_p);
    }
    if (!file_id_valid) {
      // file_id not valid, assume files are different.
      return true;
    }
    // Use dev/ino to check if the files are the same, even when the names
    // are different (possible with links).  Still need to compare the
    // name above, for when the file doesn't exist yet.
    // Problem: The dev/ino changes when a file is deleted (and created
    // again) and remains the same when renamed/moved.  We don't want to
    // stat() each buffer each time, that would be too slow.  Get the
    // dev/ino again when they appear to match, but not when they appear
    // to be different: Could skip a buffer when it's actually the same
    // file.
    if (buf_same_file_id(buf, file_id_p)) {
      buf_set_file_id(buf);
      if (buf_same_file_id(buf, file_id_p)) {
        return false;
      }
    }
  }
  return true;
}

// ---- buflist_findpat() ----
int buflist_findpat(const char *pattern, const char *pattern_end, bool unlisted, bool diffmode,
                    bool curtab_only)
  FUNC_ATTR_NONNULL_ARG(1)
{
  int match = -1;

  if (pattern_end == pattern + 1 && (*pattern == '%' || *pattern == '#')) {
    match = *pattern == '%' ? curbuf->b_fnum : curwin->w_alt_fnum;
    buf_T *found_buf = buflist_findnr(match);
    if (diffmode && !(found_buf && diff_mode_buf(found_buf))) {
      match = -1;
    }
  } else {
    // Try four ways of matching a listed buffer:
    // attempt == 0: without '^' or '$' (at any position)
    // attempt == 1: with '^' at start (only at position 0)
    // attempt == 2: with '$' at end (only match at end)
    // attempt == 3: with '^' at start and '$' at end (only full match)
    // Repeat this for finding an unlisted buffer if there was no matching
    // listed buffer.

    char *pat = file_pat_to_reg_pat(pattern, pattern_end, NULL, false);
    if (pat == NULL) {
      return -1;
    }
    char *patend = pat + strlen(pat) - 1;
    bool toggledollar = (patend > pat && *patend == '$');

    // First try finding a listed buffer.  If not found and "unlisted"
    // is true, try finding an unlisted buffer.

    int find_listed = true;
    while (true) {
      for (int attempt = 0; attempt <= 3; attempt++) {
        // may add '^' and '$'
        if (toggledollar) {
          *patend = (attempt < 2) ? NUL : '$';           // add/remove '$'
        }
        char *p = pat;
        if (*p == '^' && !(attempt & 1)) {               // add/remove '^'
          p++;
        }

        regmatch_T regmatch;
        regmatch.regprog = vim_regcomp(p, magic_isset() ? RE_MAGIC : 0);

        FOR_ALL_BUFFERS_BACKWARDS(buf) {
          if (regmatch.regprog == NULL) {
            // invalid pattern, possibly after switching engine
            xfree(pat);
            return -1;
          }
          if (buf->b_p_bl == find_listed
              && (!diffmode || diff_mode_buf(buf))
              && buflist_match(&regmatch, buf, false) != NULL) {
            if (curtab_only) {
              // Ignore the match if the buffer is not open in
              // the current tab.
              bool found_window = false;
              FOR_ALL_WINDOWS_IN_TAB(wp, curtab) {
                if (wp->w_buffer == buf) {
                  found_window = true;
                  break;
                }
              }
              if (!found_window) {
                continue;
              }
            }
            if (match >= 0) {                   // already found a match
              match = -2;
              break;
            }
            match = buf->b_fnum;                // remember first match
          }
        }

        vim_regfree(regmatch.regprog);
        if (match >= 0) {                       // found one match
          break;
        }
      }

      // Only search for unlisted buffers if there was no match with
      // a listed buffer.
      if (!unlisted || !find_listed || match != -1) {
        break;
      }
      find_listed = false;
    }

    xfree(pat);
  }

  if (match == -2) {
    semsg(_("E93: More than one match for %s"), pattern);
  } else if (match < 0) {
    semsg(_("E94: No matching buffer for %s"), pattern);
  }
  return match;
}

// ---- buflist_findnr / buflist_nr2name ----
buf_T *buflist_findnr(int nr)
{
  if (nr == 0) {
    nr = curwin->w_alt_fnum;
  }

  return handle_get_buffer((handle_T)nr);
}

char *buflist_nr2name(int n, int fullname, int helptail)
{
  buf_T *buf = buflist_findnr(n);
  if (buf == NULL) {
    return NULL;
  }
  return home_replace_save(helptail ? buf : NULL,
                           fullname ? buf->b_ffname : buf->b_fname);
}

// ---- buflist_findfmark / buflist_findlnum ----
fmark_T *buflist_findfmark(buf_T *buf)
  FUNC_ATTR_PURE
{
  static fmark_T no_position = { { 1, 0, 0 }, 0, 0, INIT_FMARKV, NULL };

  WinInfo *const wip = find_wininfo(buf, false, false);
  return (wip == NULL) ? &no_position : &(wip->wi_mark);
}

linenr_T buflist_findlnum(buf_T *buf)
  FUNC_ATTR_PURE
{
  return buflist_findfmark(buf)->mark.lnum;
}

// ---- buflist_name_nr() ----
int buflist_name_nr(int fnum, char **fname, linenr_T *lnum)
{
  buf_T *buf = buflist_findnr(fnum);
  if (buf == NULL || buf->b_fname == NULL) {
    return FAIL;
  }

  *fname = buf->b_fname;
  *lnum = buflist_findlnum(buf);

  return OK;
}

// ---- bt_prompt / bt_normal / bt_quickfix / bt_nofilename ----
bool bt_prompt(buf_T *buf)
  FUNC_ATTR_PURE
{
  return buf != NULL && buf->b_p_bt[0] == 'p';
}

bool bt_normal(const buf_T *const buf)
  FUNC_ATTR_PURE FUNC_ATTR_WARN_UNUSED_RESULT
{
  return buf != NULL && buf->b_p_bt[0] == NUL;
}

bool bt_quickfix(const buf_T *const buf)
  FUNC_ATTR_PURE FUNC_ATTR_WARN_UNUSED_RESULT
{
  return buf != NULL && buf->b_p_bt[0] == 'q';
}

bool bt_nofilename(const buf_T *const buf)
  FUNC_ATTR_PURE FUNC_ATTR_WARN_UNUSED_RESULT
{
  return buf != NULL && ((buf->b_p_bt[0] == 'n' && buf->b_p_bt[2] == 'f')
                         || buf->b_p_bt[0] == 'a'
                         || buf->terminal
                         || buf->b_p_bt[0] == 'p');
}
