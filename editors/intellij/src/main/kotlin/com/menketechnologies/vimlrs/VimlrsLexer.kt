package com.menketechnologies.vimlrs

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

/**
 * Hand-rolled VimL (Vimscript) lexer. Recognizes:
 *
 *   * `"` line comments (ONLY in command position) + `#!` shebang on line 1
 *   * single-quoted `'...'` (only `''` escapes) and double-quoted `"..."`
 *     (backslash escapes) strings — the classic VimL ambiguity is resolved
 *     pragmatically: a `"` that begins a statement/line (or follows `|`) is
 *     a comment, otherwise it opens a string (mirrors Vim's own runtime
 *     syntax)
 *   * numbers — decimal, `0x` hex, `0b` binary, floats (`1.5`, `1.0e3`)
 *   * statement / control keywords (`if`/`endif`/`function`/`let`/`call`/…)
 *   * common ex commands (`set`/`autocmd`/`nnoremap`/`highlight`/`syntax`/…)
 *   * scope-prefixed names `g:` `s:` `b:` `w:` `t:` `l:` `a:` `v:`
 *   * predefined `v:` specials (`v:true`/`v:count`/`v:shell_error`/…)
 *   * options `&name` / `&l:name`, env vars `$NAME`, registers `@x`
 *   * built-in functions (`len(`, `has(`, `printf(` …) — only before `(`
 *   * autoload calls `foo#bar(` (colored as a function decl)
 *   * operators (`==` `!=` `=~` `..` `->` `+=` …), brackets, comma, `|` bar,
 *     and the line-continuation `\` at the start of a continued line
 *
 * The LSP server (`vimlrs --lsp`) overlays semantic tokens; this lexer
 * provides instant feedback before the LSP turn-around.
 */
class VimlrsLexer : LexerBase() {
    private var buf: CharSequence = ""
    private var endOffset = 0
    private var pos = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null
    private var state = 0

    /// Set true right after we emit the `function` keyword (and kept set
    /// across a following `!` of `function!` and intervening whitespace).
    /// The next identifier / scope-var / autoload name token is then
    /// classified as FUNCTION_DECL rather than a plain IDENTIFIER, so the
    /// declared name pops with the declaration color. Cleared once the
    /// name is consumed, or on a newline (a `function` with no name on
    /// its line is malformed and shouldn't poison the next line).
    private var expectFunctionName = false

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        buf = buffer
        this.endOffset = endOffset
        pos = startOffset
        state = initialState
        expectFunctionName = false
        advance()
    }

    override fun getState(): Int = state
    override fun getTokenType(): IElementType? = tokenType
    override fun getTokenStart(): Int = tokenStart
    override fun getTokenEnd(): Int = tokenEnd
    override fun getBufferSequence(): CharSequence = buf
    override fun getBufferEnd(): Int = endOffset

    override fun advance() {
        tokenStart = pos
        if (pos >= endOffset) {
            tokenType = null
            tokenEnd = pos
            return
        }
        val c = buf[pos]
        when {
            // Shebang on line 1 takes priority over the `"` comment form.
            c == '#' && pos == 0 && peek(1) == '!' -> consumeShebang()
            // `"` — comment in command position, string otherwise. This is
            // the central VimL ambiguity (`:h :comment`); resolved by
            // [isCommentStart] the same way Vim's runtime syntax does.
            c == '"' && isCommentStart() -> consumeLineComment()
            c == '"' -> consumeDoubleQuotedString()
            c == '\'' -> consumeSingleQuotedString()
            c == '\n' || c == '\r' || c == ' ' || c == '\t' -> consumeWhitespace()
            // Line-continuation `\` at the start of a continued line — a
            // leading backslash (after only whitespace since BOL) folds the
            // line onto the previous one (`:h line-continuation`).
            c == '\\' && isLineContinuationStart() -> emit(1, VimlrsTokenTypes.LINE_CONTINUATION)
            // Scope-prefixed variable: `g:` `s:` `b:` `w:` `t:` `l:` `a:`
            // `v:` followed by an identifier char or digit (`a:0`).
            c in SCOPE_LETTERS && peek(1) == ':' && isScopeNameStart(peek(2)) -> consumeScopedVar()
            c == '$' && isIdentStart(peek(1)) -> consumeEnvVar()
            c == '&' && peek(1) == '&' -> emit(2, VimlrsTokenTypes.OPERATOR)
            c == '&' && isOptionStart() -> consumeOption()
            c == '@' && peek(1) != ' ' && peek(1) != '\t' && peek(1) != '\n' &&
                peek(1) != '\r' && pos + 1 < endOffset -> emit(2, VimlrsTokenTypes.REGISTER)
            c == '|' && peek(1) == '|' -> emit(2, VimlrsTokenTypes.OPERATOR)
            c == '|' -> emit(1, VimlrsTokenTypes.BAR)
            c == '(' -> emit(1, VimlrsTokenTypes.LPAREN)
            c == ')' -> emit(1, VimlrsTokenTypes.RPAREN)
            c == '{' -> emit(1, VimlrsTokenTypes.LBRACE)
            c == '}' -> emit(1, VimlrsTokenTypes.RBRACE)
            c == '[' -> emit(1, VimlrsTokenTypes.LBRACKET)
            c == ']' -> emit(1, VimlrsTokenTypes.RBRACKET)
            c == ',' -> emit(1, VimlrsTokenTypes.COMMA)
            c.isDigit() -> consumeNumber()
            c == '_' || c.isLetter() -> consumeWord()
            isOperatorStart(c) -> consumeOperator()
            else -> emit(1, TokenType.BAD_CHARACTER)
        }
    }

    private fun peek(off: Int): Char = if (pos + off in 0 until endOffset) buf[pos + off] else ' '

    private fun emit(len: Int, tt: IElementType) {
        tokenEnd = (pos + len).coerceAtMost(endOffset)
        pos = tokenEnd
        tokenType = tt
    }

    private fun consumeShebang() {
        var p = pos
        while (p < endOffset && buf[p] != '\n') p++
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.SHEBANG
    }

    /// A `"` opens a comment only when it is the FIRST non-blank char of a
    /// command — i.e. at beginning-of-line (after optional indent) or right
    /// after a `|` command separator. Anywhere else it's a double-quoted
    /// string. Scan back over spaces/tabs from `pos-1`: BOL or `|` ⇒ comment.
    private fun isCommentStart(): Boolean {
        var i = pos - 1
        while (i >= 0) {
            val ch = buf[i]
            if (ch == ' ' || ch == '\t') { i--; continue }
            return ch == '\n' || ch == '\r' || ch == '|'
        }
        return true // reached beginning of buffer
    }

    private fun consumeLineComment() {
        var p = pos
        while (p < endOffset && buf[p] != '\n') p++
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.COMMENT
    }

    /// True when the `\` at `pos` is a line continuation — preceded only by
    /// whitespace back to the start of the line (or buffer).
    private fun isLineContinuationStart(): Boolean {
        var i = pos - 1
        while (i >= 0) {
            val ch = buf[i]
            if (ch == ' ' || ch == '\t') { i--; continue }
            return ch == '\n' || ch == '\r'
        }
        return true
    }

    private fun consumeWhitespace() {
        var p = pos
        while (p < endOffset && (buf[p] == ' ' || buf[p] == '\t' || buf[p] == '\n' || buf[p] == '\r')) {
            // A newline ends any pending `function`-name expectation.
            if (buf[p] == '\n') expectFunctionName = false
            p++
        }
        tokenEnd = p; pos = p
        tokenType = TokenType.WHITE_SPACE
    }

    /// `"..."` — double-quoted string with backslash escapes. `\"` does NOT
    /// close the string. Emitted as a single STRING_DQ token.
    private fun consumeDoubleQuotedString() {
        var p = pos + 1
        while (p < endOffset) {
            val ch = buf[p]
            if (ch == '\n') break
            if (ch == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (ch == '"') { p++; break }
            p++
        }
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.STRING_DQ
    }

    /// `'...'` — literal string. The ONLY escape is `''` (a doubled single
    /// quote denotes one literal quote); backslashes are literal text.
    private fun consumeSingleQuotedString() {
        var p = pos + 1
        while (p < endOffset) {
            val ch = buf[p]
            if (ch == '\n') break
            if (ch == '\'') {
                if (p + 1 < endOffset && buf[p + 1] == '\'') { p += 2; continue } // '' → literal '
                p++; break
            }
            p++
        }
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.STRING_SQ
    }

    private fun isScopeNameStart(ch: Char): Boolean = ch == '_' || ch.isLetterOrDigit()

    /// `g:foo` / `s:Bar` / `b:changedtick` / `a:000` / `v:true`. The whole
    /// `scope:name` is one token. A `v:`-scoped name that is a known
    /// predefined special (`v:true`, `v:count`, …) gets SPECIAL_VAR; every
    /// other scope is SCOPE_VAR. Honors a pending `function`-name request
    /// (`function s:Foo()` → the name colors as a declaration).
    private fun consumeScopedVar() {
        val scope = buf[pos]
        var p = pos + 2 // past `X:`
        while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
        val text = buf.subSequence(pos, p).toString()
        tokenEnd = p; pos = p
        if (expectFunctionName) {
            expectFunctionName = false
            tokenType = VimlrsTokenTypes.FUNCTION_DECL
            return
        }
        tokenType = if (scope == 'v' && text in SPECIAL_VARS) {
            VimlrsTokenTypes.SPECIAL_VAR
        } else {
            VimlrsTokenTypes.SCOPE_VAR
        }
    }

    private fun consumeEnvVar() {
        var p = pos + 1 // past `$`
        while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.ENV_VAR
    }

    /// True when `&` at `pos` opens an option reference (`&number`,
    /// `&l:textwidth`, `&g:foo`) rather than a bare `&` operator.
    private fun isOptionStart(): Boolean {
        val n1 = peek(1)
        if (n1 == '_' || n1.isLetter()) {
            // `&l:` / `&g:` long-scope form is still option-start.
            return true
        }
        return false
    }

    private fun consumeOption() {
        var p = pos + 1 // past `&`
        // optional `l:` / `g:` scope on the option
        if (p + 1 < endOffset && (buf[p] == 'l' || buf[p] == 'g') && buf[p + 1] == ':') p += 2
        while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.OPTION
    }

    private fun consumeNumber() {
        var p = pos
        if (buf[p] == '0' && p + 1 < endOffset && (buf[p + 1] == 'x' || buf[p + 1] == 'X')) {
            p += 2
            while (p < endOffset && (buf[p].isDigit() || buf[p] in 'a'..'f' || buf[p] in 'A'..'F')) p++
            tokenEnd = p; pos = p
            tokenType = VimlrsTokenTypes.NUMBER
            return
        }
        if (buf[p] == '0' && p + 1 < endOffset && (buf[p + 1] == 'b' || buf[p + 1] == 'B')) {
            p += 2
            while (p < endOffset && (buf[p] == '0' || buf[p] == '1')) p++
            tokenEnd = p; pos = p
            tokenType = VimlrsTokenTypes.NUMBER
            return
        }
        while (p < endOffset && buf[p].isDigit()) p++
        // float fraction (`1.5`) — require a digit after the `.` so `dict.key`
        // dotted access doesn't get swallowed into the number.
        if (p < endOffset && buf[p] == '.' && p + 1 < endOffset && buf[p + 1].isDigit()) {
            p++
            while (p < endOffset && buf[p].isDigit()) p++
        }
        // exponent (`1.0e3`, `2e-5`)
        if (p < endOffset && (buf[p] == 'e' || buf[p] == 'E')) {
            var q = p + 1
            if (q < endOffset && (buf[q] == '+' || buf[q] == '-')) q++
            if (q < endOffset && buf[q].isDigit()) {
                p = q
                while (p < endOffset && buf[p].isDigit()) p++
            }
        }
        tokenEnd = p; pos = p
        tokenType = VimlrsTokenTypes.NUMBER
    }

    private fun consumeWord() {
        var p = pos
        var sawHash = false
        while (p < endOffset) {
            val ch = buf[p]
            if (ch == '_' || ch.isLetterOrDigit()) { p++; continue }
            // `#` is the autoload separator (`foo#bar#Baz`) — only swallow
            // it when flanked by word chars so a stand-alone `#` (the
            // `:number` ex command, or a count) stays its own token.
            if (ch == '#' && p + 1 < endOffset && (buf[p + 1] == '_' || buf[p + 1].isLetter())) {
                sawHash = true; p += 2; continue
            }
            break
        }
        val word = buf.subSequence(pos, p).toString()
        val nextIsParen = p < endOffset && buf[p] == '('
        tokenEnd = p; pos = p
        tokenType = classifyWord(word, nextIsParen, sawHash)
    }

    private fun classifyWord(word: String, nextIsParen: Boolean, hasHash: Boolean): IElementType {
        // A pending `function NAME` declaration claims this word as the
        // declared name (unless it's the `function(` builtin funcref call).
        if (expectFunctionName) {
            expectFunctionName = false
            return VimlrsTokenTypes.FUNCTION_DECL
        }
        // `function(`/`func(` used as the funcref builtin — not a decl.
        if (nextIsParen && word in BUILTIN_FUNCTIONS) return VimlrsTokenTypes.BUILTIN_FUNCTION
        // Autoload call `plug#begin(` — color the qualified name as a decl.
        if (nextIsParen && hasHash) return VimlrsTokenTypes.FUNCTION_DECL
        if (word in KEYWORDS) {
            // Arm the function-name expectation for the next token; the
            // `function(` funcref case was already handled above.
            if (word == "function" || word == "func") expectFunctionName = true
            return VimlrsTokenTypes.KEYWORD
        }
        if (word in COMMANDS) return VimlrsTokenTypes.COMMAND
        return VimlrsTokenTypes.IDENTIFIER
    }

    private fun isOperatorStart(c: Char): Boolean = c in "=!<>+-*/%.?:~^#$&@\\"

    /// Greedy operator matcher. Handles the multi-char VimL operators
    /// (comparisons with optional `#`/`?` case flags, the `..` concat,
    /// arrow `->`, and the compound-assignment family) before falling back
    /// to single-char punctuation.
    private fun consumeOperator() {
        val c = buf[pos]
        val n1 = peek(1)
        val n2 = peek(2)
        // Comparison operators: `==` `!=` `=~` `!~` `>=` `<=` `>` `<`, each
        // with an optional trailing `#` (match-case) or `?` (ignore-case).
        if ((c == '=' && (n1 == '=' || n1 == '~')) ||
            (c == '!' && (n1 == '=' || n1 == '~')) ||
            ((c == '>' || c == '<') && n1 == '=')
        ) {
            var len = 2
            if (n2 == '#' || n2 == '?') len = 3
            emit(len, VimlrsTokenTypes.OPERATOR)
            return
        }
        when {
            c == '&' && n1 == '&' -> emit(2, VimlrsTokenTypes.OPERATOR)
            c == '-' && n1 == '>' -> emit(2, VimlrsTokenTypes.OPERATOR)         // method/lambda arrow
            c == '.' && n1 == '.' && n2 == '=' -> emit(3, VimlrsTokenTypes.ASSIGN_OP) // Vim9 ..=
            c == '.' && n1 == '.' -> emit(2, VimlrsTokenTypes.OPERATOR)         // string concat
            c == '.' && n1 == '=' -> emit(2, VimlrsTokenTypes.ASSIGN_OP)
            c == '=' -> emit(1, VimlrsTokenTypes.ASSIGN_OP)
            (c == '+' || c == '-' || c == '*' || c == '/' || c == '%') && n1 == '=' ->
                emit(2, VimlrsTokenTypes.ASSIGN_OP)
            c == '<' || c == '>' -> emit(1, VimlrsTokenTypes.OPERATOR)
            else -> emit(1, VimlrsTokenTypes.OPERATOR)
        }
    }

    private fun isIdentStart(ch: Char): Boolean = ch == '_' || ch.isLetter()

    companion object {
        const val STATE_NORMAL = 0

        private val SCOPE_LETTERS = setOf('g', 's', 'b', 'w', 't', 'l', 'a', 'v')

        /// Predefined `v:` variables (`:h vim-variable`). Only the
        /// commonly-seen subset the LSP also documents.
        private val SPECIAL_VARS = setOf(
            "v:true", "v:false", "v:null", "v:none", "v:count", "v:count1",
            "v:version", "v:val", "v:key", "v:exception", "v:throwpoint",
            "v:lnum", "v:errmsg", "v:shell_error", "v:this_session", "v:char",
            "v:register", "v:servername",
        )

        /// Statement / control keywords. `function` (and `func`) additionally
        /// arm the declared-name highlighting in [classifyWord].
        private val KEYWORDS = setOf(
            "if", "elseif", "else", "endif",
            "while", "endwhile",
            "for", "endfor", "in",
            "function", "endfunction", "func", "endfunc",
            "return", "break", "continue",
            "try", "catch", "finally", "endtry", "throw",
            "let", "unlet", "const", "lockvar", "unlockvar",
            "call", "eval", "execute",
            "echo", "echon", "echohl", "echomsg", "echoerr", "echowindow",
            "finish",
        )

        /// Common ex commands that earn their own color slot.
        private val COMMANDS = setOf(
            "source", "runtime", "normal", "redir", "silent", "verbose",
            "set", "setlocal", "setglobal",
            "command", "delcommand",
            "autocmd", "augroup",
            "highlight", "syntax",
            "map", "nmap", "imap", "vmap", "xmap",
            "noremap", "nnoremap", "inoremap", "vnoremap", "xnoremap", "cnoremap",
            "abbreviate", "sign", "sleep",
        )

        /// VimL built-in functions — colored only when immediately followed
        /// by `(`. Curated from `:h builtin-function-list` (a representative
        /// canonical subset; the LSP refines the rest via semantic tokens).
        private val BUILTIN_FUNCTIONS = setOf(
            "abs", "add", "and", "append", "argc", "call", "ceil", "char2nr",
            "copy", "cos", "count", "deepcopy", "empty", "escape", "eval",
            "execute", "exists", "extend", "filter", "float2nr", "floor",
            "fmod", "fnameescape", "function", "get", "getenv", "getpid",
            "has", "has_key", "index", "input", "insert", "invert", "isinf",
            "isnan", "items", "join", "json_decode", "json_encode", "keys",
            "len", "line", "localtime", "map", "match", "matchstr", "max",
            "min", "nr2char", "or", "pathshorten", "pow", "printf", "rand",
            "range", "reduce", "reltime", "reltimefloat", "reltimestr",
            "remove", "repeat", "reverse", "round", "setenv", "sha256",
            "shellescape", "sin", "sort", "soundfold", "split", "sqrt",
            "srand", "str2float", "str2nr", "strcharpart", "strftime",
            "string", "strlen", "strpart", "strptime", "strridx", "strtrans",
            "submatch", "substitute", "tolower", "toupper", "trim", "type",
            "values", "xor", "flatten", "flattennew", "list2blob", "blob2list",
        )
    }
}
