package com.menketechnologies.vimlrs

import com.intellij.psi.tree.IElementType

class VimlrsTokenType(debugName: String) : IElementType(debugName, VimlrsLanguage)

/**
 * Fine-grained VimL (Vimscript) token types. Each maps to its own
 * [VimlrsColors] entry so any category can be recolored independently.
 * When adding a token here, also add the matching case in
 * [VimlrsSyntaxHighlighter.getTokenHighlights] and the matching entry in
 * [VimlrsColorSettingsPage.attrs].
 */
object VimlrsTokenTypes {
    // ── Trivia / literals ──────────────────────────────────────────────
    /// `"` in command position runs to end-of-line — the classic VimL
    /// comment form. `#!` on line 1 is also a comment (shebang).
    @JvmField val COMMENT = VimlrsTokenType("VIMLRS_COMMENT")
    @JvmField val SHEBANG = VimlrsTokenType("VIMLRS_SHEBANG")
    /// `"..."` double-quoted string (in expression position) — honors
    /// backslash escapes (`\n`, `\t`, `\"`, `\<Esc>`).
    @JvmField val STRING_DQ = VimlrsTokenType("VIMLRS_STRING_DQ")
    /// `'...'` literal string — only `''` escapes a single quote;
    /// backslashes are literal.
    @JvmField val STRING_SQ = VimlrsTokenType("VIMLRS_STRING_SQ")
    @JvmField val NUMBER = VimlrsTokenType("VIMLRS_NUMBER")

    // ── Keywords / commands ────────────────────────────────────────────
    /// Statement / control keywords: `if` / `endif` / `while` /
    /// `function` / `let` / `call` / `echo` / `try` / `return` etc.
    @JvmField val KEYWORD = VimlrsTokenType("VIMLRS_KEYWORD")
    /// Common ex commands with their own color slot: `set` / `autocmd` /
    /// `nnoremap` / `highlight` / `syntax` / `source` / `silent` etc.
    @JvmField val COMMAND = VimlrsTokenType("VIMLRS_COMMAND")
    /// Built-in function name — only colored when immediately followed
    /// by `(`: `len(`, `has(`, `printf(`, `substitute(`.
    @JvmField val BUILTIN_FUNCTION = VimlrsTokenType("VIMLRS_BUILTIN_FUNCTION")
    /// User function declaration / autoload call name: `Foo` after
    /// `function`, or `plug#begin(` autoload-style names.
    @JvmField val FUNCTION_DECL = VimlrsTokenType("VIMLRS_FUNCTION_DECL")
    @JvmField val IDENTIFIER = VimlrsTokenType("VIMLRS_IDENTIFIER")

    // ── Variables ──────────────────────────────────────────────────────
    /// Scope-prefixed name — `g:` `s:` `b:` `w:` `t:` `l:` `a:` `v:`
    /// followed by an identifier, lexed as ONE token (`g:loaded_foo`).
    @JvmField val SCOPE_VAR = VimlrsTokenType("VIMLRS_SCOPE_VAR")
    /// Predefined `v:` variables — `v:true` / `v:false` / `v:null` /
    /// `v:count` / `v:val` / `v:shell_error` etc.
    @JvmField val SPECIAL_VAR = VimlrsTokenType("VIMLRS_SPECIAL_VAR")
    /// `&name` option reference (and `&l:name` / `&g:name`).
    @JvmField val OPTION = VimlrsTokenType("VIMLRS_OPTION")
    /// `$NAME` environment-variable reference.
    @JvmField val ENV_VAR = VimlrsTokenType("VIMLRS_ENV_VAR")
    /// `@x` register reference (`@"`, `@a`, `@+`).
    @JvmField val REGISTER = VimlrsTokenType("VIMLRS_REGISTER")

    // ── Operators ──────────────────────────────────────────────────────
    @JvmField val OPERATOR = VimlrsTokenType("VIMLRS_OPERATOR")
    /// `=` `+=` `-=` `*=` `/=` `%=` `.=` `..=`.
    @JvmField val ASSIGN_OP = VimlrsTokenType("VIMLRS_ASSIGN_OP")
    /// `|` — the command separator / bar.
    @JvmField val BAR = VimlrsTokenType("VIMLRS_BAR")
    /// `\` at the start of a continued line (`:h line-continuation`).
    @JvmField val LINE_CONTINUATION = VimlrsTokenType("VIMLRS_LINE_CONTINUATION")

    // ── Punctuation ────────────────────────────────────────────────────
    // Split L/R variants so `lang.braceMatcher` can pair them; the umbrella
    // `PAREN`/`BRACE`/`BRACKET` names stay for the color slot fallback.
    @JvmField val PAREN = VimlrsTokenType("VIMLRS_PAREN")
    @JvmField val LPAREN = VimlrsTokenType("VIMLRS_LPAREN")
    @JvmField val RPAREN = VimlrsTokenType("VIMLRS_RPAREN")
    @JvmField val BRACE = VimlrsTokenType("VIMLRS_BRACE")
    @JvmField val LBRACE = VimlrsTokenType("VIMLRS_LBRACE")
    @JvmField val RBRACE = VimlrsTokenType("VIMLRS_RBRACE")
    @JvmField val BRACKET = VimlrsTokenType("VIMLRS_BRACKET")
    @JvmField val LBRACKET = VimlrsTokenType("VIMLRS_LBRACKET")
    @JvmField val RBRACKET = VimlrsTokenType("VIMLRS_RBRACKET")
    @JvmField val COMMA = VimlrsTokenType("VIMLRS_COMMA")

    // ── Errors ─────────────────────────────────────────────────────────
    @JvmField val BAD = VimlrsTokenType("VIMLRS_BAD")
}
