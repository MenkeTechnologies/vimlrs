package com.menketechnologies.vimlrs

import com.intellij.codeInsight.editorActions.QuoteHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.highlighter.HighlighterIterator
import com.intellij.psi.tree.IElementType

/**
 * Auto-pair `"` and `'` in VimL source via char-only scanning.
 *
 * VimL's quoting is the inverse of most languages: `'...'` is the *literal*
 * string (only `''` escapes) and `"..."` honors backslash escapes — but a
 * `"` in command position is a comment, not a string. The skip-over /
 * inside-literal logic here is purely positional so it behaves regardless
 * of which role a given `"` is playing.
 */
class VimlrsQuoteHandler : QuoteHandler {
    override fun isClosingQuote(iterator: HighlighterIterator, offset: Int): Boolean {
        val ch = charAt(iterator, offset) ?: return false
        if (!isQuoteChar(ch)) return false
        return matchingOpenBefore(iterator, offset, ch)
    }

    override fun isOpeningQuote(iterator: HighlighterIterator, offset: Int): Boolean {
        val ch = charAt(iterator, offset) ?: return false
        if (!isQuoteChar(ch)) return false
        return !matchingOpenBefore(iterator, offset, ch)
    }

    override fun hasNonClosedLiteral(
        editor: Editor,
        iterator: HighlighterIterator,
        offset: Int,
    ): Boolean = true

    override fun isInsideLiteral(iterator: HighlighterIterator): Boolean {
        val tt: IElementType? = iterator.tokenType
        return tt == VimlrsTokenTypes.STRING_DQ ||
                tt == VimlrsTokenTypes.STRING_SQ
    }

    private fun isQuoteChar(c: Char): Boolean = c == '"' || c == '\''

    private fun charAt(iterator: HighlighterIterator, offset: Int): Char? {
        val doc = iterator.document ?: return null
        if (offset < 0 || offset >= doc.textLength) return null
        return doc.charsSequence[offset]
    }

    private fun matchingOpenBefore(
        iterator: HighlighterIterator,
        offset: Int,
        quote: Char,
    ): Boolean {
        val doc = iterator.document ?: return false
        val text = doc.charsSequence
        var i = offset - 1
        while (i >= 0) {
            val c = text[i]
            if (c == '\n') return false
            // Single quotes have no backslash escaping in VimL; double
            // quotes do. Only honor backslash-escape for `"`.
            if (c == quote && !(quote == '"' && isEscaped(text, i))) return true
            i--
        }
        return false
    }

    private fun isEscaped(text: CharSequence, idx: Int): Boolean {
        var n = 0
        var i = idx - 1
        while (i >= 0 && text[i] == '\\') {
            n++; i--
        }
        return n % 2 == 1
    }
}
