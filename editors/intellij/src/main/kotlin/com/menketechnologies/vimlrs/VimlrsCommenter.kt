package com.menketechnologies.vimlrs

import com.intellij.lang.Commenter

/**
 * VimL line comments use a leading double-quote `"` (in command position).
 * Cmd/Ctrl-`/` toggles `"` + space.
 *
 * VimL has NO block-comment form — there's no `/* … *​/` or heredoc-style
 * block delimiter — so the block-comment hooks return null and Cmd+Opt+/
 * falls back to line-by-line `"` commenting.
 */
class VimlrsCommenter : Commenter {
    override fun getLineCommentPrefix(): String = "\" "
    override fun getBlockCommentPrefix(): String? = null
    override fun getBlockCommentSuffix(): String? = null
    override fun getCommentedBlockCommentPrefix(): String? = null
    override fun getCommentedBlockCommentSuffix(): String? = null
}
