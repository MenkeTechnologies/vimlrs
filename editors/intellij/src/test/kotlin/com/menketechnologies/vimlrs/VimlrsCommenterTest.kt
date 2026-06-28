package com.menketechnologies.vimlrs

import org.junit.Assert.*
import org.junit.Test

/**
 * The Commenter contract is what IntelliJ uses for Cmd+/ and Cmd+Opt+/.
 * Wrong prefixes here mean the editor's comment shortcuts silently produce
 * broken Vimscript.
 */
class VimlrsCommenterTest {
    private val c = VimlrsCommenter()

    @Test fun `line comment prefix is double-quote-space`() {
        assertEquals("\" ", c.lineCommentPrefix)
    }

    @Test fun `there is no block comment form`() {
        // VimL has no block-comment delimiters — the hooks must be null so
        // Cmd+Opt+/ degrades to line-by-line `"` commenting.
        assertNull(c.blockCommentPrefix)
        assertNull(c.blockCommentSuffix)
        assertNull(c.commentedBlockCommentPrefix)
        assertNull(c.commentedBlockCommentSuffix)
    }
}
