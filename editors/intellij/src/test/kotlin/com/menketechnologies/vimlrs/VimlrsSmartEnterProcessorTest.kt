package com.menketechnologies.vimlrs

import com.menketechnologies.vimlrs.VimlrsSmartEnterProcessor.Companion.computePlan
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Pure JUnit 4 tests for [VimlrsSmartEnterProcessor.computePlan].
 *
 * In every test the `src` parameter uses `|` to mark the user's caret
 * position (stripped before the planner is called); `expected` inserts a
 * `|` where the caret should land after the plan applies.
 */
class VimlrsSmartEnterProcessorTest {
    private fun caretOf(src: String): Pair<String, Int> {
        val i = src.indexOf('|')
        require(i >= 0) { "test fixture must contain '|' for caret: $src" }
        return src.removeRange(i, i + 1) to i
    }

    private fun applyPlan(src: String): String {
        val (text, caret) = caretOf(src)
        val lineStart = text.lastIndexOf('\n', caret - 1).let { if (it < 0) 0 else it + 1 }
        val lineEnd = text.indexOf('\n', caret).let { if (it < 0) text.length else it }
        val line = text.substring(lineStart, lineEnd)
        val plan = computePlan(line, lineStart, caret, text)
            ?: error("expected a plan for: $src")
        val sb = StringBuilder(text)
        sb.insert(plan.offset, plan.insert)
        sb.insert(plan.offset + plan.caretRel, "|")
        return sb.toString()
    }

    private fun assertPlan(src: String, expected: String) {
        assertEquals(expected, applyPlan(src))
    }

    private fun assertNoPlan(src: String) {
        val (text, caret) = caretOf(src)
        val lineStart = text.lastIndexOf('\n', caret - 1).let { if (it < 0) 0 else it + 1 }
        val lineEnd = text.indexOf('\n', caret).let { if (it < 0) text.length else it }
        val line = text.substring(lineStart, lineEnd)
        assertNull(
            "expected no plan for: $src",
            computePlan(line, lineStart, caret, text),
        )
    }

    // ── Strategy 1: keyword-bracketed blocks ───────────────────────

    @Test fun if_completes_endif() {
        assertPlan(
            "if \$x ==# 'y'|",
            "if \$x ==# 'y'\n    |\nendif",
        )
    }

    @Test fun while_completes_endwhile() {
        assertPlan(
            "while line('.') < 100|",
            "while line('.') < 100\n    |\nendwhile",
        )
    }

    @Test fun for_completes_endfor() {
        assertPlan(
            "for item in items|",
            "for item in items\n    |\nendfor",
        )
    }

    @Test fun function_completes_endfunction() {
        assertPlan(
            "function! s:Greet(name)|",
            "function! s:Greet(name)\n    |\nendfunction",
        )
    }

    @Test fun try_completes_endtry() {
        assertPlan(
            "try|",
            "try\n    |\nendtry",
        )
    }

    @Test fun else_just_adds_body_line() {
        assertPlan(
            "else|",
            "else\n    |",
        )
    }

    @Test fun elseif_just_adds_body_line() {
        assertPlan(
            "elseif \$y|",
            "elseif \$y\n    |",
        )
    }

    @Test fun catch_just_adds_body_line() {
        assertPlan(
            "catch /E484/|",
            "catch /E484/\n    |",
        )
    }

    @Test fun finally_just_adds_body_line() {
        assertPlan(
            "finally|",
            "finally\n    |",
        )
    }

    @Test fun block_preserves_indent() {
        assertPlan(
            "    if \$x|",
            "    if \$x\n        |\n    endif",
        )
    }

    @Test fun block_skips_when_closer_already_present() {
        assertNoPlan(
            "if \$x|\nendif",
        )
    }

    @Test fun if_with_unclosed_paren_closes_the_paren_first() {
        // The header's `(` isn't balanced yet — bracket balance fires
        // instead of slamming an `endif` on a half-typed condition.
        assertPlan(
            "if (a && b|",
            "if (a && b)|",
        )
    }

    // ── Strategy 2: bracket balance ────────────────────────────────

    @Test fun unclosed_paren_closes() {
        assertPlan(
            "echo (1 + 2|",
            "echo (1 + 2)|",
        )
    }

    @Test fun unclosed_bracket_closes() {
        assertPlan(
            "let l = [1, 2|",
            "let l = [1, 2]|",
        )
    }

    @Test fun nested_brackets_close_in_order() {
        assertPlan(
            "echo (a + (b|",
            "echo (a + (b))|",
        )
    }

    @Test fun balanced_line_is_noop() {
        assertNoPlan("echo (1 + 2)|")
    }

    @Test fun open_brace_is_left_alone() {
        // Dict / block braces aren't auto-closed by this strategy.
        assertNoPlan("let d = {'a': 1|")
    }

    // ── Misc edge cases ────────────────────────────────────────────

    @Test fun comment_line_is_noop() {
        assertNoPlan("\" if block coming|")
    }
}
