package com.menketechnologies.vimlrs

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as Defaults
import com.intellij.openapi.editor.HighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey

/**
 * Stable, plugin-owned [TextAttributesKey]s for every VimL token category.
 * Each key inherits a sensible default but lives in its own `VIMLRS_*`
 * namespace so users can rebind any of them in
 * *Settings → Editor → Color Scheme → vimlrs* without affecting the rest of
 * the IDE.
 */
object VimlrsColors {
    @JvmField val COMMENT = mk("VIMLRS_COMMENT", Defaults.LINE_COMMENT)
    @JvmField val SHEBANG = mk("VIMLRS_SHEBANG", Defaults.LINE_COMMENT)
    @JvmField val STRING_DQ = mk("VIMLRS_STRING_DQ", Defaults.STRING)
    @JvmField val STRING_SQ = mk("VIMLRS_STRING_SQ", Defaults.STRING)
    @JvmField val NUMBER = mk("VIMLRS_NUMBER", Defaults.NUMBER)

    @JvmField val KEYWORD = mk("VIMLRS_KEYWORD", Defaults.KEYWORD)
    // Ex commands get their own slot (defaults to METADATA) so `set`,
    // `autocmd`, `nnoremap`, `highlight`, `syntax` visually separate
    // from control-flow keywords.
    @JvmField val COMMAND = mk("VIMLRS_COMMAND", Defaults.METADATA)
    @JvmField val BUILTIN_FUNCTION = mk("VIMLRS_BUILTIN_FUNCTION", Defaults.STATIC_METHOD)
    @JvmField val FUNCTION_DECL = mk("VIMLRS_FUNCTION_DECL", Defaults.FUNCTION_DECLARATION)
    @JvmField val IDENTIFIER = mk("VIMLRS_IDENTIFIER", Defaults.IDENTIFIER)

    @JvmField val SCOPE_VAR = mk("VIMLRS_SCOPE_VAR", Defaults.GLOBAL_VARIABLE)
    @JvmField val SPECIAL_VAR = mk("VIMLRS_SPECIAL_VAR", Defaults.PREDEFINED_SYMBOL)
    @JvmField val OPTION = mk("VIMLRS_OPTION", Defaults.CONSTANT)
    @JvmField val ENV_VAR = mk("VIMLRS_ENV_VAR", Defaults.GLOBAL_VARIABLE)
    @JvmField val REGISTER = mk("VIMLRS_REGISTER", Defaults.INSTANCE_FIELD)

    @JvmField val OPERATOR = mk("VIMLRS_OPERATOR", Defaults.OPERATION_SIGN)
    @JvmField val ASSIGN_OP = mk("VIMLRS_ASSIGN_OP", Defaults.OPERATION_SIGN)
    @JvmField val BAR = mk("VIMLRS_BAR", Defaults.LABEL)
    @JvmField val LINE_CONTINUATION = mk("VIMLRS_LINE_CONTINUATION", Defaults.OPERATION_SIGN)

    @JvmField val PAREN = mk("VIMLRS_PAREN", Defaults.PARENTHESES)
    @JvmField val BRACE = mk("VIMLRS_BRACE", Defaults.BRACES)
    @JvmField val BRACKET = mk("VIMLRS_BRACKET", Defaults.BRACKETS)
    @JvmField val COMMA = mk("VIMLRS_COMMA", Defaults.COMMA)

    @JvmField val BAD_CHAR = mk("VIMLRS_BAD_CHAR", HighlighterColors.BAD_CHARACTER)

    private fun mk(name: String, fallback: TextAttributesKey): TextAttributesKey =
        TextAttributesKey.createTextAttributesKey(name, fallback)
}
