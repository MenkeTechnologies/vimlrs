package com.menketechnologies.vimlrs

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

class VimlrsSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = VimlrsLexer()

    override fun getTokenHighlights(type: IElementType): Array<TextAttributesKey> {
        val key: TextAttributesKey? = when (type) {
            VimlrsTokenTypes.COMMENT -> VimlrsColors.COMMENT
            VimlrsTokenTypes.SHEBANG -> VimlrsColors.SHEBANG
            VimlrsTokenTypes.STRING_DQ -> VimlrsColors.STRING_DQ
            VimlrsTokenTypes.STRING_SQ -> VimlrsColors.STRING_SQ
            VimlrsTokenTypes.NUMBER -> VimlrsColors.NUMBER

            VimlrsTokenTypes.KEYWORD -> VimlrsColors.KEYWORD
            VimlrsTokenTypes.COMMAND -> VimlrsColors.COMMAND
            VimlrsTokenTypes.BUILTIN_FUNCTION -> VimlrsColors.BUILTIN_FUNCTION
            VimlrsTokenTypes.FUNCTION_DECL -> VimlrsColors.FUNCTION_DECL
            VimlrsTokenTypes.IDENTIFIER -> VimlrsColors.IDENTIFIER

            VimlrsTokenTypes.SCOPE_VAR -> VimlrsColors.SCOPE_VAR
            VimlrsTokenTypes.SPECIAL_VAR -> VimlrsColors.SPECIAL_VAR
            VimlrsTokenTypes.OPTION -> VimlrsColors.OPTION
            VimlrsTokenTypes.ENV_VAR -> VimlrsColors.ENV_VAR
            VimlrsTokenTypes.REGISTER -> VimlrsColors.REGISTER

            VimlrsTokenTypes.OPERATOR -> VimlrsColors.OPERATOR
            VimlrsTokenTypes.ASSIGN_OP -> VimlrsColors.ASSIGN_OP
            VimlrsTokenTypes.BAR -> VimlrsColors.BAR
            VimlrsTokenTypes.LINE_CONTINUATION -> VimlrsColors.LINE_CONTINUATION

            VimlrsTokenTypes.PAREN -> VimlrsColors.PAREN
            VimlrsTokenTypes.LPAREN -> VimlrsColors.PAREN
            VimlrsTokenTypes.RPAREN -> VimlrsColors.PAREN
            VimlrsTokenTypes.BRACE -> VimlrsColors.BRACE
            VimlrsTokenTypes.LBRACE -> VimlrsColors.BRACE
            VimlrsTokenTypes.RBRACE -> VimlrsColors.BRACE
            VimlrsTokenTypes.BRACKET -> VimlrsColors.BRACKET
            VimlrsTokenTypes.LBRACKET -> VimlrsColors.BRACKET
            VimlrsTokenTypes.RBRACKET -> VimlrsColors.BRACKET
            VimlrsTokenTypes.COMMA -> VimlrsColors.COMMA

            TokenType.BAD_CHARACTER -> VimlrsColors.BAD_CHAR
            else -> null
        }
        return if (key == null) emptyArray() else arrayOf(key)
    }
}

class VimlrsSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        VimlrsSyntaxHighlighter()
}
