package com.menketechnologies.vimlrs

import com.intellij.codeInsight.editorActions.smartEnter.SmartEnterProcessor
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.ScrollType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile

/**
 * Complete Current Statement (Cmd-Shift-Enter) for VimL source.
 *
 * Strategy chain, tried in priority order:
 *
 *  1. **Block header** — `if` / `while` / `for` / `function` / `try`.
 *     Inserts a body line and the matching closer (`endif` / `endwhile`
 *     / `endfor` / `endfunction` / `endtry`) and drops the caret in the
 *     body. VimL blocks are keyword-bracketed (`if … endif`, `function …
 *     endfunction`) with the condition on the SAME line — there is no
 *     `then` / `do` opener and no `{ … }` body braces, so each header
 *     just needs `\n  <body>\n  <closer>` appended.
 *     `else` / `elseif` / `catch` / `finally` get only a body line — the
 *     enclosing block owns the closer.
 *
 *  2. **Bracket balance** — unclosed `(` / `[` / `{` on the current line.
 *     Closing chars are appended (before any trailing `"` comment) and
 *     the caret lands just after them.
 *
 * Skipped (return false → platform default Enter): comment lines, lines
 * whose closer is already present further down, and anything else we can't
 * structurally recognise.
 *
 * Pure-function planner lives in [Companion.computePlan] so the test suite
 * can exercise every strategy without a platform fixture — see
 * [VimlrsSmartEnterProcessorTest].
 */
class VimlrsSmartEnterProcessor : SmartEnterProcessor() {
    override fun process(project: Project, editor: Editor, file: PsiFile): Boolean {
        if (file.fileType !is VimlrsFileType) return false

        val doc = editor.document
        val caret = editor.caretModel.offset
        val text = doc.charsSequence

        val lineNum = doc.getLineNumber(caret)
        val lineStart = doc.getLineStartOffset(lineNum)
        val lineEnd = doc.getLineEndOffset(lineNum)
        val line = text.subSequence(lineStart, lineEnd).toString()

        val plan = computePlan(line, lineStart, caret, text) ?: return false

        doc.insertString(plan.offset, plan.insert)
        commit(editor)
        editor.caretModel.moveToOffset(plan.offset + plan.caretRel)
        editor.scrollingModel.scrollToCaret(ScrollType.RELATIVE)
        return true
    }

    companion object {
        /** Computed edit: insert [insert] at [offset], caret to [offset]+[caretRel]. */
        data class Plan(val offset: Int, val insert: String, val caretRel: Int)

        /**
         * Pure function: given a VimL source line, return the edit plan that
         * completes its statement, or `null` if no strategy matches. Offsets
         * are absolute (`lineStart`-based) so the caller can apply directly
         * to the document.
         */
        fun computePlan(line: String, lineStart: Int, caret: Int, text: CharSequence): Plan? {
            val trimmed = line.trimStart()
            // A leading `"` is a comment in VimL.
            if (trimmed.startsWith("\"")) return null

            tryBlock(line, lineStart, text)?.let { return it }
            tryBracketBalance(line, lineStart)?.let { return it }
            return null
        }

        // ── Strategy 1: keyword-bracketed blocks ─────────────────────

        private fun tryBlock(line: String, lineStart: Int, text: CharSequence): Plan? {
            val trimmed = line.trimStart()
            val kw = BLOCK_KEYWORD.matchAt(trimmed, 0)?.value ?: return null
            val indent = leadingIndent(line)

            // `else` / `elseif` / `catch` / `finally` — body line only; the
            // enclosing block's closer is the user's responsibility.
            if (kw in BODY_ONLY) {
                val afterLine = lineStart + line.trimEnd().length
                val body = "\n$indent    "
                return Plan(afterLine, body, body.length)
            }

            val closer = closerFor(kw) ?: return null
            // Don't slam a closer on when the user already has one below, and
            // don't complete a header whose condition `(`/`[` is still open.
            if (lineHasUnclosedBracket(line)) return null
            if (followingKeywordPresent(text, lineStart, line, closer)) return null

            val afterLine = lineStart + line.trimEnd().length
            val insert = "\n$indent    \n$indent$closer"
            // "\n" + indent + 4-space body indent → caret on the body line.
            val caretRel = 1 + indent.length + 4
            return Plan(afterLine, insert, caretRel)
        }

        /** Matching closer for a header keyword, or null for body-only kws. */
        private fun closerFor(kw: String): String? = when {
            kw == "if" -> "endif"
            kw == "while" -> "endwhile"
            kw == "for" -> "endfor"
            kw == "try" -> "endtry"
            kw.startsWith("function") -> "endfunction"
            kw.startsWith("func") -> "endfunc"
            else -> null
        }

        // ── Strategy 2: bracket balance ──────────────────────────────

        private fun tryBracketBalance(line: String, lineStart: Int): Plan? {
            val stack = ArrayDeque<Char>()
            var i = 0
            while (i < line.length) {
                val c = line[i]
                when (c) {
                    '(' -> stack.addLast(')')
                    '[' -> stack.addLast(']')
                    '{' -> stack.addLast('}')
                    ')', ']', '}' -> if (stack.lastOrNull() == c) stack.removeLast()
                    '\'' -> {
                        i++
                        while (i < line.length && line[i] != '\'') i++
                    }
                    '"' -> {
                        // Leading `"` (only whitespace before it) is a comment.
                        if (line.substring(0, i).isBlank()) break
                        i++
                        while (i < line.length && line[i] != '"') {
                            if (line[i] == '\\' && i + 1 < line.length) i++
                            i++
                        }
                    }
                }
                i++
            }
            if (stack.isEmpty()) return null
            // Don't insert `}` here — leave dict/block braces alone for now.
            val closers = stack.reversed().joinToString("")
            if (closers.any { it == '}' }) return null
            val anchor = lineStart + lengthBeforeTrailingComment(line)
            return Plan(anchor, closers, closers.length)
        }

        // ── Shared helpers ───────────────────────────────────────────

        /** True when [line] contains any unmatched `(` / `[` / `{`. */
        private fun lineHasUnclosedBracket(line: String): Boolean =
            tryBracketBalance(line, 0) != null

        /**
         * True when [word] appears as a standalone token at the start of any
         * following line in [text] before EOF — used to suppress closer
         * insertion when the user has already typed `endif` / `endfor` /
         * `endfunction` on a later line.
         */
        private fun followingKeywordPresent(
            text: CharSequence,
            lineStart: Int,
            line: String,
            word: String,
        ): Boolean {
            val from = lineStart + line.length
            var i = from
            while (i < text.length) {
                while (i < text.length && (text[i] == ' ' || text[i] == '\t')) i++
                if (i + word.length <= text.length &&
                    text.subSequence(i, i + word.length).toString() == word
                ) {
                    val afterEnd = i + word.length
                    val rightOk = afterEnd == text.length ||
                        !text[afterEnd].isLetterOrDigit() && text[afterEnd] != '_'
                    if (rightOk) return true
                }
                while (i < text.length && text[i] != '\n') i++
                if (i < text.length) i++
            }
            return false
        }

        /** Length of [line] up to the start of any trailing `" ...` comment. */
        private fun lengthBeforeTrailingComment(line: String): Int {
            var i = 0
            while (i < line.length) {
                val c = line[i]
                when (c) {
                    '\'' -> {
                        i++
                        while (i < line.length && line[i] != '\'') i++
                        if (i < line.length) i++
                        continue
                    }
                    '"' -> {
                        // Command-position `"` (only whitespace before) is a
                        // trailing comment; strip preceding whitespace.
                        if (line.substring(0, i).isBlank()) {
                            var j = i
                            while (j > 0 && (line[j - 1] == ' ' || line[j - 1] == '\t')) j--
                            return j
                        }
                        // Otherwise a string literal — skip it.
                        i++
                        while (i < line.length && line[i] != '"') {
                            if (line[i] == '\\' && i + 1 < line.length) i++
                            i++
                        }
                        if (i < line.length) i++
                        continue
                    }
                }
                i++
            }
            return line.trimEnd().length
        }

        private fun leadingIndent(line: String): String {
            val end = line.indexOfFirst { it != ' ' && it != '\t' }
            return if (end < 0) line else line.substring(0, end)
        }

        // ── Keyword tables ──────────────────────────────────────────

        /** Block-header keywords this strategy recognises at line start. */
        private val BLOCK_KEYWORD = Regex(
            "(if|elseif|else|while|for|function!?|func!?|try|catch|finally)\\b",
        )

        /** Header keywords that only get a body line (no closer of their own). */
        private val BODY_ONLY = setOf("elseif", "else", "catch", "finally")
    }
}
