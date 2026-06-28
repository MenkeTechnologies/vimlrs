package com.menketechnologies.vimlrs

import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType
import org.junit.Assert.*
import org.junit.Test

/**
 * Unit tests for [VimlrsLexer]. These run under `./gradlew test` and
 * exercise the hand-rolled tokenizer that feeds the syntax highlighter
 * before the LSP semantic-tokens response lands.
 */
class VimlrsLexerTest {

    private fun tokens(src: String): List<Pair<IElementType?, String>> {
        val lex = VimlrsLexer()
        lex.start(src, 0, src.length, 0)
        val out = mutableListOf<Pair<IElementType?, String>>()
        while (lex.tokenType != null) {
            val t = lex.tokenType
            val s = src.substring(lex.tokenStart, lex.tokenEnd)
            out += t to s
            lex.advance()
        }
        return out
    }

    private fun nonWs(src: String) = tokens(src).filter { it.first != TokenType.WHITE_SPACE }

    @Test fun `shebang on first line is its own token`() {
        val toks = nonWs("#!/usr/bin/env vimlrs\necho 1\n")
        assertEquals(VimlrsTokenTypes.SHEBANG, toks[0].first)
        assertTrue(toks[0].second.startsWith("#!"))
    }

    @Test fun `leading double-quote is a comment to end of line`() {
        val toks = nonWs("\" this is a comment with \"quotes\" inside\nlet x = 1\n")
        assertEquals(VimlrsTokenTypes.COMMENT, toks[0].first)
        // The whole line (including inner quotes) is the comment.
        assertTrue(toks[0].second.contains("inside"))
        // Code on the next line still lexes.
        assertTrue(toks.any { it.first == VimlrsTokenTypes.KEYWORD && it.second == "let" })
    }

    @Test fun `double-quote after a bar is a comment`() {
        // After a `|` command separator we're back in command position, so a
        // following `"` opens a comment (not a string).
        val toks = nonWs("echo 1 | \" trailing note\n")
        assertTrue(
            "expected COMMENT after the bar: $toks",
            toks.any { it.first == VimlrsTokenTypes.COMMENT && it.second.contains("trailing note") },
        )
        assertTrue(toks.any { it.first == VimlrsTokenTypes.BAR && it.second == "|" })
    }

    @Test fun `double-quote in expression position is a string`() {
        val toks = nonWs("let s = \"hello\"")
        assertTrue(
            "expected STRING_DQ for \"hello\": $toks",
            toks.any { it.first == VimlrsTokenTypes.STRING_DQ && it.second == "\"hello\"" },
        )
    }

    @Test fun `single-quoted string is literal with doubled-quote escape`() {
        val toks = nonWs("let s = 'it''s here'")
        val str = toks.first { it.first == VimlrsTokenTypes.STRING_SQ }
        assertEquals("'it''s here'", str.second)
    }

    @Test fun `control keywords classify as KEYWORD`() {
        for ((tt, _) in nonWs("if elseif else endif while endwhile for endfor return try catch finally endtry")) {
            assertEquals(VimlrsTokenTypes.KEYWORD, tt)
        }
    }

    @Test fun `let unlet const call echo are keywords`() {
        for ((tt, _) in nonWs("let unlet const call echo echomsg execute throw")) {
            assertEquals(VimlrsTokenTypes.KEYWORD, tt)
        }
    }

    @Test fun `ex commands classify as COMMAND`() {
        for ((tt, _) in nonWs("set setlocal autocmd augroup nnoremap highlight syntax source silent")) {
            assertEquals(VimlrsTokenTypes.COMMAND, tt)
        }
    }

    @Test fun `let assignment to a global scope var`() {
        val toks = nonWs("let g:loaded = 1")
        assertEquals(VimlrsTokenTypes.KEYWORD, toks[0].first)
        assertEquals(VimlrsTokenTypes.SCOPE_VAR, toks[1].first)
        assertEquals("g:loaded", toks[1].second)
        assertEquals(VimlrsTokenTypes.ASSIGN_OP, toks[2].first)
        assertEquals(VimlrsTokenTypes.NUMBER, toks[3].first)
    }

    @Test fun `scope prefixes all lex as one SCOPE_VAR token`() {
        for (v in listOf("g:x", "s:y", "b:z", "w:a", "t:b", "l:c", "a:000")) {
            val toks = nonWs(v)
            assertEquals("scope-var lookup failed for $v", VimlrsTokenTypes.SCOPE_VAR, toks[0].first)
            assertEquals(v, toks[0].second)
        }
    }

    @Test fun `v-colon specials classify as SPECIAL_VAR`() {
        for (v in listOf("v:true", "v:false", "v:null", "v:count", "v:val", "v:shell_error", "v:exception")) {
            val toks = nonWs(v)
            assertEquals("special-var lookup failed for $v", VimlrsTokenTypes.SPECIAL_VAR, toks[0].first)
        }
    }

    @Test fun `v-colon non-special is a plain scope var`() {
        // `v:foo` is not a predefined special, so it's a SCOPE_VAR.
        val toks = nonWs("v:foo")
        assertEquals(VimlrsTokenTypes.SCOPE_VAR, toks[0].first)
    }

    @Test fun `function declaration name colors as FUNCTION_DECL`() {
        // `function!` keyword, then `!`, then the declared name.
        val toks = nonWs("function! s:Greet(name)")
        assertEquals(VimlrsTokenTypes.KEYWORD, toks[0].first)
        assertEquals("function", toks[0].second)
        assertTrue(
            "declared name should be FUNCTION_DECL: $toks",
            toks.any { it.first == VimlrsTokenTypes.FUNCTION_DECL && it.second == "s:Greet" },
        )
    }

    @Test fun `unqualified function name colors as FUNCTION_DECL`() {
        val toks = nonWs("function Foo()")
        assertTrue(
            "declared name should be FUNCTION_DECL: $toks",
            toks.any { it.first == VimlrsTokenTypes.FUNCTION_DECL && it.second == "Foo" },
        )
    }

    @Test fun `builtin function before paren classifies as BUILTIN_FUNCTION`() {
        val toks = nonWs("call len(x)")
        assertTrue(
            "expected BUILTIN_FUNCTION for len(: $toks",
            toks.any { it.first == VimlrsTokenTypes.BUILTIN_FUNCTION && it.second == "len" },
        )
    }

    @Test fun `function used as funcref builtin is not a declaration`() {
        // `function('Foo')` is the funcref builtin, not a `:function` decl.
        val toks = nonWs("let F = function('Foo')")
        assertTrue(
            "function( should be BUILTIN_FUNCTION: $toks",
            toks.any { it.first == VimlrsTokenTypes.BUILTIN_FUNCTION && it.second == "function" },
        )
    }

    @Test fun `builtin name not followed by paren stays identifier`() {
        // `len` as a bare word (no paren) is just an identifier.
        val toks = nonWs("echo len")
        assertTrue(
            "bare len should be IDENTIFIER: $toks",
            toks.any { it.first == VimlrsTokenTypes.IDENTIFIER && it.second == "len" },
        )
    }

    @Test fun `autoload call colors the qualified name as FUNCTION_DECL`() {
        val toks = nonWs("call plug#begin('~/.vim/plugged')")
        assertTrue(
            "autoload name should be FUNCTION_DECL: $toks",
            toks.any { it.first == VimlrsTokenTypes.FUNCTION_DECL && it.second == "plug#begin" },
        )
    }

    @Test fun `option reference lexes as OPTION`() {
        for (o in listOf("&number", "&l:textwidth", "&g:foldlevel")) {
            val toks = nonWs(o)
            assertEquals("option lookup failed for $o", VimlrsTokenTypes.OPTION, toks[0].first)
            assertEquals(o, toks[0].second)
        }
    }

    @Test fun `environment variable lexes as ENV_VAR`() {
        val toks = nonWs("let p = \$HOME")
        assertTrue(
            "expected ENV_VAR for \$HOME: $toks",
            toks.any { it.first == VimlrsTokenTypes.ENV_VAR && it.second == "\$HOME" },
        )
    }

    @Test fun `register reference lexes as REGISTER`() {
        val toks = nonWs("let @a = 'x'")
        assertTrue(
            "expected REGISTER for @a: $toks",
            toks.any { it.first == VimlrsTokenTypes.REGISTER && it.second == "@a" },
        )
    }

    @Test fun `numbers decimal hex binary and float`() {
        assertEquals(VimlrsTokenTypes.NUMBER, nonWs("42")[0].first)
        assertEquals("0x1F", nonWs("0x1F")[0].second)
        assertEquals(VimlrsTokenTypes.NUMBER, nonWs("0x1F")[0].first)
        assertEquals(VimlrsTokenTypes.NUMBER, nonWs("0b1010")[0].first)
        assertEquals("3.14", nonWs("3.14")[0].second)
        assertEquals("1.0e3", nonWs("1.0e3")[0].second)
    }

    @Test fun `bar is its own token and logical-or is operator`() {
        val toks = nonWs("echo 1 | echo 2")
        assertTrue("expected BAR: $toks", toks.any { it.first == VimlrsTokenTypes.BAR && it.second == "|" })
        val toks2 = nonWs("if a || b")
        assertTrue("expected || OPERATOR: $toks2", toks2.any { it.first == VimlrsTokenTypes.OPERATOR && it.second == "||" })
    }

    @Test fun `comparison operators with case flags`() {
        val toks = nonWs("if a ==# b && c =~? d")
        val ops = toks.filter { it.first == VimlrsTokenTypes.OPERATOR }.map { it.second }
        assertTrue("expected ==# in $ops", ops.contains("==#"))
        assertTrue("expected =~? in $ops", ops.contains("=~?"))
        assertTrue("expected && in $ops", ops.contains("&&"))
    }

    @Test fun `string concat dot-dot is an operator`() {
        val toks = nonWs("let s = a .. b")
        assertTrue(
            "expected .. OPERATOR: $toks",
            toks.any { it.first == VimlrsTokenTypes.OPERATOR && it.second == ".." },
        )
    }

    @Test fun `compound assignment operators`() {
        for (op in listOf("+=", "-=", ".=")) {
            val toks = nonWs("let x $op 1")
            assertTrue(
                "expected ASSIGN_OP for $op: $toks",
                toks.any { it.first == VimlrsTokenTypes.ASSIGN_OP && it.second == op },
            )
        }
    }

    @Test fun `leading backslash is a line continuation`() {
        val src = "let l = [\n\\ 1,\n\\ 2,\n\\ ]\n"
        val toks = tokens(src)
        val conts = toks.filter { it.first == VimlrsTokenTypes.LINE_CONTINUATION }
        assertEquals("expected three line-continuation backslashes: $toks", 3, conts.size)
        conts.forEach { assertEquals("\\", it.second) }
    }

    @Test fun `representative sample produces no bad characters`() {
        val src = """
            #!/usr/bin/env vimlrs
            " greet plugin
            let g:loaded_greet = v:true
            function! s:greet(who) abort
                let l:msg = printf("hi %s", a:who)
                echomsg l:msg .. ' (' .. getpid() .. ')'
                return v:true
            endfunction
            nnoremap <silent> <leader>g :call <SID>greet('world')<CR>
        """.trimIndent() + "\n"
        val toks = tokens(src)
        assertFalse(
            "sample produced BAD_CHARACTER tokens: ${toks.filter { it.first == TokenType.BAD_CHARACTER }}",
            toks.any { it.first == TokenType.BAD_CHARACTER },
        )
    }

    @Test fun `brackets lex as distinct L and R tokens`() {
        val toks = nonWs("let d = {'a': [1, 2]}")
        val types = toks.map { it.first }
        assertTrue(types.contains(VimlrsTokenTypes.LBRACE))
        assertTrue(types.contains(VimlrsTokenTypes.RBRACE))
        assertTrue(types.contains(VimlrsTokenTypes.LBRACKET))
        assertTrue(types.contains(VimlrsTokenTypes.RBRACKET))
        assertTrue(types.contains(VimlrsTokenTypes.COMMA))
    }
}
